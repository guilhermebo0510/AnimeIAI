//! Painel Voz sintética (S24 4.13, UI pronta para o motor opcional de 4.14): motor (Kokoro PT-BR/EN, VOICEVOX JA; o de
//! alta qualidade aparece desligado com a explicação), detecção de hardware com o cartão de requisitos, controles por
//! fala (estilo/emoção, intensidade, velocidade, altura, entonação, energia, pausas, número de takes), gerar/regerar
//! takes, ouvir, comparar A/B e escolher o take (substitui a fala selecionada no corte ou entra no cabeçote).
//!
//! Os motores atuais não têm vetor de emoção: o estilo vira ajustes de prosódia (velocidade, altura, entonação, volume),
//! escalados pela intensidade, nativos no VOICEVOX e por pós-processo (`audio_edit`: mudar altura, ganho, silêncio) no
//! Kokoro. Cada take é um asset novo (geração num fio sobre uma cópia do documento). O botão "Instalar motor opcional…"
//! só abre um diálogo que descreve o que seria instalado (onde, tamanho, licença); instalar ainda não existe ("em breve").
//! Nada de rede: o painel não baixa nada.

use super::hw::{self, Reason, Verdict};
use super::player::PlayRequest;
use super::{After, node_name};
use crate::app::actions;
use crate::app::editor::Editor;
use crate::app::panels::audio::set_audio;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_audio::steps::EditStep;
use aiai_core::{AudioPlacement, AudioTrack, CoreSession, Id, NodeKind};
use egui::{Rect, Ui};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Engine {
    #[default]
    Kokoro,
    Voicevox,
}

/// Estilos: (chave i18n, velocidade relativa, semitons, entonação, volume) na intensidade 1.
pub(crate) const STYLES: [(&str, f32, f32, f32, f32); 10] = [
    ("audio.tts.st.neutral", 0.0, 0.0, 0.0, 0.0),
    ("audio.tts.st.happy", 0.06, 1.5, 0.35, 0.10),
    ("audio.tts.st.sad", -0.12, -1.5, -0.30, -0.15),
    ("audio.tts.st.angry", 0.08, 0.5, 0.45, 0.30),
    ("audio.tts.st.fear", 0.10, 2.0, -0.10, -0.10),
    ("audio.tts.st.surprise", 0.04, 3.0, 0.60, 0.05),
    ("audio.tts.st.whisper", -0.05, -0.5, -0.50, -0.50),
    ("audio.tts.st.shout", 0.05, 2.5, 0.50, 0.50),
    ("audio.tts.st.sarcastic", -0.06, -0.5, 0.25, 0.0),
    ("audio.tts.st.tired", -0.15, -1.0, -0.40, -0.20),
];

const KOKORO_VOICES: [&str; 7] = ["pf_dora", "pm_alex", "pm_santa", "af_heart", "af_bella", "am_adam", "bf_emma"];

pub(crate) struct Take {
    pub line: u64,
    pub label: String,
    pub asset: Id,
    pub secs: f64,
}

/// Pedido de geração na fila (um take por vez; o trabalho do workspace é um só).
struct Pending {
    line: u64,
    label: String,
    action: &'static str,
    params: serde_json::Value,
    pad: (f64, f64),
    post: Vec<EditStep>,
}

pub(crate) struct TtsState {
    pub engine: Engine,
    pub text: String,
    pub character: Option<Id>,
    pub style: usize,
    pub intensity: f32,
    pub speed: f32,
    pub pitch: f32,
    pub intonation: f32,
    pub energy: f32,
    pub pause_before_ms: f32,
    pub pause_after_ms: f32,
    pub takes_n: usize,
    pub kokoro_voice: String,
    /// Estilo do VOICEVOX (0 = o do perfil de voz do personagem).
    pub voicevox_style: u32,
    pub takes: Vec<Take>,
    queue: VecDeque<Pending>,
    pub compare: [Option<usize>; 2],
    pub install_dialog: bool,
    next_label: usize,
}

impl Default for TtsState {
    fn default() -> Self {
        TtsState { engine: Engine::Kokoro, text: String::new(), character: None, style: 0, intensity: 1.0, speed: 1.0, pitch: 0.0, intonation: 1.0, energy: 1.0, pause_before_ms: 0.0, pause_after_ms: 0.0, takes_n: 3, kokoro_voice: "pf_dora".into(), voicevox_style: 0, takes: Vec::new(), queue: VecDeque::new(), compare: [None, None], install_dialog: false, next_label: 0 }
    }
}

/// Prosódia efetiva de um take: estilo × intensidade + ajustes manuais + uma pequena variação por take (o take A é exato).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Prosody {
    pub speed: f32,
    pub semitones: f32,
    pub intonation: f32,
    pub volume: f32,
}

impl TtsState {
    pub(crate) fn line_key(&self) -> u64 {
        let s = format!("{:?}|{}|{}", self.engine, self.character.as_ref().map(|c| c.0.as_str()).unwrap_or(""), self.text.trim());
        s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    pub(crate) fn prosody(&self, take: usize) -> Prosody {
        let (_, sp, st, it, vo) = STYLES[self.style.min(STYLES.len() - 1)];
        let k = self.intensity;
        let j = take as f32;
        let (jsp, jst, jit) = if take == 0 { (0.0, 0.0, 0.0) } else { (0.03 * (j * 2.1).sin(), 0.3 * (j * 1.3).sin(), 0.08 * (j * 1.7).cos()) };
        Prosody {
            speed: (self.speed * (1.0 + sp * k + jsp)).clamp(0.5, 2.0),
            semitones: (self.pitch + st * k + jst).clamp(-12.0, 12.0),
            intonation: (self.intonation + it * k + jit).clamp(0.0, 2.0),
            volume: (self.energy * (1.0 + vo * k)).clamp(0.2, 2.0),
        }
    }

    pub(crate) fn add_take(&mut self, line: u64, label: String, asset: Id, secs: f64) {
        self.takes.push(Take { line, label, asset, secs });
    }

    /// Pedido da bridge para o take `i`: (ação, parâmetros, silêncios, pós-processo).
    pub(crate) fn request(&self, take: usize, name: &str) -> (&'static str, serde_json::Value, (f64, f64), Vec<EditStep>) {
        let p = self.prosody(take);
        let pad = (self.pause_before_ms as f64 / 1000.0, self.pause_after_ms as f64 / 1000.0);
        match self.engine {
            Engine::Voicevox => {
                // VOICEVOX: tudo nativo (altura em escala log: 1 semitom = ln 2 / 12; o engine aceita ±0,15)
                let mut params = json!({
                    "text": self.text.trim(),
                    "name": name,
                    "params": {
                        "speed_scale": p.speed,
                        "pitch_scale": (p.semitones as f64 * std::f64::consts::LN_2 / 12.0).clamp(-0.15, 0.15),
                        "intonation_scale": p.intonation,
                        "volume_scale": p.volume,
                        "pre_phoneme_length": 0.1 + pad.0,
                        "post_phoneme_length": 0.1 + pad.1,
                    },
                });
                if let Some(c) = &self.character {
                    params["character"] = json!(c);
                }
                if self.voicevox_style > 0 {
                    params["speaker"] = json!(self.voicevox_style);
                }
                ("TTS_LINE", params, (0.0, 0.0), Vec::new())
            }
            Engine::Kokoro => {
                let lang = match self.kokoro_voice.chars().next() {
                    Some('p') => "pt-br",
                    Some('b') => "en-gb",
                    _ => "en-us",
                };
                let mut params = json!({ "text": self.text.trim(), "name": name, "kokoro": { "voice": self.kokoro_voice, "lang": lang, "speed": p.speed } });
                if let Some(c) = &self.character {
                    params["character"] = json!(c);
                }
                let mut post = Vec::new();
                if p.semitones.abs() > 0.05 {
                    post.push(EditStep::Fx { fx: aiai_audio::fx::AudioFx::PitchShift(aiai_audio::fx::PitchShift { semitones: p.semitones, ..Default::default() }), start_s: None, end_s: None, profile_start_s: None, profile_end_s: None });
                }
                if (p.volume - 1.0).abs() > 0.01 {
                    post.push(EditStep::Gain { gain_db: 20.0 * (p.volume as f64).log10(), start_s: None, end_s: None });
                }
                ("TTS_KOKORO", params, pad, post)
            }
        }
    }
}

/// Manda o próximo take da fila quando o workspace está livre (chamado a cada quadro).
pub(crate) fn pump(core: &CoreSession, ed: &mut Editor) {
    if ed.audio_ws.job.is_some() {
        return;
    }
    let Some(p) = ed.audio_ws.tts.queue.pop_front() else { return };
    let label = format!("{} {}", tr("audio.tts.take"), p.label);
    let r = super::spawn_action(&mut ed.audio_ws, core, p.action, p.params, p.pad, p.post, label, After::Take { line: p.line, label: p.label });
    if let Err(e) = r {
        ed.notify(e);
        ed.audio_ws.tts.queue.clear();
    }
}

fn take_letter(i: usize) -> String {
    let a = (b'A' + (i % 26) as u8) as char;
    if i < 26 { a.to_string() } else { format!("{a}{}", i / 26) }
}

// ------------------------------------------------------------------------------------------------ painel

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    if !env.ed.audio_ws.hw.probed {
        env.ed.audio_ws.hw.probe();
    }
    let mut child = body_ui(ui, body, 6.0);
    egui::ScrollArea::vertical().id_salt("audio_tts").auto_shrink([false, false]).show(&mut child, |ui| tts_body(env, ui));
    install_dialog(env, ui.ctx());
}

fn gb(b: Option<u64>) -> String {
    b.map(|b| format!("{:.1} GB", hw::gib(b))).unwrap_or_else(|| "—".into())
}

fn tts_body(env: &mut Env, ui: &mut Ui) {
    tts_main(env, ui);
    // hardware e motores opcionais ficam recolhidos no fim (auditoria UX-09): primeiro o motor pronto e a fala
    ui.add_space(8.0);
    let r = egui::CollapsingHeader::new(tr("audio.tts.hardware_more")).id_salt("tts_hw").default_open(false).show(ui, |ui| hardware_card(env, ui));
    env.note("audio:tts:hardware", r.header_response.rect);
}

fn tts_main(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    // ---- motor
    section(ui, &t, "audio.tts.engine");
    let mut e = env.ed.audio_ws.tts.engine;
    ui.horizontal_wrapped(|ui| {
        let r = ui.radio_value(&mut e, Engine::Kokoro, tr("audio.tts.kokoro"));
        env.note("audio:tts:kokoro", r.rect);
        let r = ui.radio_value(&mut e, Engine::Voicevox, tr("audio.tts.voicevox"));
        env.note("audio:tts:voicevox", r.rect);
        let r = ui.add_enabled(false, egui::RadioButton::new(false, tr("audio.tts.hq")));
        env.note("audio:tts:hq", r.rect);
        r.on_disabled_hover_text(tr("audio.tts.hq.tip"));
    });
    env.ed.audio_ws.tts.engine = e;
    let status = match e {
        Engine::Kokoro => match aiai_audio::kokoro::find_script() {
            None => (false, tr("audio.tts.kokoro_no_script").to_owned()),
            Some(s) if !aiai_audio::kokoro::models_present(&s) => (false, tr("audio.tts.kokoro_no_models").to_owned()),
            Some(_) => (true, tr("audio.tts.kokoro_ready").to_owned()),
        },
        Engine::Voicevox => (true, format!("{} {}", tr("audio.tts.voicevox_state"), aiai_tools::audio::VOICEVOX_URL)),
    };
    ui.label(egui::RichText::new(&status.1).size(11.5).color(if status.0 { t.text_dim } else { t.danger }));
    dim_label(ui, &t, tr(if e == Engine::Kokoro { "audio.tts.kokoro_license" } else { "audio.tts.voicevox_license" }));
    dim_label(ui, &t, tr("audio.tts.fish_unavailable"));

    // ---- fala
    section(ui, &t, "audio.tts.line");
    lines_from_cut(env, ui);
    let chars: Vec<(Id, String)> = env.core.project.nodes.values().filter(|n| matches!(n.data, NodeKind::Character(_))).map(|n| (n.id.clone(), n.name.clone())).collect();
    ui.horizontal_wrapped(|ui| {
        let te = ui.add(egui::TextEdit::multiline(&mut env.ed.audio_ws.tts.text).hint_text(tr("audio.tts.text_hint")).desired_rows(2).desired_width(ui.available_width().min(380.0)));
        env.note("audio:tts:text", te.rect);
    });
    ui.horizontal_wrapped(|ui| {
        let cur = env.ed.audio_ws.tts.character.clone().filter(|c| chars.iter().any(|(i, _)| i == c));
        let label = cur.as_ref().and_then(|c| chars.iter().find(|(i, _)| i == c)).map(|c| c.1.clone()).unwrap_or_else(|| tr("audio.tts.no_character").to_owned());
        let mut pick = cur.clone();
        let r = egui::ComboBox::from_id_salt("audio_tts_ws_char").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut pick, None, tr("audio.tts.no_character"));
            for (i, n) in &chars {
                ui.selectable_value(&mut pick, Some(i.clone()), n);
            }
        });
        env.note("audio:tts:character", r.response.rect);
        env.ed.audio_ws.tts.character = pick;
        match env.ed.audio_ws.tts.engine {
            Engine::Kokoro => {
                let mut v = env.ed.audio_ws.tts.kokoro_voice.clone();
                egui::ComboBox::from_id_salt("audio_tts_voice").selected_text(&v).show_ui(ui, |ui| {
                    for x in KOKORO_VOICES {
                        ui.selectable_value(&mut v, x.to_owned(), x);
                    }
                });
                env.ed.audio_ws.tts.kokoro_voice = v;
            }
            Engine::Voicevox => {
                ui.label(tr("audio.tts.style_id"));
                ui.add(egui::DragValue::new(&mut env.ed.audio_ws.tts.voicevox_style).range(0..=200)).on_hover_text(tr("audio.tts.style_id.tip"));
            }
        }
    });

    // ---- estilo e prosódia
    section(ui, &t, "audio.tts.style");
    ui.horizontal_wrapped(|ui| {
        for (i, (key, ..)) in STYLES.iter().enumerate() {
            let r = ui.selectable_label(env.ed.audio_ws.tts.style == i, tr(key));
            env.note(format!("audio:tts:style:{i}"), r.rect);
            if r.clicked() {
                env.ed.audio_ws.tts.style = i;
            }
        }
    });
    let kokoro = env.ed.audio_ws.tts.engine == Engine::Kokoro;
    egui::Grid::new("audio_tts_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        let st = &mut env.ed.audio_ws.tts;
        dim_label(ui, &t, tr("audio.tts.intensity"));
        ui.add(egui::Slider::new(&mut st.intensity, 0.0..=2.0).max_decimals(2));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.speed"));
        ui.add(egui::Slider::new(&mut st.speed, 0.5..=2.0).max_decimals(2).suffix("×"));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.pitch"));
        ui.add(egui::Slider::new(&mut st.pitch, -6.0..=6.0).max_decimals(1).suffix(" st")).on_hover_text(tr(if kokoro { "audio.tts.post.tip" } else { "audio.tts.native.tip" }));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.intonation"));
        ui.add_enabled(!kokoro, egui::Slider::new(&mut st.intonation, 0.0..=2.0).max_decimals(2)).on_disabled_hover_text(tr("audio.tts.kokoro_no_intonation"));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.energy"));
        ui.add(egui::Slider::new(&mut st.energy, 0.3..=1.8).max_decimals(2));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.pause_before"));
        ui.add(egui::Slider::new(&mut st.pause_before_ms, 0.0..=1500.0).max_decimals(0).suffix(" ms"));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.pause_after"));
        ui.add(egui::Slider::new(&mut st.pause_after_ms, 0.0..=1500.0).max_decimals(0).suffix(" ms"));
        ui.end_row();
        dim_label(ui, &t, tr("audio.tts.takes_n"));
        ui.add(egui::Slider::new(&mut st.takes_n, 1..=5));
        ui.end_row();
    });
    let p = env.ed.audio_ws.tts.prosody(0);
    dim_label(ui, &t, &format!("{}: {:.2}× · {:+.1} st · {} {:.2} · vol {:.2}", tr("audio.tts.effective"), p.speed, p.semitones, tr("audio.tts.intonation"), p.intonation, p.volume));

    // ---- gerar
    let ready = !env.ed.audio_ws.tts.text.trim().is_empty() && (env.ed.audio_ws.tts.engine == Engine::Kokoro || env.ed.audio_ws.tts.character.is_some() || env.ed.audio_ws.tts.voicevox_style > 0) && status.0;
    let pending = env.ed.audio_ws.tts.queue.len() + usize::from(matches!(env.ed.audio_ws.job.as_ref().map(|j| &j.after), Some(After::Take { .. })));
    ui.horizontal_wrapped(|ui| {
        let n = env.ed.audio_ws.tts.takes_n;
        let b = ui.add_enabled(ready && pending == 0, egui::Button::new(egui::RichText::new(format!("{} ({n})", tr("audio.tts.generate"))).strong()));
        env.note("audio:tts:generate", b.rect);
        if b.on_hover_text(tr("audio.tts.generate.tip")).clicked() {
            enqueue(env, n);
        }
        let b = ui.add_enabled(ready && pending == 0, egui::Button::new(tr("audio.tts.regenerate")));
        env.note("audio:tts:regenerate", b.rect);
        if b.on_hover_text(tr("audio.tts.regenerate.tip")).clicked() {
            enqueue(env, 1);
        }
        if pending > 0 {
            ui.spinner();
            dim_label(ui, &t, &format!("{} {pending}", tr("audio.tts.generating")));
            if ui.small_button(tr("audio.tts.cancel")).clicked() {
                env.ed.audio_ws.tts.queue.clear();
            }
        }
    });
    takes(env, ui);
}

fn hardware_card(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    section(ui, &t, "audio.tts.hardware");
    let h = env.ed.audio_ws.hw.clone();
    let frame = egui::Frame::new().fill(t.bg_widget).corner_radius(4).inner_margin(egui::Margin::same(6));
    let r = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let vram = h.vram_active();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("GPU: {}", h.gpu_name.clone().unwrap_or_else(|| tr("audio.tts.gpu_unknown").to_owned()))).strong());
            if let (Some(k), Some(b)) = (&h.gpu_kind, &h.backend) {
                dim_label(ui, &t, &format!("{k} · {b}"));
            }
            dim_label(ui, &t, &format!("VRAM {}", gb(vram)));
        });
        for a in h.adapters.iter().filter(|a| h.gpu_name.as_ref().is_none_or(|n| !a.name.eq_ignore_ascii_case(n))) {
            dim_label(ui, &t, &format!("  {} · VRAM {}", a.name, gb(a.vram_bytes)));
        }
        ui.horizontal_wrapped(|ui| {
            dim_label(ui, &t, &format!("RAM {} ({} {})", gb(h.ram_total), gb(h.ram_free), tr("audio.tts.free")));
            for (d, f) in &h.disks {
                dim_label(ui, &t, &format!("· {} {} {}", d.display(), gb(*f), tr("audio.tts.free")));
            }
        });
        // requisitos do motor opcional
        let (v, why) = hw::hq_verdict(vram, h.ram_total, h.install_free(), &hw::HQ_REQ);
        ui.separator();
        ui.label(egui::RichText::new(tr("audio.tts.req_title")).size(11.5).strong());
        dim_label(ui, &t, tr("audio.tts.req_line"));
        let col = match v {
            Verdict::Good => t.st_ok,
            Verdict::Fits => t.st_review,
            Verdict::CpuOnly => t.st_progress,
            Verdict::Blocked => t.st_retake,
        };
        let vr = ui.label(egui::RichText::new(tr(v.key())).strong().background_color(col).color(t.text));
        env.note("audio:tts:verdict", vr.rect);
        for w in why {
            let s = match w {
                Reason::VramUnknown => tr("audio.tts.why.vram_unknown").to_owned(),
                Reason::VramLow(g) => format!("{} ({g:.1} GB < {:.0} GB)", tr("audio.tts.why.vram_low"), hw::HQ_REQ.vram_min),
                Reason::VramBelowRec(g) => format!("{} ({g:.1} GB < {:.0} GB)", tr("audio.tts.why.vram_below"), hw::HQ_REQ.vram_rec),
                Reason::VramOk(g) => format!("{} ({g:.1} GB)", tr("audio.tts.why.vram_ok")),
                Reason::RamLow(g) => format!("{} ({g:.1} GB < {:.0} GB)", tr("audio.tts.why.ram_low"), hw::HQ_REQ.ram_min),
                Reason::DiskLow(g) => format!("{} ({g:.1} GB < {:.0} GB)", tr("audio.tts.why.disk_low"), hw::HQ_REQ.disk_need),
                Reason::DiskOk(g) => format!("{} ({g:.1} GB)", tr("audio.tts.why.disk_ok")),
            };
            dim_label(ui, &t, &format!("• {s}"));
        }
        ui.horizontal_wrapped(|ui| {
            let b = ui.button(tr("audio.tts.install"));
            env.note("audio:tts:install", b.rect);
            if b.on_hover_text(tr("audio.tts.install.tip")).clicked() {
                env.ed.audio_ws.tts.install_dialog = true;
            }
            let b = ui.small_button(tr("audio.tts.refresh_hw"));
            env.note("audio:tts:refresh_hw", b.rect);
            if b.clicked() {
                env.ed.audio_ws.hw.probe();
            }
        });
    });
    env.note("audio:tts:hardware", r.response.rect);
}

/// Falas da cena do corte ativo (roteiro): clique preenche o texto e o personagem.
fn lines_from_cut(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let info = crate::app::panels::scene_info::gather(&env.core.project, &cut);
    let Some((_, scene)) = &info.script else {
        dim_label(ui, &t, tr("audio.tts.no_script_lines"));
        return;
    };
    let lines: Vec<_> = scene.elements.iter().filter(|e| e.kind == aiai_tools::preprod::ElementKind::Dialogue && !e.text.trim().is_empty()).collect();
    if lines.is_empty() {
        dim_label(ui, &t, tr("audio.tts.no_script_lines"));
        return;
    }
    egui::CollapsingHeader::new(format!("{} ({})", tr("audio.tts.script_lines"), lines.len())).id_salt("audio_tts_lines").default_open(true).show(ui, |ui| {
        for (i, e) in lines.iter().enumerate() {
            let who = e.character.as_ref().map(|c| node_name(&env.core.project, c)).or_else(|| e.speaker.clone()).unwrap_or_default();
            let linked = info.linked.contains(&e.id);
            let text = if env.ed.audio_ws.tts.engine == Engine::Kokoro && !e.translation.trim().is_empty() { e.translation.clone() } else { e.text.clone() };
            let label = format!("{}{who}: {}", if linked { "● " } else { "" }, text.chars().take(60).collect::<String>());
            let r = ui.selectable_label(env.ed.audio_ws.tts.text.trim() == text.trim(), egui::RichText::new(label).size(11.5));
            env.note(format!("audio:tts:line:{i}"), r.rect);
            if r.clicked() {
                let st = &mut env.ed.audio_ws.tts;
                st.text = text;
                st.character = e.character.clone();
                if let Some(em) = &e.emotion {
                    if let Some(k) = style_from_text(em) {
                        st.style = k;
                    }
                }
            }
        }
    });
}

/// Emoção escrita no roteiro → estilo (pt/en/ja, palavras comuns).
pub(crate) fn style_from_text(s: &str) -> Option<usize> {
    let s = s.to_lowercase();
    let table: [(usize, &[&str]); 9] = [
        (1, &["alegr", "feliz", "happy", "joy", "嬉", "楽"]),
        (2, &["trist", "sad", "悲"]),
        (3, &["raiva", "brav", "angry", "怒"]),
        (4, &["medo", "fear", "afraid", "怖"]),
        (5, &["surpres", "surpris", "驚"]),
        (6, &["sussurr", "whisper", "囁", "ささや"]),
        (7, &["grit", "shout", "yell", "叫"]),
        (8, &["sarc", "irôn", "iron"]),
        (9, &["cansad", "tired", "疲"]),
    ];
    table.iter().find(|(_, ws)| ws.iter().any(|w| s.contains(w))).map(|(k, _)| *k)
}

fn enqueue(env: &mut Env, n: usize) {
    let line = env.ed.audio_ws.tts.line_key();
    let short: String = env.ed.audio_ws.tts.text.trim().chars().take(24).collect();
    for k in 0..n {
        let st = &mut env.ed.audio_ws.tts;
        let letter = take_letter(st.next_label);
        st.next_label += 1;
        let name = format!("Take {letter}: {short}");
        let (action, params, pad, post) = st.request(k, &name);
        st.queue.push_back(Pending { line, label: letter, action, params, pad, post });
    }
}

fn takes(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    section(ui, &t, "audio.tts.takes");
    let line = env.ed.audio_ws.tts.line_key();
    let idx: Vec<usize> = env.ed.audio_ws.tts.takes.iter().enumerate().filter(|(_, tk)| tk.line == line && env.core.project.node(&tk.asset).is_some()).map(|(i, _)| i).collect();
    if idx.is_empty() {
        dim_label(ui, &t, tr("audio.tts.no_takes"));
        return;
    }
    let sel_dialogue = env.ed.audio.sel.clone().and_then(|s| {
        let cut = env.ed.cut.clone()?;
        super::cut_audio(&env.core.project, &cut).1.into_iter().find(|a| a.id == s && a.track == AudioTrack::Dialogue)
    });
    let mut play: Option<usize> = None;
    let mut choose: Option<usize> = None;
    for &i in &idx {
        let tk = &env.ed.audio_ws.tts.takes[i];
        let (label, secs, asset) = (tk.label.clone(), tk.secs, tk.asset.clone());
        ui.horizontal(|ui| {
            let key = format!("tts:{}", asset.0);
            let playing = env.ed.audio_ws.player.is_playing(&key);
            if env.icon_button_sized(ui, if playing { IconId::Pause } else { IconId::Play }, "audio.tts.listen", playing, true, &format!("audio:tts:play:{label}"), 20.0, false).clicked() {
                play = Some(i);
            }
            ui.label(egui::RichText::new(format!("{label}  {secs:.2} s")).monospace());
            for (slot, ab) in [(0usize, "A"), (1, "B")] {
                let on = env.ed.audio_ws.tts.compare[slot] == Some(i);
                if ui.selectable_label(on, ab).on_hover_text(tr("audio.tts.compare.tip")).clicked() {
                    env.ed.audio_ws.tts.compare[slot] = if on { None } else { Some(i) };
                }
            }
            let b = ui.button(tr(if sel_dialogue.is_some() { "audio.tts.use_replace" } else { "audio.tts.use_place" }));
            env.note(format!("audio:tts:use:{label}"), b.rect);
            if b.on_hover_text(tr("audio.tts.use.tip")).clicked() {
                choose = Some(i);
            }
        });
    }
    let [a, b] = env.ed.audio_ws.tts.compare;
    ui.horizontal(|ui| {
        let r = ui.add_enabled(a.is_some() && b.is_some() && a != b, egui::Button::new(tr("audio.tts.compare")));
        env.note("audio:tts:compare", r.rect);
        if r.on_hover_text(tr("audio.tts.compare.tip")).clicked() {
            compare(env, a.unwrap_or(0), b.unwrap_or(0));
        }
    });
    if let Some(i) = play {
        let asset = env.ed.audio_ws.tts.takes[i].asset.clone();
        let key = format!("tts:{}", asset.0);
        if env.ed.audio_ws.player.is_playing(&key) {
            env.ed.audio_ws.player.stop();
        } else if let Some(l) = env.ed.audio_ws.load(env.core, &asset) {
            let n = l.buf.frames();
            let label = format!("{} {}", tr("audio.tts.take"), env.ed.audio_ws.tts.takes[i].label);
            env.ed.audio_ws.player.play(PlayRequest { buf: l.buf, start: 0, end: n, looping: false, rack: None, key, label });
        }
    }
    if let Some(i) = choose {
        use_take(env, i, sel_dialogue);
    }
}

/// A/B: o take A, meio segundo de silêncio e o take B, num só buffer.
fn compare(env: &mut Env, a: usize, b: usize) {
    let (ia, ib) = (env.ed.audio_ws.tts.takes[a].asset.clone(), env.ed.audio_ws.tts.takes[b].asset.clone());
    let (Some(la), Some(lb)) = (env.ed.audio_ws.load(env.core, &ia), env.ed.audio_ws.load(env.core, &ib)) else { return };
    let conf = aiai_audio::edit::conform(&lb.buf, la.buf.rate, la.buf.channels());
    let gap = aiai_audio::edit::insert_silence(&la.buf, la.buf.frames(), la.buf.rate as usize / 2);
    let all = aiai_audio::edit::insert(&gap, gap.frames(), &conf);
    let n = all.frames();
    let label = format!("A/B {} · {}", env.ed.audio_ws.tts.takes[a].label, env.ed.audio_ws.tts.takes[b].label);
    env.ed.audio_ws.player.play(PlayRequest { buf: Arc::new(all), start: 0, end: n, looping: false, rack: None, key: "tts:cmp".into(), label });
}

/// Escolher o take: troca o asset da fala selecionada (diálogo) ou coloca o take no cabeçote da trilha de diálogo.
fn use_take(env: &mut Env, i: usize, sel: Option<AudioPlacement>) {
    let Some(cut) = env.ed.active_cut(env.core) else {
        env.ed.notify(tr("panel.multitrack.empty"));
        return;
    };
    let asset = env.ed.audio_ws.tts.takes[i].asset.clone();
    let (_, mut list) = super::cut_audio(&env.core.project, &cut);
    match sel {
        Some(s) => {
            if let Some(a) = list.iter_mut().find(|a| a.id == s.id) {
                a.asset = asset;
                a.trim_start = 0.0;
                a.duration = None;
            }
        }
        None => {
            let id = aiai_core::IdGen::Uuid7.next();
            list.push(AudioPlacement { id: id.clone(), asset, track: AudioTrack::Dialogue, start_frame: env.ed.frame, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: env.ed.audio_ws.tts.character.clone() });
            env.ed.audio.sel = Some(id);
        }
    }
    let r = set_audio(env.core, &cut, &list);
    actions::notify_err(env.ed, r);
}

fn install_dialog(env: &mut Env, ctx: &egui::Context) {
    if !env.ed.audio_ws.tts.install_dialog {
        return;
    }
    let t = env.t;
    let dir = hw::hq_install_dir();
    let free = env.ed.audio_ws.hw.install_free();
    let mut open = true;
    let mut close = false;
    let r = egui::Window::new(tr("audio.tts.install_title")).id(egui::Id::new("audio_tts_install")).collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.set_max_width(460.0);
        egui::Grid::new("audio_tts_install_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for (k, v) in [
                ("audio.tts.inst.engine", "Style-Bert-VITS2 (JP-Extra)".to_owned()),
                ("audio.tts.inst.source", "github.com/litagin02/Style-Bert-VITS2".to_owned()),
                ("audio.tts.inst.folder", dir.display().to_string()),
                ("audio.tts.inst.size", format!("{} ({} {})", tr("audio.tts.inst.size_v"), gb(free), tr("audio.tts.free"))),
                ("audio.tts.inst.license", tr("audio.tts.inst.license_v").to_owned()),
                ("audio.tts.inst.hw", tr("audio.tts.req_line").to_owned()),
            ] {
                dim_label(ui, &t, tr(k));
                ui.label(egui::RichText::new(v).size(12.0));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr("audio.tts.inst.note")).size(11.5).color(t.text_dim));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = ui.add_enabled(false, egui::Button::new(tr("audio.tts.inst.confirm")));
            env.note("audio:tts:install_confirm", b.rect);
            b.on_disabled_hover_text(tr("audio.tts.inst.soon"));
            let c = ui.button(tr("audio.tts.inst.close"));
            env.note("audio:tts:install_close", c.rect);
            if c.clicked() {
                close = true;
            }
        });
    });
    if let Some(r) = r {
        env.note("audio:tts:install_dialog", r.response.rect);
    }
    if !open || close {
        env.ed.audio_ws.tts.install_dialog = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_scale_with_intensity_and_take_a_is_exact() {
        let mut st = TtsState { style: 1, ..Default::default() };
        let p0 = st.prosody(0);
        assert!(p0.speed > 1.0 && p0.semitones > 0.0, "alegre: mais rápido e mais agudo");
        st.intensity = 0.0;
        assert_eq!(st.prosody(0), Prosody { speed: 1.0, semitones: 0.0, intonation: 1.0, volume: 1.0 }, "intensidade 0 = neutro");
        st.intensity = 2.0;
        assert!(st.prosody(0).semitones > p0.semitones);
        // takes B, C... variam de leve e de forma determinística
        assert_ne!(st.prosody(1), st.prosody(0));
        assert_eq!(st.prosody(2), st.prosody(2));
    }

    #[test]
    fn requests_map_to_each_engine() {
        let mut st = TtsState { text: " olá ".into(), pitch: 2.0, pause_before_ms: 200.0, energy: 0.5, ..Default::default() };
        let (a, p, pad, post) = st.request(0, "Take A");
        assert_eq!(a, "TTS_KOKORO");
        assert_eq!(p["text"], "olá");
        assert_eq!(p["kokoro"]["lang"], "pt-br");
        assert_eq!(pad, (0.2, 0.0));
        assert_eq!(post.len(), 2, "altura e energia por pós-processo: {post:?}");
        st.engine = Engine::Voicevox;
        st.character = Some(Id("c1".into()));
        let (a, p, pad, post) = st.request(0, "Take A");
        assert_eq!(a, "TTS_LINE");
        assert!(post.is_empty() && pad == (0.0, 0.0), "VOICEVOX faz tudo nativo");
        let ps = p["params"]["pitch_scale"].as_f64().unwrap();
        assert!((ps - 2.0 * std::f64::consts::LN_2 / 12.0).abs() < 1e-6);
        assert!((p["params"]["pre_phoneme_length"].as_f64().unwrap() - 0.3).abs() < 1e-9);
        assert_eq!(p["character"], "c1");
        // a requisição tem de passar na validação da bridge (sem chaves estranhas)
        let r: Result<aiai_tools::audio::TtsLineRequest, _> = serde_json::from_value(p);
        assert!(r.is_ok(), "{r:?}");
        st.engine = Engine::Kokoro;
        let (_, p, _, _) = st.request(0, "x");
        assert!(serde_json::from_value::<aiai_tools::audio::KokoroLineRequest>(p).is_ok());
    }

    #[test]
    fn script_emotions_pick_a_style() {
        assert_eq!(style_from_text("Feliz, animada"), Some(1));
        assert_eq!(style_from_text("whispering"), Some(6));
        assert_eq!(style_from_text("怒り"), Some(3));
        assert_eq!(style_from_text("neutra"), None);
        assert_eq!(take_letter(0), "A");
        assert_eq!(take_letter(27), "B1");
    }
}

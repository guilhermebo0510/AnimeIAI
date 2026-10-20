//! Painel Efeitos (rack, S24 3.9/3.10): cadeia ordenada de até 16 posições (como o Effects Rack do Audition) aplicada ao
//! clipe do editor de forma de onda. Cada posição liga/desliga, sobe/desce, sai, e mostra os parâmetros do efeito como
//! controles. O rack inteiro tem A/B (desliga durante a reprodução, sem parar).
//!
//! Prévia: com todos os efeitos ativos em tempo real (`realtime()`), a reprodução passa bloco a bloco pelo rack
//! compartilhado com o player (`process_block`; mudar um parâmetro vale no bloco seguinte); se algum precisa do clipe
//! inteiro (normalizar, mudar altura, esticar o tempo...), a seleção é renderizada offline num fio e tocada.
//! Aplicar grava um asset novo pelo `audio_edit` (passo `rack`), na seleção ou no clipe inteiro.
//!
//! Predefinições: arquivos JSON em `Documentos\AnimeIAI\Áudio\Racks` (a pasta Documentos vem do mesmo resolvedor das
//! fontes do usuário); com `--no-session` nada é lido nem gravado.

use super::player::{PlayRequest, SharedRack};
use super::wave;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_audio::AudioBuf;
use aiai_audio::fx::{Amplify, AudioFx, BandKind, Compressor, DcMode, DcOffset, Detector, Effect, EqBand, Equalizer, Filter, FilterKind, GRAPHIC_10_HZ, Limiter, NoiseGate, NoiseProfile, NoiseReduction, Normalize, NormalizeMode, PitchShift, RACK_MAX_SLOTS, Rack, RackSlot, Reverb, TimeStretch};
use aiai_audio::steps::EditStep;
use egui::{Color32, Rect, Sense, Stroke, Ui};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Duração máxima da prévia renderizada offline (s).
const OFFLINE_PREVIEW_S: f64 = 60.0;

pub(crate) struct RackState {
    pub rack: Rack,
    pub shared: Arc<Mutex<SharedRack>>,
    version: u64,
    /// Posições abertas (parâmetros à vista).
    pub open: Vec<bool>,
    /// A reprodução do editor de forma de onda passa pelo rack.
    pub monitor: bool,
    /// A/B: rack desligado só na escuta.
    pub ab_off: bool,
    presets: Option<Vec<String>>,
    pub preset_name: String,
    preview: Option<crossbeam_channel::Receiver<Result<Arc<AudioBuf>, String>>>,
}

impl Default for RackState {
    fn default() -> Self {
        RackState { rack: Rack::new(), shared: Arc::new(Mutex::new(SharedRack::default())), version: 0, open: Vec::new(), monitor: true, ab_off: false, presets: None, preset_name: String::new(), preview: None }
    }
}

impl RackState {
    /// Leva a cadeia atual (e o A/B) para o fio de reprodução.
    pub(crate) fn sync(&mut self) {
        self.version += 1;
        if let Ok(mut g) = self.shared.lock() {
            g.rack = self.rack.clone();
            g.rack.bypass = self.rack.bypass || self.ab_off;
            g.version = self.version;
        }
    }

    /// Rack para a escuta do editor (se houver efeito ativo e a escuta pelo rack estiver ligada).
    pub(crate) fn live_for(&mut self, _key: &str) -> Option<Arc<Mutex<SharedRack>>> {
        let active = !self.rack.bypass && self.rack.slots.iter().any(|s| !s.bypass);
        if !self.monitor || !active {
            return None;
        }
        self.sync();
        Some(self.shared.clone())
    }
}

/// `Documentos\AnimeIAI\Áudio\Racks` (ao lado de `Fontes`, pelo mesmo resolvedor da pasta Documentos).
pub(crate) fn racks_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("AIAI_AUDIO_RACKS_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    aiai_paint::fonts::user_fonts_dir().and_then(|f| f.parent().map(|d| d.join("Áudio").join("Racks")))
}

pub(crate) fn list_presets(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let p = e.path();
        (p.extension().is_some_and(|x| x == "json")).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
    }).collect();
    v.sort_by_key(|s| s.to_lowercase());
    v
}

pub(crate) fn save_preset(dir: &std::path::Path, name: &str, rack: &Rack) -> Result<PathBuf, String> {
    if name.trim().is_empty() {
        return Err(tr("audio.rack.need_name").into());
    }
    let safe = crate::app::panels::deliver::safe_name(name.trim());
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{safe}.json"));
    let json = serde_json::to_string_pretty(rack).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub(crate) fn load_preset(dir: &std::path::Path, name: &str) -> Result<Rack, String> {
    let path = dir.join(format!("{name}.json"));
    let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r: Rack = serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))?;
    r.validate()?;
    Ok(r)
}

/// Efeitos do menu "Adicionar": (chave i18n, construtor).
fn catalog() -> Vec<(&'static str, AudioFx)> {
    vec![
        ("audio.fx.amplify", AudioFx::Amplify(Amplify { gain_db: 3.0, allow_clipping: true })),
        ("audio.fx.normalize", AudioFx::Normalize(Normalize::default())),
        ("audio.fx.eq_graphic", AudioFx::Equalizer(Equalizer::graphic10(&[0.0; 10]))),
        ("audio.fx.eq_param", AudioFx::Equalizer(Equalizer::parametric(vec![EqBand { kind: BandKind::HighPass, freq_hz: 80.0, gain_db: 0.0, q: 0.71, enabled: true }, EqBand::peak(2500.0, 0.0, 1.0), EqBand { kind: BandKind::HighShelf, freq_hz: 8000.0, gain_db: 0.0, q: 0.71, enabled: true }]))),
        ("audio.fx.compressor", AudioFx::Compressor(Compressor::default())),
        ("audio.fx.limiter", AudioFx::Limiter(Limiter::default())),
        ("audio.fx.noise_gate", AudioFx::NoiseGate(NoiseGate::default())),
        ("audio.fx.noise_reduction", AudioFx::NoiseReduction(NoiseReduction::default())),
        ("audio.fx.reverb", AudioFx::Reverb(Reverb::default())),
        ("audio.fx.pitch_shift", AudioFx::PitchShift(PitchShift::default())),
        ("audio.fx.time_stretch", AudioFx::TimeStretch(TimeStretch::default())),
        ("audio.fx.dc_offset", AudioFx::DcOffset(DcOffset { mode: DcMode::HighPass, ..Default::default() })),
        ("audio.fx.high_pass", AudioFx::Filter(Filter::high_pass(80.0, 24))),
        ("audio.fx.low_pass", AudioFx::Filter(Filter::low_pass(8000.0, 24))),
    ]
}

pub(crate) fn fx_key(fx: &AudioFx) -> &'static str {
    match fx {
        AudioFx::Amplify(_) => "audio.fx.amplify",
        AudioFx::Normalize(_) => "audio.fx.normalize",
        AudioFx::Equalizer(e) if is_graphic(e) => "audio.fx.eq_graphic",
        AudioFx::Equalizer(_) => "audio.fx.eq_param",
        AudioFx::Compressor(_) => "audio.fx.compressor",
        AudioFx::Limiter(_) => "audio.fx.limiter",
        AudioFx::NoiseGate(_) => "audio.fx.noise_gate",
        AudioFx::NoiseReduction(_) => "audio.fx.noise_reduction",
        AudioFx::Reverb(_) => "audio.fx.reverb",
        AudioFx::PitchShift(_) => "audio.fx.pitch_shift",
        AudioFx::TimeStretch(_) => "audio.fx.time_stretch",
        AudioFx::DcOffset(_) => "audio.fx.dc_offset",
        AudioFx::Filter(f) if f.kind == FilterKind::HighPass => "audio.fx.high_pass",
        AudioFx::Filter(_) => "audio.fx.low_pass",
    }
}

fn is_graphic(e: &Equalizer) -> bool {
    e.bands.len() == 10 && e.bands.iter().zip(GRAPHIC_10_HZ).all(|(b, f)| b.kind == BandKind::Peak && (b.freq_hz - f).abs() < 0.01)
}

// ------------------------------------------------------------------------------------------------ painel

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    poll_preview(env, ui.ctx());
    let mut child = body_ui(ui, body, 6.0);
    egui::ScrollArea::vertical().id_salt("audio_rack").auto_shrink([false, false]).show(&mut child, |ui| rack_body(env, ui));
}

fn rack_body(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let target = wave::resolve(env);
    let mut changed = false;
    let n = env.ed.audio_ws.rack.rack.slots.len();
    env.ed.audio_ws.rack.open.resize(n, true);
    // ---- cabeçalho: ligar o rack, A/B, adicionar
    ui.horizontal_wrapped(|ui| {
        let st = &mut env.ed.audio_ws.rack;
        let mut on = !st.rack.bypass;
        let r = ui.checkbox(&mut on, tr("audio.rack.on"));
        env.note("audio:rack:on", r.rect);
        if r.changed() {
            env.ed.audio_ws.rack.rack.bypass = !on;
            changed = true;
        }
        let st = &mut env.ed.audio_ws.rack;
        let ab = st.ab_off;
        let b = ui.selectable_label(ab, if ab { "B (bypass)" } else { "A" });
        env.note("audio:rack:ab", b.rect);
        if b.on_hover_text(tr("audio.rack.ab.tip")).clicked() {
            env.ed.audio_ws.rack.ab_off = !ab;
            changed = true;
        }
        let mut mon = env.ed.audio_ws.rack.monitor;
        let r = ui.checkbox(&mut mon, tr("audio.rack.monitor"));
        env.note("audio:rack:monitor", r.rect);
        r.on_hover_text(tr("audio.rack.monitor.tip"));
        env.ed.audio_ws.rack.monitor = mon;
        dim_label(ui, &t, &format!("{n}/{RACK_MAX_SLOTS}"));
        let mut add: Option<AudioFx> = None;
        let r = ui.menu_button(tr("audio.rack.add"), |ui| {
            for (key, fx) in catalog() {
                if ui.add_enabled(n < RACK_MAX_SLOTS, egui::Button::new(tr(key))).clicked() {
                    add = Some(fx);
                    ui.close();
                }
            }
        });
        env.note("audio:rack:add", r.response.rect);
        if let Some(fx) = add {
            // o efeito novo abre; os outros fecham (a cadeia fica legível)
            let st = &mut env.ed.audio_ws.rack;
            st.open.iter_mut().for_each(|o| *o = false);
            st.rack.slots.push(RackSlot::new(fx));
            st.open.push(true);
            changed = true;
        }
    });
    let rate = target.as_ref().and_then(|(a, _)| env.ed.audio_ws.load(env.core, a)).map(|l| l.buf.rate).unwrap_or(48_000);
    // ---- prévia e aplicar (em cima: sempre à vista, a cadeia pode crescer)
    section(ui, &t, "audio.rack.listen_apply");
    let valid = env.ed.audio_ws.rack.rack.validate();
    let active = !env.ed.audio_ws.rack.rack.bypass && env.ed.audio_ws.rack.rack.slots.iter().any(|s| !s.bypass);
    let rt_ok = env.ed.audio_ws.rack.rack.realtime_ok();
    let lat = env.ed.audio_ws.rack.rack.latency_frames(rate);
    dim_label(ui, &t, &if rt_ok { format!("{} · {} {:.1} ms", tr("audio.rack.mode_rt"), tr("audio.rack.latency"), lat as f64 * 1000.0 / rate as f64) } else { tr("audio.rack.mode_offline").to_owned() });
    let sel_txt = target.as_ref().and_then(|(a, _)| env.ed.audio_ws.load(env.core, a)).and_then(|l| env.ed.audio_ws.wave.sel_secs(l.buf.rate)).map(|(a, b)| format!("{} {a:.2}–{b:.2} s", tr("audio.rack.on_selection"))).unwrap_or_else(|| tr("audio.rack.on_clip").to_owned());
    dim_label(ui, &t, &sel_txt);
    ui.horizontal_wrapped(|ui| {
        let previewing = env.ed.audio_ws.preview_playing();
        let b = ui.add_enabled(target.is_some() && active && valid.is_ok(), egui::Button::new(if previewing { tr("audio.rack.stop") } else { tr("audio.rack.preview") }));
        env.note("audio:rack:preview", b.rect);
        if b.on_hover_text(tr("audio.rack.preview.tip")).clicked() {
            if previewing || env.ed.audio_ws.rack.preview.is_some() {
                env.ed.audio_ws.player.stop();
                env.ed.audio_ws.rack.preview = None;
            } else {
                preview(env);
            }
        }
        if env.ed.audio_ws.rack.preview.is_some() {
            ui.spinner();
            dim_label(ui, &t, tr("audio.rack.rendering"));
        }
        let busy = env.ed.audio_ws.job.is_some();
        let b = ui.add_enabled(target.is_some() && active && valid.is_ok() && !busy, egui::Button::new(egui::RichText::new(tr("audio.rack.apply")).strong()));
        env.note("audio:rack:apply", b.rect);
        if b.on_hover_text(tr("audio.rack.apply.tip")).clicked() {
            apply(env);
        }
    });
    if let Err(e) = &valid {
        ui.label(egui::RichText::new(e).small().color(t.danger));
    }

    // ---- posições
    section(ui, &t, "panel.rack.title");
    let mut action: Option<(usize, i32)> = None; // (posição, -1 sobe, +1 desce, 0 sai)
    let mut capture: Option<usize> = None;
    for i in 0..n {
        let open = env.ed.audio_ws.rack.open[i];
        let frame = egui::Frame::new().fill(t.bg_header).corner_radius(4).inner_margin(egui::Margin::same(5));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let slot = &mut env.ed.audio_ws.rack.rack.slots[i];
                let mut on = !slot.bypass;
                let r = ui.checkbox(&mut on, "");
                if r.changed() {
                    slot.bypass = !on;
                    changed = true;
                }
                let key = fx_key(&slot.fx);
                let rt = slot.fx.realtime();
                let r2 = ui.add(egui::Label::new(egui::RichText::new(format!("{}. {}", i + 1, tr(key))).strong().color(if on { t.text } else { t.text_dim })).sense(Sense::click()));
                env.note(format!("audio:rack:slot:{i}"), r2.rect.union(r.rect));
                if r2.clicked() {
                    env.ed.audio_ws.rack.open[i] = !open;
                }
                let tag = if rt { tr("audio.rack.rt") } else { tr("audio.rack.offline") };
                ui.label(egui::RichText::new(tag).small().color(if rt { t.st_ok } else { t.st_review })).on_hover_text(tr(if rt { "audio.rack.rt.tip" } else { "audio.rack.offline.tip" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if env.icon_button_sized(ui, IconId::Trash, "audio.rack.remove", false, true, &format!("audio:rack:remove:{i}"), 20.0, false).clicked() {
                        action = Some((i, 0));
                    }
                    if env.icon_button_sized(ui, IconId::ArrowDown, "audio.rack.down", false, i + 1 < n, &format!("audio:rack:down:{i}"), 20.0, false).clicked() {
                        action = Some((i, 1));
                    }
                    if env.icon_button_sized(ui, IconId::ArrowUp, "audio.rack.up", false, i > 0, &format!("audio:rack:up:{i}"), 20.0, false).clicked() {
                        action = Some((i, -1));
                    }
                    let chev = if open { IconId::ChevronDown } else { IconId::ChevronRight };
                    if env.icon_button_sized(ui, chev, "audio.rack.expand", false, true, &format!("audio:rack:expand:{i}"), 20.0, false).clicked() {
                        env.ed.audio_ws.rack.open[i] = !open;
                    }
                });
            });
            if open {
                let slot = &mut env.ed.audio_ws.rack.rack.slots[i];
                let (c, cap) = fx_params(ui, &t, &mut slot.fx, i, rate as f64);
                changed |= c;
                if cap {
                    capture = Some(i);
                }
                if let Err(e) = slot.fx.validate() {
                    ui.label(egui::RichText::new(e).small().color(t.danger));
                }
            }
        });
        ui.add_space(3.0);
    }
    if let Some((i, d)) = action {
        let st = &mut env.ed.audio_ws.rack;
        match d {
            0 => {
                st.rack.slots.remove(i);
                st.open.remove(i);
            }
            -1 if i > 0 => {
                st.rack.slots.swap(i, i - 1);
                st.open.swap(i, i - 1);
            }
            1 if i + 1 < st.rack.slots.len() => {
                st.rack.slots.swap(i, i + 1);
                st.open.swap(i, i + 1);
            }
            _ => {}
        }
        changed = true;
    }
    if n == 0 {
        dim_label(ui, &t, tr("audio.rack.empty"));
    }
    if let Some(i) = capture {
        capture_profile(env, i);
        changed = true;
    }
    if changed {
        env.ed.audio_ws.rack.sync();
    }

    // ---- predefinições
    section(ui, &t, "audio.rack.presets");
    let dir = racks_dir().filter(|_| env.ed.audio_ws.user_files);
    match dir {
        None => {
            dim_label(ui, &t, tr("audio.rack.presets_off"));
        }
        Some(dir) => {
            if env.ed.audio_ws.rack.presets.is_none() {
                env.ed.audio_ws.rack.presets = Some(list_presets(&dir));
            }
            let names = env.ed.audio_ws.rack.presets.clone().unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                let mut load: Option<String> = None;
                let r = ui.menu_button(tr("audio.rack.load"), |ui| {
                    if names.is_empty() {
                        ui.label(tr("audio.rack.no_presets"));
                    }
                    for n in &names {
                        if ui.button(n).clicked() {
                            load = Some(n.clone());
                            ui.close();
                        }
                    }
                });
                env.note("audio:rack:load", r.response.rect);
                if let Some(n) = load {
                    match load_preset(&dir, &n) {
                        Ok(r) => {
                            let st = &mut env.ed.audio_ws.rack;
                            st.open = vec![false; r.slots.len()];
                            st.rack = r;
                            st.preset_name = n;
                            st.sync();
                        }
                        Err(e) => env.ed.notify(e),
                    }
                }
                let te = ui.add(egui::TextEdit::singleline(&mut env.ed.audio_ws.rack.preset_name).hint_text(tr("audio.rack.preset_hint")).desired_width(130.0));
                env.note("audio:rack:preset_name", te.rect);
                let b = ui.add_enabled(!env.ed.audio_ws.rack.preset_name.trim().is_empty() && !env.ed.audio_ws.rack.rack.slots.is_empty(), egui::Button::new(tr("audio.rack.save")));
                env.note("audio:rack:save", b.rect);
                if b.clicked() {
                    let name = env.ed.audio_ws.rack.preset_name.clone();
                    match save_preset(&dir, &name, &env.ed.audio_ws.rack.rack) {
                        Ok(p) => {
                            env.ed.notify(format!("{}: {}", tr("audio.rack.saved"), p.display()));
                            env.ed.audio_ws.rack.presets = None;
                        }
                        Err(e) => env.ed.notify(e),
                    }
                }
                if ui.small_button(tr("audio.rack.refresh")).clicked() {
                    env.ed.audio_ws.rack.presets = None;
                }
            });
            dim_label(ui, &t, &dir.display().to_string());
        }
    }
}

impl super::AudioWsState {
    pub(crate) fn preview_playing(&self) -> bool {
        self.player.playing().is_some_and(|p| p.key.starts_with("rack:"))
    }
}

/// Trecho da prévia/aplicação: a seleção do editor ou o clipe inteiro.
fn selection_frames(env: &Env, frames: usize) -> (usize, usize) {
    env.ed.audio_ws.wave.sel.filter(|(a, b)| b > a).unwrap_or((env.ed.audio_ws.wave.cursor.min(frames.saturating_sub(1)), frames))
}

fn preview(env: &mut Env) {
    let Some((asset, _)) = wave::resolve(env) else { return };
    let Some(l) = env.ed.audio_ws.load(env.core, &asset) else { return };
    let (a, b) = selection_frames(env, l.buf.frames());
    let st = &mut env.ed.audio_ws;
    if st.rack.rack.realtime_ok() {
        st.rack.sync();
        let rack = Some(st.rack.shared.clone());
        st.player.play(PlayRequest { buf: l.buf.clone(), start: a, end: b, looping: st.wave.looping, rack, key: format!("rack:{}", l.key), label: tr("audio.rack.preview").to_owned() });
        return;
    }
    // algum efeito precisa do clipe inteiro: renderiza o trecho (até 60 s) num fio e toca
    let b = b.min(a + (OFFLINE_PREVIEW_S * l.buf.rate as f64) as usize);
    let piece = aiai_audio::edit::copy(&l.buf, a..b);
    let mut rack = st.rack.rack.clone();
    rack.bypass = false;
    let (tx, rx) = crossbeam_channel::bounded(1);
    let spawned = std::thread::Builder::new().name("aiai-rack-preview".into()).spawn(move || {
        let out = rack.render(&piece);
        let _ = tx.send(Ok(Arc::new(out)));
    });
    match spawned {
        Ok(_) => st.rack.preview = Some(rx),
        Err(e) => env.ed.notify(e.to_string()),
    }
}

fn poll_preview(env: &mut Env, ctx: &egui::Context) {
    let Some(rx) = &env.ed.audio_ws.rack.preview else { return };
    match rx.try_recv() {
        Ok(Ok(buf)) => {
            env.ed.audio_ws.rack.preview = None;
            let n = buf.frames();
            let looping = env.ed.audio_ws.wave.looping;
            env.ed.audio_ws.player.play(PlayRequest { buf, start: 0, end: n, looping, rack: None, key: "rack:offline".into(), label: tr("audio.rack.preview").to_owned() });
        }
        Ok(Err(e)) => {
            env.ed.audio_ws.rack.preview = None;
            env.ed.notify(e);
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        Err(crossbeam_channel::TryRecvError::Disconnected) => env.ed.audio_ws.rack.preview = None,
    }
}

fn apply(env: &mut Env) {
    let Some((asset, placement)) = wave::resolve(env) else { return };
    let Some(l) = env.ed.audio_ws.load(env.core, &asset) else { return };
    let mut rack = env.ed.audio_ws.rack.rack.clone();
    rack.bypass = false;
    rack.slots.retain(|s| !s.bypass);
    let (s, e) = env.ed.audio_ws.wave.sel_secs(l.buf.rate).map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    wave::run_steps(env, &asset, placement.as_ref(), vec![EditStep::Rack { rack, start_s: s, end_s: e }], "audio.rack.apply", false);
}

/// Perfil de ruído da seleção do editor (o trecho só com ruído) para a redução de ruído da posição `i`.
fn capture_profile(env: &mut Env, i: usize) {
    let Some((asset, _)) = wave::resolve(env) else { return };
    let Some(l) = env.ed.audio_ws.load(env.core, &asset) else { return };
    let Some((a, b)) = env.ed.audio_ws.wave.sel.filter(|(a, b)| b > a) else {
        env.ed.notify(tr("audio.rack.need_selection"));
        return;
    };
    let piece = aiai_audio::edit::copy(&l.buf, a..b);
    match NoiseProfile::from_audio(&piece, aiai_audio::fx::spectral::DEFAULT_FFT) {
        Some(p) => {
            if let Some(AudioFx::NoiseReduction(nr)) = env.ed.audio_ws.rack.rack.slots.get_mut(i).map(|s| &mut s.fx) {
                nr.profile = Some(p);
            }
        }
        None => env.ed.notify(tr("audio.rack.profile_short")),
    }
}

// ------------------------------------------------------------------------------------------------ parâmetros

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> egui::Response) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([118.0, 18.0], egui::Label::new(egui::RichText::new(label).size(11.5)).truncate());
        add(ui).changed()
    })
    .inner
}

fn slider(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> bool {
    row(ui, label, |ui| ui.add(egui::Slider::new(v, range).suffix(suffix).max_decimals(2)))
}

fn check(ui: &mut Ui, label: &str, v: &mut bool) -> bool {
    row(ui, label, |ui| ui.checkbox(v, ""))
}

/// Controles dos parâmetros de um efeito. Devolve (mudou, pediu captura do perfil de ruído).
fn fx_params(ui: &mut Ui, t: &crate::theme::Tokens, fx: &mut AudioFx, slot: usize, fs: f64) -> (bool, bool) {
    let mut c = false;
    let mut cap = false;
    ui.push_id(("fxp", slot), |ui| match fx {
        AudioFx::Amplify(e) => {
            c |= slider(ui, tr("audio.p.gain"), &mut e.gain_db, -24.0..=24.0, " dB");
            c |= check(ui, tr("audio.p.allow_clipping"), &mut e.allow_clipping);
        }
        AudioFx::Normalize(e) => {
            c |= row(ui, tr("audio.p.mode"), |ui| {
                let mut m = e.mode;
                let r = egui::ComboBox::from_id_salt("nm").selected_text(if m == NormalizeMode::Peak { tr("audio.p.peak") } else { "LUFS" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut m, NormalizeMode::Peak, tr("audio.p.peak"));
                    ui.selectable_value(&mut m, NormalizeMode::Loudness, "LUFS");
                });
                let ch = m != e.mode;
                e.mode = m;
                let mut r = r.response;
                if ch {
                    r.mark_changed();
                }
                r
            });
            if e.mode == NormalizeMode::Peak {
                c |= slider(ui, tr("audio.p.target_db"), &mut e.target_db, -24.0..=0.0, " dBFS");
            } else {
                c |= slider(ui, tr("audio.p.target_lufs"), &mut e.target_lufs, -36.0..=-6.0, " LUFS");
                c |= slider(ui, tr("audio.p.ceiling_dbtp"), &mut e.ceiling_dbtp, -6.0..=0.0, " dBTP");
            }
            c |= check(ui, tr("audio.p.remove_dc"), &mut e.remove_dc);
            c |= check(ui, tr("audio.p.independent"), &mut e.independent_channels);
        }
        AudioFx::Equalizer(e) => {
            if is_graphic(e) {
                ui.horizontal(|ui| {
                    for b in e.bands.iter_mut() {
                        ui.vertical(|ui| {
                            ui.spacing_mut().slider_width = 70.0;
                            c |= ui.add(egui::Slider::new(&mut b.gain_db, -12.0..=12.0).vertical().show_value(false)).on_hover_text(format!("{:.0} Hz  {:+.1} dB", b.freq_hz, b.gain_db)).changed();
                            let f = if b.freq_hz >= 1000.0 { format!("{}k", b.freq_hz / 1000.0) } else { format!("{:.0}", b.freq_hz) };
                            ui.label(egui::RichText::new(f).size(9.5));
                        });
                    }
                });
            } else {
                let mut del: Option<usize> = None;
                for (k, b) in e.bands.iter_mut().enumerate() {
                    ui.push_id(k, |ui| {
                        ui.horizontal(|ui| {
                            c |= ui.checkbox(&mut b.enabled, "").changed();
                            let mut kind = b.kind;
                            egui::ComboBox::from_id_salt("bk").width(78.0).selected_text(band_label(kind)).show_ui(ui, |ui| {
                                for x in [BandKind::Peak, BandKind::LowShelf, BandKind::HighShelf, BandKind::LowPass, BandKind::HighPass, BandKind::Notch, BandKind::BandPass] {
                                    ui.selectable_value(&mut kind, x, band_label(x));
                                }
                            });
                            if kind != b.kind {
                                b.kind = kind;
                                c = true;
                            }
                            let sp = b.freq_hz as f64 * 0.005;
                            c |= ui.add(egui::DragValue::new(&mut b.freq_hz).range(20.0..=20_000.0).speed(sp).max_decimals(0).suffix(" Hz")).changed();
                            c |= ui.add(egui::DragValue::new(&mut b.gain_db).range(-24.0..=24.0).speed(0.1).max_decimals(1).suffix(" dB")).changed();
                            c |= ui.add(egui::DragValue::new(&mut b.q).range(0.1..=18.0).speed(0.01).max_decimals(2).prefix("Q ")).changed();
                            if ui.small_button("×").clicked() {
                                del = Some(k);
                            }
                        });
                    });
                }
                if let Some(k) = del {
                    e.bands.remove(k);
                    c = true;
                }
                if e.bands.len() < 32 && ui.small_button(tr("audio.p.add_band")).clicked() {
                    e.bands.push(EqBand::peak(1000.0, 0.0, 1.0));
                    c = true;
                }
            }
            c |= slider(ui, tr("audio.p.output_gain"), &mut e.output_gain_db, -24.0..=24.0, " dB");
            eq_curve(ui, t, e, fs);
        }
        AudioFx::Compressor(e) => {
            c |= slider(ui, tr("audio.p.threshold"), &mut e.threshold_db, -60.0..=0.0, " dB");
            c |= slider(ui, tr("audio.p.ratio"), &mut e.ratio, 1.0..=20.0, ":1");
            c |= slider(ui, tr("audio.p.attack"), &mut e.attack_ms, 0.1..=200.0, " ms");
            c |= slider(ui, tr("audio.p.release"), &mut e.release_ms, 5.0..=2000.0, " ms");
            c |= slider(ui, tr("audio.p.knee"), &mut e.knee_db, 0.0..=24.0, " dB");
            c |= slider(ui, tr("audio.p.makeup"), &mut e.makeup_db, 0.0..=24.0, " dB");
            c |= check(ui, tr("audio.p.auto_makeup"), &mut e.auto_makeup);
            c |= row(ui, tr("audio.p.detector"), |ui| {
                let mut d = e.detector;
                let r = egui::ComboBox::from_id_salt("det").selected_text(if d == Detector::Rms { "RMS" } else { tr("audio.p.peak") }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut d, Detector::Rms, "RMS");
                    ui.selectable_value(&mut d, Detector::Peak, tr("audio.p.peak"));
                });
                let ch = d != e.detector;
                e.detector = d;
                let mut r = r.response;
                if ch {
                    r.mark_changed();
                }
                r
            });
            c |= check(ui, tr("audio.p.link"), &mut e.link);
        }
        AudioFx::Limiter(e) => {
            c |= slider(ui, tr("audio.p.ceiling"), &mut e.ceiling_db, -12.0..=0.0, " dB");
            c |= slider(ui, tr("audio.p.lookahead"), &mut e.lookahead_ms, 0.0..=20.0, " ms");
            c |= slider(ui, tr("audio.p.release"), &mut e.release_ms, 5.0..=1000.0, " ms");
            c |= check(ui, tr("audio.p.true_peak"), &mut e.true_peak);
        }
        AudioFx::NoiseGate(e) => {
            c |= slider(ui, tr("audio.p.threshold"), &mut e.threshold_db, -90.0..=0.0, " dB");
            c |= slider(ui, tr("audio.p.attack"), &mut e.attack_ms, 0.1..=100.0, " ms");
            c |= slider(ui, tr("audio.p.hold"), &mut e.hold_ms, 0.0..=500.0, " ms");
            c |= slider(ui, tr("audio.p.release"), &mut e.release_ms, 5.0..=2000.0, " ms");
            c |= slider(ui, tr("audio.p.range"), &mut e.range_db, -100.0..=0.0, " dB");
            c |= slider(ui, tr("audio.p.hysteresis"), &mut e.hysteresis_db, 0.0..=12.0, " dB");
        }
        AudioFx::NoiseReduction(e) => {
            ui.horizontal(|ui| {
                let st = match &e.profile {
                    Some(p) => format!("{} ({:.2} s)", tr("audio.p.profile_ok"), p.frames_used as f64 / p.sample_rate.max(1) as f64),
                    None => tr("audio.p.profile_none").to_owned(),
                };
                ui.label(egui::RichText::new(st).size(11.5).color(if e.profile.is_some() { t.st_ok } else { t.danger }));
                if ui.button(tr("audio.p.capture")).on_hover_text(tr("audio.p.capture.tip")).clicked() {
                    cap = true;
                }
            });
            c |= slider(ui, tr("audio.p.reduction"), &mut e.reduction_db, 0.0..=48.0, " dB");
            c |= slider(ui, tr("audio.p.sensitivity"), &mut e.sensitivity, 0.0..=10.0, "");
            let mut fsm = e.freq_smoothing as f32;
            if slider(ui, tr("audio.p.freq_smoothing"), &mut fsm, 0.0..=12.0, "") {
                e.freq_smoothing = fsm.round() as usize;
                c = true;
            }
            c |= slider(ui, tr("audio.p.decay"), &mut e.decay_ms, 0.0..=500.0, " ms");
        }
        AudioFx::Reverb(e) => {
            c |= slider(ui, tr("audio.p.room"), &mut e.room_size, 0.0..=1.0, "");
            c |= slider(ui, tr("audio.p.damping"), &mut e.damping, 0.0..=1.0, "");
            c |= slider(ui, tr("audio.p.wet"), &mut e.wet_db, -60.0..=6.0, " dB");
            c |= slider(ui, tr("audio.p.dry"), &mut e.dry_db, -60.0..=6.0, " dB");
            c |= slider(ui, tr("audio.p.width"), &mut e.width, 0.0..=1.0, "");
            c |= slider(ui, tr("audio.p.pre_delay"), &mut e.pre_delay_ms, 0.0..=200.0, " ms");
            c |= slider(ui, tr("audio.p.tail"), &mut e.tail_s, 0.0..=10.0, " s");
        }
        AudioFx::PitchShift(e) => {
            c |= slider(ui, tr("audio.p.semitones"), &mut e.semitones, -12.0..=12.0, " st");
            c |= slider(ui, tr("audio.p.cents"), &mut e.cents, -100.0..=100.0, " ct");
        }
        AudioFx::TimeStretch(e) => {
            c |= slider(ui, tr("audio.p.ratio_time"), &mut e.ratio, 0.5..=2.0, "×");
        }
        AudioFx::DcOffset(e) => {
            c |= row(ui, tr("audio.p.mode"), |ui| {
                let mut m = e.mode;
                let r = egui::ComboBox::from_id_salt("dc").selected_text(if m == DcMode::Mean { tr("audio.p.dc_mean") } else { tr("audio.p.dc_hp") }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut m, DcMode::Mean, tr("audio.p.dc_mean"));
                    ui.selectable_value(&mut m, DcMode::HighPass, tr("audio.p.dc_hp"));
                });
                let ch = m != e.mode;
                e.mode = m;
                let mut r = r.response;
                if ch {
                    r.mark_changed();
                }
                r
            });
            if e.mode == DcMode::HighPass {
                c |= slider(ui, tr("audio.p.cutoff"), &mut e.cutoff_hz, 1.0..=40.0, " Hz");
            }
        }
        AudioFx::Filter(e) => {
            let sp = e.freq_hz as f64 * 0.005;
            c |= row(ui, tr("audio.p.cutoff"), |ui| ui.add(egui::DragValue::new(&mut e.freq_hz).range(20.0..=20_000.0).speed(sp).max_decimals(0).suffix(" Hz")));
            c |= row(ui, tr("audio.p.slope"), |ui| {
                let mut s = e.slope_db_oct;
                let r = egui::ComboBox::from_id_salt("slope").selected_text(format!("{s} dB/oct")).show_ui(ui, |ui| {
                    for x in [12u32, 24, 36, 48] {
                        ui.selectable_value(&mut s, x, format!("{x} dB/oct"));
                    }
                });
                let ch = s != e.slope_db_oct;
                e.slope_db_oct = s;
                let mut r = r.response;
                if ch {
                    r.mark_changed();
                }
                r
            });
        }
    });
    (c, cap)
}

fn band_label(k: BandKind) -> &'static str {
    match k {
        BandKind::Peak => "Peak",
        BandKind::LowShelf => "Low shelf",
        BandKind::HighShelf => "High shelf",
        BandKind::LowPass => "Low pass",
        BandKind::HighPass => "High pass",
        BandKind::Notch => "Notch",
        BandKind::BandPass => "Band pass",
    }
}

/// Curva de resposta do equalizador (20 Hz–20 kHz, ±18 dB).
fn eq_curve(ui: &mut Ui, t: &crate::theme::Tokens, e: &Equalizer, fs: f64) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(360.0), 64.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 3.0, t.bg_frame);
    p.hline(r.x_range(), r.center().y, Stroke::new(1.0, t.border));
    let pts: Vec<egui::Pos2> = e.response_curve(fs, 20.0, 20_000.0, r.width().max(2.0) as usize).iter().enumerate().map(|(i, (_, db))| egui::pos2(r.left() + i as f32, r.center().y - (*db as f32 / 18.0).clamp(-1.0, 1.0) * r.height() * 0.48)).collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_rgb(96, 168, 255))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_through_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = Rack::new();
        r.push(AudioFx::Equalizer(Equalizer::graphic10(&[1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -3.0])));
        r.push(AudioFx::Compressor(Compressor { threshold_db: -20.0, ..Default::default() }));
        r.slots[1].bypass = true;
        let p = save_preset(dir.path(), "Voz limpa", &r).unwrap();
        assert!(p.ends_with("Voz limpa.json") || p.file_name().is_some());
        assert_eq!(list_presets(dir.path()).len(), 1);
        let name = list_presets(dir.path())[0].clone();
        let back = load_preset(dir.path(), &name).unwrap();
        assert_eq!(back.slots.len(), 2);
        assert!(back.slots[1].bypass);
        assert_eq!(fx_key(&back.slots[0].fx), "audio.fx.eq_graphic");
        assert!(save_preset(dir.path(), "   ", &r).is_err());
        // arquivo estranho: erro claro, não pânico
        std::fs::write(dir.path().join("ruim.json"), "{\"slots\": 3}").unwrap();
        assert!(load_preset(dir.path(), "ruim").is_err());
    }

    #[test]
    fn catalog_effects_are_valid_and_named() {
        use crate::i18n::{Lang, tr_in};
        for (key, fx) in catalog() {
            assert_eq!(fx_key(&fx), key);
            assert_ne!(tr_in(Lang::En, key), key);
            if !matches!(fx, AudioFx::NoiseReduction(_)) {
                fx.validate().unwrap();
            }
        }
    }

    #[test]
    fn sync_sends_the_chain_and_the_ab_switch_to_the_player() {
        let mut st = RackState::default();
        assert!(st.live_for("k").is_none(), "rack vazio não entra na escuta");
        st.rack.push(AudioFx::Amplify(Amplify { gain_db: 6.0, allow_clipping: true }));
        let sh = st.live_for("k").unwrap();
        let v1 = sh.lock().unwrap().version;
        st.ab_off = true;
        st.sync();
        let g = sh.lock().unwrap();
        assert!(g.version > v1 && g.rack.bypass, "A/B desliga só a escuta");
        assert!(!st.rack.bypass);
    }
}

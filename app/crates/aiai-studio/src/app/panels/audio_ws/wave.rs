//! Painel Forma de onda (S24 3.8): editor de UM clipe (o selecionado no corte ou qualquer asset de áudio), como o editor
//! de forma de onda do Audition/Audacity: visão geral do arquivo inteiro com a janela visível, régua de tempo, forma de
//! onda por canal desenhada pela pirâmide (`WaveformCache`: custo por pixel, não por amostra — arquivos longos continuam
//! leves) ou espectrograma, seleção por arrasto, cursor, cabeçote da reprodução, zoom/rolagem.
//!
//! Barra de ferramentas: tocar seleção (laço), recortar/copiar/colar/apagar/silenciar/aparar, fade in/out (curva),
//! ganho e inverter no tempo. Cada edição vira passos `EditStep` aplicados por `audio_edit` num asset NOVO; o clipe do
//! corte passa a apontar para ele (desfazer volta). O histórico mostra a cadeia de versões (`params.edited_from`) com os
//! passos de cada uma e volta a qualquer versão.
//!
//! Teclas (com o ponteiro sobre o painel): Espaço toca/para, Delete apaga, Ctrl+X/C/V, Ctrl+L silencia, Ctrl+T apara,
//! Ctrl+A seleciona tudo, Home/End, +/- zoom, Ctrl+E zoom na seleção, Esc limpa a seleção.

use super::{After, Loaded, asset_params, audio_assets, cut_audio, fmt_time, node_name};
use crate::app::actions;
use crate::app::commands::NodeId;
use crate::app::editor::Editor;
use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_audio::AudioBuf;
use aiai_audio::edit::{FadeCurve, FadeDir};
use aiai_audio::steps::EditStep;
use aiai_core::{AssetType, Command, Id, NodeKind};
use egui::{Color32, ColorImage, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui};
use serde_json::{Value, json};
use std::sync::Arc;

/// O que o editor mostra.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// Clipe colocado num corte (a edição redireciona o clipe para o asset novo).
    Placement { cut: NodeId, id: Id },
    /// Asset solto (a edição abre o asset novo).
    Asset(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum View {
    #[default]
    Wave,
    Spectrum,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaveKey {
    Delete,
    Silence,
    Trim,
    Copy,
    Cut,
    Paste,
    SelectAll,
    PlayStop,
    Home,
    End,
    ZoomIn,
    ZoomOut,
    ZoomSel,
    Escape,
}

/// Área de transferência: o trecho copiado (e de onde veio, para colar no mesmo arquivo como `duplicate`).
pub(crate) struct Clipboard {
    pub key: String,
    pub asset: Id,
    pub start: usize,
    pub end: usize,
    pub buf: Arc<AudioBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Select(usize),
    Overview,
}

#[derive(Default)]
struct Spec {
    tex: Option<TextureHandle>,
    /// (chave do blob, início, fim, largura, altura) do que a textura mostra / do pedido em andamento.
    shown: Option<(String, usize, usize, usize, usize)>,
    pending: Option<(String, usize, usize, usize, usize)>,
    rx: Option<crossbeam_channel::Receiver<((String, usize, usize, usize, usize), ColorImage)>>,
}

pub(crate) struct WaveState {
    pub target: Option<Target>,
    seen_sel: Option<Id>,
    /// Primeiro quadro visível e quadros por pixel (0 = ajustar ao painel).
    pub view_start: f64,
    pub fpp: f64,
    pub amp: f32,
    pub sel: Option<(usize, usize)>,
    pub cursor: usize,
    drag: Option<Drag>,
    pub view: View,
    pub gain_db: f32,
    pub curve: FadeCurve,
    pub snap_zero: bool,
    pub looping: bool,
    pub clip: Option<Clipboard>,
    spec: Spec,
    /// Teclas recolhidas antes dos atalhos globais (o ponteiro estava sobre o painel).
    pub keys: Vec<WaveKey>,
    pub rect: Option<Rect>,
    loaded_key: Option<String>,
    lane_w: f32,
}

impl Default for WaveState {
    fn default() -> Self {
        WaveState { target: None, seen_sel: None, view_start: 0.0, fpp: 0.0, amp: 1.0, sel: None, cursor: 0, drag: None, view: View::Wave, gain_db: 3.0, curve: FadeCurve::Linear, snap_zero: false, looping: false, clip: None, spec: Spec::default(), keys: Vec::new(), rect: None, loaded_key: None, lane_w: 600.0 }
    }
}

impl WaveState {
    /// Solta o espectrograma (orçamento de VRAM, `gpu_owners`): é refeito quando a vista volta.
    pub(crate) fn release_gpu(&mut self) -> bool {
        self.spec.shown = None;
        self.spec.tex.take().is_some()
    }

    pub(crate) fn open_asset(&mut self, id: Id) {
        self.target = Some(Target::Asset(id));
        self.sel = None;
    }

    /// Seleção em segundos (para o rack e a redução de ruído).
    pub(crate) fn sel_secs(&self, rate: u32) -> Option<(f64, f64)> {
        self.sel.map(|(a, b)| (a as f64 / rate as f64, b as f64 / rate as f64))
    }
}

/// O asset que o editor mostra agora e o clipe do corte, se for um.
pub(crate) fn resolve(env: &mut Env) -> Option<(Id, Option<(NodeId, aiai_core::AudioPlacement)>)> {
    let cut = env.ed.active_cut(env.core);
    let st = &mut env.ed.audio_ws.wave;
    // a seleção do Mixer/Multipista manda: clicou num clipe, o editor abre aquele clipe
    if env.ed.audio.sel != st.seen_sel {
        st.seen_sel = env.ed.audio.sel.clone();
        if let (Some(id), Some(c)) = (&st.seen_sel, &cut) {
            st.target = Some(Target::Placement { cut: c.clone(), id: id.clone() });
            st.sel = None;
        }
    }
    let p = &env.core.project;
    let valid = match &st.target {
        Some(Target::Placement { cut, id }) => cut_audio(p, cut).1.iter().any(|a| &a.id == id),
        Some(Target::Asset(a)) => p.node(a).is_some_and(|n| matches!(&n.data, NodeKind::Asset(x) if x.asset == AssetType::AudioClip)),
        None => false,
    };
    if !valid {
        st.target = cut.as_ref().and_then(|c| cut_audio(p, c).1.first().map(|a| Target::Placement { cut: c.clone(), id: a.id.clone() })).or_else(|| audio_assets(p).first().map(|(id, _)| Target::Asset(id.clone())));
    }
    match st.target.clone()? {
        Target::Placement { cut, id } => {
            let a = cut_audio(p, &cut).1.into_iter().find(|a| a.id == id)?;
            Some((a.asset.clone(), Some((cut, a))))
        }
        Target::Asset(a) => Some((a, None)),
    }
}

/// Teclas do editor (chamado antes dos atalhos globais, com o retângulo do painel do quadro anterior).
pub(crate) fn collect_keys(ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.audio_ws.wave;
    let Some(rect) = st.rect.take() else { return };
    let over = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| rect.contains(p));
    if !over || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return;
    }
    use egui::{Key as K, Modifiers as M};
    let found = ctx.input_mut(|i| {
        let mut v = Vec::new();
        for (m, k, w) in [
            (M::COMMAND, K::X, WaveKey::Cut),
            (M::COMMAND, K::C, WaveKey::Copy),
            (M::COMMAND, K::V, WaveKey::Paste),
            (M::COMMAND, K::L, WaveKey::Silence),
            (M::COMMAND, K::T, WaveKey::Trim),
            (M::COMMAND, K::A, WaveKey::SelectAll),
            (M::COMMAND, K::E, WaveKey::ZoomSel),
            (M::NONE, K::Delete, WaveKey::Delete),
            (M::NONE, K::Backspace, WaveKey::Delete),
            (M::NONE, K::Space, WaveKey::PlayStop),
            (M::NONE, K::Home, WaveKey::Home),
            (M::NONE, K::End, WaveKey::End),
            (M::NONE, K::Plus, WaveKey::ZoomIn),
            (M::NONE, K::Equals, WaveKey::ZoomIn),
            (M::NONE, K::Minus, WaveKey::ZoomOut),
            (M::NONE, K::Escape, WaveKey::Escape),
        ] {
            if i.consume_key(m, k) {
                v.push(w);
            }
        }
        v
    });
    st.keys.extend(found);
}

// ------------------------------------------------------------------------------------------------ painel

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    env.ed.audio_ws.wave.rect = Some(body);
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some((asset, placement)) = resolve(env) else {
        env.ed.audio_ws.wave.keys.clear();
        ui.label(egui::RichText::new(tr("panel.waveform.empty")).color(t.text_dim));
        return;
    };
    let Some(loaded) = env.ed.audio_ws.load(env.core, &asset) else {
        env.ed.audio_ws.wave.keys.clear();
        ui.label(egui::RichText::new(tr("audio.no_audio")).color(t.danger));
        return;
    };
    let frames = loaded.buf.frames();
    let rate = loaded.buf.rate.max(1);
    {
        let st = &mut env.ed.audio_ws.wave;
        if st.loaded_key.as_deref() != Some(&loaded.key) {
            // arquivo novo: a visão ajusta; a seleção vale se ainda couber (edições mantêm o lugar)
            if st.loaded_key.is_some() && st.fpp > 0.0 {
                st.view_start = st.view_start.min(frames as f64);
            } else {
                st.fpp = 0.0;
                st.view_start = 0.0;
            }
            st.loaded_key = Some(loaded.key.clone());
            st.sel = st.sel.filter(|(a, b)| *b <= frames && a < b);
            st.cursor = st.cursor.min(frames);
        }
    }
    header(env, ui, &asset, placement.as_ref(), &loaded);
    toolbar(env, ui, &asset, placement.as_ref(), &loaded);
    let wide = ui.available_width() > 560.0;
    let hist_w = if wide { 170.0 } else { 0.0 };
    let avail = ui.available_rect_before_wrap();
    let main = Rect::from_min_max(avail.min, egui::pos2(avail.right() - hist_w - if wide { 8.0 } else { 0.0 }, avail.bottom() - if wide { 22.0 } else { 120.0 }));
    editor_area(env, ui, main, &loaded, placement.as_ref());
    let info = Rect::from_min_max(egui::pos2(main.left(), main.bottom() + 2.0), egui::pos2(main.right(), main.bottom() + 20.0));
    selection_info(env, ui, info, rate);
    let hist = if wide { Rect::from_min_max(egui::pos2(main.right() + 8.0, avail.top()), avail.max) } else { Rect::from_min_max(egui::pos2(avail.left(), info.bottom() + 4.0), avail.max) };
    history(env, ui, hist, &asset, placement.as_ref());
    process_keys(env, &asset, placement.as_ref(), &loaded);
    ui.allocate_rect(avail, Sense::hover());
}

fn header(env: &mut Env, ui: &mut Ui, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>, l: &Loaded) {
    let t = env.t;
    let p = &env.core.project;
    let current = match placement {
        Some((_, a)) => format!("{} · {}", tr(crate::app::panels::audio::track_key(a.track)), node_name(p, &a.asset)),
        None => node_name(p, asset),
    };
    let cut = env.ed.active_cut(env.core);
    let pls: Vec<(Id, String)> = cut.as_ref().map(|c| cut_audio(&env.core.project, c).1.iter().map(|a| (a.id.clone(), format!("{} · {}", tr(crate::app::panels::audio::track_key(a.track)), node_name(&env.core.project, &a.asset)))).collect()).unwrap_or_default();
    let assets = audio_assets(&env.core.project);
    let mut pick: Option<Target> = None;
    ui.horizontal(|ui| {
        let r = egui::ComboBox::from_id_salt("aw_target").width((ui.available_width() - 260.0).clamp(120.0, 420.0)).selected_text(current).show_ui(ui, |ui| {
            if let Some(c) = &cut {
                ui.label(egui::RichText::new(tr("audio.wave.cut_clips")).small().color(t.text_dim));
                for (id, label) in &pls {
                    if ui.selectable_label(placement.is_some_and(|(_, a)| &a.id == id), label).clicked() {
                        pick = Some(Target::Placement { cut: c.clone(), id: id.clone() });
                    }
                }
                ui.separator();
            }
            ui.label(egui::RichText::new(tr("audio.wave.assets")).small().color(t.text_dim));
            for (id, name) in &assets {
                if ui.selectable_label(placement.is_none() && id == asset, name).clicked() {
                    pick = Some(Target::Asset(id.clone()));
                }
            }
        });
        env.note("audio:wave:target", r.response.rect);
        let b = &l.buf;
        let ch = match b.channels() {
            1 => tr("audio.mono").to_owned(),
            2 => tr("audio.stereo").to_owned(),
            n => format!("{n} ch"),
        };
        dim_label(ui, &t, &format!("{:.1} kHz · {ch} · {}", b.rate as f64 / 1000.0, fmt_time(b.duration_s())));
    });
    if let Some(tg) = pick {
        if let Target::Placement { id, .. } = &tg {
            env.ed.audio.sel = Some(id.clone());
            env.ed.audio_ws.wave.seen_sel = Some(id.clone());
        }
        env.ed.audio_ws.wave.target = Some(tg);
        env.ed.audio_ws.wave.sel = None;
    }
}

fn after_for(placement: Option<&(NodeId, aiai_core::AudioPlacement)>, trimmed: bool) -> After {
    match placement {
        Some((cut, a)) => After::Retarget { cut: cut.clone(), placement: a.id.clone(), trimmed },
        None => After::Open,
    }
}

/// Manda os passos para `audio_edit` (num fio); o resultado volta como um passo de desfazer.
pub(crate) fn run_steps(env: &mut Env, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>, steps: Vec<EditStep>, label_key: &str, trimmed: bool) {
    let label = tr(label_key).to_owned();
    let r = super::spawn_edit(&mut env.ed.audio_ws, env.core, asset, steps, label, after_for(placement, trimmed));
    actions::notify_err(env.ed, r);
}

fn secs(f: usize, rate: u32) -> f64 {
    f as f64 / rate as f64
}

fn toolbar(env: &mut Env, ui: &mut Ui, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>, l: &Loaded) {
    let t = env.t;
    let rate = l.buf.rate.max(1);
    let busy = env.ed.audio_ws.job.is_some();
    let sel = env.ed.audio_ws.wave.sel;
    let has_sel = sel.is_some_and(|(a, b)| b > a);
    let playing = env.ed.audio_ws.player.is_playing(&l.key);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let r = env.icon_button(ui, if playing { IconId::Pause } else { IconId::Play }, "audio.wave.play", playing, true, "audio:wave:play");
        if r.clicked() {
            toggle_play(env, l);
        }
        let lp = env.ed.audio_ws.wave.looping;
        if env.icon_button(ui, IconId::Loop, "audio.wave.loop", lp, true, "audio:wave:loop").clicked() {
            env.ed.audio_ws.wave.looping = !lp;
        }
        ui.separator();
        let en = !busy;
        let mut op: Option<WaveKey> = None;
        for (k, key, need_sel, name) in [
            (WaveKey::Cut, "audio.wave.cut", true, "audio:wave:cut"),
            (WaveKey::Copy, "audio.wave.copy", true, "audio:wave:copy"),
            (WaveKey::Paste, "audio.wave.paste", false, "audio:wave:paste"),
            (WaveKey::Delete, "audio.wave.delete", true, "audio:wave:delete"),
            (WaveKey::Silence, "audio.wave.silence", true, "audio:wave:silence"),
            (WaveKey::Trim, "audio.wave.trim", true, "audio:wave:trim"),
        ] {
            let ok = en && (!need_sel || has_sel) && (k != WaveKey::Paste || env.ed.audio_ws.wave.clip.is_some());
            let b = ui.add_enabled(ok, egui::Button::new(tr(key)).small());
            env.note(name, b.rect);
            if b.on_hover_text(tr(&format!("{key}.tip"))).clicked() {
                op = Some(k);
            }
        }
        if let Some(k) = op {
            env.ed.audio_ws.wave.keys.push(k);
        }
        ui.separator();
        let b = ui.add_enabled(en && has_sel, egui::Button::new(tr("audio.wave.fade_in")).small());
        env.note("audio:wave:fade_in", b.rect);
        let fade_in = b.on_hover_text(tr("audio.wave.fade.tip")).clicked();
        let b = ui.add_enabled(en && has_sel, egui::Button::new(tr("audio.wave.fade_out")).small());
        env.note("audio:wave:fade_out", b.rect);
        let fade_out = b.on_hover_text(tr("audio.wave.fade.tip")).clicked();
        // menu (não ComboBox): botão comum, quebra de linha junto com os outros
        let mut c = env.ed.audio_ws.wave.curve;
        let r = ui.menu_button(tr(curve_key(c)), |ui| {
            for k in [FadeCurve::Linear, FadeCurve::Log, FadeCurve::SCurve, FadeCurve::EqualPower] {
                if ui.selectable_value(&mut c, k, tr(curve_key(k))).clicked() {
                    ui.close();
                }
            }
        });
        env.note("audio:wave:curve", r.response.rect);
        env.ed.audio_ws.wave.curve = c;
        if let Some((a, b)) = sel.filter(|_| fade_in || fade_out) {
            let dir = if fade_in { FadeDir::In } else { FadeDir::Out };
            run_steps(env, asset, placement, vec![EditStep::Fade { start_s: Some(secs(a, rate)), end_s: Some(secs(b, rate)), dir, curve: c }], if fade_in { "audio.wave.fade_in" } else { "audio.wave.fade_out" }, false);
        }
        ui.separator();
        let mut g = env.ed.audio_ws.wave.gain_db;
        let r = ui.add(egui::DragValue::new(&mut g).range(-48.0..=24.0).speed(0.1).max_decimals(1).suffix(" dB"));
        env.note("audio:wave:gain_db", r.rect);
        env.ed.audio_ws.wave.gain_db = g;
        let b = ui.add_enabled(en, egui::Button::new(tr("audio.wave.gain")).small());
        env.note("audio:wave:gain", b.rect);
        if b.on_hover_text(tr("audio.wave.gain.tip")).clicked() {
            let (s, e) = sel.map(|(a, b)| (Some(secs(a, rate)), Some(secs(b, rate)))).unwrap_or((None, None));
            run_steps(env, asset, placement, vec![EditStep::Gain { gain_db: g as f64, start_s: s, end_s: e }], "audio.wave.gain", false);
        }
        let b = ui.add_enabled(en, egui::Button::new(tr("audio.wave.reverse")).small());
        env.note("audio:wave:reverse", b.rect);
        if b.on_hover_text(tr("audio.wave.reverse.tip")).clicked() {
            let (s, e) = sel.map(|(a, b)| (Some(secs(a, rate)), Some(secs(b, rate)))).unwrap_or((None, None));
            run_steps(env, asset, placement, vec![EditStep::Reverse { start_s: s, end_s: e }], "audio.wave.reverse", false);
        }
        ui.separator();
        let mut v = env.ed.audio_ws.wave.view;
        for (k, key, name) in [(View::Wave, "audio.wave.view_wave", "audio:wave:view_wave"), (View::Spectrum, "audio.wave.view_spec", "audio:wave:view_spec"), (View::Both, "audio.wave.view_both", "audio:wave:view_both")] {
            let r = ui.selectable_value(&mut v, k, tr(key));
            env.note(name, r.rect);
        }
        env.ed.audio_ws.wave.view = v;
        ui.separator();
        if env.icon_button(ui, IconId::ZoomOut, "audio.wave.zoom_out", false, true, "audio:wave:zoom_out").clicked() {
            env.ed.audio_ws.wave.keys.push(WaveKey::ZoomOut);
        }
        if env.icon_button(ui, IconId::Plus, "audio.wave.zoom_in", false, true, "audio:wave:zoom_in").clicked() {
            env.ed.audio_ws.wave.keys.push(WaveKey::ZoomIn);
        }
        if env.icon_button(ui, IconId::Fit, "audio.wave.zoom_fit", false, true, "audio:wave:zoom_fit").clicked() {
            env.ed.audio_ws.wave.fpp = 0.0;
            env.ed.audio_ws.wave.view_start = 0.0;
        }
        let b = ui.add_enabled(has_sel, egui::Button::new(tr("audio.wave.zoom_sel")).small());
        env.note("audio:wave:zoom_sel", b.rect);
        if b.clicked() {
            env.ed.audio_ws.wave.keys.push(WaveKey::ZoomSel);
        }
        let mut amp = env.ed.audio_ws.wave.amp;
        let r = ui.add(egui::DragValue::new(&mut amp).range(1.0..=32.0).speed(0.05).max_decimals(1).prefix("amp ×"));
        env.ed.audio_ws.wave.amp = amp;
        r.on_hover_text(tr("audio.wave.amp.tip"));
        let mut z = env.ed.audio_ws.wave.snap_zero;
        let r = ui.checkbox(&mut z, tr("audio.wave.zero"));
        env.note("audio:wave:zero", r.rect);
        r.on_hover_text(tr("audio.wave.zero.tip"));
        env.ed.audio_ws.wave.snap_zero = z;
        if busy {
            ui.spinner();
            if let Some(j) = &env.ed.audio_ws.job {
                dim_label(ui, &t, &format!("{} ({:.0} s)", j.label, j.started.elapsed().as_secs_f32()));
            }
        }
    });
}

fn curve_key(c: FadeCurve) -> &'static str {
    match c {
        FadeCurve::Linear => "audio.curve.linear",
        FadeCurve::Log => "audio.curve.log",
        FadeCurve::SCurve => "audio.curve.s",
        FadeCurve::EqualPower => "audio.curve.equal_power",
    }
}

fn toggle_play(env: &mut Env, l: &Loaded) {
    let st = &mut env.ed.audio_ws;
    if st.player.is_playing(&l.key) {
        st.player.stop();
        return;
    }
    let n = l.buf.frames();
    let (a, b) = match st.wave.sel.filter(|(a, b)| b > a) {
        Some(s) => s,
        None => (st.wave.cursor.min(n.saturating_sub(1)), n),
    };
    let rack = st.rack.live_for(&l.key);
    st.player.play(super::player::PlayRequest { buf: l.buf.clone(), start: a, end: b, looping: st.wave.looping, rack, key: l.key.clone(), label: tr("panel.waveform.title").to_owned() });
}

/// Visão: (início, quadros por pixel) dentro do arquivo.
fn view_of(st: &WaveState, frames: usize, w: f32) -> (f64, f64) {
    let w = w.max(1.0) as f64;
    let fit = (frames.max(1) as f64 / w).max(1.0 / 32.0);
    let fpp = if st.fpp <= 0.0 { fit } else { st.fpp.clamp(1.0 / 32.0, fit.max(1.0 / 32.0)) };
    let span = fpp * w;
    let start = st.view_start.clamp(0.0, (frames as f64 - span).max(0.0));
    (start, fpp)
}

fn zoom_at(st: &mut WaveState, frames: usize, w: f32, factor: f64, anchor_x: f32) {
    let (start, fpp) = view_of(st, frames, w);
    let anchor = start + anchor_x as f64 * fpp;
    let fit = (frames.max(1) as f64 / w.max(1.0) as f64).max(1.0 / 32.0);
    let new = (fpp * factor).clamp(1.0 / 32.0, fit);
    st.fpp = if (new - fit).abs() < 1e-9 { 0.0 } else { new };
    st.view_start = (anchor - anchor_x as f64 * new).max(0.0);
}

const OVERVIEW_H: f32 = 18.0;
const RULER_H: f32 = 18.0;

fn editor_area(env: &mut Env, ui: &mut Ui, area: Rect, l: &Loaded, placement: Option<&(NodeId, aiai_core::AudioPlacement)>) {
    let t = env.t;
    if area.width() < 40.0 || area.height() < 60.0 {
        return;
    }
    let frames = l.buf.frames();
    let rate = l.buf.rate.max(1);
    let painter = ui.painter().with_clip_rect(area);
    let ov = Rect::from_min_size(area.min, egui::vec2(area.width(), OVERVIEW_H));
    let ruler = Rect::from_min_size(egui::pos2(area.left(), ov.bottom() + 2.0), egui::vec2(area.width(), RULER_H));
    let lanes = Rect::from_min_max(egui::pos2(area.left(), ruler.bottom()), area.max);
    env.note("audio:wave:lanes", lanes);
    let w = lanes.width();
    env.ed.audio_ws.wave.lane_w = w;
    let (start, fpp) = view_of(&env.ed.audio_ws.wave, frames, w);
    let x_of = |f: f64| lanes.left() + ((f - start) / fpp) as f32;
    let f_of = |x: f32| (start + (x - lanes.left()) as f64 * fpp).clamp(0.0, frames as f64) as usize;

    // ---- visão geral (arquivo inteiro) com a janela visível
    painter.rect_filled(ov, 2.0, t.bg_frame);
    let q = l.pyr.query(&l.buf, None, 0, frames, ov.width().max(1.0) as usize);
    let mid = ov.center().y;
    for (i, m) in q.iter().enumerate() {
        let x = ov.left() + i as f32 + 0.5;
        painter.vline(x, (mid - m.max.abs().max(m.min.abs()).min(1.0) * ov.height() * 0.48)..=(mid + m.max.abs().max(m.min.abs()).min(1.0) * ov.height() * 0.48), Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)));
    }
    let sx = |f: f64| ov.left() + (f / frames.max(1) as f64) as f32 * ov.width();
    let win = Rect::from_min_max(egui::pos2(sx(start), ov.top()), egui::pos2(sx(start + fpp * w as f64).max(sx(start) + 2.0), ov.bottom()));
    painter.rect(win, 2.0, t.accent.gamma_multiply(0.12), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    env.note("audio:wave:overview", ov);
    let ovr = ui.interact(ov, ui.id().with("aw_ov"), Sense::click_and_drag());
    if ovr.drag_started() || ovr.clicked() {
        env.ed.audio_ws.wave.drag = Some(Drag::Overview);
    }
    if env.ed.audio_ws.wave.drag == Some(Drag::Overview) {
        if let Some(p) = ovr.interact_pointer_pos() {
            let center = ((p.x - ov.left()) / ov.width()).clamp(0.0, 1.0) as f64 * frames as f64;
            let st = &mut env.ed.audio_ws.wave;
            if st.fpp <= 0.0 {
                st.fpp = fpp;
            }
            st.view_start = (center - fpp * w as f64 / 2.0).max(0.0);
        }
        if !ovr.dragged() {
            env.ed.audio_ws.wave.drag = None;
        }
    }

    // ---- régua
    painter.rect_filled(ruler, 0.0, t.bg_header);
    let span_s = fpp * w as f64 / rate as f64;
    let step = nice_step(span_s / (w as f64 / 90.0).max(1.0));
    let mut tk = (start / rate as f64 / step).floor() * step;
    while tk <= (start + fpp * w as f64) / rate as f64 {
        let x = x_of(tk * rate as f64);
        if x >= lanes.left() {
            painter.vline(x, (ruler.bottom() - 6.0)..=ruler.bottom(), Stroke::new(1.0, t.text_dim));
            painter.text(egui::pos2(x + 3.0, ruler.top() + 2.0), egui::Align2::LEFT_TOP, fmt_tick(tk, step), FontId::monospace(10.0), t.text_dim);
        }
        let half = x_of((tk + step / 2.0) * rate as f64);
        painter.vline(half, (ruler.bottom() - 3.0)..=ruler.bottom(), Stroke::new(1.0, t.border));
        tk += step;
    }
    let rr = ui.interact(ruler, ui.id().with("aw_ruler"), Sense::click_and_drag());
    env.note("audio:wave:ruler", ruler);
    if let Some(p) = rr.interact_pointer_pos().filter(|_| rr.clicked() || rr.dragged()) {
        env.ed.audio_ws.wave.cursor = f_of(p.x);
    }

    // ---- faixas: forma de onda e/ou espectrograma
    let view = env.ed.audio_ws.wave.view;
    let (wave_r, spec_r) = match view {
        View::Wave => (Some(lanes), None),
        View::Spectrum => (None, Some(lanes)),
        View::Both => {
            let h = lanes.height() * 0.55;
            (Some(Rect::from_min_max(lanes.min, egui::pos2(lanes.right(), lanes.top() + h))), Some(Rect::from_min_max(egui::pos2(lanes.left(), lanes.top() + h + 3.0), lanes.max)))
        }
    };
    painter.rect_filled(lanes, 0.0, t.bg_frame);
    // trecho fora do usado pelo clipe no corte: escurecido
    let used = placement.map(|(_, a)| {
        let s = (a.trim_start as f64 * rate as f64).max(0.0);
        let e = a.duration.map(|d| s + d as f64 * rate as f64).unwrap_or(frames as f64).min(frames as f64);
        (s, e)
    });
    if let Some(r) = wave_r {
        draw_wave(&painter, r, l, start, fpp, env.ed.audio_ws.wave.amp, &t);
    }
    if let Some(r) = spec_r {
        draw_spec(env, ui, &painter, r, l, start, fpp);
    }
    if let Some((s, e)) = used {
        let dim = Color32::from_black_alpha(110);
        let (xs, xe) = (x_of(s), x_of(e));
        if xs > lanes.left() {
            painter.rect_filled(Rect::from_min_max(lanes.min, egui::pos2(xs.min(lanes.right()), lanes.bottom())), 0.0, dim);
        }
        if xe < lanes.right() {
            painter.rect_filled(Rect::from_min_max(egui::pos2(xe.max(lanes.left()), lanes.top()), lanes.max), 0.0, dim);
        }
    }
    // seleção
    if let Some((a, b)) = env.ed.audio_ws.wave.sel {
        let r = Rect::from_min_max(egui::pos2(x_of(a as f64).max(lanes.left()), lanes.top()), egui::pos2(x_of(b as f64).min(lanes.right()), lanes.bottom()));
        if r.width() > 0.0 {
            painter.rect_filled(r, 0.0, t.accent.gamma_multiply(0.18));
            painter.vline(r.left(), lanes.y_range(), Stroke::new(1.0, t.accent.gamma_multiply(0.7)));
            painter.vline(r.right(), lanes.y_range(), Stroke::new(1.0, t.accent.gamma_multiply(0.7)));
        }
    }
    // cursor e cabeçote
    let cx = x_of(env.ed.audio_ws.wave.cursor as f64);
    if (lanes.left()..=lanes.right()).contains(&cx) {
        painter.vline(cx, ruler.top()..=lanes.bottom(), Stroke::new(1.0, t.text.gamma_multiply(0.7)));
    }
    // cabeçote da escuta deste arquivo (direta ou a prévia do rack em tempo real, que toca o mesmo buffer)
    let rack_key = format!("rack:{}", l.key);
    let mine = env.ed.audio_ws.player.is_playing(&l.key) || env.ed.audio_ws.player.is_playing(&rack_key);
    if let Some(pos) = env.ed.audio_ws.player.position().filter(|_| mine) {
        let px = x_of(pos as f64);
        if (lanes.left()..=lanes.right()).contains(&px) {
            painter.vline(px, ruler.top()..=lanes.bottom(), Stroke::new(1.6, Color32::from_rgb(255, 196, 64)));
        } else if env.ed.audio_ws.wave.drag.is_none() {
            // segue o cabeçote quando ele sai da janela
            let st = &mut env.ed.audio_ws.wave;
            if st.fpp > 0.0 {
                st.view_start = pos as f64;
            }
        }
    }

    // ---- interação nas faixas
    let resp = ui.interact(lanes, ui.id().with("aw_lanes"), Sense::click_and_drag());
    let shift = ui.input(|i| i.modifiers.shift);
    if resp.drag_started() {
        if let Some(p) = resp.interact_pointer_pos() {
            let f = f_of(p.x);
            let anchor = if shift { env.ed.audio_ws.wave.cursor } else { f };
            env.ed.audio_ws.wave.drag = Some(Drag::Select(anchor));
        }
    }
    if let (Some(Drag::Select(a)), Some(p)) = (env.ed.audio_ws.wave.drag, resp.interact_pointer_pos()) {
        let f = f_of(p.x);
        let st = &mut env.ed.audio_ws.wave;
        st.sel = Some((a.min(f), a.max(f))).filter(|(x, y)| y > x);
        st.cursor = a.min(f);
        if resp.drag_stopped() || !resp.dragged() {
            st.drag = None;
            if st.snap_zero {
                st.sel = st.sel.map(|(x, y)| (aiai_audio::edit::nearest_zero_crossing(&l.buf, x, 512), aiai_audio::edit::nearest_zero_crossing(&l.buf, y, 512))).filter(|(x, y)| y > x);
            }
        }
        // rola junto quando o arrasto passa da borda
        if p.x > lanes.right() - 4.0 || p.x < lanes.left() + 4.0 {
            let dir = if p.x > lanes.right() - 4.0 { 1.0 } else { -1.0 };
            if st.fpp <= 0.0 {
                st.fpp = fpp;
            }
            st.view_start = (start + dir * fpp * 20.0).max(0.0);
        }
    }
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let f = f_of(p.x);
            let st = &mut env.ed.audio_ws.wave;
            if shift {
                let c = st.cursor;
                st.sel = Some((c.min(f), c.max(f))).filter(|(x, y)| y > x);
            } else {
                st.sel = None;
                st.cursor = f;
            }
        }
    }
    if resp.double_clicked() {
        env.ed.audio_ws.wave.sel = Some((0, frames)).filter(|(a, b)| b > a);
    }
    if resp.hovered() {
        let (dy, dx, ctrl) = ui.input(|i| (i.smooth_scroll_delta.y, i.smooth_scroll_delta.x, i.modifiers.command));
        if let Some(p) = resp.hover_pos() {
            if dy.abs() > 0.1 && (ctrl || dx.abs() < 0.1) {
                // roda = zoom no ponteiro (Ctrl também); deslocamento horizontal = rolar
                zoom_at(&mut env.ed.audio_ws.wave, frames, w, (-dy as f64 * 0.004).exp(), p.x - lanes.left());
            } else if dx.abs() > 0.1 {
                let st = &mut env.ed.audio_ws.wave;
                if st.fpp <= 0.0 {
                    st.fpp = fpp;
                }
                st.view_start = (start - dx as f64 * fpp).max(0.0);
            }
            let f = f_of(p.x);
            resp.clone().on_hover_text_at_pointer(fmt_time(f as f64 / rate as f64));
        }
    }
    painter.rect_stroke(lanes, 0.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
}

fn draw_wave(painter: &egui::Painter, r: Rect, l: &Loaded, start: f64, fpp: f64, amp: f32, t: &crate::theme::Tokens) {
    let nch = l.buf.channels().max(1);
    let px = r.width().max(1.0) as usize;
    let lane_h = r.height() / nch as f32;
    let wave_col = Color32::from_rgb(96, 168, 255);
    let rms_col = Color32::from_rgb(160, 205, 255);
    for c in 0..nch {
        let lane = Rect::from_min_size(egui::pos2(r.left(), r.top() + c as f32 * lane_h), egui::vec2(r.width(), lane_h));
        let mid = lane.center().y;
        let half = lane_h * 0.47;
        painter.hline(lane.x_range(), mid, Stroke::new(1.0, t.border));
        if c > 0 {
            painter.hline(lane.x_range(), lane.top(), Stroke::new(1.0, t.border));
        }
        let y = |v: f32| mid - (v * amp).clamp(-1.0, 1.0) * half;
        if fpp >= 1.0 {
            let end = (start + fpp * px as f64).ceil() as usize;
            let q = l.pyr.query(&l.buf, Some(c), start as usize, end, px);
            for (i, m) in q.iter().enumerate() {
                let x = r.left() + i as f32 + 0.5;
                let (a, b) = (y(m.max), y(m.min));
                painter.vline(x, a..=b.max(a + 1.0), Stroke::new(1.0, wave_col));
                if fpp > 2.0 && m.rms > 0.0 {
                    painter.vline(x, y(m.rms)..=y(-m.rms), Stroke::new(1.0, rms_col));
                }
            }
        } else {
            // zoom de amostra: linha ligando as amostras (com pontos quando bem perto)
            let s0 = start.floor() as usize;
            let s1 = ((start + fpp * px as f64).ceil() as usize + 1).min(l.buf.frames());
            let ch = &l.buf.ch[c];
            let pts: Vec<Pos2> = (s0..s1).map(|i| egui::pos2(r.left() + ((i as f64 - start) / fpp) as f32, y(ch[i]))).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.2, wave_col)));
            if fpp < 0.2 {
                for p in pts {
                    painter.circle_filled(p, 2.0, rms_col);
                }
            }
        }
        if nch == 2 {
            painter.text(lane.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, if c == 0 { "L" } else { "R" }, FontId::proportional(10.0), t.text_dim);
        }
    }
}

/// Espectrograma da janela visível, calculado num fio (coluna a coluna direto do buffer: nada de copiar o trecho
/// inteiro) e mostrado como textura; enquanto o novo não chega, o anterior fica esticado.
fn draw_spec(env: &mut Env, ui: &mut Ui, painter: &egui::Painter, r: Rect, l: &Loaded, start: f64, fpp: f64) {
    let t = env.t;
    let (w, h) = ((r.width().max(8.0) as usize).min(2048), (r.height().max(8.0) as usize).min(512));
    let s0 = start as usize;
    let s1 = ((start + fpp * r.width() as f64) as usize).min(l.buf.frames()).max(s0 + 1);
    let want = (l.key.clone(), s0, s1, w, h);
    let sp = &mut env.ed.audio_ws.wave.spec;
    if let Some(rx) = &sp.rx {
        if let Ok((k, img)) = rx.try_recv() {
            sp.tex = Some(ui.ctx().load_texture("audio_spec", img, TextureOptions::LINEAR));
            sp.shown = Some(k);
            sp.rx = None;
            sp.pending = None;
        }
    }
    if sp.shown.as_ref() != Some(&want) && sp.pending.as_ref() != Some(&want) && sp.rx.is_none() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let buf = l.buf.clone();
        let k = want.clone();
        if std::thread::Builder::new().name("aiai-spectro".into()).spawn(move || {
            let img = spectro_image(&buf, k.1, k.2, k.3, k.4);
            let _ = tx.send((k, img));
        }).is_ok()
        {
            sp.rx = Some(rx);
            sp.pending = Some(want.clone());
        }
    }
    if sp.rx.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
    painter.rect_filled(r, 0.0, Color32::BLACK);
    if let (Some(tex), Some(k)) = (&sp.tex, &sp.shown) {
        // a textura cobre [k.1, k.2): posiciona pela visão atual
        let xa = r.left() + ((k.1 as f64 - start) / fpp) as f32;
        let xb = r.left() + ((k.2 as f64 - start) / fpp) as f32;
        painter.image(tex.id(), Rect::from_min_max(egui::pos2(xa, r.top()), egui::pos2(xb, r.bottom())), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    // escala de frequência (log de 20 Hz a Nyquist)
    let nyq = l.buf.rate as f64 / 2.0;
    for f in [100.0, 1000.0, 5000.0, 10000.0] {
        if f >= nyq || (r.height() < 90.0 && f == 5000.0) || r.height() < 40.0 {
            continue;
        }
        let tpos = (f / 20.0f64).ln() / (nyq / 20.0).ln();
        let y = r.bottom() - tpos as f32 * r.height();
        painter.hline(r.left()..=(r.left() + 6.0), y, Stroke::new(1.0, t.text_dim));
        painter.text(egui::pos2(r.left() + 8.0, y), egui::Align2::LEFT_CENTER, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }, FontId::monospace(9.5), t.text_dim);
    }
}

/// Imagem `w × h` do espectrograma de `[s0, s1)`: uma FFT de 2048 por coluna centrada nela, em dB (−100 a 0), frequência
/// em escala log de 20 Hz a Nyquist (agudos em cima), cores de "magma".
pub(crate) fn spectro_image(buf: &AudioBuf, s0: usize, s1: usize, w: usize, h: usize) -> ColorImage {
    use aiai_audio::fft::{Fft, WindowKind, window};
    let n = 2048usize;
    let fft = Fft::new(n);
    let win = window(WindowKind::Hann, n);
    let norm = 2.0 / win.iter().sum::<f32>().max(1e-9);
    let nch = buf.channels().max(1);
    let len = buf.frames();
    let bins = n / 2 + 1;
    let nyq = buf.rate as f64 / 2.0;
    let (f_lo, f_hi) = (20.0f64, nyq.max(40.0));
    // linhas → faixas de bins (log)
    let rows: Vec<(usize, usize)> = (0..h)
        .map(|row| {
            let t0 = 1.0 - (row + 1) as f64 / h as f64;
            let t1 = 1.0 - row as f64 / h as f64;
            let fa = f_lo * (f_hi / f_lo).powf(t0);
            let fb = f_lo * (f_hi / f_lo).powf(t1);
            let ba = ((fa / nyq) * (bins - 1) as f64).floor() as usize;
            let bb = (((fb / nyq) * (bins - 1) as f64).ceil() as usize).min(bins - 1);
            (ba.min(bins - 1), bb.max(ba.min(bins - 1)))
        })
        .collect();
    let mut img = ColorImage::new([w, h], vec![Color32::BLACK; w * h]);
    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut db = vec![0.0f32; bins];
    let span = (s1 - s0) as f64;
    for col in 0..w {
        let center = s0 as f64 + (col as f64 + 0.5) * span / w as f64;
        for k in 0..n {
            let idx = center as isize + k as isize - (n / 2) as isize;
            let v = if idx >= 0 && (idx as usize) < len { buf.ch.iter().map(|c| c[idx as usize]).sum::<f32>() / nch as f32 } else { 0.0 };
            re[k] = v * win[k];
            im[k] = 0.0;
        }
        fft.forward(&mut re, &mut im);
        for k in 0..bins {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm;
            db[k] = (20.0 * mag.max(1e-12).log10()).clamp(-100.0, 0.0);
        }
        for (row, (ba, bb)) in rows.iter().enumerate() {
            let m = db[*ba..=*bb].iter().cloned().fold(-100.0f32, f32::max);
            img.pixels[row * w + col] = magma(((m + 100.0) / 100.0).clamp(0.0, 1.0));
        }
    }
    img
}

/// Mapa de cor escuro → claro (preto, roxo, vermelho, laranja, amarelo-claro).
fn magma(v: f32) -> Color32 {
    const STOPS: [(f32, [f32; 3]); 5] = [(0.0, [0.0, 0.0, 0.02]), (0.35, [0.30, 0.06, 0.42]), (0.6, [0.78, 0.18, 0.36]), (0.82, [0.98, 0.55, 0.18]), (1.0, [0.99, 0.96, 0.75])];
    let mut i = 0;
    while i + 1 < STOPS.len() - 1 && v > STOPS[i + 1].0 {
        i += 1;
    }
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    let tt = ((v - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let c = |k: usize| ((a.1[k] + (b.1[k] - a.1[k]) * tt) * 255.0) as u8;
    Color32::from_rgb(c(0), c(1), c(2))
}

fn nice_step(raw: f64) -> f64 {
    let steps = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    steps.iter().copied().find(|s| *s >= raw).unwrap_or(1200.0)
}

fn fmt_tick(s: f64, step: f64) -> String {
    if step < 0.01 {
        format!("{s:.3}")
    } else if step < 1.0 {
        format!("{s:.2}")
    } else {
        fmt_time(s).trim_end_matches(".000").to_owned()
    }
}

fn selection_info(env: &mut Env, ui: &mut Ui, r: Rect, rate: u32) {
    let t = env.t;
    let st = &env.ed.audio_ws.wave;
    let s = |f: usize| fmt_time(f as f64 / rate as f64);
    let text = match st.sel {
        Some((a, b)) => format!("{}  {} – {}  ({:.3} s)", tr("audio.wave.selection"), s(a), s(b), (b - a) as f64 / rate as f64),
        None => format!("{}  {}", tr("audio.wave.cursor"), s(st.cursor)),
    };
    ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, text, FontId::monospace(11.0), t.text_dim);
    if let Some(e) = &env.ed.audio_ws.player.last_error {
        ui.painter().text(r.right_center(), egui::Align2::RIGHT_CENTER, e, FontId::proportional(11.0), t.danger);
    }
}

/// Resumo de um passo guardado em `params.steps` ("apagar 0,250–0,500 s", "fx eq"...).
pub(crate) fn step_summary(v: &Value) -> String {
    let op = v.get("op").and_then(Value::as_str).unwrap_or("?");
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let range = match (f("start_s"), f("end_s")) {
        (Some(a), Some(b)) => format!(" {a:.3}–{b:.3} s"),
        (Some(a), None) => format!(" {a:.3} s–"),
        (None, Some(b)) => format!(" –{b:.3} s"),
        _ => String::new(),
    };
    let extra = match op {
        "fx" => v.pointer("/fx/type").and_then(Value::as_str).map(|s| format!(" {s}")).unwrap_or_default(),
        "rack" => v.pointer("/rack/slots").and_then(Value::as_array).map(|s| format!(" ({})", s.iter().filter_map(|x| x.pointer("/fx/type").and_then(Value::as_str)).collect::<Vec<_>>().join(", "))).unwrap_or_default(),
        "gain" => f("gain_db").map(|g| format!(" {g:+.1} dB")).unwrap_or_default(),
        "fade" => v.get("dir").and_then(Value::as_str).map(|d| format!(" {d}")).unwrap_or_default(),
        "paste" | "duplicate" => f("at_s").map(|a| format!(" @ {a:.3} s")).unwrap_or_default(),
        _ => String::new(),
    };
    format!("{op}{extra}{range}")
}

/// Cadeia de versões do asset (o atual primeiro), seguindo `params.edited_from`.
pub(crate) fn revisions(p: &aiai_core::Project, asset: &Id) -> Vec<(Id, String, Vec<String>)> {
    let mut out = Vec::new();
    let mut cur = Some(asset.clone());
    while let Some(id) = cur.take() {
        if out.len() >= 64 || out.iter().any(|(x, _, _): &(Id, String, Vec<String>)| x == &id) || p.node(&id).is_none() {
            break;
        }
        let params = asset_params(p, &id);
        let steps: Vec<String> = params.get("steps").and_then(Value::as_array).map(|a| a.iter().map(step_summary).collect()).unwrap_or_default();
        out.push((id.clone(), node_name(p, &id), steps));
        cur = params.get("edited_from").and_then(|v| serde_json::from_value::<Id>(v.clone()).ok());
    }
    out
}

fn history(env: &mut Env, ui: &mut Ui, r: Rect, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>) {
    let t = env.t;
    if r.height() < 30.0 || r.width() < 60.0 {
        return;
    }
    env.note("audio:wave:history", r);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
    child.set_clip_rect(r);
    let ui = &mut child;
    ui.painter().rect_filled(r, 4.0, t.bg_frame);
    let revs = revisions(&env.core.project, asset);
    let mut go: Option<Id> = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("audio.wave.history")).size(11.5).strong().color(t.text_dim));
        let can_undo = !env.core.log().is_empty();
        let b = ui.add_enabled(can_undo, egui::Button::new(tr("audio.wave.undo")).small());
        env.note("audio:wave:undo", b.rect);
        if b.clicked() {
            let r = actions::run(env.core, "UNDO", json!({}));
            actions::notify_err(env.ed, r);
        }
        let b = ui.button(egui::RichText::new(tr("audio.wave.redo")).small());
        env.note("audio:wave:redo", b.rect);
        if b.clicked() {
            let r = actions::run(env.core, "REDO", json!({}));
            actions::notify_err(env.ed, r);
        }
    });
    egui::ScrollArea::vertical().id_salt("aw_hist").auto_shrink([false, false]).show(ui, |ui| {
        // do mais antigo (original) ao atual, como uma lista de histórico
        for (i, (id, name, steps)) in revs.iter().enumerate().rev() {
            let current = i == 0;
            let label = if steps.is_empty() { format!("{}  {name}", tr("audio.wave.original")) } else { steps.join(" · ") };
            let resp = ui.selectable_label(current, egui::RichText::new(label).size(11.5).color(if current { t.text } else { t.text_dim }));
            env.note(format!("audio:wave:rev:{}", revs.len() - 1 - i), resp.rect);
            if resp.on_hover_text(format!("{name}\n{}", tr("audio.wave.rev.tip"))).clicked() && !current {
                go = Some(id.clone());
            }
        }
        if revs.len() <= 1 {
            dim_label(ui, &t, tr("audio.wave.no_history"));
        }
    });
    if let Some(id) = go {
        match placement {
            Some((cut, a)) => {
                let (_, mut list) = cut_audio(&env.core.project, cut);
                if let Some(x) = list.iter_mut().find(|x| x.id == a.id) {
                    x.asset = id;
                    x.trim_start = 0.0;
                    x.duration = None;
                }
                let cmd = Command::SetProps { id: cut.clone(), name: None, data: json!({ "audio": list }) };
                let r = serde_json::to_value(&cmd).map_err(|e| e.to_string()).and_then(|c| actions::run(env.core, "APPLY_COMMAND", json!({ "command": c })));
                actions::notify_err(env.ed, r);
            }
            None => env.ed.audio_ws.wave.open_asset(id),
        }
    }
}

// ------------------------------------------------------------------------------------------------ teclas e operações

fn process_keys(env: &mut Env, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>, l: &Loaded) {
    let keys = std::mem::take(&mut env.ed.audio_ws.wave.keys);
    let rate = l.buf.rate.max(1);
    let frames = l.buf.frames();
    let busy = env.ed.audio_ws.job.is_some();
    for k in keys {
        let sel = env.ed.audio_ws.wave.sel.filter(|(a, b)| b > a);
        let range = sel.map(|(a, b)| (secs(a, rate), secs(b, rate)));
        let w = env.ed.audio_ws.wave.lane_w;
        match k {
            WaveKey::PlayStop => toggle_play(env, l),
            WaveKey::Home => {
                env.ed.audio_ws.wave.cursor = 0;
                env.ed.audio_ws.wave.view_start = 0.0;
            }
            WaveKey::End => {
                env.ed.audio_ws.wave.cursor = frames;
                env.ed.audio_ws.wave.view_start = frames as f64;
            }
            WaveKey::Escape => env.ed.audio_ws.wave.sel = None,
            WaveKey::SelectAll => env.ed.audio_ws.wave.sel = Some((0, frames)).filter(|(a, b)| b > a),
            WaveKey::ZoomIn | WaveKey::ZoomOut => {
                let st = &mut env.ed.audio_ws.wave;
                let (start, fpp) = view_of(st, frames, w);
                let anchor_x = ((st.cursor as f64 - start) / fpp).clamp(0.0, w as f64) as f32;
                zoom_at(st, frames, w, if k == WaveKey::ZoomIn { 0.5 } else { 2.0 }, anchor_x);
            }
            WaveKey::ZoomSel => {
                if let Some((a, b)) = sel {
                    let st = &mut env.ed.audio_ws.wave;
                    st.fpp = ((b - a) as f64 * 1.1 / w.max(1.0) as f64).max(1.0 / 32.0);
                    st.view_start = a as f64 - (b - a) as f64 * 0.05;
                }
            }
            WaveKey::Copy => {
                if let Some((a, b)) = sel {
                    env.ed.audio_ws.wave.clip = Some(Clipboard { key: l.key.clone(), asset: asset.clone(), start: a, end: b, buf: Arc::new(aiai_audio::edit::copy(&l.buf, a..b)) });
                }
            }
            _ if busy => env.ed.notify(tr("audio.job_busy")),
            WaveKey::Cut => {
                if let Some((a, b)) = range {
                    let (fa, fb) = sel.unwrap_or_default();
                    env.ed.audio_ws.wave.clip = Some(Clipboard { key: l.key.clone(), asset: asset.clone(), start: fa, end: fb, buf: Arc::new(aiai_audio::edit::copy(&l.buf, fa..fb)) });
                    env.ed.audio_ws.wave.sel = None;
                    env.ed.audio_ws.wave.cursor = fa;
                    run_steps(env, asset, placement, vec![EditStep::Delete { start_s: a, end_s: b }], "audio.wave.cut", false);
                }
            }
            WaveKey::Delete => {
                if let Some((a, b)) = range {
                    env.ed.audio_ws.wave.sel = None;
                    env.ed.audio_ws.wave.cursor = sel.map(|s| s.0).unwrap_or(0);
                    run_steps(env, asset, placement, vec![EditStep::Delete { start_s: a, end_s: b }], "audio.wave.delete", false);
                }
            }
            WaveKey::Silence => {
                if let Some((a, b)) = range {
                    run_steps(env, asset, placement, vec![EditStep::Silence { start_s: a, end_s: b }], "audio.wave.silence", false);
                }
            }
            WaveKey::Trim => {
                if let Some((a, b)) = range {
                    env.ed.audio_ws.wave.sel = None;
                    env.ed.audio_ws.wave.cursor = 0;
                    run_steps(env, asset, placement, vec![EditStep::Trim { start_s: a, end_s: b }], "audio.wave.trim", true);
                }
            }
            WaveKey::Paste => paste(env, asset, placement, l, sel),
        }
    }
}

/// Colar no cursor (ou no lugar da seleção). Do mesmo arquivo: passos `delete` + `duplicate` do `audio_edit`. De outro
/// arquivo: o trecho entra no buffer (conformado à taxa/canais) e vira um asset novo com a mesma origem registrada.
fn paste(env: &mut Env, asset: &Id, placement: Option<&(NodeId, aiai_core::AudioPlacement)>, l: &Loaded, sel: Option<(usize, usize)>) {
    let Some(clip) = &env.ed.audio_ws.wave.clip else { return };
    let rate = l.buf.rate.max(1);
    let at = sel.map(|s| s.0).unwrap_or(env.ed.audio_ws.wave.cursor).min(l.buf.frames());
    if clip.key == l.key {
        let (cs, ce) = (clip.start, clip.end);
        let mut steps = Vec::new();
        let (mut s, mut e) = (cs, ce);
        if let Some((a, b)) = sel {
            // substituir a seleção: só quando o trecho copiado não se sobrepõe a ela
            if ce <= a || cs >= b {
                steps.push(EditStep::Delete { start_s: secs(a, rate), end_s: secs(b, rate) });
                if cs >= b {
                    s -= b - a;
                    e -= b - a;
                }
            }
        }
        steps.push(EditStep::Duplicate { start_s: secs(s, rate), end_s: secs(e, rate), at_s: secs(at, rate) });
        env.ed.audio_ws.wave.sel = Some((at, at + (ce - cs)));
        run_steps(env, asset, placement, steps, "audio.wave.paste", false);
        return;
    }
    let src = clip.asset.clone();
    let (cs, ce, piece) = (clip.start, clip.end, clip.buf.clone());
    let target = l.buf.clone();
    let st = &mut env.ed.audio_ws;
    if st.job.is_some() {
        env.ed.notify(tr("audio.job_busy"));
        return;
    }
    let p = &env.core.project;
    let (name, params0, source) = match p.node(asset).map(|n| (n.name.clone(), &n.data)) {
        Some((n, NodeKind::Asset(a))) => (n, a.params.clone(), a.source.clone()),
        _ => return,
    };
    let src_rate = piece.rate;
    let from_id = asset.clone();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let after = after_for(placement, false);
    let spawned = std::thread::Builder::new().name("aiai-audio-paste".into()).spawn(move || {
        let r = (|| -> Result<super::JobOut, String> {
            let conf = aiai_audio::edit::conform(&piece, target.rate, target.channels());
            let mut out = target.as_ref().clone();
            if let Some((a, b)) = sel {
                out = aiai_audio::edit::delete_range(&out, a..b);
            }
            let out = aiai_audio::edit::insert(&out, at.min(out.frames()), &conf);
            let bytes = aiai_tools::audio::wav_bytes(&out, aiai_tools::audio::WavBits::Pcm24).map_err(|e| e.to_string())?;
            let h = aiai_core::BlobHash::of(&bytes);
            let mut params = json!({
                "edited_from": from_id,
                "steps": [{ "op": "paste", "at_s": at as f64 / target.rate as f64, "from": src, "start_s": cs as f64 / src_rate as f64, "end_s": ce as f64 / src_rate as f64 }],
                "sample_rate": out.rate, "channels": out.channels(),
            });
            for k in ["credit", "engine", "speaker_name", "style", "text"] {
                if let Some(v) = params0.get(k) {
                    params[k] = v.clone();
                }
            }
            let id = aiai_core::IdGen::Uuid7.next();
            let nm: String = format!("{name} (editado)").chars().take(60).collect();
            let cmd = Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: nm, data: NodeKind::Asset(aiai_core::AssetData { asset: AssetType::AudioClip, blob: Some(h), params, source: if source.is_empty() { "audio_edit".into() } else { source } }) };
            Ok(super::JobOut { commands: vec![cmd], asset: id, blobs: vec![Arc::new(bytes)], duration_s: out.duration_s() })
        })();
        let _ = tx.send(r);
    });
    match spawned {
        Ok(_) => st.job = Some(super::Job { rx, after, label: tr("audio.wave.paste").to_owned(), started: std::time::Instant::now() }),
        Err(e) => env.ed.notify(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_math_clamps_and_zooms_around_the_anchor() {
        let mut st = WaveState::default();
        let (s, f) = view_of(&st, 48_000, 480.0);
        assert_eq!((s, f), (0.0, 100.0), "ajustar: arquivo inteiro na largura");
        zoom_at(&mut st, 48_000, 480.0, 0.5, 240.0);
        let (s, f) = view_of(&st, 48_000, 480.0);
        assert_eq!(f, 50.0);
        assert!((s - 12_000.0).abs() < 1e-6, "o meio continua no meio: {s}");
        // não passa do ajuste nem do zoom de amostra
        zoom_at(&mut st, 48_000, 480.0, 100.0, 0.0);
        assert_eq!(st.fpp, 0.0);
        for _ in 0..20 {
            zoom_at(&mut st, 48_000, 480.0, 0.5, 0.0);
        }
        assert!((view_of(&st, 48_000, 480.0).1 - 1.0 / 32.0).abs() < 1e-12);
    }

    #[test]
    fn spectrogram_image_puts_a_tone_on_its_row() {
        let rate = 48_000;
        let buf = AudioBuf::mono(rate, (0..rate as usize).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin()).collect());
        let img = spectro_image(&buf, 0, buf.frames(), 32, 64);
        assert_eq!(img.size, [32, 64]);
        // a linha mais clara na coluna do meio fica perto de 1 kHz (escala log de 20 Hz a 24 kHz)
        let col = 16;
        let lum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        let best = (0..64).max_by_key(|&r| lum(img.pixels[r * 32 + col])).unwrap();
        let t = 1.0 - (best as f64 + 0.5) / 64.0;
        let f = 20.0 * (24_000.0f64 / 20.0).powf(t);
        assert!((600.0..1700.0).contains(&f), "pico em {f:.0} Hz");
    }

    #[test]
    fn step_summaries_and_revision_chain() {
        assert_eq!(step_summary(&json!({ "op": "delete", "start_s": 0.25, "end_s": 0.5 })), "delete 0.250–0.500 s");
        assert_eq!(step_summary(&json!({ "op": "fx", "fx": { "type": "eq" } })), "fx eq");
        assert_eq!(step_summary(&json!({ "op": "gain", "gain_db": -3.0 })), "gain -3.0 dB");
        let (mut core, _cut, asset, _pl) = super::super::tests::doc_with_clip();
        let mut st = super::super::AudioWsState::default();
        super::super::spawn_edit(&mut st, &core, &asset, vec![EditStep::Silence { start_s: 0.0, end_s: 0.1 }], "s".into(), After::Open).unwrap();
        let out = super::super::tests::wait_job(&mut st);
        super::super::apply_job(&mut core, &out, &After::Open, "s").unwrap();
        let revs = revisions(&core.project, &out.asset);
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].0, out.asset);
        assert_eq!(revs[0].2, vec!["silence 0.000–0.100 s".to_owned()]);
        assert_eq!(revs[1].0, asset);
    }
}

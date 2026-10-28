//! Painel Multipista (S24 3.11): as faixas de áudio do corte como no Audacity — cabeçalho por faixa (nome, Mudo, Solo,
//! ganho, pan), clipes com a forma de onda (pirâmide em cache), as alças de fade e o nome; faixa de rótulos com o texto das
//! falas; régua em timecode + quadros com o cabeçote do corte (o mesmo quadro do Monitor: a imagem acompanha).
//!
//! - Arrastar um clipe muda o início (em quadros) e arrastar as alças muda os fades: um passo de desfazer ao soltar.
//! - Clique num clipe = seleção do workspace (o editor de forma de onda e o Mixer abrem o mesmo clipe).
//! - Tocar mixa o corte respeitando Mudo/Solo/ganho/pan das faixas (escuta) e move o cabeçote do corte junto.
//!   "Gravar no corte" leva ganho e pan das faixas para os clipes (a mixagem final e a entrega passam a ouvir igual).

use super::player::PlayRequest;
use super::{cut_audio, fmt_time, node_name};
use crate::app::actions;
use crate::app::commands::NodeId;
use crate::app::editor::Editor;
use crate::app::panels::audio::{TRACKS, set_audio};
use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_audio::mixer::{MixSettings, MixTrack};
use aiai_core::{AudioPlacement, AudioTrack, CoreSession};
use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui};
use std::sync::Arc;

const HEADER_W: f32 = 168.0;
const RULER_H: f32 = 22.0;
const LABEL_H: f32 = 20.0;

#[derive(Clone, Debug)]
enum DragKind {
    Move,
    FadeIn,
    FadeOut,
}

#[derive(Clone, Debug)]
struct ClipDrag {
    id: aiai_core::Id,
    kind: DragKind,
    from_x: f32,
    orig: AudioPlacement,
}

pub(crate) struct MtState {
    pub mute: [bool; 4],
    pub solo: [bool; 4],
    pub gain: [f32; 4],
    pub pan: [f32; 4],
    /// Altura das faixas: 0 pequena, 1 média, 2 grande.
    pub height: usize,
    /// Pixels por segundo (0 = o corte inteiro na largura).
    pub pps: f32,
    drag: Option<ClipDrag>,
    /// Prévia do arrasto (início em quadros, fades).
    preview: Option<AudioPlacement>,
}

impl Default for MtState {
    fn default() -> Self {
        MtState { mute: [false; 4], solo: [false; 4], gain: [0.0; 4], pan: [0.0; 4], height: 1, pps: 0.0, drag: None, preview: None }
    }
}

impl MtState {
    /// A faixa soa na escuta? (Solo em alguma = só as em solo; Mudo cala.)
    pub(crate) fn audible(&self, k: usize) -> bool {
        let any_solo = self.solo.iter().any(|s| *s);
        !self.mute[k] && (!any_solo || self.solo[k])
    }
}

fn track_index(t: AudioTrack) -> usize {
    TRACKS.iter().position(|(x, _)| *x == t).unwrap_or(0)
}

fn mix_index(t: MixTrack) -> usize {
    MixTrack::ALL.iter().position(|x| *x == t).unwrap_or(0)
}

/// Mixagem de escuta do corte: Mudo/Solo tiram clipes, o ganho da faixa vai no barramento e o pan soma no clipe.
pub(crate) fn monitor_mix(core: &CoreSession, cut: &NodeId, st: &MtState) -> Result<aiai_audio::AudioBuf, String> {
    let p = &core.project;
    let cd = crate::app::commands::cut_data(p, cut).ok_or("corte?")?;
    let mut clips = aiai_tools::audio::cut_clips(p, &core.blobs, cut).map_err(|e| e.to_string())?;
    clips.retain(|c| st.audible(mix_index(c.track)));
    for c in &mut clips {
        c.pan = (c.pan + st.pan[mix_index(c.track)]).clamp(-1.0, 1.0);
    }
    let mut s = MixSettings { rate: p.settings.audio_sample_rate.max(8000), ..Default::default() };
    s.dialogue.gain_db = st.gain[0];
    s.sfx.gain_db = st.gain[1];
    s.bgm.gain_db = st.gain[2];
    s.ambience.gain_db = st.gain[3];
    let samples = (cd.duration as f64 / p.settings.fps() * s.rate as f64).round() as usize;
    Ok(aiai_audio::mixer::render(&clips, samples, &s))
}

/// Enquanto a multipista toca, o cabeçote do corte segue a reprodução (o Monitor mostra o quadro).
pub(crate) fn follow_playback(core: &CoreSession, ed: &mut Editor) {
    let Some(p) = ed.audio_ws.player.playing() else { return };
    let Some(cut) = p.key.strip_prefix("mt:") else { return };
    if ed.cut.as_ref().is_none_or(|c| c.0 != cut) {
        return;
    }
    let rate = p.rate.max(1) as f64;
    let Some(pos) = ed.audio_ws.player.position() else { return };
    let fps = core.project.settings.fps();
    let dur = ed.cut.as_ref().and_then(|c| crate::app::commands::cut_data(&core.project, c)).map(|c| c.duration).unwrap_or(1);
    ed.frame = ((pos as f64 / rate * fps) as u32).min(dur.saturating_sub(1));
    // o som ao vivo da linha do tempo não toca trecho de scrub a cada quadro que a multipista anda
    let cut = ed.cut.clone();
    ed.live.follow_silently(cut.as_ref(), ed.frame);
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.label(egui::RichText::new(tr("panel.multitrack.empty")).color(t.text_dim));
        return;
    };
    let (duration, list) = cut_audio(&env.core.project, &cut);
    let fps = env.core.project.settings.fps().max(1.0);
    let cut_s = duration.max(1) as f64 / fps;
    toolbar(env, ui, &cut, &list, cut_s);
    let area = ui.available_rect_before_wrap();
    if area.height() < 60.0 {
        return;
    }
    // as faixas cabem no painel: a altura escolhida encolhe até caber (a de baixo ficava fora, sem rolagem)
    let fit = ((area.height() - RULER_H - 2.0 - LABEL_H) / TRACKS.len() as f32 - 3.0).max(30.0);
    let lane_h = [44.0f32, 72.0, 110.0][env.ed.audio_ws.mt.height.min(2)].min(fit);
    let x0 = area.left() + HEADER_W;
    let span_w = (area.right() - x0 - 4.0).max(40.0);
    let pps = if env.ed.audio_ws.mt.pps <= 0.0 { span_w / cut_s as f32 } else { env.ed.audio_ws.mt.pps };
    let content_w = (cut_s as f32 * pps).max(span_w);
    let painter = ui.painter().with_clip_rect(area);

    // ---- régua: timecode + quadros, com o cabeçote do corte
    let ruler = Rect::from_min_max(egui::pos2(x0, area.top()), egui::pos2(x0 + span_w, area.top() + RULER_H));
    painter.rect_filled(ruler, 0.0, t.bg_header);
    // com zoom maior que a largura, a janela acompanha o cabeçote do corte
    let view0 = if content_w > span_w {
        // mantém o cabeçote à vista
        let xh = env.ed.frame as f32 / fps as f32 * pps;
        (xh - span_w * 0.3).clamp(0.0, content_w - span_w)
    } else {
        0.0
    };
    let x_of = |s: f64| x0 + s as f32 * pps - view0;
    let step_s = [0.04, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| s * pps as f64 >= 130.0).unwrap_or(120.0);
    let mut s = (view0 as f64 / pps as f64 / step_s).floor() * step_s;
    while x_of(s) <= ruler.right() {
        let x = x_of(s);
        if x >= ruler.left() {
            painter.vline(x, (ruler.bottom() - 7.0)..=ruler.bottom(), Stroke::new(1.0, t.text_dim));
            let f = (s * fps).round() as u32;
            painter.text(egui::pos2(x + 3.0, ruler.top() + 2.0), egui::Align2::LEFT_TOP, format!("{}  {}{}", fmt_time(s).trim_end_matches('0').trim_end_matches('.'), f, tr("unit.frames_short")), FontId::monospace(9.5), t.text_dim);
        }
        s += step_s;
    }
    let rr = ui.interact(ruler, ui.id().with("mt_ruler"), Sense::click_and_drag());
    env.note("audio:mt:ruler", ruler);
    if let Some(p) = rr.interact_pointer_pos().filter(|_| rr.clicked() || rr.dragged()) {
        let sec = ((p.x - x0 + view0) / pps).max(0.0) as f64;
        env.ed.frame = ((sec * fps) as u32).min(duration.saturating_sub(1));
    }
    painter.text(egui::pos2(area.left() + 4.0, ruler.center().y), egui::Align2::LEFT_CENTER, format!("{} {}  {}", tr("editing.frame_short"), env.ed.frame + 1, fmt_time(env.ed.frame as f64 / fps)), FontId::monospace(10.5), t.text);

    // ---- faixas
    let mut y = ruler.bottom() + 2.0;
    let mut commit: Option<Vec<AudioPlacement>> = None;
    let mut clicked_clip = false;
    for (k, (track, key)) in TRACKS.iter().enumerate() {
        let row = Rect::from_min_max(egui::pos2(area.left(), y), egui::pos2(area.right(), y + lane_h));
        if row.top() > area.bottom() {
            break;
        }
        let head = Rect::from_min_max(row.min, egui::pos2(x0 - 4.0, row.bottom()));
        let lane = Rect::from_min_max(egui::pos2(x0, row.top()), egui::pos2(x0 + span_w, row.bottom()));
        header(env, ui, head, k, key);
        let audible = env.ed.audio_ws.mt.audible(k);
        painter.rect_filled(lane, 2.0, if audible { t.bg_frame } else { t.bg_frame.gamma_multiply(0.6) });
        // fundo da faixa registrado antes dos clipes: os clipes (por cima) recebem o clique
        let lr = ui.interact(lane, ui.id().with(("mt_lane", k)), Sense::click());
        for a in list.iter().filter(|a| a.track == *track) {
            let shown = env.ed.audio_ws.mt.preview.as_ref().filter(|p| p.id == a.id).cloned().unwrap_or_else(|| a.clone());
            let secs = crate::app::panels::audio::clip_seconds(&mut env.ed.audio, env.core, &shown);
            let start = shown.start_frame as f64 / fps;
            let r = Rect::from_min_max(egui::pos2(x_of(start), lane.top() + 2.0), egui::pos2(x_of(start + secs as f64).max(x_of(start) + 4.0), lane.bottom() - 2.0));
            let sel = env.ed.audio.sel.as_ref() == Some(&a.id);
            env.note(format!("audio:mt:clip:{}", a.id.0), r.intersect(lane));
            let base = [Color32::from_rgb(70, 110, 170), Color32::from_rgb(150, 100, 60), Color32::from_rgb(90, 130, 90), Color32::from_rgb(110, 90, 140)][k];
            let fill = if audible { base.gamma_multiply(0.55) } else { base.gamma_multiply(0.25) };
            painter.rect(r, 3.0, fill, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { t.accent } else { base }), StrokeKind::Inside);
            // forma de onda do trecho usado (pirâmide)
            if let Some(l) = env.ed.audio_ws.load(env.core, &shown.asset) {
                let wr = r.shrink2(egui::vec2(1.0, 12.0_f32.min(r.height() * 0.25)));
                let px = wr.width().max(1.0) as usize;
                let rate = l.buf.rate as f64;
                let a0 = (shown.trim_start as f64 * rate) as usize;
                let a1 = a0 + (secs as f64 * rate) as usize;
                let q = l.pyr.query(&l.buf, None, a0, a1.min(l.buf.frames()), px);
                let (mid, half) = (wr.center().y, wr.height() * 0.5);
                let wp = painter.with_clip_rect(wr.intersect(lane));
                let col = Color32::from_rgb(200, 220, 255).gamma_multiply(if audible { 0.85 } else { 0.4 });
                for (i, m) in q.iter().enumerate() {
                    let x = wr.left() + i as f32 + 0.5;
                    wp.vline(x, (mid - m.max.clamp(-1.0, 1.0) * half)..=(mid - m.min.clamp(-1.0, 1.0) * half).max(mid - m.max.clamp(-1.0, 1.0) * half + 1.0), Stroke::new(1.0, col));
                }
            }
            // fades: triângulos com alça no topo
            let fi = r.left() + shown.fade_in * pps;
            let fo = r.right() - shown.fade_out * pps;
            let shade = Color32::from_black_alpha(90);
            if shown.fade_in > 0.0 {
                painter.add(egui::Shape::convex_polygon(vec![r.left_top(), egui::pos2(fi, r.top()), r.left_bottom()], shade, Stroke::NONE));
            }
            if shown.fade_out > 0.0 {
                painter.add(egui::Shape::convex_polygon(vec![egui::pos2(fo, r.top()), r.right_top(), r.right_bottom()], shade, Stroke::NONE));
            }
            let hi = Rect::from_center_size(egui::pos2(fi.max(r.left() + 4.0), r.top() + 4.0), egui::vec2(8.0, 8.0));
            let ho = Rect::from_center_size(egui::pos2(fo.min(r.right() - 4.0), r.top() + 4.0), egui::vec2(8.0, 8.0));
            painter.rect_filled(hi, 1.0, t.text.gamma_multiply(0.8));
            painter.rect_filled(ho, 1.0, t.text.gamma_multiply(0.8));
            let name = node_name(&env.core.project, &shown.asset);
            painter.with_clip_rect(r.shrink(2.0)).text(r.left_bottom() + egui::vec2(4.0, -2.0), egui::Align2::LEFT_BOTTOM, format!("{name}  {:+.1} dB", shown.gain_db), FontId::proportional(10.0), t.text);
            // interação: alças primeiro, depois o corpo
            // a área de clique fica dentro da faixa (clipe mais longo que o corte não invade o resto)
            let rh_in = ui.interact(hi.expand(3.0).intersect(lane), ui.id().with(("mt_fi", &a.id.0)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let rh_out = ui.interact(ho.expand(3.0).intersect(lane), ui.id().with(("mt_fo", &a.id.0)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let body_r = ui.interact(r.intersect(lane), ui.id().with(("mt_clip", &a.id.0)), Sense::click_and_drag());
            for (resp, kind) in [(&rh_in, DragKind::FadeIn), (&rh_out, DragKind::FadeOut), (&body_r, DragKind::Move)] {
                if resp.drag_started() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        env.ed.audio_ws.mt.drag = Some(ClipDrag { id: a.id.clone(), kind, from_x: p.x, orig: a.clone() });
                    }
                }
            }
            if let Some(d) = env.ed.audio_ws.mt.drag.clone().filter(|d| d.id == a.id) {
                let active = rh_in.dragged() || rh_out.dragged() || body_r.dragged();
                let stopped = rh_in.drag_stopped() || rh_out.drag_stopped() || body_r.drag_stopped();
                let px = ui.input(|i| i.pointer.interact_pos()).map(|p| p.x).unwrap_or(d.from_x);
                let dx = (px - d.from_x) / pps;
                let mut n = d.orig.clone();
                match d.kind {
                    DragKind::Move => n.start_frame = ((d.orig.start_frame as f64 + dx as f64 * fps).round().max(0.0) as u32).min(duration.saturating_sub(1)),
                    DragKind::FadeIn => n.fade_in = (d.orig.fade_in + dx).clamp(0.0, secs * 0.95),
                    DragKind::FadeOut => n.fade_out = (d.orig.fade_out - dx).clamp(0.0, secs * 0.95),
                }
                if active {
                    env.ed.audio_ws.mt.preview = Some(n.clone());
                }
                if stopped || !active {
                    env.ed.audio_ws.mt.drag = None;
                    env.ed.audio_ws.mt.preview = None;
                    if n != d.orig {
                        let mut new_list = list.clone();
                        if let Some(x) = new_list.iter_mut().find(|x| x.id == a.id) {
                            *x = n;
                        }
                        commit = Some(new_list);
                    }
                }
            }
            if body_r.clicked() {
                env.ed.audio.sel = Some(a.id.clone());
                clicked_clip = true;
            }
            body_r.on_hover_text(format!("{name}\n{} {:.2} s · {:.2} s · fade {:.2}/{:.2} s", tr("audio.start"), start, secs, a.fade_in, a.fade_out));
        }
        if lr.clicked() && !clicked_clip {
            if let Some(p) = lr.interact_pointer_pos() {
                let sec = ((p.x - x0 + view0) / pps).max(0.0) as f64;
                env.ed.frame = ((sec * fps) as u32).min(duration.saturating_sub(1));
            }
        }
        y = row.bottom() + 3.0;
    }
    // ---- faixa de rótulos: texto das falas (TTS) no início de cada clipe de diálogo
    let lrow = Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x0 + span_w, y + LABEL_H));
    if lrow.bottom() <= area.bottom() {
        painter.text(egui::pos2(area.left() + 4.0, lrow.center().y), egui::Align2::LEFT_CENTER, tr("audio.mt.labels"), FontId::proportional(11.0), t.text_dim);
        painter.rect_filled(lrow, 2.0, t.bg_frame);
        for a in list.iter().filter(|a| a.track == AudioTrack::Dialogue) {
            let params = super::asset_params(&env.core.project, &a.asset);
            let text = params.get("text").and_then(|v| v.as_str()).map(|s| s.to_owned()).unwrap_or_else(|| node_name(&env.core.project, &a.asset));
            let x = x_of(a.start_frame as f64 / fps);
            painter.vline(x, lrow.y_range(), Stroke::new(1.0, t.accent));
            let short: String = text.chars().take(40).collect();
            painter.text(egui::pos2(x + 3.0, lrow.center().y), egui::Align2::LEFT_CENTER, short, FontId::proportional(10.5), t.text);
        }
        y = lrow.bottom();
    }
    // cabeçote do corte por cima de tudo
    let xh = x_of(env.ed.frame as f64 / fps);
    if (x0..=x0 + span_w).contains(&xh) {
        painter.vline(xh, ruler.top()..=y, Stroke::new(1.6, t.accent));
    }
    ui.allocate_rect(area, Sense::hover());
    if let Some(new_list) = commit {
        let r = set_audio(env.core, &cut, &new_list);
        actions::notify_err(env.ed, r);
    }
}

fn header(env: &mut Env, ui: &mut Ui, r: Rect, k: usize, key: &str) {
    let t = env.t;
    ui.painter().rect_filled(r, 3.0, t.bg_header);
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(4.0)));
    // preso ao painel: a faixa de baixo não desenha por cima da barra de status
    c.set_clip_rect(r.intersect(ui.clip_rect()));
    let ui = &mut c;
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr(key)).strong().size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let st = &mut env.ed.audio_ws.mt;
            let s = ui.selectable_label(st.solo[k], egui::RichText::new("S").strong()).on_hover_text(tr("audio.mt.solo"));
            if s.clicked() {
                st.solo[k] = !st.solo[k];
            }
            let m = ui.selectable_label(st.mute[k], egui::RichText::new("M").strong()).on_hover_text(tr("audio.mt.mute"));
            if m.clicked() {
                st.mute[k] = !st.mute[k];
            }
            env.note(format!("audio:mt:solo:{k}"), s.rect);
            env.note(format!("audio:mt:mute:{k}"), m.rect);
        });
    });
    if r.height() >= 40.0 {
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 70.0;
            let st = &mut env.ed.audio_ws.mt;
            let g = ui.add(egui::Slider::new(&mut st.gain[k], -24.0..=12.0).show_value(false).trailing_fill(true)).on_hover_text(format!("{} {:+.1} dB", tr("audio.gain"), st.gain[k]));
            let gr = g.rect;
            ui.label(egui::RichText::new(format!("{:+.0}", st.gain[k])).small().monospace());
            env.note(format!("audio:mt:gain:{k}"), gr);
        });
    }
    if r.height() >= 60.0 {
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 70.0;
            let st = &mut env.ed.audio_ws.mt;
            let p = ui.add(egui::Slider::new(&mut st.pan[k], -1.0..=1.0).show_value(false).trailing_fill(true)).on_hover_text(format!("{} {:+.2}", tr("audio.pan"), st.pan[k]));
            let pr = p.rect;
            ui.label(egui::RichText::new(if st.pan[k].abs() < 0.01 { "C".to_owned() } else if st.pan[k] < 0.0 { format!("L{:.0}", -st.pan[k] * 100.0) } else { format!("R{:.0}", st.pan[k] * 100.0) }).small().monospace());
            env.note(format!("audio:mt:pan:{k}"), pr);
        });
    }
}

fn toolbar(env: &mut Env, ui: &mut Ui, cut: &NodeId, list: &[AudioPlacement], cut_s: f64) {
    let t = env.t;
    let key = format!("mt:{}", cut.0);
    let playing = env.ed.audio_ws.player.is_playing(&key);
    ui.horizontal_wrapped(|ui| {
        let r = env.icon_button(ui, if playing { IconId::Pause } else { IconId::Play }, "audio.mt.play", playing, !list.is_empty(), "audio:mt:play");
        if r.clicked() {
            if playing {
                env.ed.audio_ws.player.stop();
            } else {
                match monitor_mix(env.core, cut, &env.ed.audio_ws.mt) {
                    Ok(m) => {
                        let fps = env.core.project.settings.fps().max(1.0);
                        let n = m.frames();
                        let start = ((env.ed.frame as f64 / fps) * m.rate as f64) as usize;
                        let start = if start + m.rate as usize / 10 >= n { 0 } else { start };
                        // o som ao vivo da linha do tempo não toca junto
                        env.ed.playback.playing = false;
                        env.ed.audio_ws.player.play(PlayRequest { buf: Arc::new(m), start, end: n, looping: false, rack: None, key: key.clone(), label: tr("panel.multitrack.title").to_owned() });
                    }
                    Err(e) => env.ed.notify(e),
                }
            }
        }
        ui.separator();
        let mut h = env.ed.audio_ws.mt.height;
        for (i, k) in ["audio.mt.h_small", "audio.mt.h_mid", "audio.mt.h_big"].iter().enumerate() {
            let r = ui.selectable_value(&mut h, i, tr(k));
            env.note(format!("audio:mt:height:{i}"), r.rect);
        }
        env.ed.audio_ws.mt.height = h;
        ui.separator();
        let mut z = if env.ed.audio_ws.mt.pps <= 0.0 { 0.0 } else { env.ed.audio_ws.mt.pps };
        // 0 = o corte inteiro na largura do painel
        let fit_label = tr("audio.mt.zoom.fit");
        let r = ui.add(egui::DragValue::new(&mut z).range(0.0..=2000.0).speed(2.0).max_decimals(0).prefix(format!("{} ", tr("audio.mt.zoom"))).custom_formatter(move |v, _| if v <= 0.0 { fit_label.to_owned() } else { format!("{v:.0} px/s") }));
        env.note("audio:mt:zoom", r.rect);
        r.on_hover_text(tr("audio.mt.zoom.tip"));
        env.ed.audio_ws.mt.pps = z;
        dim_label(ui, &t, &format!("{} · {}", fmt_time(cut_s), crate::app::panels::edit::project_cuts(&env.core.project).into_iter().find(|c| &c.0 == cut).map(|c| c.1).unwrap_or_default()));
        ui.separator();
        let st = &env.ed.audio_ws.mt;
        let dirty = st.gain.iter().any(|g| g.abs() > 0.01) || st.pan.iter().any(|p| p.abs() > 0.001);
        let b = ui.add_enabled(dirty && !list.is_empty(), egui::Button::new(tr("audio.mt.bake")));
        env.note("audio:mt:bake", b.rect);
        if b.on_hover_text(tr("audio.mt.bake.tip")).clicked() {
            let mut new_list = list.to_vec();
            for a in &mut new_list {
                let k = track_index(a.track);
                a.gain_db = (a.gain_db + env.ed.audio_ws.mt.gain[k]).clamp(-60.0, 12.0);
                a.pan = (a.pan + env.ed.audio_ws.mt.pan[k]).clamp(-1.0, 1.0);
            }
            let r = set_audio(env.core, cut, &new_list);
            if actions::notify_err(env.ed, r).is_some() {
                env.ed.audio_ws.mt.gain = [0.0; 4];
                env.ed.audio_ws.mt.pan = [0.0; 4];
            }
        }
        let any = env.ed.audio_ws.mt.mute.iter().any(|m| *m) || env.ed.audio_ws.mt.solo.iter().any(|s| *s);
        if any {
            dim_label(ui, &t, tr("audio.mt.monitor_only"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_and_mute_pick_the_audible_tracks() {
        let mut st = MtState::default();
        assert!((0..4).all(|k| st.audible(k)));
        st.mute[1] = true;
        assert!(!st.audible(1) && st.audible(0));
        st.solo[2] = true;
        assert!(st.audible(2) && !st.audible(0) && !st.audible(3));
        st.mute[2] = true;
        assert!(!st.audible(2), "mudo vence o solo");
    }

    #[test]
    fn monitor_mix_respects_mute_and_track_gain() {
        let (core, cut, _asset, _pl) = super::super::tests::doc_with_clip();
        let mut st = MtState::default();
        let full = monitor_mix(&core, &cut, &st).unwrap();
        assert!(full.peak() > 0.1, "o clipe toca");
        st.mute[track_index(AudioTrack::Sfx)] = true;
        assert!(monitor_mix(&core, &cut, &st).unwrap().peak() < 1e-6, "SE mudo");
        st.mute = [false; 4];
        st.gain[track_index(AudioTrack::Sfx)] = -6.0;
        let quiet = monitor_mix(&core, &cut, &st).unwrap();
        let ratio = quiet.peak() / full.peak();
        assert!((ratio - 0.501).abs() < 0.05, "-6 dB no barramento: {ratio}");
    }
}

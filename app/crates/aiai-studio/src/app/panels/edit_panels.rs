//! Painéis da Edição (S24 3.15–3.18): monitor de origem (abre um corte, som ou imagem da Mídia, marcas I/O, inserir `,`
//! e sobrescrever `.` nas trilhas de destino, arrastar o trecho para a linha do tempo), Transições (lista com prévia
//! animada; arrastar até um ponto de corte, duplo clique aplica, estrela = padrão do Ctrl+D), Efeitos do clipe
//! (opacidade, velocidade, filtros do `aiai-fx` e rack de áudio do `aiai-audio` do item selecionado, com chaves no
//! cabeçote) e Legendas (várias trilhas S, estilo, lista editável com cps, gerar das falas, importar/exportar SRT/VTT).

use super::edit::{self, Focus, MediaDrag, Play, sync};
use super::edit_timeline::TrDrag;
use super::{commit_on_release, text_commit_multi};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::{Key, KeyTracks};
use aiai_fx::EffectInstance;
use aiai_tools::edit::captions::{self, CaptionIssue};
use aiai_tools::edit::ops::Edge;
use aiai_tools::edit::{CaptionStyle, EditDoc, EditItem, ItemSource, TrackKind, TransitionKind, media_frames};
use egui::{Color32, FontId, Id as EId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};
use std::sync::Arc;

// ------------------------------------------------------------------------------------------------ monitor de origem

/// Duração (quadros) da fonte aberta; imagem e cor valem 3 s.
fn source_len(env: &Env) -> u32 {
    let fps = env.core.project.settings.fps();
    env.ed.edit.source.src.as_ref().map(|s| media_frames(&env.core.project, s).unwrap_or((fps * 3.0).round() as u32)).unwrap_or(1).max(1)
}

/// Som da fonte (estéreo intercalado na taxa do projeto) para tocar no monitor de origem.
fn source_sound(p: &aiai_core::Project, b: &aiai_core::BlobStore, src: &ItemSource) -> Result<Vec<f32>, String> {
    let sr = p.settings.audio_sample_rate.max(8000);
    let buf = match src {
        ItemSource::Cut { cut } | ItemSource::CutAudio { cut, .. } => aiai_tools::audio::mix_cut(p, b, cut, None).map_err(|e| e.to_string())?,
        ItemSource::Audio { asset } => aiai_audio::resample::resample(&aiai_tools::audio::asset_audio(p, b, asset).map_err(|e| e.to_string())?, sr),
        _ => return Ok(Vec::new()),
    };
    let l = buf.ch.first().cloned().unwrap_or_default();
    let r = buf.ch.get(1).cloned().unwrap_or_else(|| l.clone());
    Ok(l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect())
}

/// Liga/desliga a reprodução do monitor de origem (com o som da fonte).
pub(crate) fn toggle_source_play(env: &mut Env, rate: f64) {
    if env.ed.edit.source.play.is_some() && rate == 1.0 {
        env.ed.edit.stop_source();
        return;
    }
    let Some(src) = env.ed.edit.source.src.clone() else { return };
    env.ed.edit.stop_play();
    env.ed.edit.stop_source();
    let len = source_len(env);
    let f = env.ed.edit.source.frame;
    let start = if rate > 0.0 && f + 1 >= len { env.ed.edit.source.mark_in.unwrap_or(0) } else { f };
    let (p, b) = (env.core.project.clone(), env.core.blobs.clone());
    let sound = env.ed.live.enabled();
    env.ed.edit.source.play = Some(Play::start(start, rate, sound, move || source_sound(&p, &b, &src)));
    env.ed.edit.source.frame = start;
    env.ed.edit.focus = Focus::Source;
}

/// Trecho marcado na fonte: (entrada, duração).
fn source_range(env: &Env) -> (u32, u32) {
    let len = source_len(env);
    let s = env.ed.edit.source.mark_in.unwrap_or(0).min(len - 1);
    let e = env.ed.edit.source.mark_out.unwrap_or(len).clamp(s + 1, len);
    (s, e - s)
}

/// Inserir (`,`) ou sobrescrever (`.`) o trecho marcado da fonte no cabeçote do programa, nas trilhas de destino.
pub(crate) fn place_from_source(env: &mut Env, overwrite: bool) {
    let Some(src) = env.ed.edit.source.src.clone() else {
        env.ed.toast = Some(tr("editing.source_empty").to_owned());
        return;
    };
    let (s, dur) = source_range(env);
    let at = env.ed.edit.frame;
    let items = edit::items_for(env, &src, s, dur, at);
    if items.is_empty() {
        env.ed.toast = Some(tr("editing.no_target").to_owned());
        return;
    }
    let mut made = Vec::new();
    edit::modify(env, |d, _| {
        if items.iter().any(|i| d.is_locked(i.track)) {
            return false;
        }
        made = if overwrite { d.overwrite_items(items.clone()) } else { d.insert_items(items.clone(), at) };
        !made.is_empty()
    });
    if !made.is_empty() {
        env.ed.edit.sel = made;
        env.ed.edit.sel_tr = None;
        env.ed.edit.frame = at + dur;
    }
}

/// Picos (0..1) da fonte de som, em `n` colunas.
fn wave_peaks(p: &aiai_core::Project, b: &aiai_core::BlobStore, src: &ItemSource, n: usize) -> Vec<f32> {
    let Ok(st) = source_sound(p, b, src) else { return Vec::new() };
    let frames = st.len() / 2;
    if frames == 0 {
        return Vec::new();
    }
    (0..n)
        .map(|k| {
            let (a, z) = (k * frames / n, ((k + 1) * frames / n).max(k * frames / n + 1).min(frames));
            st[a * 2..z * 2].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        })
        .collect()
}

pub(crate) fn source_monitor(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let _ = sync(env);
    let fps = env.core.project.settings.fps();
    let full = ui.max_rect();
    let focus = env.ed.edit.focus == Focus::Source;
    let Some(src) = env.ed.edit.source.src.clone() else {
        ui.painter().text(full.left_top(), egui::Align2::LEFT_TOP, tr("editing.source"), FontId::proportional(11.0), t.text_dim);
        ui.add_space(28.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr("editing.source_hint")).color(t.text_dim)));
        return;
    };
    let ctx = ui.ctx().clone();
    let len = source_len(env);
    if let Some(p) = env.ed.edit.source.play.as_mut() {
        let last = env.ed.edit.source.mark_out.map(|o| o.saturating_sub(1)).unwrap_or(len - 1);
        match p.tick(fps, env.core.project.settings.audio_sample_rate, last) {
            Some(f) => env.ed.edit.source.frame = f,
            None => env.ed.edit.stop_source(),
        }
        ctx.request_repaint();
    }
    env.ed.edit.source.frame = env.ed.edit.source.frame.min(len - 1);
    let frame = env.ed.edit.source.frame;
    let (mi, mo) = (env.ed.edit.source.mark_in, env.ed.edit.source.mark_out);
    ui.painter().text(
        full.left_top(),
        egui::Align2::LEFT_TOP,
        format!("{}  ·  {}  ·  {}", tr("editing.source"), env.ed.edit.source.name, edit::timecode(frame, fps)),
        FontId::proportional(11.0),
        if focus { t.accent } else { t.text_dim },
    );
    // transporte + botões (que quebram em duas linhas num monitor estreito)
    let bar_h = 88.0;
    let area = Rect::from_min_max(egui::pos2(full.left(), full.top() + 16.0), egui::pos2(full.right(), full.bottom() - bar_h - 2.0));
    let is_sound = matches!(src, ItemSource::Audio { .. });
    let img_rect = if is_sound {
        // som: forma de onda da fonte inteira com o cabeçote
        let painter = ui.painter();
        painter.rect_filled(area, 2.0, t.bg_frame);
        let key = serde_json::to_string(&src).unwrap_or_default();
        if env.ed.edit.source.wave.as_ref().is_none_or(|(k, _)| *k != key) {
            let peaks = wave_peaks(&env.core.project, &env.core.blobs, &src, 400);
            env.ed.edit.source.wave = Some((key, Arc::new(peaks)));
        }
        if let Some((_, peaks)) = &env.ed.edit.source.wave {
            let n = peaks.len().max(1) as f32;
            for (k, v) in peaks.iter().enumerate() {
                let x = area.left() + k as f32 / n * area.width();
                let h = v.min(1.0) * area.height() * 0.45;
                painter.vline(x, (area.center().y - h)..=(area.center().y + h), Stroke::new(1.0, Color32::from_rgb(80, 170, 110)));
            }
        }
        let x = area.left() + (frame as f32 + 0.5) / len as f32 * area.width();
        painter.vline(x, area.y_range(), Stroke::new(1.5, t.accent));
        env.note("edit:source_image", area);
        area
    } else {
        let key = format!("src:{}", serde_json::to_string(&src).unwrap_or_default());
        let (project, blobs, s2) = (env.core.project.clone(), env.core.blobs.clone(), src.clone());
        env.ed.edit.source.preview.request(&ctx, (env.core.change_counter, key, frame), move || {
            move || {
                // a fonte sozinha numa edição de um item (o mesmo render do programa: corte, imagem ou cor)
                let mut d = EditDoc::with_default_tracks();
                let v = d.tracks_of(TrackKind::Video)[0].id;
                d.items.push(EditItem::new(1, v, 0, frame + 1, s2));
                aiai_tools::edit::render_edit_frame(&project, &blobs, &d, frame, false).map_err(|e| e.to_string())
            }
        });
        let tex = env.ed.edit.source.preview.texture.clone();
        edit::paint_monitor(env, ui, area, tex.as_ref(), "edit:source_image")
    };
    if let Some(e) = &env.ed.edit.source.preview.error {
        ui.painter().text(img_rect.center(), egui::Align2::CENTER_CENTER, e, FontId::proportional(12.0), t.accent);
    }
    let r = ui.interact(area, EId::new("edit_source_area"), Sense::click_and_drag());
    if r.clicked() {
        env.ed.edit.focus = Focus::Source;
    }
    // arrastar a imagem do monitor = levar o trecho marcado para a linha do tempo
    if r.drag_started() {
        let (s, dur) = source_range(env);
        egui::DragAndDrop::set_payload(&ctx, MediaDrag { src: src.clone(), src_in: s, dur, label: env.ed.edit.source.name.clone() });
        env.ed.edit.focus = Focus::Source;
    }

    // barra: transporte e régua com as marcas
    let bar = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - bar_h), full.max);
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut f = frame;
    bui.horizontal(|ui| {
        let playing = env.ed.edit.source.play.is_some();
        if env.icon_button(ui, if playing { IconId::Pause } else { IconId::Play }, if playing { "timeline.pause" } else { "timeline.play" }, playing, true, "edit:source_play").clicked() {
            toggle_source_play(env, 1.0);
            f = env.ed.edit.source.frame;
        }
        if env.icon_button(ui, IconId::StepBack, "editing.prev", false, f > 0, "edit:source_prev").clicked() {
            f = f.saturating_sub(1);
        }
        if env.icon_button(ui, IconId::StepForward, "editing.next", false, f + 1 < len, "edit:source_next").clicked() {
            f = (f + 1).min(len - 1);
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(60.0);
        let sr = ui.add(egui::Slider::new(&mut f, 0..=(len - 1).max(1)).show_value(false));
        env.note("edit:source_scrub", sr.rect);
        if sr.changed() {
            env.ed.edit.stop_source();
            env.ed.edit.focus = Focus::Source;
        }
        // marcas sobre o trilho
        let rail = sr.rect;
        let xf = |fr: u32| rail.left() + fr as f32 / len as f32 * rail.width();
        let (a, b) = (mi.unwrap_or(0), mo.unwrap_or(len));
        if mi.is_some() || mo.is_some() {
            ui.painter().rect_filled(Rect::from_min_max(egui::pos2(xf(a), rail.bottom() - 4.0), egui::pos2(xf(b), rail.bottom())), 1.0, t.accent);
        }
        ui.label(egui::RichText::new(format!("{} / {}", edit::fmt_time(f, fps), edit::fmt_time(len, fps))).size(11.5).color(t.text_dim));
    });
    env.ed.edit.source.frame = f.min(len - 1);
    bui.horizontal_wrapped(|ui| {
        let f = env.ed.edit.source.frame;
        let b = ui.button(format!("{} (I)", tr("editing.mark_in")));
        env.note("edit:source_in", b.rect);
        if b.clicked() {
            env.ed.edit.source.mark_in = Some(f);
            if env.ed.edit.source.mark_out.is_some_and(|o| o <= f) {
                env.ed.edit.source.mark_out = None;
            }
            env.ed.edit.focus = Focus::Source;
        }
        let b = ui.button(format!("{} (O)", tr("editing.mark_out")));
        env.note("edit:source_out", b.rect);
        if b.clicked() {
            env.ed.edit.source.mark_out = Some(f + 1);
            if env.ed.edit.source.mark_in.is_some_and(|i| i > f) {
                env.ed.edit.source.mark_in = None;
            }
            env.ed.edit.focus = Focus::Source;
        }
        let b = ui.button(tr("editing.mark_clear"));
        env.note("edit:source_clear", b.rect);
        if b.clicked() {
            env.ed.edit.source.mark_in = None;
            env.ed.edit.source.mark_out = None;
        }
        ui.separator();
        let b = ui.button(format!("{} (,)", tr("editing.insert")));
        env.note("edit:source_insert", b.rect);
        if b.on_hover_text(tr("editing.insert.tip")).clicked() {
            place_from_source(env, false);
        }
        let b = ui.button(format!("{} (.)", tr("editing.overwrite")));
        env.note("edit:source_overwrite", b.rect);
        if b.on_hover_text(tr("editing.overwrite.tip")).clicked() {
            place_from_source(env, true);
        }
        let (_, dur) = source_range(env);
        dim_label(ui, &t, &format!("{:.2} s", dur as f64 / fps));
    });
}

// ------------------------------------------------------------------------------------------------ Transições

/// Miniatura animada da transição (A laranja saindo, B azul entrando) em `r`, no ponto `k` 0..1.
fn paint_transition_glyph(p: &egui::Painter, r: Rect, kind: TransitionKind, k: f32) {
    let a = Color32::from_rgb(220, 140, 60);
    let b = Color32::from_rgb(70, 120, 200);
    let lerp = |c1: Color32, c2: Color32, k: f32| {
        let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
        Color32::from_rgb(m(c1.r(), c2.r()), m(c1.g(), c2.g()), m(c1.b(), c2.b()))
    };
    let (w, h) = (r.width(), r.height());
    let pc = p.with_clip_rect(r);
    match kind {
        TransitionKind::WipeLeft | TransitionKind::WipeRight | TransitionKind::WipeUp | TransitionKind::WipeDown => {
            pc.rect_filled(r, 0.0, a);
            let br = match kind {
                TransitionKind::WipeLeft => Rect::from_min_max(egui::pos2(r.right() - k * w, r.top()), r.max),
                TransitionKind::WipeRight => Rect::from_min_max(r.min, egui::pos2(r.left() + k * w, r.bottom())),
                TransitionKind::WipeUp => Rect::from_min_max(egui::pos2(r.left(), r.bottom() - k * h), r.max),
                _ => Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + k * h)),
            };
            pc.rect_filled(br, 0.0, b);
        }
        TransitionKind::SlideLeft | TransitionKind::SlideRight | TransitionKind::PushLeft | TransitionKind::PushRight => {
            let dir = if matches!(kind, TransitionKind::SlideLeft | TransitionKind::PushLeft) { 1.0 } else { -1.0 };
            let off = (1.0 - k) * w * dir;
            let push = matches!(kind, TransitionKind::PushLeft | TransitionKind::PushRight);
            pc.rect_filled(if push { r.translate(egui::vec2(off - w * dir, 0.0)) } else { r }, 0.0, a);
            pc.rect_filled(r.translate(egui::vec2(off, 0.0)), 0.0, b);
        }
        TransitionKind::Iris => {
            pc.rect_filled(r, 0.0, a);
            pc.circle_filled(r.center(), k * (w * w + h * h).sqrt() / 2.0, b);
        }
        TransitionKind::DipBlack | TransitionKind::DipWhite => {
            let c = if kind == TransitionKind::DipBlack { Color32::BLACK } else { Color32::WHITE };
            pc.rect_filled(r, 0.0, if k < 0.5 { lerp(a, c, k * 2.0) } else { lerp(c, b, (k - 0.5) * 2.0) });
        }
        TransitionKind::FadeBlack | TransitionKind::FadeWhite => {
            let c = if kind == TransitionKind::FadeBlack { Color32::BLACK } else { Color32::WHITE };
            pc.rect_filled(r, 0.0, lerp(c, b, k));
        }
        TransitionKind::Flash => {
            pc.rect_filled(r, 0.0, lerp(Color32::WHITE, b, k * k));
        }
        TransitionKind::Crossfade => {
            pc.rect_filled(r, 0.0, Color32::from_rgb(40, 60, 50));
            pc.line_segment([r.left_top(), r.right_bottom()], Stroke::new(2.0, a));
            pc.line_segment([r.left_bottom(), r.right_top()], Stroke::new(2.0, b));
        }
        _ => {
            pc.rect_filled(r, 0.0, lerp(a, b, k));
        }
    }
}

/// Aplica a transição: à transição selecionada (troca o tipo), ao clipe selecionado (entrada) ou ao ponto de corte no
/// cabeçote na trilha de destino de vídeo.
fn apply_transition(env: &mut Env, kind: TransitionKind) {
    let trn = edit::new_transition(env, kind);
    let (sel_tr, sel, f, vt) = (env.ed.edit.sel_tr, env.ed.edit.sel.first().copied(), env.ed.edit.frame, env.ed.edit.target_v);
    let fps = env.core.project.settings.fps();
    let mut target = None;
    edit::modify(env, |d, _| {
        let (id, edge) = if let Some((i, e)) = sel_tr {
            (i, e)
        } else if let Some(i) = sel.filter(|i| d.item(*i).is_some_and(|x| x.caption_text().is_none())) {
            (i, Edge::In)
        } else {
            let Some(track) = vt else { return false };
            match d.edit_point_near(track, f, (fps * 0.5) as u32) {
                Some((_, Some(b), _)) => (b, Edge::In),
                Some((Some(a), None, _)) => (a, Edge::Out),
                _ => return false,
            }
        };
        let mut t2 = trn.clone();
        if let Some(cur) = sel_tr.and_then(|(i, e)| d.item(i).and_then(|x| if e == Edge::In { x.transition_in.clone() } else { x.transition_out.clone() })) {
            t2.frames = cur.frames;
            t2.align = cur.align;
        }
        target = Some((id, edge));
        d.set_transition(id, edge, Some(t2))
    });
    if target.is_some() {
        env.ed.edit.sel_tr = target;
        env.ed.edit.sel.clear();
    } else {
        env.ed.toast = Some(tr("editing.tr_no_target").to_owned());
    }
}

pub(crate) fn transitions(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let fps = env.core.project.settings.fps();
    let ctx = ui.ctx().clone();
    egui::Grid::new("edit_tr_defaults").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("editing.transition_frames"));
        let mut fr = env.ed.edit.tr_frames;
        let suffix = format!(" {} ({:.2} s)", tr("unit.frames_short"), fr as f64 / fps);
        let r = ui.add(egui::DragValue::new(&mut fr).range(1..=240).speed(0.3).suffix(suffix));
        env.note("edit:tr_frames", r.rect);
        env.ed.edit.tr_frames = fr;
        ui.end_row();
        dim_label(ui, &t, tr("editing.tr_align"));
        ui.horizontal(|ui| {
            for (a, k) in [(aiai_tools::edit::TransitionAlign::Start, "editing.tr_align.start"), (aiai_tools::edit::TransitionAlign::Center, "editing.tr_align.center"), (aiai_tools::edit::TransitionAlign::End, "editing.tr_align.end")] {
                if ui.selectable_label(env.ed.edit.tr_align == a, tr(k)).clicked() {
                    env.ed.edit.tr_align = a;
                }
            }
        });
        ui.end_row();
    });
    dim_label(ui, &t, tr("editing.tr_hint"));
    let time = ctx.input(|i| i.time) as f32;
    egui::ScrollArea::vertical().id_salt("edit_transitions").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for kind in TransitionKind::ALL.into_iter().filter(|k| *k != TransitionKind::Cut) {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click_and_drag());
            env.note(format!("edit:transition:{}", kind.key()), rect);
            let painter = ui.painter().clone();
            if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let g = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 4.0), egui::vec2(38.0, 22.0));
            let k = if resp.hovered() {
                ctx.request_repaint();
                (time * 0.7).fract()
            } else {
                0.5
            };
            paint_transition_glyph(&painter, g, kind, k);
            painter.rect_stroke(g, 2.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
            painter.text(egui::pos2(g.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, tr(&edit::transition_key(kind)), FontId::proportional(12.0), t.text);
            let star = Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), egui::vec2(18.0, 18.0));
            let is_default = env.ed.edit.tr_default == kind;
            if is_default || resp.hovered() {
                painter.text(star.center(), egui::Align2::CENTER_CENTER, if is_default { "★" } else { "☆" }, FontId::proportional(14.0), if is_default { t.accent } else { t.text_dim });
            }
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(&ctx, TrDrag(kind));
            }
            let resp = resp.on_hover_text(tr("editing.tr_item.tip"));
            if resp.clicked() && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| star.contains(p)) {
                env.ed.edit.tr_default = kind;
            } else if resp.double_clicked() {
                apply_transition(env, kind);
            }
        }
    });
    if let Some(p) = egui::DragAndDrop::payload::<TrDrag>(&ctx) {
        // rótulo acompanhando o ponteiro enquanto arrasta
        if let Some(pos) = ctx.pointer_hover_pos() {
            egui::Area::new(EId::new("edit_tr_drag_label")).fixed_pos(pos + egui::vec2(14.0, 8.0)).order(egui::Order::Tooltip).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(tr(&edit::transition_key(p.0)));
                });
            });
        }
    }
}

// ------------------------------------------------------------------------------------------------ Efeitos do clipe

/// Filtros de vídeo que o painel oferece (efeitos do `aiai-fx` com valores de partida).
fn filter_presets() -> Vec<(&'static str, Value)> {
    vec![
        ("editing.fx.grade", json!({"type": "grade"})),
        ("editing.fx.blur", json!({"type": "gaussian_blur", "radius": 4.0})),
        ("editing.fx.dir_blur", json!({"type": "directional_blur", "angle": 0.0, "length": 12.0})),
        ("editing.fx.vignette", json!({"type": "vignette", "amount": 0.4, "softness": 0.5})),
        ("editing.fx.grain", json!({"type": "grain", "amount": 0.06, "size": 1.0})),
        ("editing.fx.diffusion", json!({"type": "diffusion", "radius": 8.0, "amount": 0.35})),
        ("editing.fx.glow", json!({"type": "glow", "threshold": 0.8, "radius": 10.0, "intensity": 0.6})),
        ("editing.fx.chroma", json!({"type": "chromatic_aberration", "amount": 2.0})),
        ("editing.fx.overlay", json!({"type": "color_overlay", "color": [1.0, 0.55, 0.2, 0.25]})),
        ("editing.fx.two_tone", json!({"type": "two_tone", "threshold": 0.5, "dark": [0.08, 0.08, 0.16], "light": [1.0, 0.95, 0.85]})),
        ("editing.fx.invert", json!({"type": "invert"})),
        ("editing.fx.deband", json!({"type": "deband", "amount": 0.5})),
    ]
}

/// Efeitos de áudio que o painel oferece (rack do `aiai-audio`).
fn audio_presets() -> Vec<(&'static str, Value)> {
    vec![
        ("editing.afx.eq", json!({"type": "eq", "bands": [{"kind": "low_shelf", "freq_hz": 120.0, "gain_db": 0.0, "q": 0.7}, {"kind": "peak", "freq_hz": 1000.0, "gain_db": 0.0, "q": 1.0}, {"kind": "high_shelf", "freq_hz": 8000.0, "gain_db": 0.0, "q": 0.7}]})),
        ("editing.afx.compressor", json!({"type": "compressor"})),
        ("editing.afx.limiter", json!({"type": "limiter"})),
        ("editing.afx.highpass", json!({"type": "filter", "kind": "high_pass", "freq_hz": 80.0})),
        ("editing.afx.reverb", json!({"type": "reverb"})),
        ("editing.afx.amplify", json!({"type": "amplify", "gain_db": 0.0})),
    ]
}

/// Editor genérico dos números de um efeito em JSON (folhas numéricas até 3 níveis; cores RGB/RGBA com seletor;
/// booleanos). `keys`/`local`: chaves por campo de primeiro nível (◆ liga/desliga a chave no cabeçote). Devolve o
/// valor mudado (só quando um gesto termina).
fn json_params(env: &mut Env, ui: &mut Ui, salt: &str, v: &Value, keys: Option<(&KeyTracks, u32)>) -> (Option<Value>, Option<KeyTracks>) {
    let t = env.t;
    let mut out: Option<Value> = None;
    let mut out_keys: Option<KeyTracks> = None;
    let Some(obj) = v.as_object() else { return (None, None) };
    egui::Grid::new(("edit_fxp", salt.to_owned())).num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
        for (k, val) in obj {
            if k == "type" || k == "kind" || k == "detector" || k == "mode" || k == "st" {
                continue;
            }
            let pkey = format!("editing.param.{k}");
            let label = if tr(&pkey) == pkey { k.replace('_', " ") } else { tr(&pkey).to_owned() };
            match val {
                Value::Number(n) => {
                    dim_label(ui, &t, &label);
                    let cur = n.as_f64().unwrap_or(0.0) as f32;
                    let keyed = keys.and_then(|(kt, local)| kt.0.get(k).map(|_| kt.eval(k, local as f32).unwrap_or(cur)));
                    let shown = keyed.unwrap_or(cur);
                    if let Some(nv) = commit_on_release(ui, EId::new(("edit_fxn", salt.to_owned(), k.clone())), shown, |ui, x| ui.add(egui::DragValue::new(x).speed(0.01 * shown.abs().max(1.0)).max_decimals(3))) {
                        match keys {
                            Some((kt, local)) if kt.0.contains_key(k) => {
                                let mut nk = kt.clone();
                                set_key(&mut nk, k, local, nv);
                                out_keys = Some(nk);
                            }
                            _ => {
                                let mut o = v.clone();
                                o[k] = json!(nv);
                                out = Some(o);
                            }
                        }
                    }
                    if let Some((kt, local)) = keys {
                        let has = kt.0.get(k).is_some_and(|ks| ks.iter().any(|x| x.frame == local));
                        let on = kt.0.contains_key(k);
                        let b = ui.add(egui::Button::selectable(on, egui::RichText::new(if has { "◆" } else { "◇" }).size(11.0)));
                        if b.on_hover_text(tr("editing.key.tip")).clicked() {
                            let mut nk = kt.clone();
                            if has {
                                if let Some(ks) = nk.0.get_mut(k) {
                                    ks.retain(|x| x.frame != local);
                                    if ks.is_empty() {
                                        nk.0.remove(k);
                                    }
                                }
                            } else {
                                set_key(&mut nk, k, local, shown);
                            }
                            out_keys = Some(nk);
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
                Value::Bool(b) => {
                    dim_label(ui, &t, &label);
                    let mut x = *b;
                    if ui.checkbox(&mut x, "").changed() {
                        let mut o = v.clone();
                        o[k] = json!(x);
                        out = Some(o);
                    }
                    ui.label("");
                    ui.end_row();
                }
                Value::Array(a) if (a.len() == 3 || a.len() == 4) && a.iter().all(Value::is_number) => {
                    dim_label(ui, &t, &label);
                    let f: Vec<f32> = a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
                    if f.len() == 3 {
                        let mut c = [f[0], f[1], f[2]];
                        if ui.color_edit_button_rgb(&mut c).changed() {
                            let mut o = v.clone();
                            o[k] = json!(c);
                            out = Some(o);
                        }
                    } else {
                        let mut c = egui::Rgba::from_rgba_unmultiplied(f[0], f[1], f[2], f[3]);
                        if egui::color_picker::color_edit_button_rgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend).changed() {
                            let u = c.to_rgba_unmultiplied();
                            let mut o = v.clone();
                            o[k] = json!(u);
                            out = Some(o);
                        }
                    }
                    ui.label("");
                    ui.end_row();
                }
                Value::Array(a) => {
                    // bandas do equalizador e afins: um nível abaixo, sem chaves
                    for (i, el) in a.iter().enumerate() {
                        if let Some(eo) = el.as_object() {
                            for (ek, ev) in eo {
                                let Some(n) = ev.as_f64() else { continue };
                                dim_label(ui, &t, &format!("{} {} · {}", label, i + 1, tr(&format!("editing.param.{ek}"))));
                                if let Some(nv) = commit_on_release(ui, EId::new(("edit_fxa", salt.to_owned(), k.clone(), i, ek.clone())), n as f32, |ui, x| ui.add(egui::DragValue::new(x).speed(0.05 * (n.abs() as f32).max(1.0)).max_decimals(2))) {
                                    let mut o = v.clone();
                                    o[k][i][ek] = json!(nv);
                                    out = Some(o);
                                }
                                ui.label("");
                                ui.end_row();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });
    (out, out_keys)
}

/// Põe/atualiza a chave `prop` no quadro `frame` (ordenada).
fn set_key(kt: &mut KeyTracks, prop: &str, frame: u32, value: f32) {
    let ks = kt.0.entry(prop.to_owned()).or_default();
    match ks.iter_mut().find(|k| k.frame == frame) {
        Some(k) => k.value = value,
        None => {
            ks.push(Key { frame, value, ease: Default::default() });
            ks.sort_by_key(|k| k.frame);
        }
    }
}

pub(crate) fn clip_effects(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        dim_label(ui, &t, tr("editing.no_edit_long"));
        return;
    };
    let Some(it) = env.ed.edit.sel.first().and_then(|i| o.doc.item(*i)).cloned() else {
        dim_label(ui, &t, tr("editing.fx_hint"));
        return;
    };
    let local = env.ed.edit.frame.saturating_sub(it.start).min(it.dur.saturating_sub(1));
    let mut next = it.clone();
    let mut changed = false;
    egui::ScrollArea::vertical().id_salt("edit_clip_fx").auto_shrink([false, false]).show(ui, |ui| {
        ui.label(egui::RichText::new(format!("{}  ·  {}", edit::source_label(&env.core.project, &it.source), o.doc.track_label(it.track))).strong());
        dim_label(ui, &t, &format!("{} {}", tr("editing.key_at"), edit::timecode(env.ed.edit.frame, env.core.project.settings.fps())));
        let kind = it.source.track_kind();
        // ---- movimento: opacidade (vídeo, legenda) e velocidade
        section(ui, &t, if kind == TrackKind::Audio { "editing.fx.audio" } else { "editing.fx.motion" });
        let props: Vec<(&str, f32, f32, f32)> = match kind {
            TrackKind::Audio => vec![("volume", it.volume, 0.0, 4.0), ("pan", it.pan, -1.0, 1.0)],
            _ => vec![("opacity", it.opacity, 0.0, 1.0)],
        };
        egui::Grid::new(("edit_motion", it.id)).num_columns(3).spacing([6.0, 4.0]).show(ui, |ui| {
            for (prop, base, lo, hi) in props {
                dim_label(ui, &t, tr(&format!("editing.param.{prop}")));
                let keyed = it.keys.0.contains_key(prop);
                let shown = it.keyed(prop, base, local as f32);
                if let Some(v) = commit_on_release(ui, EId::new(("edit_mprop", it.id, prop)), shown, |ui, x| ui.add(egui::Slider::new(x, lo..=hi).max_decimals(2))) {
                    if keyed {
                        // a chave guarda o fator sobre o valor base (o balanço guarda o valor)
                        let kv = if prop == "pan" { v } else if base > 0.0 { v / base } else { v };
                        set_key(&mut next.keys, prop, local, kv);
                    } else {
                        match prop {
                            "volume" => next.volume = v,
                            "pan" => next.pan = v,
                            _ => next.opacity = v,
                        }
                    }
                    changed = true;
                }
                let has = it.keys.0.get(prop).is_some_and(|ks| ks.iter().any(|k| k.frame == local));
                let b = ui.add(egui::Button::selectable(keyed, egui::RichText::new(if has { "◆" } else { "◇" }).size(11.0)));
                env.note(format!("edit:fx:key:{prop}"), b.rect);
                if b.on_hover_text(tr("editing.key.tip")).clicked() {
                    if has {
                        if let Some(ks) = next.keys.0.get_mut(prop) {
                            ks.retain(|k| k.frame != local);
                            if ks.is_empty() {
                                next.keys.0.remove(prop);
                            }
                        }
                    } else {
                        let kv = if prop == "pan" { shown } else if base > 0.0 { shown / base } else { 1.0 };
                        set_key(&mut next.keys, prop, local, kv);
                    }
                    changed = true;
                }
                ui.end_row();
            }
            if kind != TrackKind::Subtitle {
                dim_label(ui, &t, tr("editing.param.speed"));
                if let Some(v) = commit_on_release(ui, EId::new(("edit_speed", it.id)), it.speed * 100.0, |ui, x| ui.add(egui::DragValue::new(x).range(10.0..=1000.0).speed(1.0).suffix(" %"))) {
                    // mesma mídia em outra velocidade: a duração muda (para no vizinho)
                    let s = (v / 100.0).clamp(0.1, 10.0);
                    let src_len = it.dur as f32 * it.speed;
                    next.speed = s;
                    next.dur = ((src_len / s).round() as u32).max(1);
                    changed = true;
                }
                ui.label("");
                ui.end_row();
            }
        });
        // ---- filtros de vídeo (aiai-fx)
        if kind == TrackKind::Video {
            section(ui, &t, "editing.fx.filters");
            let mut remove = None;
            let mut swap = None;
            for (k, f) in it.filters.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut en = f.enabled;
                    if ui.checkbox(&mut en, "").changed() {
                        next.filters[k].enabled = en;
                        changed = true;
                    }
                    ui.label(egui::RichText::new(tr(&format!("editing.fxname.{}", f.effect.name()))).strong());
                    if env.icon_button(ui, IconId::ArrowUp, "editing.fx_up", false, k > 0, &format!("edit:fx:{k}:up")).clicked() {
                        swap = Some((k, k - 1));
                    }
                    if env.icon_button(ui, IconId::ArrowDown, "editing.fx_down", false, k + 1 < it.filters.len(), &format!("edit:fx:{k}:down")).clicked() {
                        swap = Some((k, k + 1));
                    }
                    if env.icon_button(ui, IconId::Trash, "editing.fx_delete", false, true, &format!("edit:fx:{k}:delete")).clicked() {
                        remove = Some(k);
                    }
                });
                let v = serde_json::to_value(&f.effect).unwrap_or(Value::Null);
                let (nv, nk) = json_params(env, ui, &format!("{}:{k}", it.id), &v, Some((&f.keys, local)));
                if let Some(e) = nv.and_then(|x| serde_json::from_value(x).ok()) {
                    next.filters[k].effect = e;
                    changed = true;
                }
                if let Some(keys) = nk {
                    next.filters[k].keys = keys;
                    changed = true;
                }
                ui.add_space(4.0);
            }
            if let Some(k) = remove {
                next.filters.remove(k);
                changed = true;
            }
            if let Some((a, b)) = swap {
                next.filters.swap(a, b);
                changed = true;
            }
            let r = egui::ComboBox::from_id_salt(("edit_add_fx", it.id)).selected_text(tr("editing.fx_add")).width(170.0).show_ui(ui, |ui| {
                for (key, v) in filter_presets() {
                    if ui.selectable_label(false, tr(key)).clicked() {
                        if let Ok(effect) = serde_json::from_value(v) {
                            next.filters.push(EffectInstance { effect, enabled: true, posterize_time: 1, keys: KeyTracks::default() });
                            changed = true;
                        }
                    }
                }
            });
            env.note("edit:fx_add", r.response.rect);
        }
        // ---- rack de áudio (aiai-audio)
        if kind == TrackKind::Audio {
            section(ui, &t, "editing.fx.rack");
            let mut remove = None;
            for (k, slot) in it.audio_fx.0.slots.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut on = !slot.bypass;
                    if ui.checkbox(&mut on, "").changed() {
                        next.audio_fx.0.slots[k].bypass = !on;
                        changed = true;
                    }
                    ui.label(egui::RichText::new(tr(&format!("editing.afxname.{}", slot.fx.name()))).strong());
                    if env.icon_button(ui, IconId::Trash, "editing.fx_delete", false, true, &format!("edit:afx:{k}:delete")).clicked() {
                        remove = Some(k);
                    }
                });
                let v = serde_json::to_value(&slot.fx).unwrap_or(Value::Null);
                let (nv, _) = json_params(env, ui, &format!("a{}:{k}", it.id), &v, None);
                if let Some(fx) = nv.and_then(|x| serde_json::from_value(x).ok()) {
                    next.audio_fx.0.slots[k].fx = fx;
                    changed = true;
                }
            }
            if let Some(k) = remove {
                next.audio_fx.0.slots.remove(k);
                changed = true;
            }
            let r = egui::ComboBox::from_id_salt(("edit_add_afx", it.id)).selected_text(tr("editing.fx_add")).width(170.0).show_ui(ui, |ui| {
                for (key, v) in audio_presets() {
                    if ui.selectable_label(false, tr(key)).clicked() {
                        if let Ok(fx) = serde_json::from_value::<aiai_audio::fx::AudioFx>(v) {
                            next.audio_fx.0.push(fx);
                            changed = true;
                        }
                    }
                }
            });
            env.note("edit:afx_add", r.response.rect);
        }
    });
    // trilha travada: os efeitos e a opacidade do clipe também ficam travados (auditoria M-03)
    if changed && o.doc.is_locked(it.track) {
        env.ed.toast = Some(tr("editing.track_locked").to_owned());
        changed = false;
    }
    if changed {
        let mut d = (*o.doc).clone();
        if let Some(x) = d.item_mut(it.id) {
            *x = next;
        }
        // velocidade nova que bateria no vizinho: avisa e não grava
        if let Err(e) = d.validate(&env.core.project) {
            env.ed.toast = Some(e.to_string());
        } else {
            edit::save(env, &o.id, &d);
        }
    }
}

// ------------------------------------------------------------------------------------------------ Legendas

/// Legenda nova no cabeçote, na trilha S aberta (2 s ou até a próxima).
pub(crate) fn add_caption_at_playhead(env: &mut Env) {
    let Some(track) = env.ed.edit.cap_track else { return };
    let f = env.ed.edit.frame;
    let fps = env.core.project.settings.fps();
    let mut made = None;
    edit::modify(env, |d, _| {
        if d.is_locked(track) {
            return false;
        }
        let next = d.items_on(track).iter().filter(|i| i.start > f).map(|i| i.start).min().unwrap_or(u32::MAX);
        let dur = ((fps * 2.0).round() as u32).min(next.saturating_sub(f)).max(1);
        d.clear_range(track, f, f + dur, &[]);
        let id = d.next_item_id();
        d.items.push(EditItem::new(id, track, f, dur, ItemSource::Caption { text: tr("editing.new_subtitle").to_owned(), speaker: None, style: None }));
        made = Some(id);
        true
    });
    if let Some(id) = made {
        env.ed.edit.sel = vec![id];
        env.ed.edit.sel_tr = None;
    }
}

fn poll_file_job(env: &mut Env, o: &edit::Opened) {
    let Some((job, rx)) = env.ed.edit.file_job.as_ref() else { return };
    let job = *job;
    let Ok(res) = rx.try_recv() else { return };
    env.ed.edit.file_job = None;
    let Some(path) = res else { return };
    let Some(track) = env.ed.edit.cap_track else { return };
    let fps = env.core.project.settings.fps();
    match job {
        edit::FileJob::ImportCaptions => {
            let r = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| captions::parse_captions(&t));
            match r {
                Ok(subs) => {
                    let mut d = (*o.doc).clone();
                    match captions::import_into(&mut d, track, &subs, fps, false) {
                        Ok(ids) => {
                            if edit::save(env, &o.id, &d) {
                                env.ed.toast = Some(format!("{} {}", ids.len(), tr("editing.captions_imported")));
                            }
                        }
                        Err(e) => env.ed.toast = Some(e),
                    }
                }
                Err(e) => env.ed.toast = Some(e),
            }
        }
        edit::FileJob::ExportSrt | edit::FileJob::ExportVtt => {
            let subs = captions::track_subtitles(&o.doc, track, fps);
            let text = if job == edit::FileJob::ExportSrt { captions::write_srt(&subs) } else { captions::write_vtt(&subs) };
            env.ed.toast = Some(match std::fs::write(&path, text) {
                Ok(()) => format!("{} {}", tr("editing.captions_exported"), path.display()),
                Err(e) => e.to_string(),
            });
        }
    }
}

fn start_file_job(env: &mut Env, job: edit::FileJob) {
    if env.ed.edit.file_job.is_some() {
        return;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let start = env.core.path.clone();
    let _ = std::thread::Builder::new().name("aiai-captions-file".into()).spawn(move || {
        let mut d = rfd::FileDialog::new();
        if let Some(p) = start {
            d = d.set_directory(p);
        }
        let r = match job {
            edit::FileJob::ImportCaptions => d.add_filter("SRT/VTT", &["srt", "vtt"]).pick_file(),
            edit::FileJob::ExportSrt => d.add_filter("SRT", &["srt"]).set_file_name("legendas.srt").save_file(),
            edit::FileJob::ExportVtt => d.add_filter("WebVTT", &["vtt"]).set_file_name("legendas.vtt").save_file(),
        };
        let _ = tx.send(r);
    });
    env.ed.edit.file_job = Some((job, rx));
}

pub(crate) fn captions_panel(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        dim_label(ui, &t, tr("editing.no_edit_long"));
        return;
    };
    poll_file_job(env, &o);
    if env.ed.edit.file_job.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let fps = env.core.project.settings.fps();
    let tracks: Vec<(u32, String)> = o.doc.tracks_of(TrackKind::Subtitle).iter().map(|x| (x.id, if x.lang.is_empty() { o.doc.track_label(x.id) } else { format!("{} · {}", o.doc.track_label(x.id), x.lang) })).collect();
    let Some(track) = env.ed.edit.cap_track else {
        dim_label(ui, &t, tr("editing.cap_no_track"));
        return;
    };
    let Some(trk) = o.doc.track(track).cloned() else { return };
    // ---- trilha: escolha, idioma, queimar, ver
    ui.horizontal_wrapped(|ui| {
        let cur = tracks.iter().find(|x| x.0 == track).map(|x| x.1.clone()).unwrap_or_default();
        let r = egui::ComboBox::from_id_salt("edit_cap_track").width(110.0).selected_text(cur).show_ui(ui, |ui| {
            for (id, name) in &tracks {
                if ui.selectable_label(*id == track, name).clicked() {
                    env.ed.edit.cap_track = Some(*id);
                }
            }
        });
        env.note("edit:cap_track", r.response.rect);
        let b = ui.button(tr("editing.add_track_s"));
        env.note("edit:cap_add_track", b.rect);
        if b.on_hover_text(tr("editing.add_track.tip")).clicked() {
            let mut d = (*o.doc).clone();
            let id = d.add_track(TrackKind::Subtitle);
            if edit::save(env, &o.id, &d) {
                env.ed.edit.cap_track = Some(id);
            }
        }
        dim_label(ui, &t, tr("editing.cap_lang"));
        let (r, c) = super::text_commit_opt(ui, EId::new(("edit_cap_lang", track)), &trk.lang, 44.0, "pt");
        env.note("edit:cap_lang", r.rect);
        let mut patch: Option<aiai_tools::edit::EditTrack> = c.map(|l| aiai_tools::edit::EditTrack { lang: l, ..trk.clone() });
        let mut burn = trk.burn;
        if ui.checkbox(&mut burn, tr("editing.cap_burn")).on_hover_text(tr("editing.track_burn.tip")).changed() {
            patch = Some(aiai_tools::edit::EditTrack { burn, ..trk.clone() });
        }
        let mut show = !trk.hidden;
        if ui.checkbox(&mut show, tr("editing.cap_show")).changed() {
            patch = Some(aiai_tools::edit::EditTrack { hidden: !show, ..trk.clone() });
        }
        if let Some(p) = patch {
            let mut d = (*o.doc).clone();
            if let Some(x) = d.track_mut(track) {
                *x = p;
            }
            edit::save(env, &o.id, &d);
        }
    });
    // ---- ações
    ui.horizontal_wrapped(|ui| {
        let b = ui.button(format!("{} (T)", tr("editing.cap_add")));
        env.note("edit:cap_add", b.rect);
        if b.on_hover_text(tr("editing.add_subtitle.tip")).clicked() {
            add_caption_at_playhead(env);
        }
        let b = ui.button(tr("editing.cap_generate"));
        env.note("edit:cap_generate", b.rect);
        if b.on_hover_text(tr("editing.cap_generate.tip")).clicked() {
            match captions::captions_from_script(&env.core.project, &o.doc, track, &trk.lang) {
                Ok(items) if items.is_empty() => env.ed.toast = Some(tr("editing.cap_generate_none").to_owned()),
                Ok(items) => {
                    let n = items.len();
                    let mut d = (*o.doc).clone();
                    d.overwrite_items(items);
                    if edit::save(env, &o.id, &d) {
                        env.ed.toast = Some(format!("{n} {}", tr("editing.captions_generated")));
                    }
                }
                Err(e) => env.ed.toast = Some(e.to_string()),
            }
        }
        let b = ui.button(tr("editing.cap_import"));
        env.note("edit:cap_import", b.rect);
        if b.clicked() {
            start_file_job(env, edit::FileJob::ImportCaptions);
        }
        let b = ui.button("SRT…");
        env.note("edit:cap_export_srt", b.rect);
        if b.on_hover_text(tr("editing.cap_export.tip")).clicked() {
            start_file_job(env, edit::FileJob::ExportSrt);
        }
        let b = ui.button("VTT…");
        env.note("edit:cap_export_vtt", b.rect);
        if b.on_hover_text(tr("editing.cap_export.tip")).clicked() {
            start_file_job(env, edit::FileJob::ExportVtt);
        }
    });
    // ---- estilo da trilha
    let style = trk.style.clone().unwrap_or_default();
    let mut ns = style.clone();
    egui::CollapsingHeader::new(tr("editing.cap_style")).id_salt(("edit_cap_style", track)).show(ui, |ui| {
        egui::Grid::new(("edit_cap_style_g", track)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let pct = |ui: &mut Ui, key: &str, id: &str, v: f32, lo: f32, hi: f32| -> Option<f32> {
                dim_label(ui, &t, tr(key));
                let r = commit_on_release(ui, EId::new(("edit_cs", track, id.to_owned())), v * 100.0, |ui, x| ui.add(egui::Slider::new(x, lo..=hi).max_decimals(1).suffix(" %")));
                ui.end_row();
                r.map(|x| x / 100.0)
            };
            if let Some(v) = pct(ui, "editing.cap_size", "size", style.size, 2.0, 20.0) {
                ns.size = v;
            }
            if let Some(v) = pct(ui, "editing.cap_y", "y", style.y, 5.0, 98.0) {
                ns.y = v;
            }
            if let Some(v) = pct(ui, "editing.cap_width", "w", style.max_width, 30.0, 100.0) {
                ns.max_width = v;
            }
            dim_label(ui, &t, tr("editing.cap_color"));
            let mut c = egui::Rgba::from_rgba_unmultiplied(style.color[0], style.color[1], style.color[2], style.color[3]);
            if egui::color_picker::color_edit_button_rgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend).changed() {
                ns.color = c.to_rgba_unmultiplied();
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.cap_outline"));
            ui.horizontal(|ui| {
                let mut on = style.outline.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    ns.outline = on.then_some([0.04, 0.04, 0.07, 1.0]);
                }
                if let Some(oc) = style.outline {
                    let mut c = egui::Rgba::from_rgba_unmultiplied(oc[0], oc[1], oc[2], oc[3]);
                    if egui::color_picker::color_edit_button_rgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend).changed() {
                        ns.outline = Some(c.to_rgba_unmultiplied());
                    }
                }
            });
            ui.end_row();
            dim_label(ui, &t, tr("editing.cap_box"));
            ui.horizontal(|ui| {
                let mut on = style.background.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    ns.background = on.then_some([0.0, 0.0, 0.0, 0.55]);
                }
                if let Some(bc) = style.background {
                    let mut c = egui::Rgba::from_rgba_unmultiplied(bc[0], bc[1], bc[2], bc[3]);
                    if egui::color_picker::color_edit_button_rgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend).changed() {
                        ns.background = Some(c.to_rgba_unmultiplied());
                    }
                }
            });
            ui.end_row();
        });
    });
    if ns != style {
        let mut d = (*o.doc).clone();
        if let Some(x) = d.track_mut(track) {
            x.style = (ns != CaptionStyle::default()).then_some(ns);
        }
        edit::save(env, &o.id, &d);
    }
    // ---- verificação: limites e resumo
    let lim = env.ed.edit.cap_limits;
    let checks = captions::check_captions(&o.doc, Some(track), fps, &lim);
    let bad = checks.iter().filter(|c| !c.issues.is_empty()).count();
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("editing.cap_max_cps"));
        ui.add(egui::DragValue::new(&mut env.ed.edit.cap_limits.max_cps).range(5.0..=40.0).speed(0.1));
        dim_label(ui, &t, tr("editing.cap_max_cpl"));
        ui.add(egui::DragValue::new(&mut env.ed.edit.cap_limits.max_chars_per_line).range(10..=80));
        let txt = if bad == 0 { tr("editing.cap_check_ok").to_owned() } else { format!("{bad} {}", tr("editing.cap_check_bad")) };
        ui.label(egui::RichText::new(txt).size(11.5).color(if bad == 0 { t.st_ok } else { t.danger }));
    });
    // ---- lista
    let items: Vec<EditItem> = o.doc.items_on(track).into_iter().cloned().collect();
    let mut doc_next: Option<EditDoc> = None;
    let sel_id = env.ed.edit.sel.first().copied();
    egui::ScrollArea::vertical().id_salt(("edit_cap_list", track)).auto_shrink([false, false]).show(ui, |ui| {
        if items.is_empty() {
            dim_label(ui, &t, tr("editing.cap_empty"));
        }
        for (n, it) in items.iter().enumerate() {
            let check = checks.iter().find(|c| c.item == it.id);
            let issues: Vec<&CaptionIssue> = check.map(|c| c.issues.iter().collect()).unwrap_or_default();
            let sel = sel_id == Some(it.id);
            let frame = egui::Frame::new().fill(if sel { t.accent_dim } else { t.bg_widget }).corner_radius(4.0).inner_margin(egui::Margin::same(4));
            frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let head = ui.add(egui::Label::new(egui::RichText::new(format!("{}  {} → {}", n + 1, edit::timecode(it.start, fps), edit::timecode(it.end(), fps))).size(11.0).color(t.text_dim)).sense(Sense::click()));
                        env.note(format!("edit:cap:{n}"), head.rect);
                        if head.clicked() {
                            env.ed.edit.sel = vec![it.id];
                            env.ed.edit.sel_tr = None;
                            env.ed.edit.frame = it.start;
                            env.ed.edit.stop_play();
                        }
                        if let Some(c) = check {
                            let col = if issues.is_empty() { t.text_dim } else { t.danger };
                            let tip = issues.iter().map(|i| tr(&format!("editing.cap_issue.{}", serde_json::to_value(i).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default())).to_owned()).collect::<Vec<_>>().join("\n");
                            let l = ui.label(egui::RichText::new(format!("{:.0} cps", c.cps.min(999.0))).size(11.0).color(col));
                            if !tip.is_empty() {
                                l.on_hover_text(tip);
                            }
                        }
                        if it.anchor.is_some() {
                            ui.label(egui::RichText::new("⚓").size(11.0)).on_hover_text(tr("editing.anchor.tip"));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if env.icon_button(ui, IconId::Trash, "editing.sub_delete", false, true, &format!("edit:cap:{n}:delete")).clicked() {
                                let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                                d.delete_items(&[it.id], false);
                                doc_next = Some(d);
                            }
                        });
                    });
                    let text = it.caption_text().unwrap_or("").to_owned();
                    let (tr_, c) = text_commit_multi(ui, EId::new(("edit_cap_text", it.id)), &text, ui.available_width(), "");
                    env.note(format!("edit:cap:{n}:text"), tr_.rect);
                    if let Some(x) = c.filter(|x| !x.trim().is_empty()) {
                        let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                        if let Some(ItemSource::Caption { text, .. }) = d.item_mut(it.id).map(|i| &mut i.source) {
                            *text = x;
                        }
                        doc_next = Some(d);
                    }
                    if sel {
                        // tempo, falante e âncora da legenda escolhida
                        ui.horizontal_wrapped(|ui| {
                            dim_label(ui, &t, tr("editing.sub_start"));
                            if let Some(v) = commit_on_release(ui, EId::new(("edit_cap_s", it.id)), it.start, |ui, x| ui.add(egui::DragValue::new(x).speed(0.3))) {
                                let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                                let _ = d.move_items(&[it.id], TrackKind::Subtitle, v as i64 - it.start as i64, 0);
                                doc_next = Some(d);
                            }
                            dim_label(ui, &t, tr("editing.duration"));
                            if let Some(v) = commit_on_release(ui, EId::new(("edit_cap_d", it.id)), it.dur, |ui, x| ui.add(egui::DragValue::new(x).range(1..=10_000).speed(0.3))) {
                                let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                                d.trim(&[it.id], Edge::Out, v as i64 - it.dur as i64, false, &aiai_tools::edit::ops::no_media_limit);
                                doc_next = Some(d);
                            }
                            let speaker = match &it.source {
                                ItemSource::Caption { speaker, .. } => speaker.clone().unwrap_or_default(),
                                _ => String::new(),
                            };
                            dim_label(ui, &t, tr("editing.cap_speaker"));
                            let (_, c) = super::text_commit_opt(ui, EId::new(("edit_cap_spk", it.id)), &speaker, 80.0, "");
                            if let Some(x) = c {
                                let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                                if let Some(ItemSource::Caption { speaker, .. }) = d.item_mut(it.id).map(|i| &mut i.source) {
                                    *speaker = (!x.is_empty()).then_some(x);
                                }
                                doc_next = Some(d);
                            }
                            // prender ao clipe de imagem sob o início (ou soltar)
                            let label = if it.anchor.is_some() { tr("editing.anchor_release") } else { tr("editing.anchor_attach") };
                            let b = ui.button(label);
                            env.note("edit:cap_anchor", b.rect);
                            if b.on_hover_text(tr("editing.anchor.tip")).clicked() {
                                let mut d = doc_next.take().unwrap_or_else(|| (*o.doc).clone());
                                let under = d.items.iter().filter(|i| i.source.track_kind() == TrackKind::Video && i.start <= it.start && it.start < i.end()).max_by_key(|i| d.tracks.iter().position(|t| t.id == i.track)).map(|i| i.id);
                                if let Some(x) = d.item_mut(it.id) {
                                    x.anchor = if it.anchor.is_some() { None } else { under };
                                }
                                doc_next = Some(d);
                            }
                        });
                    }
            });
        }
    });
    if let Some(d) = doc_next {
        edit::save(env, &o.id, &d);
    }
}

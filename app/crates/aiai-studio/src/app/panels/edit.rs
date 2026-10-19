//! Workspace Edição: painel Mídia (documento de edição aberto, cortes do projeto para montar), Prévia (o quadro da edição
//! como o MP4 vai mostrar: transição, legenda queimada, créditos; renderizado em segundo plano), Linha do tempo da edição
//! (clipes proporcionais à duração, transições, faixa de legendas, créditos, cabeçote) e, no painel Propriedades, o clipe
//! ou a legenda selecionada e os créditos.
//!
//! Toda mudança regrava o documento inteiro por `EDIT_SET` (a mesma ação do MCP; um passo de desfazer por gesto). A edição
//! é validada pelo motor antes de gravar (trecho vazio, corte inexistente): o erro volta como aviso, o documento não muda.

use super::commit_on_release;
use super::production::{CutThumbs, THUMB_BUDGET};
use crate::app::actions::{self, DocKind};
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::{CoreSession, Id, NodeKind, Project};
use aiai_tools::edit::{EditClip, EditDoc, SubtitleEntry, Transition, TransitionKind, load_edit};
use egui::{Color32, FontId, Id as EId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const TRANSITIONS: [(TransitionKind, &str); 7] = [
    (TransitionKind::Cut, "editing.tr.cut"),
    (TransitionKind::FadeBlack, "editing.tr.fade_black"),
    (TransitionKind::FadeWhite, "editing.tr.fade_white"),
    (TransitionKind::Dissolve, "editing.tr.dissolve"),
    (TransitionKind::WipeLeft, "editing.tr.wipe_left"),
    (TransitionKind::WipeRight, "editing.tr.wipe_right"),
    (TransitionKind::Flash, "editing.tr.flash"),
];

pub(crate) fn transition_key(k: TransitionKind) -> &'static str {
    TRANSITIONS.iter().find(|(x, _)| *x == k).map(|(_, s)| *s).unwrap_or("editing.tr.cut")
}

/// Posição de cada clipe na linha do tempo (início global, duração, deslocamento no corte), créditos e total.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Layout {
    pub items: Vec<(u32, u32, u32)>,
    pub credits_start: u32,
    pub total: u32,
}

/// Documento de edição decodificado + linha do tempo, por `change_counter` (o erro de validação fica guardado como texto).
pub(crate) struct Opened {
    pub id: NodeId,
    pub doc: Arc<EditDoc>,
    pub layout: Result<Layout, String>,
}

/// Prévia de um quadro da edição em segundo plano (um render por vez; o pedido mais novo vence).
#[derive(Default)]
pub(crate) struct EditPreview {
    /// (change_counter, edição, quadro) do último render pedido.
    asked: Option<(u64, String, u32)>,
    job: Option<crossbeam_channel::Receiver<(u32, Result<(u32, u32, Vec<u8>), String>)>>,
    pub texture: Option<egui::TextureHandle>,
    pub shown_frame: Option<u32>,
    pub error: Option<String>,
}

impl EditPreview {
    pub(crate) fn update(&mut self, ctx: &egui::Context, core: &CoreSession, edit: &NodeId, doc: &Arc<EditDoc>, frame: u32) {
        self.request(ctx, (core.change_counter, edit.0.clone(), frame), || {
            let (project, blobs, doc) = (core.project.clone(), core.blobs.clone(), doc.clone());
            move || aiai_tools::edit::render_edit_frame(&project, &blobs, &doc, frame, true).map_err(|e| e.to_string())
        });
    }

    /// Recolhe o render pronto e, se `key` (change_counter, documento, quadro) mudou, pede outro com `render` (um por vez;
    /// o pedido mais novo vence). `make` (que copia o projeto para a thread) só roda quando há render novo a fazer.
    pub(crate) fn request<F>(&mut self, ctx: &egui::Context, key: (u64, String, u32), make: impl FnOnce() -> F)
    where
        F: FnOnce() -> Result<aiai_paint::Image, String> + Send + 'static,
    {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok((f, res)) => {
                    self.job = None;
                    match res {
                        Ok((w, h, rgba)) => {
                            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                            match &mut self.texture {
                                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                                None => self.texture = Some(ctx.load_texture("edit-preview", img, egui::TextureOptions::LINEAR)),
                            }
                            self.shown_frame = Some(f);
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                    ctx.request_repaint();
                }
                Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(30)),
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.job = None,
            }
        }
        if self.job.is_some() || self.asked.as_ref() == Some(&key) {
            return;
        }
        let frame = key.2;
        self.asked = Some(key);
        let render = make();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new().name("aiai-preview".into()).spawn(move || {
            let out = render().map(|img| (img.width, img.height, img.to_rgba8()));
            let _ = tx.send((frame, out));
        });
        if spawned.is_ok() {
            self.job = Some(rx);
        }
    }
}

/// Estado do workspace Edição que não mora no documento.
#[derive(Default)]
pub(crate) struct EditState {
    pub sel_edit: Option<NodeId>,
    /// Cabeçote (quadro global da edição).
    pub frame: u32,
    pub sel_clip: Option<usize>,
    pub sel_sub: Option<usize>,
    cache: Option<(u64, NodeId, Arc<EditDoc>, Result<Layout, String>)>,
    pub preview: EditPreview,
    pub thumbs: CutThumbs,
    /// Pixels por quadro na linha do tempo (`None` = ajustar à largura).
    pub zoom: Option<f32>,
}

/// Abre/corrige o documento de edição selecionado e devolve-o com a linha do tempo.
pub(crate) fn sync(env: &mut Env) -> Option<Opened> {
    let docs = actions::docs_of(&env.core.project, DocKind::Edit);
    let st = &mut env.ed.edit;
    let id = match &st.sel_edit {
        Some(c) if docs.iter().any(|(i, _)| i == c) => c.clone(),
        _ => {
            let first = docs.first()?.0.clone();
            st.sel_edit = Some(first.clone());
            st.sel_clip = None;
            st.sel_sub = None;
            first
        }
    };
    let core = &*env.core;
    let fresh = st.cache.as_ref().is_some_and(|(c, i, _, _)| *c == core.change_counter && *i == id);
    if !fresh {
        let doc = Arc::new(load_edit(&core.project, &id).unwrap_or_default());
        let fps = core.project.settings.fps() as f32;
        let layout = doc.timeline(&core.project, fps).map(|t| Layout { items: t.items, credits_start: t.credits_start, total: t.total }).map_err(|e| e.to_string());
        st.cache = Some((core.change_counter, id.clone(), doc, layout));
    }
    let (_, _, doc, layout) = st.cache.as_ref()?;
    let (doc, layout) = (doc.clone(), layout.clone());
    if st.sel_clip.is_some_and(|k| k >= doc.clips.len()) {
        st.sel_clip = None;
    }
    if st.sel_sub.is_some_and(|k| k >= doc.subtitles.len()) {
        st.sel_sub = None;
    }
    if let Ok(l) = &layout {
        st.frame = st.frame.min(l.total.saturating_sub(1));
    }
    Some(Opened { id, doc, layout })
}

/// Regrava a edição (`EDIT_SET` com `id`): um passo de desfazer.
pub(crate) fn save(env: &mut Env, id: &NodeId, doc: &EditDoc) -> bool {
    let mut params = serde_json::to_value(doc).unwrap_or(Value::Null);
    if let Some(o) = params.as_object_mut() {
        o.insert("id".into(), json!(id));
    }
    let r = actions::run(env.core, "EDIT_SET", params);
    actions::notify_err(env.ed, r).is_some()
}

/// Cortes do projeto na ordem do documento: (id, código "C003", nome da cena, duração).
pub(crate) fn project_cuts(p: &Project) -> Vec<(NodeId, String, String, u32)> {
    let mut out = Vec::new();
    for ep in &p.roots.episodes {
        let Some(epn) = p.node(ep) else { continue };
        for sc in &epn.children {
            let Some(scn) = p.node(sc) else { continue };
            for c in &scn.children {
                if let Some(NodeKind::Cut(cd)) = p.node(c).map(|n| &n.data) {
                    out.push((c.clone(), format!("C{:03}", cd.number), scn.name.clone(), cd.duration));
                }
            }
        }
    }
    out
}

fn cut_code(p: &Project, cut: &Id) -> String {
    match p.node(cut).map(|n| &n.data) {
        Some(NodeKind::Cut(cd)) => format!("C{:03}", cd.number),
        _ => "?".into(),
    }
}

fn fmt_time(frames: u32, fps: f64) -> String {
    let s = frames as f64 / fps.max(1.0);
    format!("{}:{:05.2}", (s / 60.0).floor() as u32, s % 60.0)
}

// ------------------------------------------------------------------------------------------------ Mídia

pub(crate) fn media(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let docs = actions::docs_of(&env.core.project, DocKind::Edit);
    let opened = sync(env);
    let name = opened.as_ref().and_then(|o| docs.iter().find(|(i, _)| *i == o.id)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("editing.no_edit").to_owned());
    let cb = egui::ComboBox::from_id_salt("edit_doc").width((ui.available_width() - 8.0).max(60.0)).selected_text(name).show_ui(ui, |ui| {
        for (id, n) in &docs {
            if ui.selectable_label(opened.as_ref().map(|o| &o.id) == Some(id), n).clicked() {
                env.ed.edit.sel_edit = Some(id.clone());
                env.ed.edit.sel_clip = None;
                env.ed.edit.sel_sub = None;
            }
        }
    });
    env.note("edit:doc_picker", cb.response.rect);
    let cuts = project_cuts(&env.core.project);
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(!cuts.is_empty(), egui::Button::new(tr("editing.new_edit")));
        env.note("edit:new_edit", b.rect);
        if b.on_hover_text(tr("editing.new_edit.tip")).clicked() {
            let name = actions::next_doc_name(&env.core.project, DocKind::Edit, tr("editing.default_name"));
            let doc = EditDoc { clips: cuts.iter().map(|(c, ..)| EditClip { cut: c.clone(), in_frame: 0, out_frame: None, transition_in: None }).collect(), credits_seconds: 6.0, ..Default::default() };
            let mut params = serde_json::to_value(&doc).unwrap_or(Value::Null);
            params["name"] = json!(name);
            let r = actions::run(env.core, "EDIT_SET", params);
            if let Some(v) = actions::notify_err(env.ed, r) {
                env.ed.edit.sel_edit = v.get("edit").and_then(Value::as_str).map(|s| Id(s.to_owned()));
                env.ed.edit.frame = 0;
            }
        }
    });
    ui.add_space(2.0);
    dim_label(ui, &t, tr("editing.media_hint"));
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    egui::ScrollArea::vertical().id_salt("edit_media").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if cuts.is_empty() {
            ui.label(egui::RichText::new(tr("panel.media.empty")).color(t.text_dim));
        }
        let fps = env.core.project.settings.fps();
        let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
        for (i, (cut, code, scene, dur)) in cuts.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click());
            env.note(format!("edit:media:{i}"), rect);
            let painter = ui.painter().clone();
            if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let th = 36.0;
            let thumb = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 4.0), egui::vec2((th * w / h).min(72.0), th));
            painter.rect_filled(thumb, 2.0, Color32::WHITE);
            if let Some(tex) = env.ed.edit.thumbs.get(&ctx, env.core, cut, &mut budget) {
                painter.image(tex, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let clip = Rect::from_min_max(egui::pos2(thumb.right() + 8.0, rect.top()), rect.max);
            let tp = painter.with_clip_rect(clip);
            tp.text(egui::pos2(clip.left(), rect.top() + 6.0), egui::Align2::LEFT_TOP, format!("{code}  ·  {:.1} s", *dur as f64 / fps), FontId::proportional(12.5), t.text);
            tp.text(egui::pos2(clip.left(), rect.top() + 24.0), egui::Align2::LEFT_TOP, scene, FontId::proportional(11.0), t.text_dim);
            let resp = resp.on_hover_text(tr("editing.media_add.tip"));
            if resp.double_clicked() {
                if let Some(o) = &opened {
                    let mut d = (*o.doc).clone();
                    let at = env.ed.edit.sel_clip.map(|k| k + 1).unwrap_or(d.clips.len());
                    d.clips.insert(at, EditClip { cut: cut.clone(), in_frame: 0, out_frame: None, transition_in: None });
                    if save(env, &o.id, &d) {
                        env.ed.edit.sel_clip = Some(at);
                    }
                }
            }
        }
    });
    let alive: std::collections::HashSet<&str> = cuts.iter().map(|c| c.0.0.as_str()).collect();
    env.ed.edit.thumbs.retain(|k| alive.contains(k));
}

// ------------------------------------------------------------------------------------------------ Prévia

pub(crate) fn preview(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr("editing.no_edit_long")).color(t.text_dim)));
        return;
    };
    let layout = match &o.layout {
        Ok(l) => l.clone(),
        Err(e) => {
            ui.label(egui::RichText::new(format!("{}: {e}", tr("editing.invalid"))).color(t.accent));
            return;
        }
    };
    let fps = env.core.project.settings.fps();
    let frame = env.ed.edit.frame;
    let ctx = ui.ctx().clone();
    env.ed.edit.preview.update(&ctx, env.core, &o.id, &o.doc, frame);

    // área da imagem (proporção do projeto) e barra de transporte embaixo
    let bar_h = 30.0;
    let area = Rect::from_min_max(ui.max_rect().min, egui::pos2(ui.max_rect().right(), ui.max_rect().bottom() - bar_h - 4.0));
    let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
    let s = (area.width() / w).min(area.height() / h);
    let img_rect = Rect::from_center_size(area.center(), egui::vec2(w * s, h * s));
    let painter = ui.painter();
    painter.rect_filled(img_rect, 0.0, Color32::BLACK);
    if let Some(tex) = &env.ed.edit.preview.texture {
        painter.image(tex.id(), img_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    env.note("edit:preview_image", img_rect);
    if let Some(e) = &env.ed.edit.preview.error {
        painter.text(img_rect.center(), egui::Align2::CENTER_CENTER, e, FontId::proportional(12.0), t.accent);
    } else if env.ed.edit.preview.shown_frame != Some(frame) {
        painter.text(img_rect.right_top() + egui::vec2(-8.0, 8.0), egui::Align2::RIGHT_TOP, tr("editing.rendering"), FontId::proportional(11.0), Color32::from_gray(200));
    }

    let bar = Rect::from_min_max(egui::pos2(ui.max_rect().left(), ui.max_rect().bottom() - bar_h), ui.max_rect().max);
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let last = layout.total.saturating_sub(1);
    let mut f = frame;
    let prev = env.icon_button(&mut bui, crate::icons::IconId::StepBack, "editing.prev", false, f > 0, "edit:prev");
    let next = env.icon_button(&mut bui, crate::icons::IconId::StepForward, "editing.next", false, f < last, "edit:next");
    if prev.clicked() {
        f = f.saturating_sub(1);
    }
    if next.clicked() {
        f = (f + 1).min(last);
    }
    let slider_w = (bui.available_width() - 190.0).max(60.0);
    bui.spacing_mut().slider_width = slider_w;
    let r = bui.add(egui::Slider::new(&mut f, 0..=last.max(1)).show_value(false));
    env.note("edit:scrub", r.rect);
    bui.label(egui::RichText::new(format!("{}  /  {}   ·   {} {}", fmt_time(f, fps), fmt_time(layout.total, fps), tr("editing.frame_short"), f + 1)).size(12.0).color(t.text_dim));
    env.ed.edit.frame = f.min(last);
}

// ------------------------------------------------------------------------------------------------ Linha do tempo

const RULER_H: f32 = 20.0;
const CLIP_H: f32 = 40.0;
const SUB_H: f32 = 24.0;
const LABEL_W: f32 = 74.0;

pub(crate) fn timeline(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        ui.label(egui::RichText::new(tr("editing.no_edit_long")).color(t.text_dim));
        return;
    };
    let Ok(layout) = o.layout.clone() else {
        ui.label(egui::RichText::new(tr("editing.invalid")).color(t.accent));
        return;
    };
    let fps = env.core.project.settings.fps();

    // ---- barra: zoom, legenda no cabeçote, apagar
    ui.horizontal(|ui| {
        let b = ui.button(tr("editing.fit"));
        env.note("edit:fit", b.rect);
        if b.clicked() {
            env.ed.edit.zoom = None;
        }
        let b = env.icon_button(ui, IconId::ZoomOut, "editing.zoom_out", false, true, "edit:zoom_out");
        let full = (ui.available_width() + 120.0 - LABEL_W).max(50.0) / layout.total.max(1) as f32;
        let cur = env.ed.edit.zoom.unwrap_or(full);
        if b.clicked() {
            env.ed.edit.zoom = Some((cur / 1.5).max(0.2));
        }
        let b = env.icon_button(ui, IconId::ToolZoom, "editing.zoom_in", false, true, "edit:zoom_in");
        if b.clicked() {
            env.ed.edit.zoom = Some((cur * 1.5).min(40.0));
        }
        ui.separator();
        let b = ui.button(tr("editing.add_subtitle"));
        env.note("edit:add_subtitle", b.rect);
        if b.on_hover_text(tr("editing.add_subtitle.tip")).clicked() {
            let mut d = (*o.doc).clone();
            let start = env.ed.edit.frame;
            let end = (start + (2.0 * fps).round() as u32).min(layout.total.max(start + 1));
            d.subtitles.push(SubtitleEntry { start_frame: start, end_frame: end, text: tr("editing.new_subtitle").to_owned() });
            d.subtitles.sort_by_key(|s| s.start_frame);
            let k = d.subtitles.iter().position(|s| s.start_frame == start).unwrap_or(0);
            if save(env, &o.id, &d) {
                env.ed.edit.sel_sub = Some(k);
                env.ed.edit.sel_clip = None;
            }
        }
        dim_label(ui, &t, &format!("{} {} · {} · {:.1} s", o.doc.clips.len(), tr("editing.clips"), layout.total, layout.total as f64 / fps));
    });

    let avail = ui.available_rect_before_wrap();
    let full = ((avail.width() - LABEL_W - 8.0).max(50.0)) / layout.total.max(1) as f32;
    let ppf = env.ed.edit.zoom.unwrap_or(full);
    let content_w = LABEL_W + layout.total as f32 * ppf + 8.0;
    let content_h = RULER_H + CLIP_H + SUB_H + 12.0;
    egui::ScrollArea::horizontal().id_salt("edit_tl").auto_shrink([false, true]).show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w.max(avail.width()), content_h), Sense::hover());
        let x0 = rect.left() + LABEL_W;
        let xf = |f: u32| x0 + f as f32 * ppf;
        let painter = ui.painter().clone();
        // rótulos das faixas
        let ruler = Rect::from_min_size(egui::pos2(x0, rect.top()), egui::vec2(layout.total as f32 * ppf, RULER_H));
        let clips_row = Rect::from_min_size(egui::pos2(x0, ruler.bottom() + 4.0), egui::vec2(ruler.width(), CLIP_H));
        let subs_row = Rect::from_min_size(egui::pos2(x0, clips_row.bottom() + 4.0), egui::vec2(ruler.width(), SUB_H));
        painter.text(egui::pos2(rect.left() + 2.0, clips_row.center().y), egui::Align2::LEFT_CENTER, tr("editing.track_video"), FontId::proportional(11.5), t.text_dim);
        painter.text(egui::pos2(rect.left() + 2.0, subs_row.center().y), egui::Align2::LEFT_CENTER, tr("editing.track_subs"), FontId::proportional(11.5), t.text_dim);
        // régua: marca a cada segundo (e rótulo quando couber)
        painter.rect_filled(ruler, 2.0, t.bg_widget);
        let step_s = if ppf * fps as f32 >= 40.0 { 1 } else if ppf * fps as f32 >= 12.0 { 5 } else { 10 };
        let mut s = 0u32;
        while (s as f64 * fps) < layout.total as f64 {
            let x = xf((s as f64 * fps).round() as u32);
            painter.vline(x, ruler.y_range(), Stroke::new(1.0, t.border));
            painter.text(egui::pos2(x + 3.0, ruler.center().y), egui::Align2::LEFT_CENTER, format!("{s}s"), FontId::proportional(10.5), t.text_dim);
            s += step_s;
        }
        let rr = ui.interact(ruler, EId::new("edit_ruler"), Sense::click_and_drag());
        env.note("edit:ruler", ruler);
        if let Some(p) = rr.interact_pointer_pos().filter(|_| rr.clicked() || rr.dragged()) {
            env.ed.edit.frame = (((p.x - x0) / ppf).floor().max(0.0) as u32).min(layout.total.saturating_sub(1));
        }
        // clipes
        painter.rect_filled(clips_row, 2.0, t.bg_frame);
        for (k, (c, &(start, dur, _))) in o.doc.clips.iter().zip(&layout.items).enumerate() {
            let r = Rect::from_min_max(egui::pos2(xf(start), clips_row.top()), egui::pos2(xf(start + dur), clips_row.bottom())).shrink2(egui::vec2(0.5, 0.0));
            let resp = ui.interact(r, EId::new(("edit_clip", k)), Sense::click());
            env.note(format!("edit:clip:{k}"), r);
            let sel = env.ed.edit.sel_clip == Some(k);
            painter.rect(r, 3.0, if sel { t.accent_dim } else { t.bg_widget }, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { t.accent } else { t.border }), StrokeKind::Inside);
            let cp = painter.with_clip_rect(r.shrink(2.0));
            cp.text(r.left_top() + egui::vec2(5.0, 4.0), egui::Align2::LEFT_TOP, cut_code(&env.core.project, &c.cut), FontId::proportional(12.0), t.text);
            cp.text(r.left_bottom() + egui::vec2(5.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{:.1} s", dur as f64 / fps), FontId::proportional(10.5), t.text_dim);
            if let Some(tr_in) = c.transition_in.clone().filter(|x| x.kind != TransitionKind::Cut && x.frames > 0) {
                let w = (tr_in.frames as f32 * ppf).max(4.0).min(r.width());
                let tri = [r.left_bottom(), egui::pos2(r.left() + w, r.top()), egui::pos2(r.left() + w, r.bottom())];
                painter.add(egui::Shape::convex_polygon(tri.to_vec(), t.accent.gamma_multiply(0.35), Stroke::NONE));
            }
            if resp.on_hover_text(format!("{} · {}", cut_code(&env.core.project, &c.cut), tr(transition_key(c.transition_in.as_ref().map(|x| x.kind).unwrap_or_default())))).clicked() {
                env.ed.edit.sel_clip = Some(k);
                env.ed.edit.sel_sub = None;
                env.ed.edit.frame = start;
            }
        }
        if layout.total > layout.credits_start {
            let r = Rect::from_min_max(egui::pos2(xf(layout.credits_start), clips_row.top()), egui::pos2(xf(layout.total), clips_row.bottom())).shrink2(egui::vec2(0.5, 0.0));
            painter.rect(r, 3.0, t.bg_header, Stroke::new(1.0, t.border), StrokeKind::Inside);
            painter.with_clip_rect(r.shrink(2.0)).text(r.left_top() + egui::vec2(5.0, 4.0), egui::Align2::LEFT_TOP, tr("editing.credits"), FontId::proportional(12.0), t.text_dim);
            env.note("edit:credits_block", r);
        }
        // legendas
        painter.rect_filled(subs_row, 2.0, t.bg_frame);
        for (k, s) in o.doc.subtitles.iter().enumerate() {
            let r = Rect::from_min_max(egui::pos2(xf(s.start_frame), subs_row.top()), egui::pos2(xf(s.end_frame.max(s.start_frame + 1)), subs_row.bottom())).shrink2(egui::vec2(0.5, 1.0));
            let resp = ui.interact(r, EId::new(("edit_sub", k)), Sense::click());
            env.note(format!("edit:sub:{k}"), r);
            let sel = env.ed.edit.sel_sub == Some(k);
            painter.rect(r, 3.0, if sel { t.accent_dim } else { t.st_progress }, Stroke::new(1.0, if sel { t.accent } else { t.border }), StrokeKind::Inside);
            painter.with_clip_rect(r.shrink(2.0)).text(r.left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, actions::excerpt(&s.text, 60), FontId::proportional(11.0), t.text);
            if resp.on_hover_text(&s.text).clicked() {
                env.ed.edit.sel_sub = Some(k);
                env.ed.edit.sel_clip = None;
                env.ed.edit.frame = s.start_frame;
            }
        }
        // cabeçote
        let x = xf(env.ed.edit.frame) + ppf * 0.5;
        painter.vline(x, ruler.top()..=subs_row.bottom(), Stroke::new(1.5, t.accent));
    });
}

// ------------------------------------------------------------------------------------------------ Propriedades

pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(o) = sync(env) else {
        dim_label(ui, &t, tr("editing.no_edit_long"));
        return;
    };
    let fps = env.core.project.settings.fps();
    let mut doc = (*o.doc).clone();
    let mut changed = false;

    if let Some(k) = env.ed.edit.sel_clip {
        let c = doc.clips[k].clone();
        let dur = match env.core.project.node(&c.cut).map(|n| &n.data) {
            Some(NodeKind::Cut(cd)) => cd.duration,
            _ => 0,
        };
        section(ui, &t, "editing.clip");
        ui.label(egui::RichText::new(format!("{}  ·  {} {}", cut_code(&env.core.project, &c.cut), dur, tr("unit.frames"))).strong());
        egui::Grid::new("edit_clip_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let out = c.out_frame.unwrap_or(dur).min(dur);
            dim_label(ui, &t, tr("editing.in"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_in", k, &o.id.0)), c.in_frame, |ui, v| ui.add(egui::DragValue::new(v).range(0..=out.saturating_sub(1)).speed(0.3))) {
                doc.clips[k].in_frame = v;
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.out"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_out", k, &o.id.0)), out, |ui, v| ui.add(egui::DragValue::new(v).range(c.in_frame + 1..=dur.max(c.in_frame + 1)).speed(0.3))) {
                doc.clips[k].out_frame = if v >= dur { None } else { Some(v) };
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.transition"));
            let cur = c.transition_in.clone().unwrap_or(Transition { kind: TransitionKind::Cut, frames: 0 });
            let mut kind = cur.kind;
            let r = egui::ComboBox::from_id_salt(("edit_tr", k)).width(150.0).selected_text(tr(transition_key(kind))).show_ui(ui, |ui| {
                for (x, key) in TRANSITIONS {
                    ui.selectable_value(&mut kind, x, tr(key));
                }
            });
            env.note("edit:props:transition", r.response.rect);
            if kind != cur.kind {
                let frames = if kind == TransitionKind::Cut { 0 } else if cur.frames == 0 { (0.5 * fps).round() as u32 } else { cur.frames };
                doc.clips[k].transition_in = (kind != TransitionKind::Cut).then_some(Transition { kind, frames });
                changed = true;
            }
            ui.end_row();
            if cur.kind != TransitionKind::Cut {
                dim_label(ui, &t, tr("editing.transition_frames"));
                if let Some(v) = commit_on_release(ui, EId::new(("edit_trf", k, &o.id.0)), cur.frames, |ui, v| ui.add(egui::DragValue::new(v).range(1..=240).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
                    doc.clips[k].transition_in = Some(Transition { kind: cur.kind, frames: v });
                    changed = true;
                }
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let n = doc.clips.len();
            let up = env.icon_button(ui, crate::icons::IconId::ArrowUp, "editing.clip_up", false, k > 0, "edit:clip_up");
            let down = env.icon_button(ui, crate::icons::IconId::ArrowDown, "editing.clip_down", false, k + 1 < n, "edit:clip_down");
            let del = env.icon_button(ui, crate::icons::IconId::Trash, "editing.clip_delete", false, n > 1, "edit:clip_delete");
            if up.clicked() {
                doc.clips.swap(k, k - 1);
                env.ed.edit.sel_clip = Some(k - 1);
                changed = true;
            }
            if down.clicked() {
                doc.clips.swap(k, k + 1);
                env.ed.edit.sel_clip = Some(k + 1);
                changed = true;
            }
            if del.clicked() {
                doc.clips.remove(k);
                env.ed.edit.sel_clip = None;
                changed = true;
            }
        });
    } else if let Some(k) = env.ed.edit.sel_sub {
        let s = doc.subtitles[k].clone();
        section(ui, &t, "editing.subtitle");
        let total = o.layout.as_ref().map(|l| l.total).unwrap_or(u32::MAX);
        egui::Grid::new("edit_sub_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            dim_label(ui, &t, tr("editing.sub_start"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_ss", k, &o.id.0)), s.start_frame, |ui, v| ui.add(egui::DragValue::new(v).range(0..=s.end_frame.saturating_sub(1)).speed(0.3))) {
                doc.subtitles[k].start_frame = v;
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.sub_end"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_se", k, &o.id.0)), s.end_frame, |ui, v| ui.add(egui::DragValue::new(v).range(s.start_frame + 1..=total.max(s.start_frame + 1)).speed(0.3))) {
                doc.subtitles[k].end_frame = v;
                changed = true;
            }
            ui.end_row();
        });
        dim_label(ui, &t, &format!("{} – {}", fmt_time(s.start_frame, fps), fmt_time(s.end_frame, fps)));
        let w = (ui.available_width() - 4.0).max(80.0);
        let (r, c) = super::text_commit_multi(ui, EId::new(("edit_st", k, &o.id.0)), &s.text, w, "");
        env.note("edit:props:sub_text", r.rect);
        if let Some(x) = c.filter(|x| !x.trim().is_empty()) {
            doc.subtitles[k].text = x;
            changed = true;
        }
        let del = env.icon_button(ui, crate::icons::IconId::Trash, "editing.sub_delete", false, true, "edit:sub_delete");
        if del.clicked() {
            doc.subtitles.remove(k);
            env.ed.edit.sel_sub = None;
            changed = true;
        }
    } else {
        dim_label(ui, &t, tr("editing.props_hint"));
    }

    // ---- edição inteira: créditos e fade final
    section(ui, &t, "editing.credits");
    let w = (ui.available_width() - 4.0).max(80.0);
    let (r, c) = super::text_commit_multi(ui, EId::new(("edit_credits", &o.id.0)), &doc.credits.join("\n"), w, tr("editing.credits_hint"));
    env.note("edit:props:credits", r.rect);
    if let Some(x) = c {
        doc.credits = x.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
        changed = true;
    }
    egui::Grid::new("edit_doc_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("editing.credits_seconds"));
        if let Some(v) = commit_on_release(ui, EId::new(("edit_cs", &o.id.0)), doc.credits_seconds, |ui, v| ui.add(egui::DragValue::new(v).range(1.0..=60.0).speed(0.1).suffix(" s"))) {
            doc.credits_seconds = v;
            changed = true;
        }
        ui.end_row();
        dim_label(ui, &t, tr("editing.fade_out"));
        if let Some(v) = commit_on_release(ui, EId::new(("edit_fo", &o.id.0)), doc.fade_out_end, |ui, v| ui.add(egui::DragValue::new(v).range(0..=240).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
            doc.fade_out_end = v;
            changed = true;
        }
        ui.end_row();
    });
    if changed {
        save(env, &o.id, &doc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;

    fn session_with_two_cuts() -> (CoreSession, NodeId, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let a = crate::app::commands::quick_start(&mut core, 24, None).unwrap();
        let scene = core.project.node(&a).and_then(|n| n.parent.clone()).unwrap();
        let b = crate::app::commands::add_cut(&mut core, &scene, 12, None).unwrap();
        (core, a, b)
    }

    #[test]
    fn edit_set_round_trip_through_the_bridge_with_undo() {
        let (mut core, a, b) = session_with_two_cuts();
        let doc = EditDoc { clips: vec![EditClip { cut: a.clone(), in_frame: 0, out_frame: None, transition_in: None }, EditClip { cut: b.clone(), in_frame: 2, out_frame: Some(10), transition_in: Some(Transition { kind: TransitionKind::Dissolve, frames: 4 }) }], credits_seconds: 6.0, ..Default::default() };
        let mut params = serde_json::to_value(&doc).unwrap();
        params["name"] = json!("Edição");
        let v = actions::run(&mut core, "EDIT_SET", params).unwrap();
        let id = Id(v["edit"].as_str().unwrap().to_owned());
        assert_eq!(actions::docs_of(&core.project, DocKind::Edit).len(), 1);
        let back = load_edit(&core.project, &id).unwrap();
        assert_eq!(back, doc);
        let tl = back.timeline(&core.project, core.project.settings.fps() as f32).unwrap();
        assert_eq!(tl.items, vec![(0, 24, 0), (20, 8, 2)], "dissolve de 4 sobrepõe o anterior");
        // um trecho vazio é recusado e não muda o documento
        let mut bad = back.clone();
        bad.clips[1].in_frame = 10;
        let mut p = serde_json::to_value(&bad).unwrap();
        p["id"] = json!(id);
        assert!(actions::run(&mut core, "EDIT_SET", p).is_err());
        assert_eq!(load_edit(&core.project, &id).unwrap(), doc);
        core.undo().unwrap();
        assert!(actions::docs_of(&core.project, DocKind::Edit).is_empty());
    }

    #[test]
    fn project_cuts_follow_document_order_and_transition_keys_exist() {
        let (core, a, b) = session_with_two_cuts();
        let cuts = project_cuts(&core.project);
        assert_eq!(cuts.iter().map(|c| c.0.clone()).collect::<Vec<_>>(), vec![a, b]);
        use crate::i18n::{Lang, tr_in};
        for (k, key) in TRANSITIONS {
            assert_eq!(transition_key(k), key);
            assert_ne!(tr_in(Lang::En, key), key);
        }
        assert_eq!(fmt_time(36, 24.0), "0:01.50");
    }
}

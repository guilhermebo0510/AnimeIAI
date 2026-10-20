//! Painel Texto: todos os controles de estilo da camada de texto (fonte com prévia, tamanho, cor, negrito/itálico,
//! rastreamento, entrelinha, alinhamento, vertical, caixa, posição, rotação, contorno e sombra) e o texto em si. Vale para a
//! sessão de digitação em curso, para a camada de texto ativa ou, sem nenhuma, para o estilo do próximo texto.
//! As mesmas peças, em linha, são as opções da ferramenta Texto na barra do canvas.

use super::{commit_on_release, text_commit_multi};
use crate::app::commands as cmds;
use crate::app::text_tool as tt;
use crate::app::ui_kit::{Env, body_ui, color32_to_rgba, dim_label, rgba_to_color32, section};
use crate::i18n::tr;
use aiai_core::*;
use aiai_paint::text_layer::{TextAlign, TextParams, TextShadow, TextStroke};
use egui::{Color32, Id, Sense, Stroke, TextureHandle, Ui, Vec2};
use std::collections::HashMap;
use std::ops::RangeInclusive;

#[derive(Default)]
pub(crate) struct PanelState {
    pub search: String,
    previews: HashMap<String, TextureHandle>,
    made_frame: u64,
    made_count: u32,
}

impl PanelState {
    /// Esquece as prévias de fonte (orçamento de VRAM, `gpu_owners`): voltam aos poucos quando o painel aparece.
    pub(crate) fn release_gpu(&mut self) -> bool {
        let had = !self.previews.is_empty();
        self.previews.clear();
        had
    }
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: egui::Rect) {
    let mut child = body_ui(ui, body, 8.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        ui.label(egui::RichText::new(tr("panel.text.title")).strong().size(13.0));
        ui.add_space(2.0);
        controls(env, ui, false);
    });
}

fn num(ui: &mut Ui, id: &'static str, cur: f32, range: RangeInclusive<f32>, speed: f64, suffix: &str, decimals: usize) -> Option<f32> {
    commit_on_release(ui, Id::new(("text_num", id)), cur, |ui, v| ui.add(egui::DragValue::new(v).range(range).speed(speed).suffix(suffix.to_owned()).max_decimals(decimals)))
}

/// Botão de cor que só entrega a cor ao soltar o mouse (um passo por ajuste).
fn color_button(env: &Env, ui: &mut Ui, id: &'static str, cur: &ColorRef, alpha: bool, tip: &str) -> Option<Rgba> {
    let cur_rgba = cmds::resolve_color(&env.core.project, cur);
    let key = Id::new(("text_color", id));
    let mut c32: Color32 = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| rgba_to_color32(cur_rgba));
    let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, if alpha { egui::color_picker::Alpha::OnlyBlend } else { egui::color_picker::Alpha::Opaque });
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(key, c32));
    }
    r.on_hover_text(tip);
    let ch = ui.input(|i| i.pointer.any_down());
    let mut out = None;
    let pending = ui.data(|d| d.get_temp::<Color32>(key)).is_some();
    if pending && !ch {
        ui.data_mut(|d| d.remove_temp::<Color32>(key));
        let rgba = color32_to_rgba(c32);
        if rgba != cur_rgba {
            out = Some(rgba);
        }
    }
    out
}

/// Botão de alinhamento desenhado com linhas (esquerda, centro, direita, justificado).
fn align_button(env: &Env, ui: &mut Ui, a: TextAlign, on: bool, vertical: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(24.0, 22.0), Sense::click());
    let t = env.t;
    let p = ui.painter();
    if on {
        p.rect(rect, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    let ink = if on || resp.hovered() { t.text } else { t.text_dim };
    let r = rect.shrink2(Vec2::new(5.0, 5.0));
    // cinco traços; o comprimento e o lado dependem do alinhamento (no tategaki, giram)
    for k in 0..4 {
        let f = k as f32 / 3.0;
        let full = matches!(a, TextAlign::Justify) || k % 2 == 0;
        let len = if full { 1.0 } else { 0.62 };
        let (x0, x1) = match a {
            TextAlign::Left | TextAlign::Justify => (0.0, len),
            TextAlign::Center => ((1.0 - len) / 2.0, (1.0 + len) / 2.0),
            TextAlign::Right => (1.0 - len, 1.0),
        };
        let y = r.top() + r.height() * f;
        let (p0, p1) = (egui::pos2(r.left() + r.width() * x0, y), egui::pos2(r.left() + r.width() * x1, y));
        if vertical {
            let rot = |q: egui::Pos2| egui::pos2(r.center().x + (q.y - r.center().y), r.center().y + (q.x - r.center().x));
            p.line_segment([rot(p0), rot(p1)], Stroke::new(1.4, ink));
        } else {
            p.line_segment([p0, p1], Stroke::new(1.4, ink));
        }
    }
    resp
}

fn align_tip(a: TextAlign) -> &'static str {
    match a {
        TextAlign::Left => "text.align.left",
        TextAlign::Center => "text.align.center",
        TextAlign::Right => "text.align.right",
        TextAlign::Justify => "text.align.justify",
    }
}

const ALIGNS: [TextAlign; 4] = [TextAlign::Left, TextAlign::Center, TextAlign::Right, TextAlign::Justify];

/// Controles de estilo. `compact`: só o essencial, em linha (barra do canvas).
pub(crate) fn controls(env: &mut Env, ui: &mut Ui, compact: bool) {
    let t = env.t;
    let (layer, p, has_target) = tt::current_params(env);
    let font_known = aiai_paint::fonts::catalog_if_ready().map(|c| (c.family_faces(&p.font).iter().any(|f| f.bold), c.family_faces(&p.font).iter().any(|f| f.italic)));
    if compact {
        controls_compact(env, ui, &p, font_known);
        return;
    }
    if !has_target {
        dim_label(ui, &t, tr("text.hint_defaults"));
        ui.add_space(2.0);
    } else if layer.is_none() {
        dim_label(ui, &t, tr("text.hint_new"));
        ui.add_space(2.0);
    }
    // ---- texto
    if has_target {
        let id = Id::new("text_content");
        let (r, out) = text_commit_multi(ui, id, &p.content, ui.available_width(), tr("text.content_hint"));
        env.note("text:content", r.rect);
        if let Some(v) = out {
            tt::edit(env, "text.cmd_style", |q| q.content = v);
        }
    }
    section(ui, &t, "text.sec.character");
    // ---- fonte
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("text.font"));
        if let Some(f) = font_picker(env, ui, &p.font) {
            tt::edit(env, "text.cmd_style", |q| q.font = f);
        }
    });
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("text.size"));
        if let Some(v) = num(ui, "size", p.size, 1.0..=2000.0, 0.5, " px", 1) {
            tt::edit(env, "text.cmd_style", |q| q.size = v);
        }
        style_toggles(env, ui, &p, font_known);
        ui.add_space(4.0);
        dim_label(ui, &t, tr("text.color"));
        if let Some(c) = color_button(env, ui, "fill", &p.color, false, tr("text.color.tip")) {
            env.ed.set_color_direct(c);
            tt::edit(env, "text.cmd_style", |q| q.color = ColorRef::Direct { rgba: c });
        }
    });
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("text.tracking"));
        if let Some(v) = num(ui, "tracking", p.tracking, -200.0..=2000.0, 1.0, "", 0) {
            tt::edit(env, "text.cmd_style", |q| q.tracking = v);
        }
        dim_label(ui, &t, tr("text.leading"));
        if let Some(v) = num(ui, "leading", p.leading * 100.0, 20.0..=500.0, 1.0, " %", 0) {
            tt::edit(env, "text.cmd_style", |q| q.leading = v / 100.0);
        }
    });
    section(ui, &t, "text.sec.paragraph");
    ui.horizontal_wrapped(|ui| {
        for a in ALIGNS {
            let b = align_button(env, ui, a, p.align == a, p.vertical).on_hover_text(tr(align_tip(a)));
            env.note(format!("text:align.{}", a_key(a)), b.rect);
            if b.clicked() {
                tt::edit(env, "text.cmd_style", |q| q.align = a);
            }
        }
        ui.add_space(6.0);
        let mut v = p.vertical;
        if ui.checkbox(&mut v, tr("text.vertical")).on_hover_text(tr("text.vertical.tip")).changed() {
            tt::edit(env, "text.cmd_style", |q| q.vertical = v);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let mut boxed = p.box_size.is_some();
        if ui.checkbox(&mut boxed, tr("text.box")).on_hover_text(tr("text.box.tip")).changed() {
            let (w, h) = (p.size * 8.0, p.size * 3.0);
            tt::edit(env, "text.cmd_style", |q| q.box_size = if boxed { Some([w, h]) } else { None });
        }
        if let Some([w, h]) = p.box_size {
            if let Some(v) = num(ui, "boxw", w, 4.0..=20000.0, 1.0, " px", 0) {
                tt::edit(env, "text.cmd_style", |q| q.box_size = Some([v, h]));
            }
            dim_label(ui, &t, "×");
            if let Some(v) = num(ui, "boxh", h, 4.0..=20000.0, 1.0, " px", 0) {
                tt::edit(env, "text.cmd_style", |q| q.box_size = Some([w, v]));
            }
        }
    });
    section(ui, &t, "text.sec.position");
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, "X");
        if let Some(v) = num(ui, "posx", p.pos[0], -100000.0..=100000.0, 1.0, "", 0) {
            tt::edit(env, "text.cmd_style", |q| q.pos[0] = v);
        }
        dim_label(ui, &t, "Y");
        if let Some(v) = num(ui, "posy", p.pos[1], -100000.0..=100000.0, 1.0, "", 0) {
            tt::edit(env, "text.cmd_style", |q| q.pos[1] = v);
        }
        dim_label(ui, &t, tr("text.rotation"));
        if let Some(v) = num(ui, "rot", p.rotation, -360.0..=360.0, 0.5, "°", 1) {
            tt::edit(env, "text.cmd_style", |q| q.rotation = v);
        }
    });
    // ---- contorno
    section(ui, &t, "text.sec.stroke");
    ui.horizontal_wrapped(|ui| {
        let mut on = p.stroke.is_some();
        if ui.checkbox(&mut on, tr("text.stroke")).changed() {
            tt::edit(env, "text.cmd_style", |q| q.stroke = on.then(TextStroke::default));
        }
        if let Some(s) = &p.stroke {
            if let Some(v) = num(ui, "strokew", s.width, 0.0..=400.0, 0.2, " px", 1) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.stroke.as_mut() {
                        x.width = v;
                    }
                });
            }
            if let Some(c) = color_button(env, ui, "stroke", &s.color, false, tr("text.stroke.color")) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.stroke.as_mut() {
                        x.color = ColorRef::Direct { rgba: c };
                    }
                });
            }
        }
    });
    // ---- sombra
    section(ui, &t, "text.sec.shadow");
    ui.horizontal_wrapped(|ui| {
        let mut on = p.shadow.is_some();
        if ui.checkbox(&mut on, tr("text.shadow")).changed() {
            tt::edit(env, "text.cmd_style", |q| q.shadow = on.then(TextShadow::default));
        }
        if let Some(s) = &p.shadow {
            if let Some(c) = color_button(env, ui, "shadow", &s.color, true, tr("text.shadow.color")) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.shadow.as_mut() {
                        x.color = ColorRef::Direct { rgba: c };
                    }
                });
            }
        }
    });
    if let Some(s) = &p.shadow {
        ui.horizontal_wrapped(|ui| {
            dim_label(ui, &t, "X");
            if let Some(v) = num(ui, "shx", s.offset[0], -400.0..=400.0, 0.5, "", 1) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.shadow.as_mut() {
                        x.offset[0] = v;
                    }
                });
            }
            dim_label(ui, &t, "Y");
            if let Some(v) = num(ui, "shy", s.offset[1], -400.0..=400.0, 0.5, "", 1) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.shadow.as_mut() {
                        x.offset[1] = v;
                    }
                });
            }
            dim_label(ui, &t, tr("text.blur"));
            if let Some(v) = num(ui, "shblur", s.blur, 0.0..=400.0, 0.5, " px", 1) {
                tt::edit(env, "text.cmd_style", |q| {
                    if let Some(x) = q.shadow.as_mut() {
                        x.blur = v;
                    }
                });
            }
        });
    }
    ui.add_space(4.0);
    dim_label(ui, &t, tr("text.license_hint"));
}

fn a_key(a: TextAlign) -> &'static str {
    match a {
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
        TextAlign::Justify => "justify",
    }
}

fn style_toggles(env: &mut Env, ui: &mut Ui, p: &TextParams, known: Option<(bool, bool)>) {
    let t = env.t;
    for (label, on, which) in [("B", p.bold, 0), ("I", p.italic, 1)] {
        let real = known.map(|k| if which == 0 { k.0 } else { k.1 }).unwrap_or(true);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
        let pa = ui.painter();
        if on {
            pa.rect(rect, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            pa.rect_filled(rect, 4.0, t.bg_hover);
        }
        let ink = if on || resp.hovered() { t.text } else { t.text_dim };
        let galley = pa.layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), ink);
        pa.galley(rect.center() - galley.size() / 2.0, galley, ink);
        let tip = if which == 0 { "text.bold" } else { "text.italic" };
        let mut tipt = tr(tip).to_owned();
        if on && !real {
            tipt.push_str(" (");
            tipt.push_str(tr("text.synthetic"));
            tipt.push(')');
        }
        env.note(format!("text:{}", if which == 0 { "bold" } else { "italic" }), rect);
        if resp.on_hover_text(tipt).clicked() {
            tt::edit(env, "text.cmd_style", |q| if which == 0 { q.bold = !on } else { q.italic = !on });
        }
    }
}

fn controls_compact(env: &mut Env, ui: &mut Ui, p: &TextParams, known: Option<(bool, bool)>) {
    if let Some(f) = font_picker(env, ui, &p.font) {
        tt::edit(env, "text.cmd_style", |q| q.font = f);
    }
    if let Some(v) = num(ui, "csize", p.size, 1.0..=2000.0, 0.5, " px", 0) {
        tt::edit(env, "text.cmd_style", |q| q.size = v);
    }
    style_toggles(env, ui, p, known);
    for a in ALIGNS {
        let b = align_button(env, ui, a, p.align == a, p.vertical).on_hover_text(tr(align_tip(a)));
        if b.clicked() {
            tt::edit(env, "text.cmd_style", |q| q.align = a);
        }
    }
}

// ------------------------------------------------------------------------------------------------ fonte com prévia

fn to_color_image(img: &aiai_paint::Image) -> egui::ColorImage {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    egui::ColorImage {
        size: [img.width as usize, img.height as usize],
        source_size: egui::vec2(img.width as f32, img.height as f32),
        pixels: img.data.iter().map(|p| Color32::from_rgba_premultiplied(b(p[0]), b(p[1]), b(p[2]), b(p[3]))).collect(),
    }
}

/// Prévia de uma família: o nome dela escrito na própria fonte, branco com alfa (o painel tinge com a cor do tema).
/// No máximo 3 novas por quadro (cada uma lê o arquivo da fonte); as que faltam chegam nos quadros seguintes.
fn preview(ctx: &egui::Context, st: &mut PanelState, family: &str) -> Option<TextureHandle> {
    if let Some(t) = st.previews.get(family) {
        return Some(t.clone());
    }
    let frame = ctx.cumulative_pass_nr();
    if st.made_frame != frame {
        st.made_frame = frame;
        st.made_count = 0;
    }
    if st.made_count >= 3 {
        ctx.request_repaint();
        return None;
    }
    st.made_count += 1;
    let p = TextParams { content: family.to_owned(), font: family.to_owned(), size: 17.0, pos: [4.0, 2.0], color: ColorRef::Direct { rgba: Rgba::WHITE }, ..Default::default() };
    let lay = aiai_paint::text_layer::layout(&p);
    let colors = aiai_render::text::TextColors { fill: [1.0, 1.0, 1.0, 1.0], stroke: [0.0; 4], shadow: [0.0; 4] };
    let img = aiai_render::text::render_text_layer(&p, &lay, &colors, aiai_paint::Affine::IDENTITY, 250, 26).unwrap_or_else(|| aiai_paint::Image::new(250, 26));
    let tex = ctx.load_texture(format!("fontprev-{family}"), to_color_image(&img), egui::TextureOptions::LINEAR);
    st.previews.insert(family.to_owned(), tex.clone());
    Some(tex)
}

/// Seletor de fonte: lista com busca; cada linha mostra o nome na própria fonte.
fn font_picker(env: &mut Env, ui: &mut Ui, cur: &str) -> Option<String> {
    let t = env.t;
    let mut pick: Option<String> = None;
    let cat = aiai_paint::fonts::catalog_if_ready();
    if cat.is_none() {
        aiai_paint::fonts::warm_up();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let r = egui::ComboBox::from_id_salt("text_font").width(150.0).selected_text(cur.to_owned()).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
        ui.set_min_width(280.0);
        let sr = ui.add(egui::TextEdit::singleline(&mut env.ed.text.panel.search).hint_text(tr("text.font_search")).desired_width(f32::INFINITY));
        env.notes.push(("text:font_search".into(), sr.rect));
        let Some(cat) = cat else {
            ui.label(tr("text.fonts_loading"));
            return;
        };
        let q = env.ed.text.panel.search.to_lowercase();
        let fams: Vec<String> = cat.families().into_iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q)).collect();
        let row_h = 26.0;
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show_rows(ui, row_h, fams.len(), |ui, range| {
            for i in range {
                let fam = &fams[i];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                let selected = fam.eq_ignore_ascii_case(cur);
                let p = ui.painter();
                if selected {
                    p.rect_filled(rect, 3.0, t.accent_dim);
                } else if resp.hovered() {
                    p.rect_filled(rect, 3.0, t.bg_hover);
                }
                match preview(&ctx, &mut env.ed.text.panel, fam) {
                    Some(tex) => {
                        let r = egui::Rect::from_min_size(rect.left_top() + Vec2::new(4.0, 0.0), Vec2::new(250.0, 26.0));
                        p.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), t.text);
                    }
                    None => {
                        p.text(rect.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, fam, egui::FontId::proportional(12.0), t.text_dim);
                    }
                }
                if resp.clicked() {
                    pick = Some(fam.clone());
                    ui.close();
                }
            }
        });
    });
    env.note("text:font", r.response.rect);
    r.response.on_hover_text(tr("text.font.tip"));
    pick
}

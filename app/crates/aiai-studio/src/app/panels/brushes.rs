//! Painel Pincéis (como o de subferramentas dos programas de pintura): os pincéis por categoria, com a miniatura de um
//! traço feito pelo próprio motor, busca, "Salvar como pincel novo" e, embaixo, as propriedades da ferramenta ativa
//! (tamanho, opacidade, estabilizador, ponta, pressão, textura, cor...). Escolher a caneta, o pincel ou a borracha
//! traz este painel para a frente.

use crate::app::brushes::{CATEGORIES, cat_label, cat_of};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::workspace::ToolId;
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui};

#[derive(Default)]
pub(crate) struct BrushPanelState {
    /// Categoria aberta (`None` = todas, agrupadas).
    pub cat: Option<String>,
    pub search: String,
    /// Formulário "salvar como pincel novo": (nome, categoria).
    pub saving: Option<(String, String)>,
    thumbs: Vec<egui::TextureHandle>,
    thumbs_key: (u64, bool),
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let tool = *env.tool;
    ui.label(egui::RichText::new(tr(tool.title_key())).strong().size(13.0));
    ui.add_space(2.0);
    if tool != ToolId::Brush {
        egui::ScrollArea::vertical().id_salt("brushes_scroll").auto_shrink([false, false]).show(ui, |ui| {
            if !matches!(tool, ToolId::Pen | ToolId::Eraser) {
                dim_label(ui, &env.t, tr("brushes.other_tool"));
            }
            super::properties::tool_options(env, ui, false);
        });
        return;
    }
    // em cima a lista (rolagem própria), embaixo as propriedades da ferramenta, sempre à vista
    library_header(env, ui);
    let list_h = (ui.available_height() * 0.46).clamp(110.0, 560.0);
    egui::ScrollArea::vertical().id_salt("brush_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| library(env, ui));
    save_form(env, ui);
    ui.separator();
    egui::ScrollArea::vertical().id_salt("brush_props").auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &env.t, "brushes.props");
        super::properties::tool_options(env, ui, false);
    });
}

/// Busca e categorias (fora da rolagem da lista).
fn library_header(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let dark = ui.ctx().theme() == egui::Theme::Dark;
    // miniaturas: um traço de verdade de cada pincel (refeitas quando a biblioteca ou o tema mudam)
    let key = (env.ed.brushes.version, dark);
    if env.ed.brush_panel.thumbs_key != key || env.ed.brush_panel.thumbs.len() != env.ed.brushes.items.len() {
        let presets: Vec<_> = env.ed.brushes.items.iter().map(|b| b.preset.clone()).collect();
        env.ed.brush_panel.thumbs = super::properties::brush_thumbnails(ui.ctx(), &presets, t.bg_widget, t.text);
        env.ed.brush_panel.thumbs_key = key;
    }
    let r = ui.add(egui::TextEdit::singleline(&mut env.ed.brush_panel.search).hint_text(tr("brushes.search")).desired_width(f32::INFINITY));
    env.note("brushes:search", r.rect);
    let cats = env.ed.brushes.categories();
    let counts: Vec<usize> = cats.iter().map(|c| env.ed.brushes.items.iter().filter(|b| cat_of(&b.preset) == *c).count()).collect();
    let total = env.ed.brushes.items.len();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
        let all = env.ed.brush_panel.cat.is_none();
        let r = ui.selectable_label(all, egui::RichText::new(format!("{} ({total})", tr("brushes.all"))).size(12.0));
        env.note("brushes:cat:all", r.rect);
        if r.clicked() {
            env.ed.brush_panel.cat = None;
        }
        for (c, &n) in cats.iter().zip(&counts) {
            if n == 0 {
                continue;
            }
            let on = env.ed.brush_panel.cat.as_deref() == Some(c.as_str());
            let r = ui.selectable_label(on, egui::RichText::new(format!("{} ({n})", cat_label(c))).size(12.0));
            env.note(format!("brushes:cat:{c}"), r.rect);
            if r.clicked() {
                env.ed.brush_panel.cat = Some(c.clone());
            }
        }
    });
    ui.add_space(3.0);
}

/// A lista de pincéis (linhas compactas: o traço de verdade e o nome; duas colunas num painel largo).
fn library(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let cats = env.ed.brushes.categories();
    let needle = env.ed.brush_panel.search.trim().to_lowercase();
    let show_cats: Vec<String> = match &env.ed.brush_panel.cat {
        Some(c) => vec![c.clone()],
        None => cats.clone(),
    };
    let mut pick: Option<usize> = None;
    let mut delete: Option<usize> = None;
    for c in &show_cats {
        let idx: Vec<usize> = (0..env.ed.brushes.items.len())
            .filter(|&i| cat_of(&env.ed.brushes.items[i].preset) == *c)
            .filter(|&i| needle.is_empty() || env.ed.brushes.items[i].preset.name.to_lowercase().contains(&needle))
            .collect();
        if idx.is_empty() {
            continue;
        }
        if env.ed.brush_panel.cat.is_none() {
            ui.label(egui::RichText::new(cat_label(c)).size(11.5).color(t.text_dim).strong());
        }
        // linhas compactas: o traço (miniatura feita pelo motor) à esquerda e o nome à direita
        let avail = ui.available_width();
        let cols = if avail >= 380.0 { 2 } else { 1 };
        let cw = ((avail - 6.0 * (cols as f32 - 1.0)) / cols as f32).floor();
        let rh = 30.0;
        for row in idx.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for &i in row {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(cw, rh), Sense::click());
                    env.note(format!("brushes:item:{i}"), rect);
                    let sel = env.ed.opts.brush_preset == i;
                    let p = ui.painter();
                    let fill = if sel { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget };
                    p.rect(rect, 4.0, fill, Stroke::new(1.0, if sel { t.accent } else { t.border }), StrokeKind::Inside);
                    let tw = (cw * 0.5).clamp(60.0, 150.0);
                    if let Some(tex) = env.ed.brush_panel.thumbs.get(i) {
                        let img = Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(tw, rh - 6.0));
                        p.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    let b = &env.ed.brushes.items[i];
                    let name = if b.user { format!("{} ·", b.preset.name) } else { b.preset.name.clone() };
                    let g = p.layout_no_wrap(name, egui::FontId::proportional(12.0), t.text);
                    p.with_clip_rect(rect.shrink(2.0)).galley(egui::pos2(rect.left() + tw + 9.0, rect.center().y - g.size().y / 2.0), g, t.text);
                    if resp.clicked() {
                        pick = Some(i);
                    }
                    let user = b.user;
                    resp.on_hover_text(b.preset.name.clone()).context_menu(|ui| {
                        if ui.add_enabled(user, egui::Button::new(tr("brushes.delete"))).clicked() {
                            delete = Some(i);
                            ui.close();
                        }
                    });
                }
            });
            ui.add_space(2.0);
        }
    }
    if let Some(i) = pick {
        if let Some(p) = env.ed.brushes.get(i).cloned() {
            let o = &mut env.ed.opts;
            o.brush_preset = i;
            o.brush_size = p.size;
            o.brush_opacity = p.opacity;
            o.brush_custom = None;
        }
    }
    if let Some(i) = delete {
        if let Err(e) = env.ed.brushes.remove_user(i) {
            env.ed.notify(e);
        }
        if env.ed.opts.brush_preset >= env.ed.brushes.items.len() || env.ed.opts.brush_preset == i {
            env.ed.opts.brush_preset = 1;
            env.ed.opts.brush_custom = None;
        }
    }
}

/// "Salvar como pincel novo" (fora da rolagem da lista, sempre à vista).
fn save_form(env: &mut Env, ui: &mut Ui) {
    match env.ed.brush_panel.saving.clone() {
        None => {
            let b = ui.button(tr("brushes.save_new"));
            env.note("brushes:save_new", b.rect);
            if b.on_hover_text(tr("brushes.save_new.tip")).clicked() {
                let base = env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| (p.name.clone(), cat_of(p))).unwrap_or_default();
                env.ed.brush_panel.saving = Some((format!("{} 2", base.0), base.1));
            }
        }
        Some((mut name, mut cat)) => {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(tr("brushes.save_new")).strong());
                egui::Grid::new("brush_save").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(tr("brushes.name"));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
                    env.note("brushes:save_name", r.rect);
                    ui.end_row();
                    ui.label(tr("brushes.category"));
                    egui::ComboBox::from_id_salt("brush_save_cat").selected_text(cat_label(&cat)).show_ui(ui, |ui| {
                        for (k, _) in CATEGORIES {
                            ui.selectable_value(&mut cat, k.to_owned(), cat_label(k));
                        }
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("brushes.save")));
                    env.note("brushes:save", ok.rect);
                    let cancel = ui.button(tr("confirm.cancel"));
                    if ok.clicked() {
                        let o = &env.ed.opts;
                        let mut p = o.brush_custom.clone().or_else(|| env.ed.brushes.get(o.brush_preset).cloned()).unwrap_or_else(|| aiai_paint::brush::BrushPreset::round("Pincel", 24.0, 0.9));
                        p.size = o.brush_size;
                        p.opacity = o.brush_opacity;
                        match env.ed.brushes.add_user(p, &name, &cat) {
                            Ok(i) => {
                                env.ed.opts.brush_preset = i;
                                env.ed.opts.brush_custom = None;
                                env.ed.brush_panel.saving = None;
                                env.ed.brush_panel.cat = Some(cat.clone());
                            }
                            Err(e) => env.ed.notify(e),
                        }
                        return;
                    }
                    if cancel.clicked() {
                        env.ed.brush_panel.saving = None;
                        return;
                    }
                    env.ed.brush_panel.saving = Some((name.clone(), cat.clone()));
                });
            });
        }
    }
}

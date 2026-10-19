//! Painel Propriedades: a camada ativa (tipo, papel, profundidade, deslocamento) e as opções da ferramenta ativa. As opções
//! da ferramenta também aparecem, em linha, na barra do canvas (`tool_options` com `compact = true`), porque o painel de
//! Propriedades divide aba com o de Camadas.

use super::commit_on_release;
use crate::app::commands::{self as cmds, ROLES, role_key};
use crate::app::editor::*;
use crate::app::tools;
use crate::app::ui_kit::{Env, body_ui, color32_to_rgba, dim_label, rgba_to_color32, section};
use crate::i18n::tr;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_core::*;
use egui::{Id, Ui};
use serde_json::json;

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: egui::Rect) {
    let mut child = body_ui(ui, body, 8.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        let t = env.t;
        // na pré-produção o painel mostra o documento (roteiro, painel do storyboard), não as opções de ferramenta
        match env.ws {
            WorkspaceId::Roteiro => return super::script::properties(env, ui),
            WorkspaceId::Storyboard => return super::shots::properties(env, ui),
            WorkspaceId::Producao => return super::production::properties(env, ui),
            WorkspaceId::Edicao => return super::edit::properties(env, ui),
            WorkspaceId::Entrega => return super::deliver::properties(env, ui),
            WorkspaceId::Cor => return super::satsuei::grade(env, ui),
            WorkspaceId::Personagens => return super::characters::properties(env, ui),
            _ => {}
        }
        let tool = *env.tool;
        ui.label(egui::RichText::new(tr(tool.title_key())).strong().size(13.0));
        ui.add_space(2.0);
        tool_options(env, ui, false);
        if tool == ToolId::Move || tool == ToolId::Lasso {
            ui.add_space(2.0);
            dim_label(ui, &t, tr(if tool == ToolId::Move { "opt.move_hint" } else { "opt.select_hint" }));
        }
        layer_properties(env, ui);
        if env.ws == WorkspaceId::Composicao {
            super::satsuei::layer_effects(env, ui);
            super::satsuei::camera(env, ui);
        }
        cut_properties(env, ui);
    });
}

/// Miniatura 140×28 de cada preset: uma pincelada senoidal com pressão crescente desenhada pelo `paint_stroke`.
pub(crate) fn brush_thumbnails(ctx: &egui::Context, presets: &[aiai_paint::brush::BrushPreset], bg: egui::Color32, ink: egui::Color32) -> Vec<egui::TextureHandle> {
    use aiai_paint::brush::{BrushPoint, paint_stroke};
    let (w, h) = (280u32, 56u32); // 2× para ficar nítida
    let bgc = [bg.r() as f32 / 255.0, bg.g() as f32 / 255.0, bg.b() as f32 / 255.0, 1.0];
    let inkc = [ink.r() as f32 / 255.0, ink.g() as f32 / 255.0, ink.b() as f32 / 255.0, 1.0];
    presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut img = aiai_paint::Image::filled(w, h, bgc);
            let mut q = p.clone();
            q.size = q.size.clamp(4.0, 30.0);
            let path: Vec<BrushPoint> = (0..60)
                .map(|k| {
                    let t = k as f32 / 59.0;
                    BrushPoint { x: 16.0 + t * (w as f32 - 32.0), y: h as f32 / 2.0 + 10.0 * (t * std::f32::consts::TAU).sin(), pressure: 0.25 + 0.75 * (t * std::f32::consts::PI).sin() }
                })
                .collect();
            paint_stroke(&mut img, &q, &path, inkc, 7);
            let rgba = img.to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            ctx.load_texture(format!("brush_thumb_{i}"), ci, egui::TextureOptions::LINEAR)
        })
        .collect()
}

/// Ajustes finos do pincel ativo: os parâmetros do motor, numa cópia do preset (o preset da biblioteca não muda;
/// "Salvar como pincel novo" no painel Pincéis guarda a cópia).
fn brush_settings(ui: &mut Ui, o: &mut super::super::editor::ToolOptions, base: Option<aiai_paint::brush::BrushPreset>) {
    let Some(mut p) = o.brush_custom.clone().or(base.clone()) else { return };
    let before = p.clone();
    egui::CollapsingHeader::new(tr("brush.settings")).id_salt("brush_settings").default_open(true).show(ui, |ui| {
        egui::Grid::new("brush_settings_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let slider = |ui: &mut Ui, key: &str, v: &mut f32, lo: f32, hi: f32| {
                ui.label(tr(key));
                ui.add(egui::Slider::new(v, lo..=hi).max_decimals(2));
                ui.end_row();
            };
            // ponta: redonda (dureza, achatamento, ângulo) ou forma de cenário (folha, grama, nuvem...)
            {
                use aiai_paint::brush::{BrushTip, TipShape};
                const SHAPES: [(TipShape, &str); 7] = [
                    (TipShape::Leaf, "tipshape.leaf"),
                    (TipShape::Petal, "tipshape.petal"),
                    (TipShape::GrassBlade, "tipshape.grass"),
                    (TipShape::CloudPuff, "tipshape.cloud"),
                    (TipShape::Star, "tipshape.star"),
                    (TipShape::Square, "tipshape.square"),
                    (TipShape::Rose, "tipshape.rose"),
                ];
                let current = match &p.tip {
                    BrushTip::Round { .. } => tr("tipshape.round").to_owned(),
                    BrushTip::Shape { shape, .. } => SHAPES.iter().find(|(s, _)| s == shape).map(|(_, k)| tr(k).to_owned()).unwrap_or_default(),
                    BrushTip::Image { .. } => tr("tipshape.image").to_owned(),
                };
                ui.label(tr("brush.tip"));
                let mut next: Option<BrushTip> = None;
                egui::ComboBox::from_id_salt("brush_tip_kind").selected_text(current).show_ui(ui, |ui| {
                    if ui.selectable_label(matches!(p.tip, BrushTip::Round { .. }), tr("tipshape.round")).clicked() {
                        next = Some(BrushTip::Round { hardness: 0.8, roundness: 1.0, angle: 0.0 });
                    }
                    for (s, k) in SHAPES {
                        if ui.selectable_label(matches!(&p.tip, BrushTip::Shape { shape, .. } if *shape == s), tr(k)).clicked() {
                            next = Some(BrushTip::Shape { shape: s, aspect: 1.0 });
                        }
                    }
                });
                ui.end_row();
                if let Some(n) = next {
                    p.tip = n;
                }
                match &mut p.tip {
                    BrushTip::Round { hardness, roundness, angle } => {
                        slider(ui, "brush.hardness", hardness, 0.0, 1.0);
                        slider(ui, "brush.roundness", roundness, 0.1, 1.0);
                        slider(ui, "brush.angle", angle, -90.0, 90.0);
                    }
                    BrushTip::Shape { aspect, .. } => slider(ui, "brush.aspect", aspect, 0.2, 3.0),
                    BrushTip::Image { .. } => {}
                }
            }
            slider(ui, "brush.flow", &mut p.flow, 0.02, 1.0);
            slider(ui, "brush.spacing", &mut p.spacing, 0.02, 2.0);
            slider(ui, "brush.size_pressure", &mut p.size_pressure_min, 0.0, 1.0);
            slider(ui, "brush.opacity_pressure", &mut p.opacity_pressure_min, 0.0, 1.0);
            slider(ui, "brush.flow_pressure", &mut p.flow_pressure_min, 0.0, 1.0);
            slider(ui, "brush.scatter", &mut p.scatter, 0.0, 2.0);
            slider(ui, "brush.size_jitter", &mut p.size_jitter, 0.0, 1.0);
            slider(ui, "brush.angle_jitter", &mut p.angle_jitter, 0.0, 180.0);
            slider(ui, "brush.value_jitter", &mut p.value_jitter, 0.0, 1.0);
            slider(ui, "brush.hue_jitter", &mut p.hue_jitter, 0.0, 1.0);
            slider(ui, "brush.wet_edges", &mut p.wet_edges, 0.0, 1.0);
            slider(ui, "brush.mix", &mut p.mix, 0.0, 1.0);
            slider(ui, "brush.smudge", &mut p.smudge_carry, 0.0, 1.0);
            if let Some(tx) = &mut p.texture {
                slider(ui, "brush.texture_depth", &mut tx.depth, 0.0, 1.0);
                slider(ui, "brush.texture_scale", &mut tx.scale, 0.5, 30.0);
            }
            ui.label(tr("brush.follow"));
            ui.checkbox(&mut p.follow_direction, "");
            ui.end_row();
            // modo de mistura do traço e antisserrilhado
            {
                use aiai_paint::brush::BrushMode as M;
                let key = |m: M| match m {
                    M::Normal => "bmode.normal",
                    M::Multiply => "bmode.multiply",
                    M::Screen => "bmode.screen",
                    M::Overlay => "bmode.overlay",
                    M::Add => "bmode.add",
                    M::Behind => "bmode.behind",
                    M::Erase => "bmode.erase",
                };
                ui.label(tr("brush.mode"));
                egui::ComboBox::from_id_salt("brush_mode").selected_text(tr(key(p.mode))).show_ui(ui, |ui| {
                    for m in [M::Normal, M::Multiply, M::Screen, M::Overlay, M::Add, M::Behind, M::Erase] {
                        ui.selectable_value(&mut p.mode, m, tr(key(m)));
                    }
                });
                ui.end_row();
                ui.label(tr("brush.aliased"));
                ui.checkbox(&mut p.aliased, "").on_hover_text(tr("brush.aliased.tip"));
                ui.end_row();
            }
            // duas cores no carimbo (pontas de forma): sombra derivada da cor do pincel por um tom e uma quantidade
            if matches!(p.tip, aiai_paint::brush::BrushTip::Shape { .. }) {
                ui.label(tr("brush.two_tone"));
                let mut on = p.two_tone.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    p.two_tone = on.then(|| base.as_ref().and_then(|b| b.two_tone.clone()).unwrap_or(aiai_paint::brush::TwoTone { tint: [0.55, 0.55, 0.7], amount: 0.8, color: None }));
                }
                ui.end_row();
                if let Some(tt) = &mut p.two_tone {
                    slider(ui, "brush.two_tone_amount", &mut tt.amount, 0.0, 1.0);
                    ui.label(tr("brush.two_tone_tint"));
                    ui.color_edit_button_rgb(&mut tt.tint);
                    ui.end_row();
                }
            }
        });
        if ui.add_enabled(o.brush_custom.is_some(), egui::Button::new(tr("brush.reset"))).clicked() {
            o.brush_custom = None;
        }
    });
    if p != before && p.validate().is_ok() {
        o.brush_custom = Some(p);
    }
}

fn row(ui: &mut Ui, t: &crate::theme::Tokens, compact: bool, key: &str, add: impl FnOnce(&mut Ui)) {
    if compact {
        // Na barra do canvas (linha que quebra) combo e slider são blocos aninhados que o egui não leva para a linha de baixo
        // (e `available_width` num layout que quebra é a largura inteira, não o que sobrou na linha):
        // guarda a largura do grupo rótulo+controle e quebra a linha antes quando ele não cabe no que sobrou (estouro, S20).
        let id = ui.id().with(("tb_row", key));
        let need: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
        if need > 0.0 && ui.available_rect_before_wrap().width() < need {
            ui.end_row();
        }
        let r = ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // só os campos numéricos têm rótulo; combos e cor mostram o nome ao passar o mouse
            ui.horizontal(|ui| {
                if !matches!(key, "opt.tip" | "opt.stabilizer" | "opt.preset" | "opt.select_mode" | "opt.color") {
                    ui.label(egui::RichText::new(tr(key)).size(12.0).color(t.text_dim));
                }
                add(ui);
            });
        });
        ui.data_mut(|d| d.insert_temp(id, r.response.rect.width() + ui.spacing().item_spacing.x));
    } else {
        ui.horizontal(|ui| {
            ui.set_min_width(110.0);
            let w = ui.available_width();
            let _ = w;
            ui.label(egui::RichText::new(tr(key)).size(12.0).color(t.text_dim));
            add(ui);
        });
    }
}

/// Opções da ferramenta ativa. `compact` = em linha (barra do canvas).
pub(crate) fn tool_options(env: &mut Env, ui: &mut Ui, compact: bool) {
    let t = env.t;
    let tool = *env.tool;
    // cor atual (vale para caneta, pincel e balde)
    if matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Fill | ToolId::Eyedropper) || !compact {
        row(ui, &t, compact, "opt.color", |ui| {
            let cur = cmds::resolve_color(&env.core.project, &env.ed.color);
            let mut c32 = rgba_to_color32(Rgba([cur.0[0], cur.0[1], cur.0[2], 1.0]));
            let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque);
            env.note("opt:color", r.rect);
            if r.changed() {
                env.ed.set_color_direct(color32_to_rgba(c32));
            }
            r.on_hover_text(tr("opt.color.tip"));
        });
    }
    match tool {
        ToolId::Pen => {
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.pen_shape", |ui| {
                let key = |m: PenShape| match m {
                    PenShape::Freehand => "pen.freehand",
                    PenShape::Line => "pen.line",
                    PenShape::Curve => "pen.curve",
                    PenShape::Rect => "pen.rect",
                    PenShape::Ellipse => "pen.ellipse",
                };
                egui::ComboBox::from_id_salt(("pen_shape", compact)).selected_text(tr(key(o.pen_shape))).show_ui(ui, |ui| {
                    for m in [PenShape::Freehand, PenShape::Line, PenShape::Curve, PenShape::Rect, PenShape::Ellipse] {
                        ui.selectable_value(&mut o.pen_shape, m, tr(key(m)));
                    }
                })
                .response
                .on_hover_text(tr("opt.pen_shape.tip"));
            });
            row(ui, &t, compact, "opt.width", |ui| {
                ui.add(egui::DragValue::new(&mut o.pen_width).range(0.5..=200.0).speed(0.1).max_decimals(1).suffix(" px"));
            });
            row(ui, &t, compact, "opt.tip", |ui| {
                let label = |t: VectorTip| match t {
                    VectorTip::GPen => "tip.g_pen",
                    VectorTip::MaruPen => "tip.maru_pen",
                    VectorTip::Marker => "tip.marker",
                    VectorTip::Pencil => "tip.pencil",
                };
                egui::ComboBox::from_id_salt(("pen_tip", compact)).width(100.0).selected_text(tr(label(o.pen_tip))).show_ui(ui, |ui| {
                    for tip in [VectorTip::GPen, VectorTip::MaruPen, VectorTip::Marker, VectorTip::Pencil] {
                        ui.selectable_value(&mut o.pen_tip, tip, tr(label(tip)));
                    }
                })
                .response
                .on_hover_text(tr("opt.tip"));
            });
            row(ui, &t, compact, "opt.stabilizer", |ui| {
                let label = |m: StabMode| match m {
                    StabMode::Off => "stab.off",
                    StabMode::Average => "stab.average",
                    StabMode::Lazy => "stab.lazy",
                };
                egui::ComboBox::from_id_salt(("pen_stab", compact)).width(100.0).selected_text(tr(label(o.stab_mode))).show_ui(ui, |ui| {
                    for m in [StabMode::Off, StabMode::Average, StabMode::Lazy] {
                        ui.selectable_value(&mut o.stab_mode, m, tr(label(m)));
                    }
                })
                .response
                .on_hover_text(tr("opt.stabilizer"));
                match o.stab_mode {
                    StabMode::Average => {
                        ui.add(egui::DragValue::new(&mut o.stab_window).range(1..=30).speed(0.2)).on_hover_text(tr("stab.window.tip"));
                    }
                    StabMode::Lazy => {
                        ui.add(egui::DragValue::new(&mut o.stab_radius).range(1.0..=80.0).speed(0.3).suffix(" pt").max_decimals(0)).on_hover_text(tr("stab.radius.tip"));
                    }
                    StabMode::Off => {}
                }
            });
            ui.checkbox(&mut o.pen_aliased, tr("opt.aliased"));
            ui.checkbox(&mut o.pen_autoclose, tr("opt.autoclose")).on_hover_text(tr("opt.autoclose.tip"));
        }
        ToolId::Brush => {
            // o pincel é escolhido no painel Pincéis (categorias e miniaturas): aqui só o nome, que leva até lá
            let base = env.ed.brushes.get(env.ed.opts.brush_preset).cloned();
            let name = base.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "-".into());
            let custom = if env.ed.opts.brush_custom.is_some() { " *" } else { "" };
            let mut open_panel = false;
            row(ui, &t, compact, "opt.preset", |ui| {
                let b = ui.button(format!("{name}{custom}  ›"));
                open_panel = b.on_hover_text(tr("opt.preset.tip")).clicked();
            });
            if open_panel {
                env.ed.want_panel = Some(crate::layout::PanelKind::Brushes);
            }
            let o = &mut env.ed.opts;
            let mut size_rect = None;
            row(ui, &t, compact, "opt.size", |ui| {
                size_rect = Some(ui.add(egui::DragValue::new(&mut o.brush_size).range(1.0..=600.0).speed(0.5).max_decimals(0).suffix(" px")).rect);
            });
            if let Some(r) = size_rect.filter(|_| !compact) {
                env.notes.push(("opt:brush_size".into(), r));
            }
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.opacity", |ui| {
                ui.spacing_mut().slider_width = if compact { 64.0 } else { 120.0 };
                ui.add(egui::Slider::new(&mut o.brush_opacity, 0.05..=1.0).show_value(!compact).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).trailing_fill(true)).on_hover_text(tr("opt.opacity"));
            });
            stabilizer_row(ui, &t, compact, o);
            if !compact {
                brush_settings(ui, o, base);
            }
        }
        ToolId::Eraser => {
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.size", |ui| {
                ui.add(egui::DragValue::new(&mut o.eraser_size).range(1.0..=600.0).speed(0.5).max_decimals(0).suffix(" px"));
            });
            row(ui, &t, compact, "opt.eraser_mode", |ui| {
                use aiai_tools::erase::EraseMode;
                let key = |m: EraseMode| match m {
                    EraseMode::Whole => "opt.erase_whole",
                    EraseMode::Segment => "opt.erase_segment",
                    EraseMode::Intersection => "opt.erase_intersection",
                };
                egui::ComboBox::from_id_salt(("eraser_mode", compact)).selected_text(tr(key(o.eraser_mode))).show_ui(ui, |ui| {
                    for m in [EraseMode::Segment, EraseMode::Intersection, EraseMode::Whole] {
                        ui.selectable_value(&mut o.eraser_mode, m, tr(key(m)));
                    }
                })
                .response
                .on_hover_text(tr("opt.eraser_mode.tip"));
            });
            if !compact {
                dim_label(ui, &t, tr("opt.eraser_hint"));
            }
        }
        ToolId::Fill => {
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.gap", |ui| {
                ui.add(egui::DragValue::new(&mut o.fill_gap).range(0.0..=30.0).speed(0.1).max_decimals(1).suffix(" px")).on_hover_text(tr("opt.gap.tip"));
            });
            row(ui, &t, compact, "opt.expand", |ui| {
                ui.add(egui::DragValue::new(&mut o.fill_expand).range(0..=12).speed(0.1).suffix(" px")).on_hover_text(tr("opt.expand.tip"));
            });
            let c = ui.checkbox(&mut o.fill_close_leftovers, tr("opt.close_leftovers")).on_hover_text(tr("opt.close_leftovers.tip"));
            env.note("opt:close_leftovers", c.rect);
            let b = ui.button(tr("opt.lint"));
            env.note("opt:lint", b.rect);
            if b.on_hover_text(tr("opt.lint.tip")).clicked() {
                tools::run_lint(env);
            }
            if let Some(msg) = &env.ed.lint_report {
                ui.label(egui::RichText::new(msg).size(12.0).color(t.text_dim));
            }
            if !compact {
                dim_label(ui, &t, tr("opt.fill_hint"));
            }
        }
        ToolId::Lasso => {
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.select_mode", |ui| {
                let label = |m: SelMode| match m {
                    SelMode::Rect => "sel.rect",
                    SelMode::Lasso => "sel.lasso",
                    SelMode::Ellipse => "sel.ellipse",
                    SelMode::Wand => "sel.wand",
                };
                egui::ComboBox::from_id_salt(("sel_mode", compact)).width(110.0).selected_text(tr(label(o.select_mode))).show_ui(ui, |ui| {
                    for m in [SelMode::Rect, SelMode::Lasso, SelMode::Ellipse, SelMode::Wand] {
                        ui.selectable_value(&mut o.select_mode, m, tr(label(m)));
                    }
                });
            });            if o.select_mode == SelMode::Wand {
                row(ui, &t, compact, "opt.wand_tolerance", |ui| {
                    ui.add(egui::Slider::new(&mut o.wand_tolerance, 0.0..=0.6).max_decimals(2)).on_hover_text(tr("opt.wand_tolerance"));
                });
            }
        }
        ToolId::Move => {
            let mp = ui.checkbox(&mut env.ed.opts.move_points, tr("opt.move_points")).on_hover_text(tr("opt.move_points.tip"));
            env.note("opt:move_points", mp.rect);
            if env.ed.opts.move_points {
                let b = ui.add_enabled(env.ed.node_stroke.is_some(), egui::Button::new(tr("opt.simplify")));
                env.note("opt:simplify", b.rect);
                if b.on_hover_text(tr("opt.simplify.tip")).clicked() {
                    super::super::nodes::simplify(env, 1.5);
                }
                return;
            }
            let pending = tools::has_pending_move(env.ed);
            let apply = ui.add_enabled(pending, egui::Button::new(tr("opt.move_apply")));
            env.note("opt:move_apply", apply.rect);
            if apply.on_hover_text(tr("opt.move_apply.tip")).clicked() {
                tools::commit_move(env);
            }
            let cancel = ui.add_enabled(env.ed.selection.is_some(), egui::Button::new(tr("opt.move_cancel")));
            env.note("opt:move_cancel", cancel.rect);
            if cancel.on_hover_text(tr("opt.move_cancel.tip")).clicked() {
                tools::cancel_move(env.ed);
            }
        }
        ToolId::Eyedropper | ToolId::Hand | ToolId::Zoom => {
            if !compact {
                dim_label(ui, &t, tr(match tool {
                    ToolId::Eyedropper => "opt.eyedropper_hint",
                    ToolId::Hand => "opt.hand_hint",
                    _ => "opt.zoom_hint",
                }));
            }
        }
    }
}

fn stabilizer_row(ui: &mut Ui, t: &crate::theme::Tokens, compact: bool, o: &mut ToolOptions) {
    row(ui, t, compact, "opt.stabilizer", |ui| {
        let label = |m: StabMode| match m {
            StabMode::Off => "stab.off",
            StabMode::Average => "stab.average",
            StabMode::Lazy => "stab.lazy",
        };
        egui::ComboBox::from_id_salt(("brush_stab", compact)).width(100.0).selected_text(tr(label(o.stab_mode))).show_ui(ui, |ui| {
            for m in [StabMode::Off, StabMode::Average, StabMode::Lazy] {
                ui.selectable_value(&mut o.stab_mode, m, tr(label(m)));
            }
        })
        .response
        .on_hover_text(tr("opt.stabilizer.tip"));
        // força da correção de tremura
        match o.stab_mode {
            StabMode::Average => {
                ui.add(egui::DragValue::new(&mut o.stab_window).range(1..=30).speed(0.2)).on_hover_text(tr("stab.window.tip"));
            }
            StabMode::Lazy => {
                ui.add(egui::DragValue::new(&mut o.stab_radius).range(1.0..=80.0).speed(0.3).suffix(" pt").max_decimals(0)).on_hover_text(tr("stab.radius.tip"));
            }
            StabMode::Off => {}
        }
    });
}

fn layer_properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(layer) = env.ed.active_layer(env.core) else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    let name = env.core.project.node(&layer).map(|n| n.name.clone()).unwrap_or_default();
    section(ui, &t, "props.layer");
    egui::Grid::new("layer_props_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("props.name"));
        ui.label(name);
        ui.end_row();
        dim_label(ui, &t, tr("props.type"));
        ui.label(tr(cmds::layer_type_key(l.kind)));
        ui.end_row();
        ui.label(egui::RichText::new(tr("props.role")).size(12.0).color(t.text_dim)).on_hover_text(tr("props.role.tip"));
        let mut role = l.role;
        egui::ComboBox::from_id_salt("layer_role").width(130.0).selected_text(tr(role_key(role))).show_ui(ui, |ui| {
            for r in ROLES {
                ui.selectable_value(&mut role, r, tr(role_key(r)));
            }
        });
        if role != l.role {
            let _ = cmds::set_layer_props(env.core, &layer, json!({ "role": role }));
        }
        ui.end_row();
        ui.label(egui::RichText::new(tr("props.depth")).size(12.0).color(t.text_dim)).on_hover_text(tr("props.depth.tip"));
        if let Some(v) = commit_on_release(ui, Id::new(("depth", &layer)), l.depth, |ui, v| ui.add(egui::DragValue::new(v).range(0.0..=100.0).speed(0.05).max_decimals(2))) {
            let _ = cmds::set_layer_props(env.core, &layer, json!({ "depth": v }));
        }
        ui.end_row();
        ui.label(egui::RichText::new(tr("props.offset")).size(12.0).color(t.text_dim)).on_hover_text(tr("props.offset.tip"));
        ui.horizontal(|ui| {
            for axis in 0..2 {
                if let Some(v) = commit_on_release(ui, Id::new(("offset", &layer, axis)), l.offset[axis], |ui, v| ui.add(egui::DragValue::new(v).speed(0.5).max_decimals(1).suffix(" px"))) {
                    let mut o = l.offset;
                    o[axis] = v;
                    let _ = cmds::set_layer_props(env.core, &layer, json!({ "offset": o }));
                }
            }
        });
        ui.end_row();
    });
}

fn cut_properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(cut) = env.ed.cut.clone() else { return };
    let Some(cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
    let name = env.core.project.node(&cut).map(|n| n.name.clone()).unwrap_or_default();
    let (cw, ch) = cmds::canvas_size(&env.core.project, &cut);
    section(ui, &t, "props.cut");
    egui::Grid::new("cut_props_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("props.name"));
        ui.label(name);
        ui.end_row();
        dim_label(ui, &t, tr("props.canvas"));
        ui.label(format!("{cw} x {ch} px"));
        ui.end_row();
        dim_label(ui, &t, tr("props.duration"));
        ui.label(format!("{} {}", cd.duration, tr("unit.frames")));
        ui.end_row();
    });
    // tamanho do canvas do corte: predefinições (o quadro de saída continua o do projeto; maior = cenário para a câmera)
    let (pw, ph) = (env.core.project.settings.width, env.core.project.settings.height);
    let presets: [(&str, Option<[u32; 2]>); 6] = [
        ("props.canvas_project", None),
        ("props.canvas_1080", Some([1920, 1080])),
        ("props.canvas_1440", Some([2560, 1440])),
        ("props.canvas_4k", Some([3840, 2160])),
        ("props.canvas_pan_h", Some([pw * 2, ph])),
        ("props.canvas_pan_v", Some([pw, ph * 2])),
    ];
    let mut chosen: Option<Option<[u32; 2]>> = None;
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("props.canvas_size"));
        let current = cd.canvas;
        let label = presets.iter().find(|(_, v)| *v == current).map(|(k, _)| tr(k).to_owned()).unwrap_or_else(|| format!("{cw} x {ch}"));
        let r = egui::ComboBox::from_id_salt("cut_canvas_preset").selected_text(label).show_ui(ui, |ui| {
            for (k, v) in presets {
                let text = match v {
                    Some([w, h]) => format!("{}  ({w} x {h})", tr(k)),
                    None => format!("{}  ({pw} x {ph})", tr(k)),
                };
                if ui.selectable_label(current == v, text).clicked() {
                    chosen = Some(v);
                }
            }
        });
        env.note("props:cut_canvas", r.response.rect);
    });
    ui.horizontal(|ui| {
        let id = ui.id().with(("cut_canvas_custom", cut.0.clone()));
        let mut wh: [u32; 2] = ui.data_mut(|d| *d.get_temp_mut_or(id, [cw, ch]));
        ui.add(egui::DragValue::new(&mut wh[0]).range(16..=16384).speed(4.0).suffix(" px"));
        ui.label("x");
        ui.add(egui::DragValue::new(&mut wh[1]).range(16..=16384).speed(4.0).suffix(" px"));
        ui.data_mut(|d| d.insert_temp(id, wh));
        let b = ui.add_enabled(wh != [cw, ch], egui::Button::new(tr("props.apply")));
        env.note("props:cut_canvas_apply", b.rect);
        if b.clicked() {
            chosen = Some(Some(wh));
        }
    });
    if let Some(v) = chosen {
        let data = match v {
            Some([w, h]) => serde_json::json!({ "canvas": [w, h] }),
            None => serde_json::json!({ "canvas": null }),
        };
        if let Err(e) = cmds::run(env.core, aiai_core::Command::SetProps { id: cut.clone(), name: None, data }) {
            env.ed.notify(e);
        }
    }
}

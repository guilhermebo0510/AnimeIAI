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
            let presets = tools::presets();
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.preset", |ui| {
                let name = presets.get(o.brush_preset).map(|p| p.name.as_str()).unwrap_or("-");
                egui::ComboBox::from_id_salt(("brush_preset", compact)).width(130.0).selected_text(name).show_ui(ui, |ui| {
                    for (i, p) in presets.iter().enumerate() {
                        if ui.selectable_label(o.brush_preset == i, &p.name).clicked() {
                            o.brush_preset = i;
                            o.brush_size = p.size;
                            o.brush_opacity = p.opacity;
                        }
                    }
                })
                .response
                .on_hover_text(tr("opt.preset"));
            });
            row(ui, &t, compact, "opt.size", |ui| {
                ui.add(egui::DragValue::new(&mut o.brush_size).range(1.0..=600.0).speed(0.5).max_decimals(0).suffix(" px"));
            });
            row(ui, &t, compact, "opt.opacity", |ui| {
                ui.spacing_mut().slider_width = if compact { 64.0 } else { 120.0 };
                ui.add(egui::Slider::new(&mut o.brush_opacity, 0.05..=1.0).show_value(!compact).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).trailing_fill(true)).on_hover_text(tr("opt.opacity"));
            });
            stabilizer_row(ui, &t, compact, o);
        }
        ToolId::Eraser => {
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.size", |ui| {
                ui.add(egui::DragValue::new(&mut o.eraser_size).range(1.0..=600.0).speed(0.5).max_decimals(0).suffix(" px"));
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
                };
                egui::ComboBox::from_id_salt(("sel_mode", compact)).width(110.0).selected_text(tr(label(o.select_mode))).show_ui(ui, |ui| {
                    for m in [SelMode::Rect, SelMode::Lasso, SelMode::Ellipse] {
                        ui.selectable_value(&mut o.select_mode, m, tr(label(m)));
                    }
                });
            });
        }
        ToolId::Move => {
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
        .on_hover_text(tr("opt.stabilizer"));
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
        dim_label(ui, &t, tr("props.role"));
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
        dim_label(ui, &t, tr("props.depth"));
        if let Some(v) = commit_on_release(ui, Id::new(("depth", &layer)), l.depth, |ui, v| ui.add(egui::DragValue::new(v).range(0.0..=100.0).speed(0.05).max_decimals(2))) {
            let _ = cmds::set_layer_props(env.core, &layer, json!({ "depth": v }));
        }
        ui.end_row();
        dim_label(ui, &t, tr("props.offset"));
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
}

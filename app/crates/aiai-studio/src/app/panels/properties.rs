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
            WorkspaceId::Personagens => {
                // a camada 3D ativa (referência sobre o material) mostra a seção 3D acima da ficha
                crate::app::model3d::properties(env, ui);
                return super::characters::properties(env, ui);
            }
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
            // os efeitos da camada ficam no painel Controles de efeito; aqui a câmera e a cor do corte
            super::satsuei::camera(env, ui);
            // a grade do corte (lift/gamma/gain), que ficava no antigo workspace Cor, agora é trabalho do satsuei
            ui.add_space(4.0);
            egui::CollapsingHeader::new(tr("fx.grade_section")).id_salt("props_grade").default_open(false).show(ui, |ui| super::satsuei::grade(env, ui));
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
    let mut all = o.brush_show_all;
    egui::CollapsingHeader::new(tr("brush.settings")).id_salt("brush_settings").default_open(true).show(ui, |ui| {
        preset_grid(ui, &mut p, all, base.as_ref(), "tool");
        ui.checkbox(&mut all, tr("brush.show_all")).on_hover_text(tr("brush.show_all.tip"));
        if ui.add_enabled(o.brush_custom.is_some(), egui::Button::new(tr("brush.reset"))).clicked() {
            o.brush_custom = None;
        }
    });
    o.brush_show_all = all;
    if p != before && p.validate().is_ok() {
        o.brush_custom = Some(p);
    }
}

/// Grade dos parâmetros do motor de um preset (painel da ferramenta e criador de pincel). `all` = todos; senão só os
/// que o pincel usa. `salt` separa os ids quando duas grades aparecem juntas.
pub(crate) fn preset_grid(ui: &mut Ui, p: &mut aiai_paint::brush::BrushPreset, all: bool, base: Option<&aiai_paint::brush::BrushPreset>, salt: &str) {
    let shape_tip = matches!(p.tip, aiai_paint::brush::BrushTip::Shape { .. });
    egui::Grid::new(("brush_settings_grid", salt)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let slider = |ui: &mut Ui, key: &str, v: &mut f32, lo: f32, hi: f32| {
            ui.label(tr(key));
            ui.add(egui::Slider::new(v, lo..=hi).max_decimals(2));
            ui.end_row();
        };
        // ponta: redonda (dureza, achatamento, ângulo) ou forma de cenário (folha, grama, nuvem...)
        {
            use aiai_paint::brush::{BrushTip, TipShape};
            const SHAPES: [(TipShape, &str); 14] = [
                (TipShape::Leaf, "tipshape.leaf"),
                (TipShape::Petal, "tipshape.petal"),
                (TipShape::GrassBlade, "tipshape.grass"),
                (TipShape::CloudPuff, "tipshape.cloud"),
                (TipShape::Star, "tipshape.star"),
                (TipShape::Square, "tipshape.square"),
                (TipShape::Rose, "tipshape.rose"),
                (TipShape::Sakura, "tipshape.sakura"),
                (TipShape::Feather, "tipshape.feather"),
                (TipShape::Bokeh, "tipshape.bokeh"),
                (TipShape::Tree, "tipshape.tree"),
                (TipShape::Maple, "tipshape.maple"),
                (TipShape::Pebble, "tipshape.pebble"),
                (TipShape::Glint, "tipshape.glint"),
            ];
            let current = match &p.tip {
                BrushTip::Round { .. } => tr("tipshape.round").to_owned(),
                BrushTip::Shape { shape, .. } => SHAPES.iter().find(|(s, _)| s == shape).map(|(_, k)| tr(k).to_owned()).unwrap_or_default(),
                BrushTip::Image { .. } => tr("tipshape.image").to_owned(),
            };
            ui.label(tr("brush.tip"));
            let mut next: Option<BrushTip> = None;
            egui::ComboBox::from_id_salt(("brush_tip_kind", salt)).selected_text(current).show_ui(ui, |ui| {
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
        if all || p.flow_pressure_min < 1.0 {
            slider(ui, "brush.flow_pressure", &mut p.flow_pressure_min, 0.0, 1.0);
        }
        if all || p.scatter > 0.0 || shape_tip {
            slider(ui, "brush.scatter", &mut p.scatter, 0.0, 2.0);
        }
        if all || p.size_jitter > 0.0 || shape_tip {
            slider(ui, "brush.size_jitter", &mut p.size_jitter, 0.0, 1.0);
        }
        if all || p.angle_jitter > 0.0 || shape_tip {
            slider(ui, "brush.angle_jitter", &mut p.angle_jitter, 0.0, 180.0);
        }
        if all || p.value_jitter > 0.0 || p.hue_jitter > 0.0 {
            slider(ui, "brush.value_jitter", &mut p.value_jitter, 0.0, 1.0);
            slider(ui, "brush.hue_jitter", &mut p.hue_jitter, 0.0, 1.0);
        }
        if all || p.wet_edges > 0.0 {
            slider(ui, "brush.wet_edges", &mut p.wet_edges, 0.0, 1.0);
        }
        if all || p.mix > 0.0 {
            slider(ui, "brush.mix", &mut p.mix, 0.0, 1.0);
            slider(ui, "brush.smudge", &mut p.smudge_carry, 0.0, 1.0);
        }
        if let Some(tx) = &mut p.texture {
            slider(ui, "brush.texture_depth", &mut tx.depth, 0.0, 1.0);
            slider(ui, "brush.texture_scale", &mut tx.scale, 0.5, 30.0);
        }
        let elliptic = matches!(p.tip, aiai_paint::brush::BrushTip::Round { roundness, .. } if roundness < 0.98);
        if all || p.follow_direction || shape_tip || elliptic {
            ui.label(tr("brush.follow"));
            ui.checkbox(&mut p.follow_direction, "");
            ui.end_row();
        }
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
            egui::ComboBox::from_id_salt(("brush_mode", salt)).selected_text(tr(key(p.mode))).show_ui(ui, |ui| {
                for m in [M::Normal, M::Multiply, M::Screen, M::Overlay, M::Add, M::Behind, M::Erase] {
                    ui.selectable_value(&mut p.mode, m, tr(key(m)));
                }
            });
            ui.end_row();
            if all || p.aliased {
                ui.label(tr("brush.aliased"));
                ui.checkbox(&mut p.aliased, "").on_hover_text(tr("brush.aliased.tip"));
                ui.end_row();
            }
        }
        // duas cores no carimbo (pontas de forma): sombra derivada da cor do pincel por um tom e uma quantidade
        if matches!(p.tip, aiai_paint::brush::BrushTip::Shape { .. }) {
            ui.label(tr("brush.two_tone"));
            let mut on = p.two_tone.is_some();
            if ui.checkbox(&mut on, "").changed() {
                p.two_tone = on.then(|| base.and_then(|b| b.two_tone.clone()).unwrap_or(aiai_paint::brush::TwoTone { tint: [0.55, 0.55, 0.7], amount: 0.8, color: None }));
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
}

pub(crate) const POINTS_MODES: [PointsMode; 5] = [PointsMode::Move, PointsMode::Add, PointsMode::Delete, PointsMode::Corner, PointsMode::Width];

pub(crate) fn points_mode_key(m: PointsMode) -> &'static str {
    match m {
        PointsMode::Move => "pts.mode.move",
        PointsMode::Add => "pts.mode.add",
        PointsMode::Delete => "pts.mode.delete",
        PointsMode::Corner => "pts.mode.corner",
        PointsMode::Width => "pts.mode.width",
    }
}

/// Campo numérico do Editar pontos: enquanto arrasta/digita o valor fica guardado e grava ao soltar ou sair do
/// campo (um passo de desfazer por edição, não um por quadro).
#[allow(clippy::too_many_arguments)]
fn pts_field(env: &mut Env, ui: &mut Ui, key: &'static str, label: &str, cur: f32, min: f32, max: f32, set: impl FnOnce(&mut aiai_core::Stroke, f32) -> bool) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(env.t.text_dim));
        let mut v = match env.ed.node_field {
            Some((k, v)) if k == key => v,
            _ => cur,
        };
        let r = ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(0.1).max_decimals(2).suffix(match key {
            "x" | "y" => "",
            "hang" => "°",
            _ => " px",
        }));
        env.note(format!("pts:{key}"), r.rect);
        let editing = r.dragged() || r.has_focus();
        if editing {
            env.ed.node_field = Some((key, v));
        }
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !editing) {
            env.ed.node_field = None;
            if (v - cur).abs() > 1e-4 {
                super::super::nodes::edit(env, label, |s| set(s, v));
            }
        }
    });
}

pub(crate) fn row(ui: &mut Ui, t: &crate::theme::Tokens, compact: bool, key: &str, add: impl FnOnce(&mut Ui)) {
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
    tool_options_in(env, ui, compact, false);
}

/// Opções no painel Ferramenta: os seletores que viraram itens da lista (pincel, pena, forma, modo da borracha, modo
/// da seleção) ficam de fora; sobram as propriedades do item escolhido.
pub(crate) fn tool_options_panel(env: &mut Env, ui: &mut Ui) {
    tool_options_in(env, ui, false, true);
}

fn tool_options_in(env: &mut Env, ui: &mut Ui, compact: bool, panel: bool) {
    let t = env.t;
    let tool = *env.tool;
    // cor atual (vale para caneta, pincel e balde; a borracha não tem cor). No painel Propriedades da ferramenta (S25)
    // a cor fica só no painel Cores, ao lado
    if !panel && (matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Fill | ToolId::Eyedropper) || !compact) && !matches!(tool, ToolId::Eraser | ToolId::Text | ToolId::Ruler | ToolId::Rotate | ToolId::Anchor | ToolId::Camera | ToolId::Model3d) {
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
            if !panel {
            row(ui, &t, compact, "opt.pen_shape", |ui| {
                let key = crate::app::figure::label_key;
                egui::ComboBox::from_id_salt(("pen_shape", compact)).selected_text(tr(key(o.pen_shape))).show_ui(ui, |ui| {
                    for m in std::iter::once(PenShape::Freehand).chain(crate::app::figure::DIRECT) {
                        ui.selectable_value(&mut o.pen_shape, m, tr(key(m)));
                    }
                })
                .response
                .on_hover_text(tr("opt.pen_shape.tip"));
            });
            }
            row(ui, &t, compact, "opt.width", |ui| {
                ui.add(egui::DragValue::new(&mut o.pen_width).range(0.5..=200.0).speed(0.1).max_decimals(1).suffix(" px"));
            });
            if !panel {
            row(ui, &t, compact, "opt.tip", |ui| {
                let label = crate::app::shelf::tip_key;
                egui::ComboBox::from_id_salt(("pen_tip", compact)).width(100.0).selected_text(tr(label(o.pen_tip))).show_ui(ui, |ui| {
                    for tip in VectorTip::ALL {
                        ui.selectable_value(&mut o.pen_tip, tip, tr(label(tip)));
                    }
                })
                .response
                .on_hover_text(tr("opt.tip"));
            });
            }
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
            row(ui, &t, compact, "opt.pen_simplify", |ui| {
                ui.add(egui::Slider::new(&mut o.pen_simplify, 0.0..=100.0).max_decimals(0)).on_hover_text(tr("opt.pen_simplify.tip"));
            });
            row(ui, &t, compact, "opt.pen_taper_in", |ui| {
                ui.add(egui::Slider::new(&mut o.pen_taper_in, 0.0..=400.0).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.pen_taper.tip"));
            });
            row(ui, &t, compact, "opt.pen_taper_out", |ui| {
                ui.add(egui::Slider::new(&mut o.pen_taper_out, 0.0..=400.0).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.pen_taper.tip"));
            });
            ui.checkbox(&mut o.pen_color_trace, tr("opt.pen_color_trace")).on_hover_text(tr("opt.pen_color_trace.tip"));
            ui.checkbox(&mut o.pen_weld, tr("opt.pen_weld")).on_hover_text(tr("opt.pen_weld.tip"));
            ui.checkbox(&mut o.pen_quickshape, tr("opt.pen_quickshape")).on_hover_text(tr("opt.pen_quickshape.tip"));
        }
        ToolId::Brush => {
            // o pincel é escolhido no painel Pincéis (categorias e miniaturas): aqui só o nome, que leva até lá
            let base = env.ed.brushes.get(env.ed.opts.brush_preset).cloned();
            let name = base.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "-".into());
            let custom = if env.ed.opts.brush_custom.is_some() { " *" } else { "" };
            let mut open_panel = false;
            if !panel {
                row(ui, &t, compact, "opt.preset", |ui| {
                    let b = ui.button(format!("{name}{custom}  ›"));
                    open_panel = b.on_hover_text(tr("opt.preset.tip")).clicked();
                });
            } else {
                ui.label(egui::RichText::new(format!("{name}{custom}")).strong());
            }
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
            let r = ui.checkbox(&mut o.brush_no_overflow, tr("opt.brush_no_overflow")).on_hover_text(tr("opt.brush_no_overflow.tip"));
            env.notes.push(("opt:brush_no_overflow".into(), r.rect));
            let o = &mut env.ed.opts;
            if !compact {
                brush_settings(ui, o, base);
            }
        }
        ToolId::Eraser => {
            let brush_name = env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| p.name.clone()).unwrap_or_default();
            let o = &mut env.ed.opts;
            row(ui, &t, compact, "opt.size", |ui| {
                ui.add(egui::DragValue::new(&mut o.eraser_size).range(1.0..=600.0).speed(0.5).max_decimals(0).suffix(" px"));
            });
            // borracha raster como a do Photoshop: ponta (Pincel/Lápis/Bloco), dureza, opacidade, fluxo e pressão
            let mut hard_rect = None;
            eraser_rows(ui, &t, compact, o, &brush_name, &mut hard_rect);
            if let Some(r) = hard_rect.filter(|_| !compact) {
                env.notes.push(("opt:eraser_hardness".into(), r));
            }
            let o = &mut env.ed.opts;
            if !panel {
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
            }
            if !compact {
                dim_label(ui, &t, tr("opt.eraser_hint"));
            }
        }
        ToolId::Fill => {
            // jeito do balde (S25): tolerância na camada em edição, pincel na área não preenchida
            let mode = env.ed.opts.area.fill_mode;
            if mode == crate::app::area_tools::FillMode::Editing {
                let mut r0 = None;
                row(ui, &t, compact, "opt.fill_tolerance", |ui| {
                    r0 = Some(ui.add(egui::Slider::new(&mut env.ed.opts.area.fill_tolerance, 0.0..=255.0).step_by(1.0).max_decimals(0)).on_hover_text(tr("opt.fill_tolerance.tip")).rect);
                });
                if let Some(r) = r0 {
                    env.note("opt:fill_tolerance", r);
                }
            }
            if mode == crate::app::area_tools::FillMode::Unfilled {
                let mut r0 = None;
                row(ui, &t, compact, "opt.fill_brush", |ui| {
                    r0 = Some(ui.add(egui::DragValue::new(&mut env.ed.opts.area.fill_brush).range(2.0..=1000.0).speed(0.5).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.fill_brush.tip")).rect);
                });
                if let Some(r) = r0 {
                    env.note("opt:fill_brush", r);
                }
            }
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
            let n_holes = env.ed.lint_marks.holes.len();
            if n_holes > 0 {
                ui.horizontal(|ui| {
                    let prev = ui.small_button("‹").on_hover_text(tr("lint.prev"));
                    env.note("lint:prev", prev.rect);
                    if prev.clicked() {
                        tools::lint_go(env, -1);
                    }
                    ui.label(egui::RichText::new(format!("{}/{}", env.ed.lint_marks.current + 1, n_holes)).size(12.0));
                    let next = ui.small_button("›").on_hover_text(tr("lint.next"));
                    env.note("lint:next", next.rect);
                    if next.clicked() {
                        tools::lint_go(env, 1);
                    }
                    let fill = ui.small_button(tr("lint.fill")).on_hover_text(tr("lint.fill.tip"));
                    env.note("lint:fill", fill.rect);
                    if fill.clicked() {
                        tools::lint_fill_current(env);
                    }
                    let clear = ui.small_button("✕").on_hover_text(tr("lint.clear"));
                    env.note("lint:clear", clear.rect);
                    if clear.clicked() {
                        env.ed.lint_marks = Default::default();
                        env.ed.lint_report = None;
                    }
                });
            }
            if !compact {
                dim_label(ui, &t, tr(match env.ed.opts.area.fill_mode {
                    crate::app::area_tools::FillMode::Others => "opt.fill_mode_hint.others",
                    crate::app::area_tools::FillMode::Editing => "opt.fill_mode_hint.editing",
                    crate::app::area_tools::FillMode::Enclose => "opt.fill_mode_hint.enclose",
                    crate::app::area_tools::FillMode::Unfilled => "opt.fill_mode_hint.unfilled",
                }));
            }
        }
        ToolId::Lasso => crate::app::selection::tool_options(env, ui, compact, panel),
        // Composição: o Mover leva a camada inteira (posição/rotação/escala da camada); as opções de desenho (seleção
        // automática de traço, distorção, malha, liquefazer) não valem aqui e não aparecem (auditoria UX-12)
        ToolId::Move if env.ws == crate::workspace::WorkspaceId::Composicao => {
            dim_label(ui, &t, tr("opt.move_comp_hint"));
        }
        ToolId::Move => {

            // arrastar move e aplica ao soltar (Ctrl+Z desfaz): sem Aplicar/Enter
            // o Mover só move; "Seleção automática" (Photoshop) faz o clique num traço escolhê-lo antes de arrastar
            let r = ui.checkbox(&mut env.ed.opts.move_auto, tr("opt.move_auto"));
            env.note("opt:move_auto", r.rect);
            r.on_hover_text(tr("opt.move_auto.tip"));
            let r = ui.checkbox(&mut env.ed.opts.keep_aspect, tr("opt.keep_aspect"));
            env.note("opt:keep_aspect", r.rect);
            r.on_hover_text(tr("opt.keep_aspect.tip"));
            let n = env.ed.selection.as_ref().map(|s| s.strokes.len()).unwrap_or(0);
            if n > 0 && !compact {
                dim_label(ui, &t, &format!("{} {n}", tr("opt.move_picked")));
            }
            if !compact {
                dim_label(ui, &t, tr("opt.move_distort_hint"));
            }
            // Liquefazer (como o Liquify do Clip Studio: a pincelada entorta o desenho e grava a cada pincelada) e Malha
            // (grade sobre a seleção, cada vértice arrasta; Enter grava, Esc desfaz), lado a lado
            let has_area = env.ed.selection.as_ref().is_some_and(|s| !s.whole);
            ui.horizontal_wrapped(|ui| {
                let b = ui.selectable_label(env.ed.opts.liquify, tr("opt.liquify"));
                env.note("opt:liquify", b.rect);
                if b.on_hover_text(tr("opt.liquify.tip")).clicked() {
                    env.ed.opts.liquify = !env.ed.opts.liquify;
                }
                if has_area && !env.ed.opts.liquify {
                    let on = env.ed.selection.as_ref().is_some_and(|s| s.mesh.is_some());
                    let b = ui.selectable_label(on, tr("opt.move_mesh"));
                    env.note("opt:move_mesh", b.rect);
                    if b.on_hover_text(tr("opt.move_mesh.tip")).clicked() {
                        let n = env.ed.opts.mesh_cells;
                        if let Some(s) = env.ed.selection.as_mut() {
                            if on {
                                s.mesh = None;
                            } else {
                                s.start_mesh(n);
                            }
                        }
                    }
                    if !on {
                        let r = ui.add(egui::DragValue::new(&mut env.ed.opts.mesh_cells).range(1..=8).suffix("×"));
                        env.note("opt:move_mesh_cells", r.rect);
                    }
                }
            });
            if env.ed.opts.liquify {
                use aiai_paint::warp::LiquifyKind as K;
                ui.horizontal_wrapped(|ui| {
                    for (k, key) in [(K::Push, "opt.liquify.push"), (K::Bulge, "opt.liquify.bulge"), (K::Pinch, "opt.liquify.pinch"), (K::Twirl, "opt.liquify.twirl")] {
                        let b = ui.selectable_label(env.ed.opts.liquify_kind == k, tr(key));
                        env.note(format!("opt:{}", &key[4..]), b.rect);
                        if b.clicked() {
                            env.ed.opts.liquify_kind = k;
                        }
                    }
                });
                row(ui, &t, compact, "opt.liquify.size", |ui| {
                    let r = ui.add(egui::Slider::new(&mut env.ed.opts.liquify_size, 4.0..=600.0).logarithmic(true).suffix(" px"));
                    env.note("opt:liquify_size", r.rect);
                });
                row(ui, &t, compact, "opt.liquify.strength", |ui| {
                    let r = ui.add(egui::Slider::new(&mut env.ed.opts.liquify_strength, 0.0..=1.0));
                    env.note("opt:liquify_strength", r.rect);
                });
            }
            if env.ed.selection.as_ref().is_some_and(|s| !s.whole) {
                let b = ui.button(tr("opt.move_deselect"));
                env.note("opt:move_deselect", b.rect);
                if b.on_hover_text(tr("opt.move_deselect.tip")).clicked() {
                    env.ed.selection = None;
                }
            }
        }
        // Corrigir linha: puxar, simplificar, conectar, largura, redesenhar e poeira têm opções próprias
        ToolId::Points if crate::app::line_fix::active(env) => crate::app::line_fix::options(env, ui, compact),
        ToolId::Points => {
            use super::super::nodes;
            if !panel {
                row(ui, &t, compact, "opt.points_mode", |ui| {
                    let o = &mut env.ed.opts;
                    egui::ComboBox::from_id_salt(("pts_mode", compact)).width(130.0).selected_text(tr(points_mode_key(o.points_mode))).show_ui(ui, |ui| {
                        for m in POINTS_MODES {
                            ui.selectable_value(&mut o.points_mode, m, tr(points_mode_key(m)));
                        }
                    });
                });
            }
            // na barra do canvas só o modo: campos que aparecem ao escolher um traço quebrariam a linha e o canvas
            // mudaria de tamanho (e de zoom) debaixo do mouse
            match nodes::selection(env).filter(|_| !compact) {
                None => {
                    if !compact {
                        dim_label(ui, &t, tr("pts.none"));
                    }
                }
                Some((s, sel)) => {
                    if !compact {
                        ui.label(egui::RichText::new(format!("{} · {} {}", tr("pts.stroke"), s.points.len(), tr("pts.points"))).strong());
                    }
                    pts_field(env, ui, "stroke_w", tr("pts.stroke_width"), s.width, 0.5, 200.0, |st, v| {
                        st.width = v;
                        true
                    });
                    ui.horizontal_wrapped(|ui| {
                        let b = ui.button(tr("pts.all_smooth"));
                        env.note("pts:all_smooth", b.rect);
                        if b.clicked() {
                            nodes::edit(env, "todos suaves", |st| {
                                let had = st.points.iter().any(|q| q.corner);
                                st.points.iter_mut().for_each(|q| q.corner = false);
                                had
                            });
                        }
                        let b = ui.button(tr("pts.all_corner"));
                        env.note("pts:all_corner", b.rect);
                        if b.clicked() {
                            nodes::edit(env, "todos cantos", |st| {
                                let had = st.points.iter().any(|q| !q.corner);
                                st.points.iter_mut().for_each(|q| q.corner = true);
                                had
                            });
                        }
                        let b = ui.button(tr("opt.simplify"));
                        env.note("opt:simplify", b.rect);
                        if b.on_hover_text(tr("opt.simplify.tip")).clicked() {
                            nodes::simplify(env, 1.5);
                        }
                    });
                    match sel {
                        Some(i) => {
                            let q = s.points[i];
                            if !compact {
                                ui.label(egui::RichText::new(format!("{} {i}", tr("pts.point"))).strong()).on_hover_text(tr("pts.point.tip"));
                            }
                            pts_field(env, ui, "w", tr("pts.width"), aiai_paint::width_at_point(&s, i), 0.0, 400.0, move |st, v| {
                                st.points[i].width = aiai_paint::point_width_for(st, i, v);
                                true
                            });
                            pts_field(env, ui, "x", "X", q.x, -100000.0, 100000.0, move |st, v| {
                                st.points[i].x = v;
                                true
                            });
                            pts_field(env, ui, "y", "Y", q.y, -100000.0, 100000.0, move |st, v| {
                                st.points[i].y = v;
                                true
                            });
                            if let Some(h) = aiai_paint::point_handle(&s, i).filter(|_| !q.corner) {
                                let (len, ang) = (h[0].hypot(h[1]), h[1].atan2(h[0]).to_degrees());
                                pts_field(env, ui, "hlen", tr("pts.handle_len"), len, 0.0, 5000.0, move |st, v| {
                                    let a = ang.to_radians();
                                    st.points[i].handle = Some([v * a.cos(), v * a.sin()]);
                                    true
                                });
                                pts_field(env, ui, "hang", tr("pts.handle_angle"), ang, -360.0, 360.0, move |st, v| {
                                    let a = v.to_radians();
                                    st.points[i].handle = Some([len * a.cos(), len * a.sin()]);
                                    true
                                });
                                let b = ui.add_enabled(q.handle.is_some(), egui::Button::new(tr("pts.handle_auto")));
                                env.note("pts:handle_auto", b.rect);
                                if b.on_hover_text(tr("pts.handle_auto.tip")).clicked() {
                                    nodes::auto_handle(env);
                                }
                            }
                            let mut corner = q.corner;
                            let r = ui.checkbox(&mut corner, tr("pts.corner"));
                            env.note("pts:corner", r.rect);
                            if r.on_hover_text(tr("pts.corner.tip")).changed() {
                                nodes::edit(env, "canto/suave", move |st| {
                                    st.points[i].corner = corner;
                                    true
                                });
                            }
                            let b = ui.add_enabled(s.points.len() > 2, egui::Button::new(tr("pts.delete")));
                            env.note("pts:delete", b.rect);
                            if b.clicked() {
                                nodes::delete_selected(env);
                            }
                        }
                        None => {
                            if !compact {
                                dim_label(ui, &t, tr("pts.pick_point"));
                            }
                        }
                    }
                }
            }
            if !compact {
                dim_label(ui, &t, tr(match env.ed.opts.points_mode {
                    PointsMode::Move => "opt.move_points.tip",
                    PointsMode::Add => "pts.mode.add.tip",
                    PointsMode::Delete => "pts.mode.delete.tip",
                    PointsMode::Corner => "pts.mode.corner.tip",
                    PointsMode::Width => "pts.mode.width.tip",
                }));
            }
        }
        ToolId::Text => super::text::controls(env, ui, compact),
        ToolId::Ruler => super::rulers::tool_options(env, ui, compact),
        ToolId::Model3d => crate::app::model3d::tool_options(env, ui, compact),
        ToolId::Eyedropper | ToolId::Hand | ToolId::Zoom => {
            if !compact {
                dim_label(ui, &t, tr(match tool {
                    ToolId::Eyedropper => "opt.eyedropper_hint",
                    ToolId::Hand => "opt.hand_hint",
                    _ => "opt.zoom_hint",
                }));
            }
        }
        // ferramentas do viewer da Composição: a dica de uso
        ToolId::Rotate | ToolId::Anchor | ToolId::Camera => {
            if !compact {
                dim_label(ui, &t, tr(match tool {
                    ToolId::Rotate => "comp.hint.rotate",
                    ToolId::Anchor => "comp.hint.anchor",
                    _ => "comp.hint.camera",
                }));
            }
        }
        // Modelagem: as opções ficam no cabeçalho do viewport 3D
        _ => {}
    }
}

/// Controles da borracha raster: ponta, dureza 0–100% (só na ponta Pincel redonda), opacidade, fluxo, pressão e
/// "usar o pincel atual". `hard_rect` recebe o retângulo do controle de dureza (bridge).
fn eraser_rows(ui: &mut Ui, t: &crate::theme::Tokens, compact: bool, o: &mut ToolOptions, brush_name: &str, hard_rect: &mut Option<egui::Rect>) {
    use crate::app::editor::EraserTip;
    let key = |m: EraserTip| match m {
        EraserTip::Brush => "eraser.tip.brush",
        EraserTip::Pencil => "eraser.tip.pencil",
        EraserTip::Block => "eraser.tip.block",
    };
    row(ui, t, compact, "opt.eraser_tip", |ui| {
        egui::ComboBox::from_id_salt(("eraser_tip", compact)).selected_text(tr(key(o.eraser_tip))).show_ui(ui, |ui| {
            for m in [EraserTip::Brush, EraserTip::Pencil, EraserTip::Block] {
                ui.selectable_value(&mut o.eraser_tip, m, tr(key(m)));
            }
        })
        .response
        .on_hover_text(tr("opt.eraser_tip.tip"));
    });
    let sw = if compact { 64.0 } else { 120.0 };
    let pct = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:.0}%", v);
    let hard_on = o.eraser_tip == EraserTip::Brush && !o.eraser_use_brush;
    row(ui, t, compact, "brush.hardness", |ui| {
        ui.spacing_mut().slider_width = sw;
        let mut v = o.eraser_hardness * 100.0;
        let r = ui.add_enabled(hard_on, egui::Slider::new(&mut v, 0.0..=100.0).show_value(!compact).custom_formatter(pct).trailing_fill(true));
        if r.changed() {
            o.eraser_hardness = (v / 100.0).clamp(0.0, 1.0);
        }
        *hard_rect = Some(r.rect);
        r.on_hover_text(tr("opt.eraser_hardness.tip"));
    });
    for (k, val) in [("opt.opacity", &mut o.eraser_opacity), ("opt.flow", &mut o.eraser_flow)] {
        row(ui, t, compact, k, |ui| {
            ui.spacing_mut().slider_width = sw;
            let mut v = *val * 100.0;
            if ui.add(egui::Slider::new(&mut v, 1.0..=100.0).show_value(!compact).custom_formatter(pct).trailing_fill(true)).on_hover_text(tr(k)).changed() {
                *val = (v / 100.0).clamp(0.01, 1.0);
            }
        });
    }
    if !compact {
        ui.checkbox(&mut o.eraser_pressure_size, tr("opt.pressure_size"));
        ui.checkbox(&mut o.eraser_pressure_opacity, tr("opt.pressure_opacity"));
        ui.checkbox(&mut o.eraser_use_brush, format!("{} ({brush_name})", tr("opt.eraser_use_brush"))).on_hover_text(tr("opt.eraser_use_brush.tip"));
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

pub(crate) fn layer_properties(env: &mut Env, ui: &mut Ui) {
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
    if l.kind == LayerType::Model3d {
        crate::app::model3d::properties(env, ui);
    }
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

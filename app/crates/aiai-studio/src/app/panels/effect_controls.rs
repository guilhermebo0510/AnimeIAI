//! Painel Controles de efeito da Composição (S24 3.5), como o Effect Controls do After Effects: a pilha de efeitos da
//! camada selecionada, cada efeito recolhível com ligar/desligar, subir/descer, reiniciar e tirar; cada parâmetro com o
//! cronômetro (liga a animação: grava a chave no quadro atual), o losango do quadro atual (cria/tira a chave) e o
//! valor (com o cronômetro ligado, mudar o valor grava a chave no quadro). As chaves são as de `EffectInstance.keys`,
//! as mesmas que a Linha do tempo da composição mostra e que o render avalia.

use super::commit_on_release;
use crate::app::actions;
use crate::app::comp::{self, PropRef, Unit};
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::{LayerData, LayerType};
use egui::{Id as EId, Rect, Sense, Ui};
use serde_json::{Value, json};

/// Rótulo de uma propriedade (transformação ou caminho de parâmetro de efeito).
pub(crate) fn prop_label(p: &PropRef) -> String {
    match &p.owner {
        comp::Owner::Layer(_) => tr(&format!("comp.prop.{}", p.name)).to_owned(),
        comp::Owner::Effect(..) => {
            let last = p.name.rsplit('.').next().unwrap_or(&p.name);
            super::satsuei::field_label(last)
        }
    }
}

/// Campos de valor de uma propriedade (um por trilha). Devolve os valores novos quando o gesto termina.
pub(crate) fn value_widgets(ui: &mut Ui, l: &LayerData, p: &PropRef, frame: u32, salt: &str, compact: bool) -> Option<Vec<f32>> {
    let vals = comp::values(l, p, frame);
    if vals.is_empty() {
        return None;
    }
    let u = comp::unit(p);
    let id = EId::new(("comp_val", salt, &p.owner, &p.name));
    if u == Unit::Color && (vals.len() == 3 || vals.len() == 4) {
        let mut out = None;
        if vals.len() == 3 {
            let mut c = [vals[0], vals[1], vals[2]];
            if ui.color_edit_button_rgb(&mut c).changed() {
                out = Some(c.to_vec());
            }
        } else {
            let mut c = egui::Rgba::from_rgba_unmultiplied(vals[0], vals[1], vals[2], vals[3]);
            if egui::widgets::color_picker::color_edit_button_rgba(ui, &mut c, egui::widgets::color_picker::Alpha::OnlyBlend).changed() {
                out = Some(c.to_rgba_unmultiplied().to_vec());
            }
        }
        return out;
    }
    let last = p.name.rsplit('.').next().unwrap_or(&p.name);
    let (lo, hi, speed) = match (&p.owner, u) {
        (comp::Owner::Layer(_), Unit::Percent) => (if p.name == "opacity" { 0.0 } else { 0.1 }, if p.name == "opacity" { 100.0 } else { 10000.0 }, 0.5),
        (comp::Owner::Layer(_), Unit::Degrees) => (-36000.0, 36000.0, 0.5),
        (comp::Owner::Layer(_), _) => (-100000.0, 100000.0, 0.5),
        _ => super::satsuei::field_range(last),
    };
    let scale = if u == Unit::Percent { 100.0 } else { 1.0 };
    let suffix = match u {
        Unit::Percent => "%",
        Unit::Degrees => "°",
        Unit::Px if !compact => " px",
        _ => "",
    };
    let mut out = None;
    for (i, v) in vals.iter().enumerate() {
        let shown = v * scale;
        if let Some(x) = commit_on_release(ui, id.with(i), shown, |ui, x| ui.add(egui::DragValue::new(x).range(lo..=hi).speed(speed).max_decimals(if compact { 1 } else { 3 }).suffix(suffix))) {
            let mut nv = vals.clone();
            nv[i] = x / scale;
            out = Some(nv);
        }
    }
    out
}

/// Cronômetro + losango do quadro atual de uma propriedade. Devolve (cronômetro clicado, losango clicado).
pub(crate) fn stopwatch(env: &mut Env, ui: &mut Ui, l: &LayerData, p: &PropRef, note: &str) -> (bool, bool) {
    let t = env.t;
    let anim = comp::animated(l, p);
    let sw = env.icon_button_sized(ui, IconId::Stopwatch, if anim { "comp.stopwatch_off" } else { "comp.stopwatch_on" }, anim, true, &format!("{note}:stopwatch"), 20.0, false);
    let mut diamond = false;
    if anim {
        let here = comp::key_frames(l, p).contains(&env.ed.frame);
        let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), Sense::click());
        env.note(format!("{note}:key"), r);
        paint_diamond(ui.painter(), r.center(), 5.0, if here { Some(t.accent) } else { None }, if resp.hovered() { t.text } else { t.text_dim });
        diamond = resp.on_hover_text(tr(if here { "comp.key_remove" } else { "comp.key_add" })).clicked();
    } else {
        ui.add_space(16.0 + ui.spacing().item_spacing.x);
    }
    (sw.clicked(), diamond)
}

/// Losango de chave (cheio = `fill`).
pub(crate) fn paint_diamond(p: &egui::Painter, c: egui::Pos2, r: f32, fill: Option<egui::Color32>, stroke: egui::Color32) {
    let pts = vec![c + egui::vec2(0.0, -r), c + egui::vec2(r, 0.0), c + egui::vec2(0.0, r), c + egui::vec2(-r, 0.0)];
    p.add(egui::Shape::convex_polygon(pts, fill.unwrap_or(egui::Color32::TRANSPARENT), egui::Stroke::new(1.2, stroke)));
}

/// Linha de uma propriedade: cronômetro, losango, nome e valor. Aplica o que mudou (um passo de desfazer).
pub(crate) fn prop_row(env: &mut Env, ui: &mut Ui, layer: &NodeId, l: &LayerData, p: &PropRef, note: &str) {
    let t = env.t;
    let frame = env.ed.frame;
    ui.horizontal(|ui| {
        let (sw, diamond) = stopwatch(env, ui, l, p, note);
        let lr = ui.add(egui::Label::new(egui::RichText::new(prop_label(p)).size(12.0).color(t.text_dim)).sense(Sense::click()));
        if lr.clicked() {
            env.ed.comp.sel_prop = Some(p.clone());
        }
        if let Some(v) = value_widgets(ui, l, p, frame, "ec", false) {
            comp::edit(env.core, env.ed, layer, "comp.undo.value", |l| comp::set_values(l, p, frame, &v));
        }
        if sw {
            comp::edit(env.core, env.ed, layer, "comp.undo.stopwatch", |l| comp::toggle_stopwatch(l, p, frame).map(|_| ()));
        }
        if diamond {
            comp::edit(env.core, env.ed, layer, "comp.undo.add_key", |l| comp::toggle_key_here(l, p, frame));
        }
    });
}

fn effects_op(env: &mut Env, op: &str, mut fields: Value) -> bool {
    fields["op"] = json!(op);
    let r = actions::run(env.core, "EFFECTS", fields);
    actions::notify_err(env.ed, r).is_some()
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(layer) = env.ed.active_layer(env.core) else {
        dim_label(ui, &t, tr("panel.effect_controls.empty"));
        return;
    };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    let name = env.core.project.node(&layer).map(|n| n.name.clone()).unwrap_or_default();
    ui.label(egui::RichText::new(name).strong().size(13.0));
    dim_label(ui, &t, tr(if l.kind == LayerType::Effect { "fx.scope_below" } else { "fx.scope_content" }));
    // aceitar efeito arrastado do navegador
    let st = comp::stack(&l);
    if let Err(e) = aiai_fx::stack_from_params(&l.params) {
        ui.colored_label(t.danger, e);
    }
    let resp_area = egui::ScrollArea::vertical().id_salt(("ec_scroll", &layer.0)).auto_shrink([false, false]).show(ui, |ui| {
        if st.is_empty() {
            dim_label(ui, &t, tr("comp.no_effects_hint"));
        }
        for (i, inst) in st.iter().enumerate() {
            effect_block(env, ui, &layer, &l, i, st.len(), inst);
            ui.add_space(4.0);
        }
    });
    let area = resp_area.inner_rect;
    env.note("ec:area", area);
    let drop = ui.interact(area, ui.id().with(("ec_drop", &layer.0)), Sense::hover());
    if drop.dnd_hover_payload::<super::effects_browser::FxPayload>().is_some() {
        ui.painter().rect_stroke(area, 4.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    if let Some(p) = drop.dnd_release_payload::<super::effects_browser::FxPayload>() {
        super::effects_browser::apply(env, &layer, &p);
    }
}

fn effect_block(env: &mut Env, ui: &mut Ui, layer: &NodeId, l: &LayerData, i: usize, n: usize, inst: &aiai_fx::EffectInstance) {
    let t = env.t;
    let key = (layer.clone(), i);
    let open = !env.ed.comp.fx_closed.contains(&key);
    let ty = inst.effect.name();
    egui::Frame::new().fill(t.bg_widget).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let chev = env.icon_button_sized(ui, if open { IconId::ChevronDown } else { IconId::ChevronRight }, "comp.twirl", false, true, &format!("ec:fx:{i}:twirl"), 18.0, false);
            if chev.clicked() {
                if open {
                    env.ed.comp.fx_closed.insert(key.clone());
                } else {
                    env.ed.comp.fx_closed.remove(&key);
                }
            }
            let mut on = inst.enabled;
            let r = ui.checkbox(&mut on, egui::RichText::new(super::satsuei::effect_label(ty)).strong());
            env.note(format!("ec:fx:{i}:enabled"), r.rect);
            if on != inst.enabled {
                effects_op(env, "update", json!({ "layer": layer, "index": i, "enabled": on }));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if env.icon_button(ui, IconId::Trash, "fx.remove", false, true, &format!("ec:fx:{i}:remove")).clicked() {
                    effects_op(env, "remove", json!({ "layer": layer, "index": i }));
                    env.ed.comp.sel_keys.retain(|k| k.prop.owner.layer() != layer);
                }
                let reset = env.icon_button(ui, IconId::Loop, "comp.reset_effect", false, true, &format!("ec:fx:{i}:reset"));
                if reset.clicked() {
                    if let Some(cut) = env.ed.cut.clone() {
                        let (w, h) = cmds::canvas_size(&env.core.project, &cut);
                        match super::effects_browser::default_effect(ty, w as f32, h as f32) {
                            Some(e) => {
                                effects_op(env, "update", json!({ "layer": layer, "index": i, "effect": e, "keys": {}, "posterize_time": 1 }));
                            }
                            None => env.ed.notify(tr("comp.lut_needs_file").to_owned()),
                        }
                    }
                }
                let down = env.icon_button(ui, IconId::ArrowDown, "fx.down", false, i + 1 < n, &format!("ec:fx:{i}:down"));
                let up = env.icon_button(ui, IconId::ArrowUp, "fx.up", false, i > 0, &format!("ec:fx:{i}:up"));
                let swap = if up.clicked() { Some(i - 1) } else if down.clicked() { Some(i + 1) } else { None };
                if let Some(j) = swap {
                    let mut st = comp::stack(l);
                    st.swap(i, j);
                    if effects_op(env, "set", json!({ "layer": layer, "effects": st })) {
                        // as chaves selecionadas apontam para o índice antigo
                        env.ed.comp.sel_keys.retain(|k| k.prop.owner.layer() != layer);
                    }
                }
            });
        });
        if !open {
            return;
        }
        for row in comp::effect_rows(&inst.effect) {
            let p = PropRef::effect(layer, i, &row);
            prop_row(env, ui, layer, l, &p, &format!("ec:fx:{i}:{row}"));
        }
        // campos que não são números: liga/desliga e modo de mistura; o resto (tipo do ajuste) fica para o MCP
        other_fields(env, ui, layer, i, inst);
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("fx.posterize"));
            let post = inst.posterize_time;
            if let Some(v) = commit_on_release(ui, EId::new(("ec_post", &layer.0, i)), post, |ui, v| ui.add(egui::DragValue::new(v).range(1..=6).speed(0.05).suffix(format!(" {}", tr("unit.frames_short"))))) {
                effects_op(env, "update", json!({ "layer": layer, "index": i, "posterize_time": v }));
            }
        });
    });
}

fn other_fields(env: &mut Env, ui: &mut Ui, layer: &NodeId, i: usize, inst: &aiai_fx::EffectInstance) {
    let t = env.t;
    let Ok(Value::Object(obj)) = serde_json::to_value(&inst.effect) else { return };
    for (k, v) in &obj {
        match (k.as_str(), v) {
            ("type", _) => {}
            (_, Value::Bool(b)) => {
                let mut x = *b;
                let r = ui.checkbox(&mut x, super::satsuei::field_label(k));
                env.note(format!("ec:fx:{i}:{k}"), r.rect);
                if x != *b {
                    let mut e = obj.clone();
                    e.insert(k.clone(), json!(x));
                    effects_op(env, "update", json!({ "layer": layer, "index": i, "effect": e }));
                }
            }
            ("mode", Value::String(cur)) => {
                ui.horizontal(|ui| {
                    dim_label(ui, &t, &super::satsuei::field_label(k));
                    let mut pick = None;
                    egui::ComboBox::from_id_salt(("ec_mode", &layer.0, i)).selected_text(tr(&format!("blend.{cur}"))).show_ui(ui, |ui| {
                        for m in ["normal", "multiply", "screen", "overlay", "soft_light", "hard_light", "add", "color", "luminosity"] {
                            if ui.selectable_label(cur == m, tr(&format!("blend.{m}"))).clicked() {
                                pick = Some(m);
                            }
                        }
                    });
                    if let Some(m) = pick {
                        let mut e = obj.clone();
                        e.insert(k.clone(), json!(m));
                        effects_op(env, "update", json!({ "layer": layer, "index": i, "effect": e }));
                    }
                });
            }
            (_, Value::Object(o)) if o.get("type").and_then(Value::as_str).is_some() => {
                let ty = o.get("type").and_then(Value::as_str).unwrap_or("?");
                dim_label(ui, &t, &format!("{}: {ty} {}", super::satsuei::field_label(k), tr("fx.mcp_only")));
            }
            _ => {}
        }
    }
}

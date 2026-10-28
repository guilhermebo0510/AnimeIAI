//! Painel Propriedades da camada (S25, entre o Navegador e as Camadas, como no Clip Studio): a camada ativa com o tipo,
//! os filtros (efeitos da camada) em lista — ligar/desligar, tirar, editar os parâmetros e pôr outro pelo mesmo cardápio
//! do menu Filtro — e o papel, a profundidade, o deslocamento (e a seção 3D na camada 3D). As opções da ferramenta
//! ficam no painel de propriedades da ferramenta, à esquerda; aqui é só a camada.

use crate::app::commands as cmds;
use crate::app::ui_kit::{Env, body_ui, dim_label, paint_icon, section};
use crate::i18n::tr;
use crate::icons::IconId;
use egui::{Rect, Sense, Ui};
use serde_json::json;

/// Nome legível de um filtro da pilha (o ajuste mostra qual ajuste é).
pub(crate) fn filter_name(inst: &aiai_fx::EffectInstance) -> String {
    if let aiai_fx::Effect::Adjust { adjustment } = &inst.effect {
        let ty = serde_json::to_value(adjustment).ok().and_then(|v| v.get("type").and_then(|t| t.as_str().map(str::to_owned))).unwrap_or_default();
        let k = format!("filter.{ty}");
        let s = tr(&k);
        if s != k {
            return s.to_owned();
        }
    }
    super::satsuei::effect_label(inst.effect.name())
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(layer) = env.ed.active_layer(env.core) else {
        ui.label(egui::RichText::new(tr("panel.layer_props.empty")).color(t.text_dim));
        return;
    };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    let name = env.core.project.node(&layer).map(|n| n.name.clone()).unwrap_or_default();
    egui::ScrollArea::vertical().id_salt("layer_props_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
            paint_icon(env.icons, ui.painter(), super::layers::kind_icon(l.kind), r, t.text_dim, false);
            ui.label(egui::RichText::new(&name).strong().size(13.0));
            dim_label(ui, &t, tr(cmds::layer_type_key(l.kind)));
        });
        // camada 3D: sai ou não na entrega, logo abaixo do nome
        if l.kind == aiai_core::LayerType::Model3d
            && let Some(Ok(m)) = aiai_core::model3d::Model3dParams::from_layer_params(&l.params)
        {
            crate::app::model3d::output_state(env, ui, &layer, &m, "lprops:m3d");
        }
        // ---- filtros da camada
        section(ui, &t, "lprops.filters");
        let st = crate::app::comp::stack(&l);
        if st.is_empty() {
            dim_label(ui, &t, tr("lprops.no_filters"));
        }
        let mut op: Option<serde_json::Value> = None;
        for (i, inst) in st.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if env.icon_button_sized(ui, if inst.enabled { IconId::Eye } else { IconId::EyeOff }, "lprops.filter_toggle", false, true, &format!("lprops:filter_eye:{i}"), 20.0, false).clicked() {
                    op = Some(json!({ "op": "update", "layer": layer, "index": i, "enabled": !inst.enabled }));
                }
                let label = egui::RichText::new(filter_name(inst)).size(12.5).color(if inst.enabled { t.text } else { t.text_dim });
                let r = ui.add(egui::Label::new(label).sense(Sense::click()));
                env.note(format!("lprops:filter:{i}"), r.rect);
                if r.on_hover_text(tr("lprops.filter_edit")).clicked() {
                    env.ed.workbar.filter_open = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if env.icon_button_sized(ui, IconId::Trash, "lprops.filter_remove", false, true, &format!("lprops:filter_remove:{i}"), 20.0, false).clicked() {
                        op = Some(json!({ "op": "remove", "layer": layer, "index": i }));
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            let enabled = crate::app::workbar::filterable(l.kind);
            let b = ui.add_enabled(enabled, egui::Button::new(format!("+ {}", tr("menu.filter"))));
            env.note("lprops:add_filter", b.rect);
            let canvas = env.ed.cut.as_ref().map(|c| cmds::canvas_size(&env.core.project, c)).unwrap_or((1920, 1080));
            let mut picked = None;
            egui::Popup::menu(&b).show(|ui| {
                picked = crate::app::workbar::filter_menu_items(ui, env.notes, true, canvas);
            });
            if let Some(e) = picked {
                crate::app::workbar::apply_filter(env, e);
            }
            if !st.is_empty() {
                let e = ui.button(tr("filter.edit"));
                env.note("lprops:edit_filters", e.rect);
                if e.clicked() {
                    env.ed.workbar.filter_open = true;
                }
            }
        });
        if let Some(p) = op {
            if let Err(e) = crate::app::actions::run(env.core, "EFFECTS", p) {
                env.ed.notify(e);
            }
        }
        // ---- modificadores de traço (camada vetorial): line boil, vento, espelho
        if l.kind == aiai_core::LayerType::Vector {
            stroke_mods(env, ui, &layer, &l);
        }
        // ---- partículas (o sistema da camada)
        if l.kind == aiai_core::LayerType::Particles {
            super::particles_ed::editor(env, ui, &layer, &l);
        }
        // ---- papel, profundidade, deslocamento (e 3D)
        super::properties::layer_properties(env, ui);
    });
}

/// Modificadores de traço (auditoria 4.7h): aplicados no render, quadro a quadro; o desenho guardado não muda.
fn stroke_mods(env: &mut Env, ui: &mut Ui, layer: &cmds::NodeId, l: &aiai_core::LayerData) {
    use aiai_render::stroke_mods::{Boil, Mirror, StrokeMods, Wind};
    let t = env.t;
    section(ui, &t, "lprops.stroke_mods");
    let cur: StrokeMods = l.params.get("stroke_mods").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let mut m = cur.clone();
    let canvas_w = env.ed.cut.as_ref().map(|c| cmds::canvas_size(&env.core.project, c).0 as f32).unwrap_or(1920.0);
    let lid = layer.0.clone();
    let slider = |ui: &mut Ui, key: &str, v: f32, lo: f32, hi: f32, suffix: &str| -> Option<f32> {
        super::commit_on_release(ui, egui::Id::new(("smod", &lid, key)), v, |ui, x| ui.add(egui::Slider::new(x, lo..=hi).max_decimals(1).suffix(suffix)))
    };
    // tremer a linha
    let mut on = m.boil.is_some();
    let r = ui.checkbox(&mut on, tr("lprops.boil")).on_hover_text(tr("lprops.boil.tip"));
    env.note("lprops:boil", r.rect);
    if on != m.boil.is_some() {
        m.boil = on.then_some(Boil { amount: 1.5, every: 2, scale: 40.0 });
    }
    if let Some(b) = m.boil.as_mut() {
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.amount"));
            if let Some(v) = slider(ui, "boil_amount", b.amount, 0.2, 10.0, " px") {
                b.amount = v;
            }
        });
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.boil_every"));
            let mut e = b.every;
            if ui.add(egui::DragValue::new(&mut e).range(1..=12).suffix(format!(" {}", tr("unit.frames_short")))).changed() {
                b.every = e;
            }
        });
    }
    // vento
    let mut on = m.wind.is_some();
    let r = ui.checkbox(&mut on, tr("lprops.wind")).on_hover_text(tr("lprops.wind.tip"));
    env.note("lprops:wind", r.rect);
    if on != m.wind.is_some() {
        m.wind = on.then_some(Wind { amount: 12.0, speed: 0.8, angle_deg: 0.0 });
    }
    if let Some(w) = m.wind.as_mut() {
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.amount"));
            if let Some(v) = slider(ui, "wind_amount", w.amount, 1.0, 150.0, " px") {
                w.amount = v;
            }
        });
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.wind_speed"));
            if let Some(v) = slider(ui, "wind_speed", w.speed, 0.1, 4.0, "/s") {
                w.speed = v;
            }
        });
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.wind_angle"));
            if let Some(v) = slider(ui, "wind_angle", w.angle_deg, 0.0, 359.0, "°") {
                w.angle_deg = v;
            }
        });
    }
    // espelho
    let mut on = m.mirror.is_some();
    let r = ui.checkbox(&mut on, tr("lprops.mirror")).on_hover_text(tr("lprops.mirror.tip"));
    env.note("lprops:mirror", r.rect);
    if on != m.mirror.is_some() {
        m.mirror = on.then_some(Mirror { vertical: false, center: canvas_w / 2.0 });
    }
    if let Some(mi) = m.mirror.as_mut() {
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("lprops.mirror_axis"));
            if let Some(v) = super::commit_on_release(ui, egui::Id::new(("smod", &lid, "mirror_c")), mi.center, |ui, x| ui.add(egui::DragValue::new(x).speed(1.0).suffix(" px"))) {
                mi.center = v;
            }
            let mut vert = mi.vertical;
            if ui.checkbox(&mut vert, tr("lprops.mirror_vertical")).changed() {
                mi.vertical = vert;
            }
        });
    }
    if m != cur {
        let mut params = l.params.clone();
        if !params.is_object() {
            params = json!({});
        }
        if m.boil.is_none() && m.wind.is_none() && m.mirror.is_none() {
            if let Some(o) = params.as_object_mut() {
                o.remove("stroke_mods");
            }
        } else {
            params["stroke_mods"] = serde_json::to_value(&m).unwrap_or_default();
        }
        if let Err(e) = cmds::set_layer_props(env.core, layer, json!({ "params": params })) {
            env.ed.notify(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_names_are_readable() {
        let blur: aiai_fx::EffectInstance = serde_json::from_value(json!({ "effect": { "type": "gaussian_blur", "radius": 3.0 } })).unwrap();
        assert_eq!(filter_name(&blur), tr("fx.e.gaussian_blur"));
        let lv: aiai_fx::EffectInstance = serde_json::from_value(json!({ "effect": { "type": "adjust", "adjustment": { "type": "levels", "in_black": 0.0, "in_white": 1.0, "gamma": 1.0, "out_black": 0.0, "out_white": 1.0 } } })).unwrap();
        assert_eq!(filter_name(&lv), tr("filter.levels"));
    }
}

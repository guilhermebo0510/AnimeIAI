//! Editor de partículas (auditoria 4.5g) no painel Propriedades da camada: presets prontos (pétalas, brilhos, neve,
//! chuva, poeira) e os parâmetros principais do sistema (`params.particles` da camada). Cada ajuste vira um passo de
//! desfazer quando o controle é solto.

use super::commit_on_release;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, dim_label, section};
use crate::i18n::tr;
use aiai_anim::particles::ParticleSystem;
use aiai_core::LayerData;
use egui::{Id, Ui};
use serde_json::json;

const PRESETS: [(&str, &str); 5] = [
    ("particles.p.petals", "petalas"),
    ("particles.p.sparkles", "brilhos"),
    ("particles.p.snow", "neve"),
    ("particles.p.rain", "chuva"),
    ("particles.p.dust", "poeira"),
];

fn save(env: &mut Env, layer: &NodeId, l: &LayerData, sys: &ParticleSystem) {
    let mut params = if l.params.is_object() { l.params.clone() } else { json!({}) };
    params["particles"] = serde_json::to_value(sys).unwrap_or(serde_json::Value::Null);
    if let Err(e) = cmds::set_layer_props(env.core, layer, json!({ "params": params })) {
        env.ed.notify(e);
    }
}

/// Número com rótulo; devolve o valor novo quando o controle é solto.
fn num(ui: &mut Ui, env: &mut Env, key: &str, salt: &str, cur: f32, range: std::ops::RangeInclusive<f32>, speed: f64) -> Option<f32> {
    let t = env.t;
    let mut out = None;
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr(key));
        let mut rect = egui::Rect::NOTHING;
        out = commit_on_release(ui, Id::new(("particles", salt)), cur, |ui, v| {
            let r = ui.add(egui::DragValue::new(v).range(range).speed(speed).max_decimals(1));
            rect = r.rect;
            r
        });
        env.note(format!("particles:{salt}"), rect);
    });
    out
}

pub(crate) fn editor(env: &mut Env, ui: &mut Ui, layer: &NodeId, l: &LayerData) {
    let t = env.t;
    section(ui, &t, "lprops.particles");
    let (w, h) = env.ed.cut.as_ref().map(|c| cmds::canvas_size(&env.core.project, c)).unwrap_or((1920, 1080));
    let current: Option<ParticleSystem> = l.params.get("particles").and_then(|v| serde_json::from_value(v.clone()).ok());
    let mut preset: Option<&str> = None;
    ui.horizontal_wrapped(|ui| {
        for (key, name) in PRESETS {
            let b = ui.button(tr(key));
            env.note(format!("particles:preset:{name}"), b.rect);
            if b.clicked() {
                preset = Some(name);
            }
        }
    });
    if let Some(name) = preset {
        let seed = current.as_ref().map(|s| s.seed).unwrap_or(1);
        if let Some(sys) = aiai_anim::particles::preset(name, w as f32, h as f32, seed) {
            save(env, layer, l, &sys);
        }
        return;
    }
    let Some(mut sys) = current else {
        dim_label(ui, &t, tr("particles.none"));
        return;
    };
    let before = sys.clone();
    if let Some(v) = num(ui, env, "particles.rate", "rate", sys.rate, 0.0..=600.0, 0.5) {
        sys.rate = v;
    }
    if let Some(v) = num(ui, env, "particles.life_min", "life0", sys.lifetime[0], 0.1..=30.0, 0.05) {
        sys.lifetime[0] = v.min(sys.lifetime[1]);
    }
    if let Some(v) = num(ui, env, "particles.life_max", "life1", sys.lifetime[1], 0.1..=30.0, 0.05) {
        sys.lifetime[1] = v.max(sys.lifetime[0]);
    }
    if let Some(v) = num(ui, env, "particles.speed_min", "speed0", sys.speed[0], 0.0..=3000.0, 1.0) {
        sys.speed[0] = v.min(sys.speed[1]);
    }
    if let Some(v) = num(ui, env, "particles.speed_max", "speed1", sys.speed[1], 0.0..=3000.0, 1.0) {
        sys.speed[1] = v.max(sys.speed[0]);
    }
    if let Some(v) = num(ui, env, "particles.direction", "direction", sys.direction, -360.0..=360.0, 1.0) {
        sys.direction = v;
    }
    if let Some(v) = num(ui, env, "particles.spread", "spread", sys.spread, 0.0..=360.0, 1.0) {
        sys.spread = v;
    }
    if let Some(v) = num(ui, env, "particles.gravity", "gravity", sys.gravity[1], -2000.0..=2000.0, 2.0) {
        sys.gravity[1] = v;
    }
    if let Some(v) = num(ui, env, "particles.wind", "wind", sys.wind[0], -2000.0..=2000.0, 2.0) {
        sys.wind[0] = v;
    }
    if let Some(v) = num(ui, env, "particles.turbulence", "turbulence", sys.turbulence[0], 0.0..=2000.0, 2.0) {
        sys.turbulence[0] = v;
    }
    if let Some(v) = num(ui, env, "particles.size_min", "size0", sys.size[0], 0.5..=400.0, 0.2) {
        sys.size[0] = v.min(sys.size[1]);
    }
    if let Some(v) = num(ui, env, "particles.size_max", "size1", sys.size[1], 0.5..=400.0, 0.2) {
        sys.size[1] = v.max(sys.size[0]);
    }
    if let Some(v) = num(ui, env, "particles.prewarm", "prewarm", sys.prewarm, 0.0..=20.0, 0.05) {
        sys.prewarm = v;
    }
    if let Some(v) = num(ui, env, "particles.twinkle", "twinkle", sys.twinkle, 0.0..=20.0, 0.05) {
        sys.twinkle = v;
    }
    if let Some(v) = num(ui, env, "particles.seed", "seed", sys.seed as f32, 0.0..=9999.0, 0.2) {
        sys.seed = v.round().max(0.0) as u64;
    }
    // cores sorteadas: uma amostra por cor, a primeira editável
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("particles.colors"));
        for (i, c) in sys.colors.iter_mut().enumerate() {
            let mut rgba = egui::Rgba::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            let r = egui::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::OnlyBlend);
            env.note(format!("particles:color:{i}"), r.rect);
            if r.changed() {
                let u = rgba.to_rgba_unmultiplied();
                *c = [u[0], u[1], u[2], u[3]];
            }
        }
    });
    if sys != before && sys.validate().is_ok() {
        save(env, layer, l, &sys);
    }
}

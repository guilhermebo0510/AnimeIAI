//! Composição e Cor: no painel Propriedades da Composição, a câmera do corte (`CAMERA`; a pilha de efeitos da camada
//! fica no painel Controles de efeito, `effect_controls`); no workspace Cor, o painel Navegador mostra o quadro do corte (monitor em segundo plano) e
//! o Propriedades edita a cor do corte (`GRADE`: rodas lift/gamma/gain, saturação, temperatura, contraste, e vinheta,
//! difusão e grão).
//!
//! Os efeitos são editados como o JSON da instância (`{effect: {type, ...campos}, enabled, posterize_time}`): números
//! viram campos arrastáveis, cores viram seletor, `[x, y]` vira dois campos. A ação da bridge valida e devolve o erro
//! como aviso; o documento só muda se a pilha for válida. Um ajuste = um passo de desfazer.

use super::commit_on_release;
use crate::app::actions;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, dim_label, section};
use crate::i18n::tr;
use aiai_core::{LayerType, NodeKind, Project};
use egui::{Id as EId, Ui};
use serde_json::{Map, Value, json};

/// Efeitos que o menu "Adicionar efeito" oferece, com valores iniciais razoáveis (`w`, `h` = canvas do corte).
pub(crate) fn effect_presets(w: f32, h: f32) -> Vec<(&'static str, Value)> {
    vec![
        ("fx.e.gaussian_blur", json!({ "type": "gaussian_blur", "radius": 4.0 })),
        ("fx.e.directional_blur", json!({ "type": "directional_blur", "angle": 0.0, "length": 24.0 })),
        ("fx.e.glow", json!({ "type": "glow", "threshold": 0.7, "radius": 14.0, "intensity": 0.8 })),
        ("fx.e.diffusion", json!({ "type": "diffusion", "radius": 12.0, "amount": 0.35, "threshold": 0.4 })),
        ("fx.e.halation", json!({ "type": "halation", "threshold": 0.8, "radius": 10.0, "intensity": 0.5 })),
        ("fx.e.transmitted_light", json!({ "type": "transmitted_light", "radius": 16.0, "intensity": 1.0, "color": [1.0, 0.9, 0.7] })),
        ("fx.e.light_wrap", json!({ "type": "light_wrap", "radius": 8.0, "amount": 0.5 })),
        ("fx.e.rim_light", json!({ "type": "rim_light", "angle": 45.0, "width": 4.0, "color": [1.0, 0.85, 0.6], "intensity": 0.8 })),
        ("fx.e.chromatic_aberration", json!({ "type": "chromatic_aberration", "amount": 2.0 })),
        ("fx.e.vignette", json!({ "type": "vignette", "amount": 0.35, "softness": 0.6 })),
        ("fx.e.grain", json!({ "type": "grain", "amount": 0.04, "size": 1.0 })),
        ("fx.e.deband", json!({ "type": "deband", "amount": 0.5 })),
        ("fx.e.god_rays", json!({ "type": "god_rays", "center": [w * 0.5, 0.0], "threshold": 0.7, "length": 0.3, "intensity": 0.6 })),
        ("fx.e.lens_flare", json!({ "type": "lens_flare", "position": [w * 0.75, h * 0.25], "intensity": 0.7, "color": [1.0, 0.9, 0.75] })),
        ("fx.e.drop_shadow", json!({ "type": "drop_shadow", "offset": [6.0, 6.0], "blur": 6.0, "color": [0.0, 0.0, 0.0, 0.5] })),
        ("fx.e.outline", json!({ "type": "outline", "width": 3.0, "color": [1.0, 1.0, 1.0, 1.0] })),
        ("fx.e.color_overlay", json!({ "type": "color_overlay", "color": [1.0, 0.55, 0.25, 0.3] })),
        ("fx.e.two_tone", json!({ "type": "two_tone", "threshold": 0.5, "dark": [0.08, 0.04, 0.14], "light": [1.0, 0.95, 0.9] })),
        ("fx.e.invert", json!({ "type": "invert" })),
        ("fx.e.radial_blur", json!({ "type": "radial_blur", "center": [w * 0.5, h * 0.5], "amount": 0.15 })),
        ("fx.e.color_aware_blur", json!({ "type": "color_aware_blur", "radius": 4.0, "tolerance": 0.12 })),
        ("fx.e.para", json!({ "type": "para", "from": [w * 0.5, 0.0], "to": [w * 0.5, h], "color_from": [1.0, 0.6, 0.3, 0.5], "color_to": [0.3, 0.2, 0.6, 0.0], "mode": "overlay" })),
        ("fx.e.adjust", json!({ "type": "adjust", "adjustment": { "type": "hue_saturation", "hue": 0.0, "saturation": 0.0, "lightness": 0.0 } })),
        ("fx.e.grade", default_grade()),
    ]
}

/// Nome legível de um efeito pelo `type` (cai no próprio `type` se não estiver no menu).
pub(crate) fn effect_label(ty: &str) -> String {
    effect_presets(1.0, 1.0).into_iter().find(|(_, v)| v["type"] == ty).map(|(k, _)| tr(k).to_owned()).unwrap_or_else(|| ty.to_owned())
}

/// Pilha de efeitos de uma camada (JSON das instâncias).
pub(crate) fn stack(p: &Project, layer: &NodeId) -> Vec<Value> {
    cmds::layer_data(p, layer).and_then(|l| l.params.get("effects").and_then(Value::as_array).cloned()).unwrap_or_default()
}

/// Editor genérico dos campos de um efeito (`type` fica de fora). Devolve o efeito mudado, se mudou.
/// Campos opcionais dos efeitos (vazio = o próprio efeito escolhe: centro da imagem, cor padrão) e o valor que entra ao
/// desligar "automático" (auditoria 2.27: antes ficavam "editáveis pelo MCP").
pub(crate) fn optional_field(ty: &str, k: &str, canvas: (f32, f32)) -> Option<Value> {
    match (ty, k) {
        ("glow", "color") => Some(json!([1.0, 1.0, 1.0])),
        ("vignette", "color") => Some(json!([0.0, 0.0, 0.0])),
        ("chromatic_aberration", "center") => Some(json!([canvas.0 / 2.0, canvas.1 / 2.0])),
        _ => None,
    }
}

/// Modos de mistura oferecidos nos efeitos (o "Para").
pub(crate) const FX_MODES: [&str; 9] = ["normal", "multiply", "screen", "overlay", "soft_light", "hard_light", "add", "color", "luminosity"];

fn effect_fields(ui: &mut Ui, t: &crate::theme::Tokens, salt: (&str, usize), effect: &Map<String, Value>) -> Option<Value> {
    let mut out = effect.clone();
    let mut changed = false;
    egui::Grid::new(("fx_fields", salt.0, salt.1)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (k, v) in ordered(effect) {
            dim_label(ui, t, &field_label(k));
            let id = EId::new(("fx_field", salt.0, salt.1, k.as_str()));
            // campo opcional: "automático" liga/desliga; com valor, o editor normal aparece na linha de baixo
            if let Some(def) = optional_field(salt.0, k, (0.0, 0.0)) {
                let mut auto = v.is_null();
                if ui.checkbox(&mut auto, tr("fx.auto")).on_hover_text(tr("fx.auto.tip")).changed() {
                    out.insert(k.clone(), if auto { Value::Null } else { def });
                    changed = true;
                }
                if v.is_null() {
                    ui.end_row();
                    continue;
                }
                ui.end_row();
                ui.label("");
            }
            match v {
                Value::Number(n) => {
                    let cur = n.as_f64().unwrap_or(0.0) as f32;
                    let (lo, hi, speed) = field_range(k);
                    if let Some(x) = commit_on_release(ui, id, cur, |ui, x| ui.add(egui::DragValue::new(x).range(lo..=hi).speed(speed).max_decimals(3))) {
                        out.insert(k.clone(), json!(x));
                        changed = true;
                    }
                }
                Value::Bool(b) => {
                    let mut x = *b;
                    if ui.checkbox(&mut x, "").changed() {
                        out.insert(k.clone(), json!(x));
                        changed = true;
                    }
                }
                Value::Array(a) if a.iter().all(Value::is_number) && (a.len() == 3 || a.len() == 4) && is_color_field(k) => {
                    let f: Vec<f32> = a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
                    if f.len() == 3 {
                        let mut c = [f[0], f[1], f[2]];
                        if ui.color_edit_button_rgb(&mut c).changed() {
                            out.insert(k.clone(), json!(c));
                            changed = true;
                        }
                    } else {
                        let mut c = egui::Rgba::from_rgba_unmultiplied(f[0], f[1], f[2], f[3]);
                        if egui::widgets::color_picker::color_edit_button_rgba(ui, &mut c, egui::widgets::color_picker::Alpha::OnlyBlend).changed() {
                            out.insert(k.clone(), json!(c.to_rgba_unmultiplied()));
                            changed = true;
                        }
                    }
                }
                Value::Array(a) if a.iter().all(Value::is_number) && (a.len() == 2 || a.len() == 3) => {
                    let mut f: Vec<f32> = a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
                    let mut any = false;
                    // rodas de cor do colorista (auditoria 2.24): o ponto na roda é a tonalidade de sombras/meios/altas
                    if f.len() == 3 && matches!(k.as_str(), "lift" | "gamma" | "gain") {
                        let (neutral, reach) = if k == "lift" { (0.0, 0.2) } else { (1.0, 0.5) };
                        if let Some(v) = color_wheel(ui, id.with("wheel"), [f[0], f[1], f[2]], neutral, reach) {
                            f = v.to_vec();
                            any = true;
                        }
                    }
                    ui.horizontal(|ui| {
                        for (i, x) in f.clone().iter().enumerate() {
                            if let Some(y) = commit_on_release(ui, id.with(i), *x, |ui, y| ui.add(egui::DragValue::new(y).speed(if k == "lift" { 0.002 } else { 0.01 }).max_decimals(3))) {
                                f[i] = y;
                                any = true;
                            }
                        }
                    });
                    if any {
                        out.insert(k.clone(), json!(f));
                        changed = true;
                    }
                }
                Value::String(cur) if k == "mode" => {
                    let mut pick = None;
                    egui::ComboBox::from_id_salt(id).selected_text(tr(&format!("blend.{cur}"))).show_ui(ui, |ui| {
                        for m in FX_MODES {
                            if ui.selectable_label(cur == m, tr(&format!("blend.{m}"))).clicked() {
                                pick = Some(m);
                            }
                        }
                    });
                    if let Some(m) = pick {
                        out.insert(k.clone(), json!(m));
                        changed = true;
                    }
                }
                _ => {
                    dim_label(ui, t, tr("fx.mcp_only"));
                }
            }
            ui.end_row();
        }
    });
    changed.then(|| Value::Object(out))
}

/// Roda de cor de um trio RGB em volta de `neutral` (0 no lift, 1 no gamma/gain): o ponto é o desvio de cor (a borda
/// = `reach`), a luminância fica como está. Arrastar mostra ao vivo; o valor novo volta ao soltar (um passo de desfazer).
pub(crate) fn color_wheel(ui: &mut Ui, id: EId, cur: [f32; 3], neutral: f32, reach: f32) -> Option<[f32; 3]> {
    let s3 = 3f32.sqrt();
    let mut v: [f32; 3] = ui.data(|d| d.get_temp::<[f32; 3]>(id)).unwrap_or(cur);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::click_and_drag());
    let c = rect.center();
    let r = rect.width() * 0.5 - 3.0;
    let d = [v[0] - neutral, v[1] - neutral, v[2] - neutral];
    let l = (d[0] + d[1] + d[2]) / 3.0;
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let mut q = (p - c) / r;
        if q.length() > 1.0 {
            q = q.normalized();
        }
        // tela: x para a direita, y para cima = mais verde
        let (x, y) = (q.x * reach, -q.y * reach);
        v = [neutral + l + 2.0 * x / 3.0, neutral + l - x / 3.0 + y / s3, neutral + l - x / 3.0 - y / s3];
        ui.data_mut(|dm| dm.insert_temp(id, v));
    }
    let painter = ui.painter_at(rect.expand(2.0));
    // anel de matizes
    let n = 48;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        // a mesma conversão do ponto: ângulo -> desvio RGB unitário
        let col = |a: f32| {
            let (x, y) = (a.cos(), a.sin());
            let rgb = [0.5 + 0.5 * (2.0 * x / 3.0), 0.5 + 0.5 * (-x / 3.0 + y / s3), 0.5 + 0.5 * (-x / 3.0 - y / s3)];
            egui::Color32::from_rgb((rgb[0].clamp(0.0, 1.0) * 255.0) as u8, (rgb[1].clamp(0.0, 1.0) * 255.0) as u8, (rgb[2].clamp(0.0, 1.0) * 255.0) as u8)
        };
        let p0 = c + egui::vec2(a0.cos(), -a0.sin()) * r;
        let p1 = c + egui::vec2(a1.cos(), -a1.sin()) * r;
        painter.line_segment([p0, p1], egui::Stroke::new(4.0, col((a0 + a1) * 0.5)));
    }
    painter.circle_stroke(c, r * 0.5, egui::Stroke::new(1.0, egui::Color32::from_gray(90)));
    painter.line_segment([c - egui::vec2(4.0, 0.0), c + egui::vec2(4.0, 0.0)], egui::Stroke::new(1.0, egui::Color32::from_gray(120)));
    painter.line_segment([c - egui::vec2(0.0, 4.0), c + egui::vec2(0.0, 4.0)], egui::Stroke::new(1.0, egui::Color32::from_gray(120)));
    let d = [v[0] - neutral, v[1] - neutral, v[2] - neutral];
    let l = (d[0] + d[1] + d[2]) / 3.0;
    let x = d[0] - l;
    let y = (d[1] - d[2]) * s3 / 2.0;
    let dot = c + egui::vec2(x, -y) / reach * r;
    painter.circle(dot, 4.0, egui::Color32::WHITE, egui::Stroke::new(1.5, egui::Color32::BLACK));
    let resp = resp.on_hover_text(tr("fx.wheel.tip"));
    if resp.double_clicked() {
        ui.data_mut(|dm| dm.remove_temp::<[f32; 3]>(id));
        return Some([neutral + l; 3]);
    }
    if resp.drag_stopped() || (resp.clicked() && !resp.dragged()) {
        ui.data_mut(|dm| dm.remove_temp::<[f32; 3]>(id));
        return (v != cur).then_some(v);
    }
    None
}

/// Ordem de leitura dos campos (a do colorista: sombras → meios → altas, depois saturação...); o resto em seguida.
const FIELD_ORDER: [&str; 20] = [
    "lift", "gamma", "gain", "saturation", "temperature", "tint", "contrast", "pivot", "threshold", "radius", "amount", "intensity", "softness", "size",
    "angle", "length", "width", "blur", "center", "position",
];

fn ordered(effect: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut v: Vec<(&String, &Value)> = effect.iter().filter(|(k, _)| k.as_str() != "type").collect();
    v.sort_by_key(|(k, _)| FIELD_ORDER.iter().position(|f| f == k).unwrap_or(FIELD_ORDER.len()));
    v
}

fn is_color_field(k: &str) -> bool {
    matches!(k, "color" | "color_from" | "color_to" | "dark" | "light")
}

pub(crate) fn field_label(k: &str) -> String {
    let key = format!("fx.f.{k}");
    let s = tr(&key);
    if s == key { k.replace('_', " ") } else { s.to_owned() }
}

pub(crate) fn field_range(k: &str) -> (f32, f32, f32) {
    match k {
        "radius" | "length" | "blur" | "width" => (0.0, 400.0, 0.2),
        "angle" | "rotation" => (-360.0, 360.0, 0.5),
        "threshold" | "amount" | "softness" | "intensity" | "pivot" => (0.0, 4.0, 0.005),
        "saturation" | "contrast" => (0.0, 3.0, 0.005),
        "temperature" | "tint" => (-1.0, 1.0, 0.005),
        "size" => (0.1, 20.0, 0.02),
        _ => (-10000.0, 10000.0, 0.1),
    }
}

// ------------------------------------------------------------------------------------------------ Composição

/// Câmera do corte (seção do painel Propriedades da Composição).
pub(crate) fn camera(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(cut) = env.ed.cut.clone() else { return };
    let Some(cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    let cam = cd.camera.clone();
    let center = cam.center.unwrap_or([w as f32 / 2.0, h as f32 / 2.0]);
    section(ui, &t, "fx.camera");
    let n_keys: usize = serde_json::to_value(&cam.keys).ok().and_then(|v| v.as_object().map(|o| o.values().filter_map(Value::as_array).map(Vec::len).sum())).unwrap_or(0);
    if n_keys > 0 {
        dim_label(ui, &t, &format!("{n_keys} {}", tr("fx.camera_keys")));
    }
    let run = |env: &mut Env, mut v: Value| {
        v["cut"] = json!(cut);
        let r = actions::run(env.core, "CAMERA", v);
        actions::notify_err(env.ed, r);
    };
    egui::Grid::new("fx_cam_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("fx.zoom"));
        let mut zr = egui::Rect::NOTHING;
        if let Some(z) = commit_on_release(ui, EId::new(("cam_zoom", &cut.0)), cam.zoom, |ui, z| {
            let r = ui.add(egui::DragValue::new(z).range(0.1..=10.0).speed(0.005).max_decimals(3).suffix("x"));
            zr = r.rect;
            r
        }) {
            run(env, json!({ "op": "set", "zoom": z }));
        }
        env.note("fx:camera:zoom", zr);
        ui.end_row();
        dim_label(ui, &t, tr("fx.rotation"));
        if let Some(r) = commit_on_release(ui, EId::new(("cam_rot", &cut.0)), cam.rotation, |ui, r| ui.add(egui::DragValue::new(r).range(-180.0..=180.0).speed(0.2).max_decimals(1).suffix("°"))) {
            run(env, json!({ "op": "set", "rotation": r }));
        }
        ui.end_row();
        dim_label(ui, &t, tr("fx.center"));
        ui.horizontal(|ui| {
            for axis in 0..2 {
                if let Some(v) = commit_on_release(ui, EId::new(("cam_c", &cut.0, axis)), center[axis], |ui, v| ui.add(egui::DragValue::new(v).speed(1.0).max_decimals(0).suffix(" px"))) {
                    let mut c = center;
                    c[axis] = v;
                    run(env, json!({ "op": "set", "center": c }));
                }
            }
        });
        ui.end_row();
    });
    // movimentos prontos sobre o corte inteiro
    ui.horizontal_wrapped(|ui| {
        let from = json!({ "center": center, "zoom": cam.zoom });
        let b = ui.button(tr("fx.push_in"));
        env.note("fx:camera:push_in", b.rect);
        if b.on_hover_text(tr("fx.push_in.tip")).clicked() {
            run(env, json!({ "op": "move", "from": from, "to": { "center": center, "zoom": cam.zoom * 1.15 } }));
        }
        let b = ui.button(tr("fx.pan"));
        env.note("fx:camera:pan", b.rect);
        if b.on_hover_text(tr("fx.pan.tip")).clicked() {
            run(env, json!({ "op": "move", "from": from, "to": { "center": [center[0] + w as f32 * 0.08, center[1]], "zoom": cam.zoom } }));
        }
        let b = ui.add_enabled(n_keys > 0, egui::Button::new(tr("fx.clear_keys")));
        env.note("fx:camera:clear", b.rect);
        if b.clicked() {
            run(env, json!({ "op": "clear_keys" }));
        }
    });
    // tremor paramétrico (auditoria 4.5k): força, duração e se ele amortece, a partir do quadro atual
    let key = egui::Id::new("fx_shake_params");
    let (mut amp, mut len, mut decay): (f32, u32, bool) = ui.data(|d| d.get_temp(key)).unwrap_or((12.0, 12, true));
    ui.horizontal_wrapped(|ui| {
        let b = ui.button(tr("fx.shake"));
        env.note("fx:camera:shake", b.rect);
        let r = ui.add(egui::DragValue::new(&mut amp).range(0.5..=200.0).speed(0.2).max_decimals(1).suffix(" px")).on_hover_text(tr("fx.shake.amp"));
        env.note("fx:camera:shake_amp", r.rect);
        let r = ui.add(egui::DragValue::new(&mut len).range(1..=600).speed(0.2).suffix(format!(" {}", tr("unit.frames_short")))).on_hover_text(tr("fx.shake.len"));
        env.note("fx:camera:shake_len", r.rect);
        let r = ui.checkbox(&mut decay, tr("fx.shake.decay")).on_hover_text(tr("fx.shake.decay.tip"));
        env.note("fx:camera:shake_decay", r.rect);
        if b.on_hover_text(tr("fx.shake.tip")).clicked() {
            let end = (env.ed.frame + len).min(cd.duration.saturating_sub(1)).max(env.ed.frame + 1);
            run(env, json!({ "op": "shake", "amplitude": amp, "start": env.ed.frame, "end": end, "decay": decay }));
        }
    });
    ui.data_mut(|d| d.insert_temp(key, (amp, len, decay)));
}

// ------------------------------------------------------------------------------------------------ Cor

/// Camada "Grade" do corte (efeito no topo criado pelo GRADE), se houver.
fn grade_layer(p: &Project, cut: &NodeId) -> Option<NodeId> {
    p.node(cut)?.children.iter().find(|c| p.node(c).is_some_and(|n| n.name == "Grade" && matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Effect))).cloned()
}

fn default_grade() -> Value {
    json!({ "type": "grade", "lift": [0.0, 0.0, 0.0], "gamma": [1.0, 1.0, 1.0], "gain": [1.0, 1.0, 1.0], "saturation": 1.0, "temperature": 0.0, "tint": 0.0, "contrast": 1.0, "pivot": 0.5 })
}

/// Propriedades do workspace Cor: a cor do corte ativo (GRADE).
pub(crate) fn grade(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(cut) = env.ed.active_cut(env.core) else {
        dim_label(ui, &t, tr("panel.properties.empty"));
        return;
    };
    let cuts = super::edit::project_cuts(&env.core.project);
    let code = cuts.iter().find(|c| c.0 == cut).map(|c| c.1.clone()).unwrap_or_default();
    let mut pick = None;
    let r = egui::ComboBox::from_id_salt("grade_cut").width((ui.available_width() - 8.0).clamp(80.0, 320.0)).selected_text(format!("{code}  ·  {}", tr("fx.grade_title"))).show_ui(ui, |ui| {
        for (id, c, scene, _) in &cuts {
            if ui.selectable_label(*id == cut, format!("{c} · {scene}")).clicked() {
                pick = Some(id.clone());
            }
        }
    });
    env.note("grade:cut", r.response.rect);
    if let Some(c) = pick {
        env.ed.set_cut(c);
        return;
    }
    let layer = grade_layer(&env.core.project, &cut);
    let list = layer.as_ref().map(|l| stack(&env.core.project, l)).unwrap_or_default();
    dim_label(ui, &t, tr(if layer.is_some() { "fx.grade_layer" } else { "fx.grade_none" }));
    let find = |ty: &str| list.iter().position(|i| i.pointer("/effect/type").and_then(Value::as_str) == Some(ty));
    let mut new_list = list.clone();
    let mut changed = false;

    // ---- correção primária
    section(ui, &t, "fx.primary");
    let gi = find("grade");
    let g = gi.and_then(|i| list[i].get("effect").cloned()).unwrap_or_else(default_grade);
    if let Some(obj) = g.as_object() {
        if let Some(newg) = effect_fields(ui, &t, ("grade", 0), obj) {
            match gi {
                Some(i) => new_list[i]["effect"] = newg,
                None => new_list.insert(0, json!({ "effect": newg })),
            }
            changed = true;
        }
    }
    let b = ui.add_enabled(gi.is_some(), egui::Button::new(tr("fx.reset_primary")));
    env.note("grade:reset", b.rect);
    if b.clicked() {
        if let Some(i) = gi {
            new_list[i]["effect"] = default_grade();
            changed = true;
        }
    }

    // ---- acabamento: vinheta, difusão, grão (liga/desliga e campos)
    section(ui, &t, "fx.finish");
    for (ty, key) in [("vignette", "fx.e.vignette"), ("diffusion", "fx.e.diffusion"), ("grain", "fx.e.grain")] {
        let pos = find(ty);
        let mut on = pos.is_some();
        let r = ui.checkbox(&mut on, tr(key));
        env.note(format!("grade:{ty}"), r.rect);
        if r.changed() {
            match (on, pos) {
                (true, None) => {
                    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
                    if let Some((_, v)) = effect_presets(w as f32, h as f32).into_iter().find(|(_, v)| v["type"] == ty) {
                        new_list.push(json!({ "effect": v }));
                        changed = true;
                    }
                }
                (false, Some(i)) => {
                    new_list.remove(i);
                    changed = true;
                }
                _ => {}
            }
        } else if let Some(i) = pos {
            if let Some(obj) = list[i].get("effect").and_then(Value::as_object) {
                ui.indent(("grade_fx", ty), |ui| {
                    if let Some(newe) = effect_fields(ui, &t, (ty, i), obj) {
                        new_list[i]["effect"] = newe;
                        changed = true;
                    }
                });
            }
        }
    }
    let others = list.iter().filter(|i| !matches!(i.pointer("/effect/type").and_then(Value::as_str), Some("grade" | "vignette" | "diffusion" | "grain"))).count();
    if others > 0 {
        dim_label(ui, &t, &format!("{others} {}", tr("fx.other_effects")));
    }
    if changed {
        let r = actions::run(env.core, "GRADE", json!({ "cut": cut, "effects": new_list }));
        actions::notify_err(env.ed, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;
    use aiai_core::CoreSession;

    fn session() -> (CoreSession, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, 12, None).unwrap();
        (core, cut)
    }

    #[test]
    fn every_preset_is_accepted_by_the_effects_action() {
        let (mut core, cut) = session();
        let layer = crate::app::commands::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        for (k, v) in effect_presets(64.0, 36.0) {
            let r = actions::run(&mut core, "EFFECTS", json!({ "op": "add", "layer": layer, "effect": v }));
            assert!(r.is_ok(), "{k}: {r:?}");
            assert_ne!(crate::i18n::tr_in(crate::i18n::Lang::En, k), k);
        }
        assert_eq!(stack(&core.project, &layer).len(), effect_presets(1.0, 1.0).len());
    }

    #[test]
    fn grade_round_trip_and_invalid_values_are_refused() {
        let (mut core, cut) = session();
        actions::run(&mut core, "GRADE", json!({ "cut": cut, "effects": [{ "effect": default_grade() }, { "effect": { "type": "vignette", "amount": 0.3, "softness": 0.5 } }] })).unwrap();
        let layer = grade_layer(&core.project, &cut).expect("camada Grade");
        let s = stack(&core.project, &layer);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].pointer("/effect/saturation").and_then(Value::as_f64), Some(1.0));
        // tipo desconhecido não entra
        let before = core.change_counter;
        assert!(actions::run(&mut core, "GRADE", json!({ "cut": cut, "effects": [{ "effect": { "type": "nao_existe" } }] })).is_err());
        assert_eq!(core.change_counter, before);
        core.undo().unwrap();
        assert!(grade_layer(&core.project, &cut).is_none());
    }

    #[test]
    fn camera_set_and_move_write_the_cut() {
        let (mut core, cut) = session();
        actions::run(&mut core, "CAMERA", json!({ "op": "set", "cut": cut, "zoom": 1.2 })).unwrap();
        assert_eq!(cmds::cut_data(&core.project, &cut).unwrap().camera.zoom, 1.2);
        actions::run(&mut core, "CAMERA", json!({ "op": "move", "cut": cut, "from": { "center": [32.0, 18.0], "zoom": 1.0 }, "to": { "center": [32.0, 18.0], "zoom": 1.15 } })).unwrap();
        assert!(!cmds::cut_data(&core.project, &cut).unwrap().camera.keys.is_empty());
    }
}

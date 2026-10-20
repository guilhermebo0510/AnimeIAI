//! Painel Texto, parte de estilos: preenchimento (cor, degradê ou padrão), a pilha de efeitos de camada (contornos
//! múltiplos, sombras, brilhos, chanfro, cetim, sobreposições, sombra longa), a distorção (arco, ponte, onda, bandeira,
//! inchar, subida, caminho) e os estilos guardados (prontos e os do usuário em `Documentos\AnimeIAI\Estilos`, nada com
//! `--no-session`). Cada ajuste é um passo de desfazer (ao soltar); numa digitação em curso vai direto ao texto.

use super::commit_on_release;
use super::gradient_edit as ge;
use crate::app::commands as cmds;
use crate::app::text_tool as tt;
use crate::app::ui_kit::{Env, color32_to_rgba, dim_label, rgba_to_color32, section};
use crate::i18n::tr;
use aiai_core::*;
use aiai_paint::text_layer::*;
use aiai_paint::text_style::{TextStyle, builtin_styles};
use egui::{Color32, Id, Ui, Vec2};

fn fnum(ui: &mut Ui, id: Id, cur: f32, range: std::ops::RangeInclusive<f32>, speed: f64, suffix: &str, decimals: usize) -> Option<f32> {
    commit_on_release(ui, id, cur, |ui, v| ui.add(egui::DragValue::new(v).range(range).speed(speed).suffix(suffix.to_owned()).max_decimals(decimals)))
}

/// `fnum` com o retângulo anotado para a bridge (`name`).
#[allow(clippy::too_many_arguments)]
fn fnum_noted(env: &mut Env, ui: &mut Ui, name: String, id: Id, cur: f32, range: std::ops::RangeInclusive<f32>, speed: f64, suffix: &str, decimals: usize) -> Option<f32> {
    let mut rect = egui::Rect::NOTHING;
    let out = commit_on_release(ui, id, cur, |ui, v| {
        let r = ui.add(egui::DragValue::new(v).range(range).speed(speed).suffix(suffix.to_owned()).max_decimals(decimals));
        rect = r.rect;
        r
    });
    env.note(name, rect);
    out
}

/// Botão de cor (com alfa opcional) que entrega a cor ao soltar.
fn color_btn(env: &Env, ui: &mut Ui, id: Id, cur: &ColorRef, alpha: bool) -> Option<Rgba> {
    let cur_rgba = cmds::resolve_color(&env.core.project, cur);
    let key = id.with("tcol");
    let mut c32: Color32 = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| rgba_to_color32(cur_rgba));
    let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, if alpha { egui::color_picker::Alpha::OnlyBlend } else { egui::color_picker::Alpha::Opaque });
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(key, c32));
    }
    let pending = ui.data(|d| d.get_temp::<Color32>(key)).is_some();
    if pending && !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| d.remove_temp::<Color32>(key));
        let rgba = color32_to_rgba(c32);
        if rgba != cur_rgba {
            return Some(rgba);
        }
    }
    None
}

/// Degradê editado: ao vivo numa digitação em curso, senão ao soltar.
fn grad_field(env: &mut Env, ui: &mut Ui, key: &str, cur: &Gradient) -> Option<Gradient> {
    if env.ed.text.session.is_some() {
        let mut g = cur.clone();
        return ge::editor(env, ui, key, &mut g).then_some(g);
    }
    let id = Id::new(("text_grad", key.to_owned()));
    ge::buffered(ui, id, cur, |ui, v| {
        ge::editor(env, ui, key, v);
    })
}

/// Texturas do projeto (ativos de textura com imagem): nome e blob.
fn textures(p: &Project) -> Vec<(String, BlobHash)> {
    p.roots
        .assets
        .iter()
        .filter_map(|id| p.node(id))
        .filter_map(|n| match &n.data {
            NodeKind::Asset(a) if matches!(a.asset, AssetType::Texture | AssetType::Background) => a.blob.clone().map(|b| (n.name.clone(), b)),
            _ => None,
        })
        .collect()
}

/// Tinta (cor, degradê ou padrão). `fallback` = a cor quando a tinta não é chapada.
fn paint_field(env: &mut Env, ui: &mut Ui, key: &str, cur: &Paint, allow_pattern: bool) -> Option<Paint> {
    let mode = match cur {
        Paint::Solid { .. } => 0,
        Paint::Gradient { .. } => 1,
        Paint::Pattern { .. } => 2,
    };
    let mut out = None;
    ui.horizontal(|ui| {
        for (m, k) in [(0, "text.paint.solid"), (1, "text.paint.gradient"), (2, "text.paint.pattern")] {
            if m == 2 && !allow_pattern {
                continue;
            }
            let r = ui.selectable_label(mode == m, tr(k));
            env.note(format!("text:{key}:mode{m}"), r.rect);
            if r.clicked() && mode != m {
                out = Some(match m {
                    0 => Paint::Solid { color: match cur {
                        Paint::Gradient { gradient } => gradient.stops.first().map(|s| s.color.clone()).unwrap_or_default(),
                        _ => env.ed.color.clone(),
                    } },
                    1 => Paint::Gradient { gradient: env.ed.grad.gradient.clone() },
                    _ => match textures(&env.core.project).into_iter().next() {
                        Some((_, b)) => Paint::Pattern { image: b, scale: 1.0, offset: [0.0, 0.0], angle: 0.0 },
                        None => {
                            env.ed.notify(tr("text.paint.no_texture"));
                            return;
                        }
                    },
                });
            }
        }
    });
    if out.is_some() {
        return out;
    }
    match cur {
        Paint::Solid { color } => {
            if let Some(c) = color_btn(env, ui, Id::new(("pf", key.to_owned())), color, true) {
                return Some(Paint::Solid { color: ColorRef::Direct { rgba: c } });
            }
        }
        Paint::Gradient { gradient } => {
            if let Some(g) = grad_field(env, ui, key, gradient) {
                return Some(Paint::Gradient { gradient: g });
            }
        }
        Paint::Pattern { image, scale, offset, angle } => {
            let tex = textures(&env.core.project);
            let name = tex.iter().find(|(_, b)| b == image).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".into());
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt(("pat", key.to_owned())).width(120.0).selected_text(name).show_ui(ui, |ui| {
                    for (n, b) in &tex {
                        if ui.selectable_label(b == image, n).clicked() {
                            pick = Some(b.clone());
                        }
                    }
                });
                if let Some(v) = fnum(ui, Id::new(("patscale", key.to_owned())), *scale * 100.0, 1.0..=2000.0, 1.0, " %", 0) {
                    out = Some(Paint::Pattern { image: image.clone(), scale: v / 100.0, offset: *offset, angle: *angle });
                }
                if let Some(v) = fnum(ui, Id::new(("patang", key.to_owned())), *angle, -360.0..=360.0, 1.0, "°", 0) {
                    out = Some(Paint::Pattern { image: image.clone(), scale: *scale, offset: *offset, angle: v });
                }
            });
            if let Some(b) = pick {
                return Some(Paint::Pattern { image: b, scale: *scale, offset: *offset, angle: *angle });
            }
            return out;
        }
    }
    None
}

// ------------------------------------------------------------------------------------------------ preenchimento

pub(super) fn fill_section(env: &mut Env, ui: &mut Ui, p: &TextParams) {
    let t = env.t;
    section(ui, &t, "text.sec.fill");
    let cur = p.fill.clone().unwrap_or(Paint::Solid { color: p.color.clone() });
    if let Some(np) = paint_field(env, ui, "fill", &cur, true) {
        tt::edit(env, "text.cmd_style", |q| match np {
            // tinta chapada = a cor do texto (sem `fill`)
            Paint::Solid { color } => {
                q.color = color;
                q.fill = None;
            }
            other => q.fill = Some(other),
        });
    }
}

/// Tinta do contorno legado (cor da seção Contorno ou degradê).
pub(super) fn legacy_stroke_paint(env: &mut Env, ui: &mut Ui, s: &TextStroke) {
    let cur = s.paint.clone().unwrap_or(Paint::Solid { color: s.color.clone() });
    if let Some(np) = paint_field(env, ui, "lstroke", &cur, false) {
        tt::edit(env, "text.cmd_style", |q| {
            if let Some(x) = q.stroke.as_mut() {
                match np {
                    Paint::Solid { color } => {
                        x.color = color;
                        x.paint = None;
                    }
                    other => x.paint = Some(other),
                }
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------ efeitos

fn fx_name(k: &str) -> String {
    tr(&format!("tfx.{k}")).to_owned()
}

const BLENDS: [BlendMode; 14] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Add,
    BlendMode::Subtract,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::Color,
    BlendMode::Hue,
    BlendMode::Luminosity,
];

pub(super) fn effects_section(env: &mut Env, ui: &mut Ui, p: &TextParams) {
    let t = env.t;
    section(ui, &t, "text.sec.effects");
    ui.horizontal_wrapped(|ui| {
        let r = ui.menu_button(tr("text.fx.add"), |ui| {
            for k in TextEffectKind::KEYS {
                let b = ui.button(fx_name(k));
                env.note(format!("text:fx_add:{k}"), b.rect);
                if b.clicked() {
                    if let Some(kind) = TextEffectKind::new_by_key(k) {
                        let mut e = TextEffect::new(kind);
                        // proporcional ao corpo (os padrões são para ~100 px)
                        e.scale_px((p.size / 100.0).clamp(0.2, 10.0));
                        tt::edit(env, "text.cmd_fx", |q| q.effects.push(e));
                    }
                    ui.close();
                }
            }
            if !textures(&env.core.project).is_empty() {
                if ui.button(fx_name("pattern_overlay")).clicked() {
                    if let Some((_, b)) = textures(&env.core.project).into_iter().next() {
                        let e = TextEffect::new(TextEffectKind::PatternOverlay(FxPatternOverlay { paint: Paint::Pattern { image: b, scale: 1.0, offset: [0.0, 0.0], angle: 0.0 } }));
                        tt::edit(env, "text.cmd_fx", |q| q.effects.push(e));
                    }
                    ui.close();
                }
            }
        });
        env.note("text:fx_add", r.response.rect);
        if !p.effects.is_empty() {
            let b = ui.small_button(tr("text.fx.clear"));
            if b.clicked() {
                tt::edit(env, "text.cmd_fx", |q| q.effects.clear());
            }
        }
    });
    if p.effects.is_empty() {
        dim_label(ui, &t, tr("text.fx.empty"));
    }
    let n = p.effects.len();
    for (i, e) in p.effects.iter().enumerate() {
        ui.push_id(("fx", i), |ui| effect_row(env, ui, i, n, e));
    }
}

fn set_fx(env: &mut Env, i: usize, e: TextEffect) {
    tt::edit(env, "text.cmd_fx", |q| {
        if let Some(x) = q.effects.get_mut(i) {
            *x = e;
        }
    });
}

fn effect_row(env: &mut Env, ui: &mut Ui, i: usize, n: usize, e: &TextEffect) {
    let t = env.t;
    let id = ui.make_persistent_id(("fx_open", i));
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    let mut ne = e.clone();
    let mut action: Option<i32> = None;
    let header = state.show_header(ui, |ui| {
        let r = ui.checkbox(&mut ne.enabled, "");
        env.note(format!("text:fx:{i}:enabled"), r.rect);
        let l = ui.label(egui::RichText::new(fx_name(e.kind.key())).strong());
        env.note(format!("text:fx:{i}"), l.rect);
        egui::ComboBox::from_id_salt(("fxblend", i)).width(70.0).selected_text(tr(cmds::blend_key(ne.blend))).show_ui(ui, |ui| {
            for b in BLENDS {
                ui.selectable_value(&mut ne.blend, b, tr(cmds::blend_key(b)));
            }
        });
        if let Some(v) = fnum(ui, Id::new(("fxop", i)), e.opacity * 100.0, 0.0..=100.0, 0.5, " %", 0) {
            ne.opacity = v / 100.0;
        }
        if ui.add_enabled(i > 0, egui::Button::new("▲").small()).on_hover_text(tr("text.fx.up")).clicked() {
            action = Some(-1);
        }
        if ui.add_enabled(i + 1 < n, egui::Button::new("▼").small()).on_hover_text(tr("text.fx.down")).clicked() {
            action = Some(1);
        }
        let d = ui.small_button("×").on_hover_text(tr("text.fx.remove"));
        env.note(format!("text:fx:{i}:remove"), d.rect);
        if d.clicked() {
            action = Some(0);
        }
    });
    header.body(|ui| {
        let k = |name: &str| Id::new(("fx", i, name.to_owned()));
        let key = format!("fx{i}");
        match &mut ne.kind {
            TextEffectKind::Stroke(s) => {
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.fx.width"));
                    if let Some(v) = fnum_noted(env, ui, format!("text:fx:{i}:width"), k("w"), s.width, 0.0..=400.0, 0.2, " px", 1) {
                        s.width = v;
                    }
                    for (pos, lk) in [(StrokePosition::Outside, "text.fx.outside"), (StrokePosition::Inside, "text.fx.inside"), (StrokePosition::Center, "text.fx.center")] {
                        if ui.selectable_label(s.position == pos, tr(lk)).clicked() {
                            s.position = pos;
                        }
                    }
                });
                if let Some(pt) = paint_field(env, ui, &key, &s.paint, true) {
                    s.paint = pt;
                }
            }
            TextEffectKind::DropShadow(s) | TextEffectKind::InnerShadow(s) => {
                ui.horizontal_wrapped(|ui| {
                    if let Some(c) = color_btn(env, ui, k("c"), &s.color, true) {
                        s.color = ColorRef::Direct { rgba: c };
                    }
                    dim_label(ui, &t, "X");
                    if let Some(v) = fnum(ui, k("x"), s.offset[0], -400.0..=400.0, 0.5, "", 1) {
                        s.offset[0] = v;
                    }
                    dim_label(ui, &t, "Y");
                    if let Some(v) = fnum(ui, k("y"), s.offset[1], -400.0..=400.0, 0.5, "", 1) {
                        s.offset[1] = v;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.blur"));
                    if let Some(v) = fnum(ui, k("b"), s.blur, 0.0..=400.0, 0.5, " px", 1) {
                        s.blur = v;
                    }
                    dim_label(ui, &t, tr("text.fx.spread"));
                    if let Some(v) = fnum(ui, k("s"), s.spread, 0.0..=200.0, 0.2, " px", 1) {
                        s.spread = v;
                    }
                    dim_label(ui, &t, tr("text.fx.noise"));
                    if let Some(v) = fnum(ui, k("n"), s.noise * 100.0, 0.0..=100.0, 0.5, " %", 0) {
                        s.noise = v / 100.0;
                    }
                });
            }
            TextEffectKind::OuterGlow(g) | TextEffectKind::InnerGlow(g) => {
                let inner = matches!(e.kind, TextEffectKind::InnerGlow(_));
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.fx.size"));
                    if let Some(v) = fnum_noted(env, ui, format!("text:fx:{i}:size"), k("sz"), g.size, 0.0..=400.0, 0.5, " px", 1) {
                        g.size = v;
                    }
                    dim_label(ui, &t, tr("text.fx.spread"));
                    if let Some(v) = fnum(ui, k("sp"), g.spread * 100.0, 0.0..=100.0, 0.5, " %", 0) {
                        g.spread = v / 100.0;
                    }
                    dim_label(ui, &t, tr("text.fx.noise"));
                    if let Some(v) = fnum(ui, k("n"), g.noise * 100.0, 0.0..=100.0, 0.5, " %", 0) {
                        g.noise = v / 100.0;
                    }
                    if inner {
                        ui.checkbox(&mut g.from_center, tr("text.fx.from_center"));
                    }
                });
                if let Some(pt) = paint_field(env, ui, &key, &g.paint, false) {
                    g.paint = pt;
                }
            }
            TextEffectKind::BevelEmboss(b) => {
                ui.horizontal_wrapped(|ui| {
                    for (st, lk) in [(BevelStyle::Inner, "text.fx.bevel.inner"), (BevelStyle::Outer, "text.fx.bevel.outer"), (BevelStyle::Emboss, "text.fx.bevel.emboss"), (BevelStyle::Pillow, "text.fx.bevel.pillow")] {
                        if ui.selectable_label(b.style == st, tr(lk)).clicked() {
                            b.style = st;
                        }
                    }
                    ui.checkbox(&mut b.down, tr("text.fx.down_dir"));
                });
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.fx.size"));
                    if let Some(v) = fnum(ui, k("sz"), b.size, 0.0..=200.0, 0.2, " px", 1) {
                        b.size = v;
                    }
                    dim_label(ui, &t, tr("text.fx.depth"));
                    if let Some(v) = fnum(ui, k("d"), b.depth * 100.0, 5.0..=1000.0, 1.0, " %", 0) {
                        b.depth = v / 100.0;
                    }
                    dim_label(ui, &t, tr("text.fx.soften"));
                    if let Some(v) = fnum(ui, k("so"), b.soften, 0.0..=100.0, 0.2, " px", 1) {
                        b.soften = v;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.fx.light"));
                    if let Some(v) = fnum(ui, k("an"), b.angle, -360.0..=360.0, 1.0, "°", 0) {
                        b.angle = v;
                    }
                    if let Some(v) = fnum(ui, k("al"), b.altitude, 0.0..=90.0, 0.5, "°", 0) {
                        b.altitude = v;
                    }
                    if let Some(c) = color_btn(env, ui, k("hc"), &b.highlight, false) {
                        b.highlight = ColorRef::Direct { rgba: c };
                    }
                    if let Some(v) = fnum(ui, k("ho"), b.highlight_opacity * 100.0, 0.0..=100.0, 0.5, " %", 0) {
                        b.highlight_opacity = v / 100.0;
                    }
                    if let Some(c) = color_btn(env, ui, k("sc"), &b.shadow, false) {
                        b.shadow = ColorRef::Direct { rgba: c };
                    }
                    if let Some(v) = fnum(ui, k("sho"), b.shadow_opacity * 100.0, 0.0..=100.0, 0.5, " %", 0) {
                        b.shadow_opacity = v / 100.0;
                    }
                });
            }
            TextEffectKind::Satin(s) => {
                ui.horizontal_wrapped(|ui| {
                    if let Some(c) = color_btn(env, ui, k("c"), &s.color, false) {
                        s.color = ColorRef::Direct { rgba: c };
                    }
                    if let Some(v) = fnum(ui, k("an"), s.angle, -360.0..=360.0, 1.0, "°", 0) {
                        s.angle = v;
                    }
                    dim_label(ui, &t, tr("text.fx.distance"));
                    if let Some(v) = fnum(ui, k("d"), s.distance, 0.0..=200.0, 0.2, " px", 1) {
                        s.distance = v;
                    }
                    dim_label(ui, &t, tr("text.fx.size"));
                    if let Some(v) = fnum(ui, k("sz"), s.size, 0.0..=200.0, 0.2, " px", 1) {
                        s.size = v;
                    }
                    ui.checkbox(&mut s.invert, tr("text.fx.invert"));
                });
            }
            TextEffectKind::GradientOverlay(g) => {
                if let Some(ng) = grad_field(env, ui, &key, &g.gradient) {
                    g.gradient = ng;
                }
            }
            TextEffectKind::ColorOverlay(c) => {
                if let Some(v) = color_btn(env, ui, k("c"), &c.color, true) {
                    c.color = ColorRef::Direct { rgba: v };
                }
            }
            TextEffectKind::PatternOverlay(pt) => {
                if let Some(np) = paint_field(env, ui, &key, &pt.paint, true) {
                    pt.paint = np;
                }
            }
            TextEffectKind::LongShadow(l) => {
                ui.horizontal_wrapped(|ui| {
                    dim_label(ui, &t, tr("text.fx.direction"));
                    if let Some(v) = fnum(ui, k("an"), l.angle, -360.0..=360.0, 1.0, "°", 0) {
                        l.angle = v;
                    }
                    dim_label(ui, &t, tr("text.fx.length"));
                    if let Some(v) = fnum(ui, k("len"), l.length, 0.0..=600.0, 0.5, " px", 1) {
                        l.length = v;
                    }
                    ui.checkbox(&mut l.fade, tr("text.fx.fade"));
                });
                if let Some(pt) = paint_field(env, ui, &key, &l.paint, false) {
                    l.paint = pt;
                }
            }
        }
    });
    match action {
        Some(0) => tt::edit(env, "text.cmd_fx", |q| {
            if i < q.effects.len() {
                q.effects.remove(i);
            }
        }),
        Some(d) => tt::edit(env, "text.cmd_fx", |q| {
            let j = (i as i32 + d) as usize;
            if i < q.effects.len() && j < q.effects.len() {
                q.effects.swap(i, j);
            }
        }),
        None if ne != *e => set_fx(env, i, ne),
        None => {}
    }
}

// ------------------------------------------------------------------------------------------------ distorção

fn warp_key(s: WarpStyle) -> String {
    tr(&format!("text.warp.{}", s.key())).to_owned()
}

/// Pontos do último traço do cel exposto de uma camada vetorial (o caminho do texto).
fn last_stroke_path(env: &Env, layer: &cmds::NodeId) -> Option<Vec<[f32; 2]>> {
    let cut = env.ed.cut.as_ref()?;
    let cel = cmds::exposed_cel(&env.core.project, cut, layer, env.ed.frame)?;
    let cd = cmds::cel_data(&env.core.project, &cel)?;
    let s = cd.strokes.last()?;
    let mut pts: Vec<[f32; 2]> = aiai_paint::sample_stroke(s).iter().map(|q| [q.x, q.y]).collect();
    // movimento da camada: o caminho vai para o canvas como o render desenha
    if let Some(a) = cmds::layer_placement(&env.core.project, cut, layer, env.ed.frame) {
        for q in &mut pts {
            let (x, y) = a.apply(q[0], q[1]);
            *q = [x, y];
        }
    }
    (pts.len() >= 2).then_some(pts)
}

pub(super) fn warp_section(env: &mut Env, ui: &mut Ui, p: &TextParams) {
    let t = env.t;
    section(ui, &t, "text.sec.warp");
    let cur = p.warp.clone();
    ui.horizontal_wrapped(|ui| {
        let label = match &cur {
            Some(w) => warp_key(w.style),
            None => tr("text.warp.none").to_owned(),
        };
        let mut pick: Option<Option<WarpStyle>> = None;
        let cb = egui::ComboBox::from_id_salt("text_warp_style").width(110.0).selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_none(), tr("text.warp.none")).clicked() {
                pick = Some(None);
            }
            for s in WarpStyle::ALL {
                let r = ui.selectable_label(cur.as_ref().is_some_and(|w| w.style == s), warp_key(s));
                env.note(format!("text:warp:{}", s.key()), r.rect);
                if r.clicked() {
                    pick = Some(Some(s));
                }
            }
        });
        env.note("text:warp", cb.response.rect);
        if let Some(choice) = pick {
            tt::edit(env, "text.cmd_style", |q| match choice {
                None => q.warp = None,
                Some(s) => {
                    let w = q.warp.get_or_insert_with(TextWarp::default);
                    w.style = s;
                }
            });
        }
        if let Some(w) = &cur {
            if w.style != WarpStyle::Path {
                dim_label(ui, &t, tr("text.warp.bend"));
                let r = commit_on_release(ui, Id::new("warp_bend"), w.bend * 100.0, |ui, v| ui.add(egui::Slider::new(v, -100.0..=100.0).suffix(" %").max_decimals(0)));
                if let Some(v) = r {
                    tt::edit(env, "text.cmd_style", |q| {
                        if let Some(x) = q.warp.as_mut() {
                            x.bend = v / 100.0;
                        }
                    });
                }
            }
        }
    });
    if let Some(w) = &cur {
        ui.horizontal_wrapped(|ui| {
            if w.style != WarpStyle::Path {
                dim_label(ui, &t, tr("text.warp.distort"));
                if let Some(v) = fnum(ui, Id::new("warp_dist"), w.distort * 100.0, -100.0..=100.0, 0.5, " %", 0) {
                    tt::edit(env, "text.cmd_style", |q| {
                        if let Some(x) = q.warp.as_mut() {
                            x.distort = v / 100.0;
                        }
                    });
                }
            } else {
                // caminho: o último traço de uma camada vetorial do corte
                let mut layers: Vec<(cmds::NodeId, String)> = Vec::new();
                if let Some(cut) = env.ed.cut.clone() {
                    let mut all = Vec::new();
                    crate::app::live::ordered_leaves(&env.core.project, &cut, &mut all);
                    for (id, l) in all {
                        if l.kind == LayerType::Vector {
                            let name = env.core.project.node(&id).map(|n| n.name.clone()).unwrap_or_default();
                            layers.push((id, name));
                        }
                    }
                }
                let mut chosen = None;
                let cb = egui::ComboBox::from_id_salt("text_warp_path").width(150.0).selected_text(tr("text.warp.path_from")).show_ui(ui, |ui| {
                    for (id, name) in &layers {
                        if ui.selectable_label(false, name).clicked() {
                            chosen = Some(id.clone());
                        }
                    }
                });
                env.note("text:warp:path_from", cb.response.rect);
                if let Some(l) = chosen {
                    match last_stroke_path(env, &l) {
                        Some(pts) => tt::edit(env, "text.cmd_style", |q| {
                            if let Some(x) = q.warp.as_mut() {
                                x.path = Some(pts);
                            }
                        }),
                        None => env.ed.notify(tr("text.warp.no_stroke")),
                    }
                }
                dim_label(ui, &t, tr("text.warp.offset"));
                if let Some(v) = fnum(ui, Id::new("warp_off"), w.path_offset, -100000.0..=100000.0, 1.0, " px", 0) {
                    tt::edit(env, "text.cmd_style", |q| {
                        if let Some(x) = q.warp.as_mut() {
                            x.path_offset = v;
                        }
                    });
                }
                if w.path.as_ref().is_none_or(|p| p.len() < 2) {
                    dim_label(ui, &t, tr("text.warp.need_path"));
                }
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------ estilos guardados

fn preview_tex(env: &mut Env, ctx: &egui::Context, key: &str, st: &TextStyle) -> Option<egui::TextureHandle> {
    if let Some(t) = env.ed.text.panel.style_previews.get(key) {
        return Some(t.clone());
    }
    let frame = ctx.cumulative_pass_nr();
    if env.ed.text.panel.style_made_frame != frame {
        env.ed.text.panel.style_made_frame = frame;
        env.ed.text.panel.style_made = 0;
    }
    if env.ed.text.panel.style_made >= 2 {
        ctx.request_repaint();
        return None;
    }
    env.ed.text.panel.style_made += 1;
    let (w, h) = (150u32, 54u32);
    let mut p = TextParams { content: "Aあ!".into(), size: 30.0, pos: [w as f32 / 2.0, 6.0], align: TextAlign::Center, color: ColorRef::Direct { rgba: Rgba::WHITE }, ..Default::default() };
    st.apply(&mut p);
    p.size = 30.0;
    let lay = aiai_paint::text_layer::layout(&p);
    let colors = aiai_render::text::TextColors {
        fill: cmds::resolve_color(&env.core.project, &p.color).0,
        stroke: p.stroke.as_ref().map(|s| cmds::resolve_color(&env.core.project, &s.color).0).unwrap_or([0.0; 4]),
        shadow: p.shadow.as_ref().map(|s| cmds::resolve_color(&env.core.project, &s.color).0).unwrap_or([0.0; 4]),
    };
    let img = aiai_render::text::render_text_layer(&p, &lay, &colors, aiai_paint::Affine::IDENTITY, w, h).unwrap_or_else(|| aiai_paint::Image::new(w, h));
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let ci = egui::ColorImage { size: [w as usize, h as usize], source_size: egui::vec2(w as f32, h as f32), pixels: img.data.iter().map(|q| Color32::from_rgba_premultiplied(b(q[0]), b(q[1]), b(q[2]), b(q[3]))).collect() };
    let tex = ctx.load_texture(format!("textstyle-{key}"), ci, egui::TextureOptions::LINEAR);
    env.ed.text.panel.style_previews.insert(key.to_owned(), tex.clone());
    Some(tex)
}

fn apply_style(env: &mut Env, st: &TextStyle) {
    let st = st.clone();
    tt::edit(env, "text.cmd_preset", move |q| st.apply(q));
}

fn style_button(env: &mut Env, ui: &mut Ui, key: &str, st: &TextStyle) -> egui::Response {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(150.0, 70.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, if resp.hovered() { t.bg_hover } else { Color32::from_gray(52) });
    if let Some(tex) = preview_tex(env, &ctx, key, st) {
        painter.image(tex.id(), egui::Rect::from_min_size(rect.min, Vec2::new(150.0, 54.0)), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.text(egui::pos2(rect.center().x, rect.bottom() - 8.0), egui::Align2::CENTER_CENTER, &st.name, egui::FontId::proportional(10.5), t.text_dim);
    resp.on_hover_text(&st.name)
}

pub(super) fn presets_section(env: &mut Env, ui: &mut Ui, p: &TextParams, has_target: bool) {
    let t = env.t;
    let r = egui::CollapsingHeader::new(tr("text.sec.presets")).id_salt("text_presets").default_open(false).show(ui, |ui| {
        let all = builtin_styles();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (i, st) in all.iter().enumerate() {
                let r = style_button(env, ui, &format!("b{i}"), st);
                env.note(format!("text:preset:{i}"), r.rect);
                if r.clicked() {
                    apply_style(env, st);
                }
            }
        });
        if !env.ed.text.panel.user_files {
            dim_label(ui, &t, tr("text.presets.no_session"));
            return;
        }
        let Some(dir) = aiai_paint::text_style::styles_dir() else { return };
        if env.ed.text.panel.user_styles.is_none() {
            env.ed.text.panel.user_styles = Some(aiai_paint::text_style::list_styles(&dir));
        }
        let list = env.ed.text.panel.user_styles.clone().unwrap_or_default();
        ui.add_space(4.0);
        dim_label(ui, &t, tr("text.presets.mine"));
        let mut delete = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (path, st) in &list {
                let key = format!("u:{}", path.display());
                let r = style_button(env, ui, &key, st);
                if r.clicked() {
                    apply_style(env, st);
                }
                r.context_menu(|ui| {
                    if ui.button(tr("text.presets.delete")).clicked() {
                        delete = Some((path.clone(), key.clone()));
                        ui.close();
                    }
                });
            }
        });
        if let Some((path, key)) = delete {
            if let Err(e) = aiai_paint::text_style::delete_style(&path) {
                env.ed.notify(e);
            }
            env.ed.text.panel.style_previews.remove(&key);
            env.ed.text.panel.user_styles = None;
        }
        if has_target {
            ui.horizontal(|ui| {
                let te = ui.add(egui::TextEdit::singleline(&mut env.ed.text.panel.style_name).hint_text(tr("text.presets.name")).desired_width(140.0));
                env.note("text:preset_name", te.rect);
                let b = ui.button(tr("text.presets.save"));
                env.note("text:preset_save", b.rect);
                if b.clicked() {
                    let name = env.ed.text.panel.style_name.trim().to_owned();
                    let st = TextStyle::from_params(&name, p);
                    match aiai_paint::text_style::save_style(&dir, &st) {
                        Ok(path) => {
                            env.ed.text.panel.style_previews.remove(&format!("u:{}", path.display()));
                            env.ed.text.panel.user_styles = None;
                            env.ed.notify(format!("{} {}", tr("text.presets.saved"), path.display()));
                        }
                        Err(e) => env.ed.notify(e),
                    }
                }
                if ui.small_button(tr("text.presets.reload")).clicked() {
                    env.ed.text.panel.user_styles = None;
                }
            });
        }
    });
    env.note("text:presets", r.header_response.rect);
}

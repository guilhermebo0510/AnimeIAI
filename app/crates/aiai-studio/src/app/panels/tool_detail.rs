//! Configurações avançadas da ferramenta (S25, a chave inglesa do painel Propriedades da ferramenta): os parâmetros do
//! motor do pincel organizados em categorias como no Clip Studio (Tamanho, Tinta, Suavização, Forma, Ponta, Spray, Traçado,
//! Textura, Pincel duplo, Borda de aquarela, Apagar, Correção, Início e fim). Cada parâmetro tem um olho que decide se
//! ele aparece também no painel Propriedades da ferramenta (escolha guardada por sub-ferramenta em `prefs.drawing.shown`).
//!
//! As linhas de parâmetro (`param_ui`) servem aos dois lugares: compactas no painel, inteiras aqui e no criador de pincel.
//! Só aparece o que o motor tem: dinâmicas (pressão, velocidade, aleatório), entrada e saída, estabilização por pincel,
//! borda áspera e os 4 níveis de suavização são os do modelo de caneta do motor (`aiai_paint::brush_dyn`).

use crate::app::editor::{StabMode, ToolOptions};
use crate::app::shelf;
use crate::app::ui_kit::Env;
use crate::i18n::tr;
use crate::icons::{IconId, Icons};
use crate::theme::Tokens;
use aiai_paint::brush::{BrushMode, BrushPreset, BrushTexture, BrushTip, DualBrush, TextureKind, TextureMode, TipShape, TwoTone};
use aiai_paint::brush_dyn::{DynInput, Dynamics, EdgeNoise, Stabilization, Taper, curve_eval};
use egui::{Rect, Sense, Stroke, StrokeKind, Ui};

/// Categoria das configurações avançadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Cat {
    #[default]
    Size,
    Ink,
    AntiAlias,
    Shape,
    Tip,
    Spray,
    Stroke,
    Texture,
    Dual,
    Watercolor,
    Erase,
    Correction,
    StartEnd,
}

impl Cat {
    pub const ALL: [Cat; 13] = [Cat::Size, Cat::Ink, Cat::AntiAlias, Cat::Shape, Cat::Tip, Cat::Spray, Cat::Stroke, Cat::Texture, Cat::Dual, Cat::Watercolor, Cat::Erase, Cat::Correction, Cat::StartEnd];

    pub fn title_key(self) -> &'static str {
        match self {
            Cat::Size => "tdet.cat.size",
            Cat::Ink => "tdet.cat.ink",
            Cat::AntiAlias => "tdet.cat.aa",
            Cat::Shape => "tdet.cat.shape",
            Cat::Tip => "tdet.cat.tip",
            Cat::Spray => "tdet.cat.spray",
            Cat::Stroke => "tdet.cat.stroke",
            Cat::Texture => "tdet.cat.texture",
            Cat::Dual => "tdet.cat.dual",
            Cat::Watercolor => "tdet.cat.watercolor",
            Cat::Erase => "tdet.cat.erase",
            Cat::Correction => "tdet.cat.correction",
            Cat::StartEnd => "tdet.cat.start_end",
        }
    }
}

/// Um parâmetro: chave (guardada no olho), rótulo e categoria.
pub(crate) struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub cat: Cat,
}

const fn prm(key: &'static str, label: &'static str, cat: Cat) -> Param {
    Param { key, label, cat }
}

/// Todos os parâmetros, na ordem das categorias.
pub(crate) const PARAMS: &[Param] = &[
    prm("size", "tprop.size", Cat::Size),
    prm("opacity", "tprop.opacity", Cat::Ink),
    prm("flow", "tprop.flow", Cat::Ink),
    prm("blend_mode", "tprop.blend_mode", Cat::Ink),
    prm("mix", "tprop.mix", Cat::Ink),
    prm("smudge", "tprop.smudge", Cat::Ink),
    prm("hue_jitter", "tprop.hue_jitter", Cat::Ink),
    prm("sat_jitter", "tprop.sat_jitter", Cat::Ink),
    prm("value_jitter", "tprop.value_jitter", Cat::Ink),
    prm("aa", "tprop.aa", Cat::AntiAlias),
    prm("tip_shape", "tprop.tip_shape", Cat::Shape),
    prm("follow", "tprop.follow", Cat::Shape),
    prm("follow_tilt", "tprop.follow_tilt", Cat::Shape),
    prm("two_tone", "tprop.two_tone", Cat::Shape),
    prm("extra_tips", "tprop.extra_tips", Cat::Shape),
    prm("ribbon", "tprop.ribbon", Cat::Shape),
    prm("hardness", "tprop.hardness", Cat::Tip),
    prm("thickness", "tprop.thickness", Cat::Tip),
    prm("angle", "tprop.angle", Cat::Tip),
    prm("aspect", "tprop.aspect", Cat::Tip),
    prm("edge", "tprop.edge", Cat::Tip),
    prm("scatter", "tprop.scatter", Cat::Spray),
    prm("count", "tprop.count", Cat::Spray),
    prm("size_jitter", "tprop.size_jitter", Cat::Spray),
    prm("angle_jitter", "tprop.angle_jitter", Cat::Spray),
    prm("spacing", "tprop.spacing", Cat::Stroke),
    prm("texture", "tprop.texture", Cat::Texture),
    prm("texture_depth", "tprop.texture_depth", Cat::Texture),
    prm("texture_scale", "tprop.texture_scale", Cat::Texture),
    prm("dual", "tprop.dual", Cat::Dual),
    prm("wet_edges", "tprop.wet_edges", Cat::Watercolor),
    prm("erase", "tprop.erase", Cat::Erase),
    prm("stabilizer", "tprop.stabilizer", Cat::Correction),
    prm("velocity_full", "tprop.velocity_full", Cat::Correction),
    prm("taper", "tprop.taper", Cat::StartEnd),
];

pub(crate) fn param(key: &str) -> Option<&'static Param> {
    PARAMS.iter().find(|p| p.key == key)
}

/// O parâmetro vale para este pincel (os que não valem ficam cinza nas configurações avançadas e somem do painel).
pub(crate) fn applies(p: &BrushPreset, key: &str) -> bool {
    let round = matches!(p.tip, BrushTip::Round { .. });
    let shape = matches!(p.tip, BrushTip::Shape { .. });
    match key {
        "hardness" | "thickness" | "angle" | "edge" => round,
        "aspect" | "two_tone" => shape,
        "extra_tips" => !matches!(p.tip, BrushTip::Image { .. }),
        "ribbon" => matches!(p.tip, BrushTip::Image { .. }),
        "texture_depth" | "texture_scale" => p.texture.is_some(),
        _ => true,
    }
}

/// Janela aberta: categoria escolhida.
#[derive(Default)]
pub(crate) struct Detail {
    pub cat: Cat,
    preview: Option<(BrushPreset, egui::TextureHandle)>,
}

/// Contexto de edição de um pincel: a cópia em edição (`p`, com o tamanho e a opacidade dentro) e as opções da
/// ferramenta (estabilizador do painel, quando o pincel não tem o seu).
pub(crate) struct Edit<'a> {
    pub p: &'a mut BrushPreset,
    pub o: Option<&'a mut ToolOptions>,
    pub t: Tokens,
    pub icons: &'a mut Icons,
    pub notes: &'a mut Vec<(String, Rect)>,
    /// Painel (linhas curtas, só o essencial) ou configurações avançadas (tudo).
    pub compact: bool,
    pub salt: &'static str,
}

const DYN_W: f32 = 22.0;

/// Linha de um parâmetro. Devolve se mudou algo.
pub(crate) fn param_ui(ui: &mut Ui, e: &mut Edit, key: &str) -> bool {
    let Some(prm) = param(key) else { return false };
    let enabled = applies(e.p, key);
    let before = e.p.clone();
    let before_o = e.o.as_ref().map(|o| (o.stab_mode, o.stab_window, o.stab_radius));
    ui.add_enabled_ui(enabled, |ui| match key {
        "size" => {
            let mut v = e.p.size;
            bar(ui, e, prm.label, &mut v, 0.5, 600.0, true, 1, " px", Dyn::Size);
            e.p.size = v;
        }
        "opacity" => pct_bar(ui, e, prm.label, Field::Opacity, Dyn::Opacity),
        "flow" => pct_bar(ui, e, prm.label, Field::Flow, Dyn::Flow),
        "mix" => pct_bar(ui, e, prm.label, Field::Mix, Dyn::None),
        "smudge" => pct_bar(ui, e, prm.label, Field::Smudge, Dyn::None),
        "hue_jitter" => pct_bar(ui, e, prm.label, Field::Hue, Dyn::None),
        "sat_jitter" => pct_bar(ui, e, prm.label, Field::Sat, Dyn::None),
        "value_jitter" => pct_bar(ui, e, prm.label, Field::Val, Dyn::None),
        "wet_edges" => pct_bar(ui, e, prm.label, Field::Wet, Dyn::None),
        "size_jitter" => pct_bar(ui, e, prm.label, Field::SizeJitter, Dyn::None),
        "blend_mode" => labeled(ui, e, prm.label, |ui, e| {
            let key = |m: BrushMode| match m {
                BrushMode::Normal => "bmode.normal",
                BrushMode::Multiply => "bmode.multiply",
                BrushMode::Screen => "bmode.screen",
                BrushMode::Overlay => "bmode.overlay",
                BrushMode::Add => "bmode.add",
                BrushMode::Behind => "bmode.behind",
                BrushMode::Erase => "bmode.erase",
            };
            egui::ComboBox::from_id_salt(("tprop_mode", e.salt)).width(ui.available_width().min(140.0)).selected_text(tr(key(e.p.mode))).show_ui(ui, |ui| {
                for m in [BrushMode::Normal, BrushMode::Multiply, BrushMode::Screen, BrushMode::Overlay, BrushMode::Add, BrushMode::Behind] {
                    ui.selectable_value(&mut e.p.mode, m, tr(key(m)));
                }
            });
        }),
        "aa" => labeled(ui, e, prm.label, |ui, e| aa_icons(ui, e)),
        "tip_shape" => labeled(ui, e, prm.label, |ui, e| tip_combo(ui, e)),
        "extra_tips" => labeled(ui, e, prm.label, |ui, e| extra_tips_ui(ui, e)),
        "ribbon" => {
            if matches!(e.p.tip, BrushTip::Image { .. }) {
                labeled(ui, e, prm.label, |ui, e| {
                    ui.checkbox(&mut e.p.ribbon, "").on_hover_text(tr("tprop.ribbon.tip"));
                })
            } else {
                disabled_row(ui, e, prm.label)
            }
        }
        "follow" => labeled(ui, e, prm.label, |ui, e| {
            ui.checkbox(&mut e.p.follow_direction, "").on_hover_text(tr("tprop.follow.tip"));
        }),
        "follow_tilt" => labeled(ui, e, prm.label, |ui, e| {
            ui.checkbox(&mut e.p.follow_tilt, "").on_hover_text(tr("tprop.follow_tilt.tip"));
        }),
        "two_tone" => labeled(ui, e, prm.label, |ui, e| {
            let mut on = e.p.two_tone.is_some();
            if ui.checkbox(&mut on, "").changed() {
                e.p.two_tone = on.then(|| TwoTone { tint: [0.55, 0.55, 0.7], amount: 0.8, color: None });
            }
            if let Some(tt) = &mut e.p.two_tone {
                ui.add(egui::DragValue::new(&mut tt.amount).range(0.0..=1.0).speed(0.01).max_decimals(2));
                ui.color_edit_button_rgb(&mut tt.tint).on_hover_text(tr("brush.two_tone_tint"));
            }
        }),
        "hardness" | "angle" => {
            if let BrushTip::Round { hardness, angle, .. } = &mut e.p.tip {
                if key == "hardness" {
                    let mut v = *hardness * 100.0;
                    bar(ui, e, prm.label, &mut v, 0.0, 100.0, false, 0, "%", Dyn::None);
                    if let BrushTip::Round { hardness, .. } = &mut e.p.tip {
                        *hardness = (v / 100.0).clamp(0.0, 1.0);
                    }
                } else {
                    let mut v = *angle;
                    bar(ui, e, prm.label, &mut v, -90.0, 90.0, false, 0, "°", Dyn::None);
                    if let BrushTip::Round { angle, .. } = &mut e.p.tip {
                        *angle = v;
                    }
                }
            } else {
                disabled_row(ui, e, prm.label);
            }
        }
        "thickness" => {
            let mut v = match e.p.tip {
                BrushTip::Round { roundness, .. } => roundness * 100.0,
                _ => 100.0,
            };
            bar(ui, e, prm.label, &mut v, 5.0, 100.0, false, 0, "%", Dyn::Thickness);
            if let BrushTip::Round { roundness, .. } = &mut e.p.tip {
                *roundness = (v / 100.0).clamp(0.05, 1.0);
            }
        }
        "aspect" => {
            let mut v = match e.p.tip {
                BrushTip::Shape { aspect, .. } => aspect,
                _ => 1.0,
            };
            bar(ui, e, prm.label, &mut v, 0.2, 3.0, false, 2, "", Dyn::None);
            if let BrushTip::Shape { aspect, .. } = &mut e.p.tip {
                *aspect = v;
            }
        }
        "edge" => {
            let mut v = e.p.edge.as_ref().map(|x| x.amount * 100.0).unwrap_or(0.0);
            bar(ui, e, prm.label, &mut v, 0.0, 100.0, false, 0, "%", Dyn::None);
            let amount = (v / 100.0).clamp(0.0, 1.0);
            if amount <= 0.0 {
                e.p.edge = None;
            } else {
                let (grain, seed) = e.p.edge.as_ref().map(|x| (x.grain, x.seed)).unwrap_or((0.5, 0));
                e.p.edge = Some(EdgeNoise { amount, grain, seed });
            }
        }
        "scatter" => {
            let mut v = e.p.scatter;
            bar(ui, e, prm.label, &mut v, 0.0, 2.0, false, 2, "", Dyn::None);
            e.p.scatter = v;
        }
        "count" => {
            let mut v = e.p.count as f32;
            bar(ui, e, prm.label, &mut v, 1.0, 16.0, false, 0, "", Dyn::None);
            e.p.count = v.round().clamp(1.0, 64.0) as u32;
        }
        "angle_jitter" => {
            let mut v = e.p.angle_jitter;
            bar(ui, e, prm.label, &mut v, 0.0, 180.0, false, 0, "°", Dyn::None);
            e.p.angle_jitter = v;
        }
        "spacing" => {
            let mut v = e.p.spacing * 100.0;
            bar(ui, e, prm.label, &mut v, 2.0, 200.0, true, 0, "%", Dyn::None);
            e.p.spacing = (v / 100.0).clamp(0.02, 2.0);
        }
        "texture" => labeled(ui, e, prm.label, |ui, e| texture_combo(ui, e)),
        "texture_depth" => {
            let mut v = e.p.texture.as_ref().map(|x| x.depth * 100.0).unwrap_or(0.0);
            bar(ui, e, prm.label, &mut v, 0.0, 100.0, false, 0, "%", Dyn::None);
            if let Some(x) = &mut e.p.texture {
                x.depth = (v / 100.0).clamp(0.0, 1.0);
            }
        }
        "texture_scale" => {
            let mut v = e.p.texture.as_ref().map(|x| x.scale).unwrap_or(1.0);
            bar(ui, e, prm.label, &mut v, 0.5, 30.0, true, 1, "", Dyn::None);
            if let Some(x) = &mut e.p.texture {
                x.scale = v;
            }
        }
        "dual" => dual_ui(ui, e, prm.label),
        "erase" => labeled(ui, e, prm.label, |ui, e| {
            let mut on = e.p.mode == BrushMode::Erase;
            if ui.checkbox(&mut on, "").on_hover_text(tr("tprop.erase.tip")).changed() {
                e.p.mode = if on { BrushMode::Erase } else { BrushMode::Normal };
            }
        }),
        "stabilizer" => stabilizer_ui(ui, e, prm.label),
        "velocity_full" => {
            let mut v = e.p.velocity_full();
            bar(ui, e, prm.label, &mut v, 200.0, 10000.0, true, 0, " px/s", Dyn::None);
            e.p.velocity_full = if (v - aiai_paint::brush::VELOCITY_FULL_DEFAULT).abs() < 1.0 { 0.0 } else { v };
        }
        "taper" => taper_ui(ui, e, prm.label),
        _ => {}
    });
    let after_o = e.o.as_ref().map(|o| (o.stab_mode, o.stab_window, o.stab_radius));
    *e.p != before || after_o != before_o
}

#[derive(Clone, Copy)]
enum Field {
    Opacity,
    Flow,
    Mix,
    Smudge,
    Hue,
    Sat,
    Val,
    Wet,
    SizeJitter,
}

fn field(p: &mut BrushPreset, f: Field) -> &mut f32 {
    match f {
        Field::Opacity => &mut p.opacity,
        Field::Flow => &mut p.flow,
        Field::Mix => &mut p.mix,
        Field::Smudge => &mut p.smudge_carry,
        Field::Hue => &mut p.hue_jitter,
        Field::Sat => &mut p.saturation_jitter,
        Field::Val => &mut p.value_jitter,
        Field::Wet => &mut p.wet_edges,
        Field::SizeJitter => &mut p.size_jitter,
    }
}

fn pct_bar(ui: &mut Ui, e: &mut Edit, label: &str, f: Field, d: Dyn) {
    let mut v = *field(e.p, f) * 100.0;
    let lo = if matches!(f, Field::Opacity | Field::Flow) { 1.0 } else { 0.0 };
    bar(ui, e, label, &mut v, lo, 100.0, false, 0, "%", d);
    *field(e.p, f) = (v / 100.0).clamp(0.0, 1.0);
}

/// Qual dinâmica o ícone de pressão ao lado da linha edita.
#[derive(Clone, Copy, PartialEq)]
enum Dyn {
    None,
    Size,
    Opacity,
    Flow,
    Thickness,
}

/// Linha com rótulo à esquerda e um controle à direita.
fn labeled(ui: &mut Ui, e: &mut Edit, label: &str, add: impl FnOnce(&mut Ui, &mut Edit)) {
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        let lw = if e.compact { 92.0 } else { 150.0 };
        let (r, _) = ui.allocate_exact_size(egui::vec2(lw, 20.0), Sense::hover());
        ui.painter().with_clip_rect(r).text(egui::pos2(r.left() + 2.0, r.center().y), egui::Align2::LEFT_CENTER, tr(label), egui::FontId::proportional(11.5), if ui.is_enabled() { e.t.text } else { e.t.text_dim });
        add(ui, e);
    });
}

fn disabled_row(ui: &mut Ui, e: &mut Edit, label: &str) {
    ui.add_enabled_ui(false, |ui| labeled(ui, e, label, |ui, _| {
        ui.label("—");
    }));
}

/// Linha de controle deslizante como a dos programas de pintura: rótulo em cima à esquerda, valor à direita, a barra
/// fina embaixo e, se o parâmetro tem dinâmica, o ícone de pressão na ponta (abre a curva).
#[allow(clippy::too_many_arguments)]
fn bar(ui: &mut Ui, e: &mut Edit, label: &str, v: &mut f32, lo: f32, hi: f32, log: bool, decimals: usize, suffix: &str, d: Dyn) {
    let t = e.t;
    let w = ui.available_width();
    let h = 26.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    let enabled = ui.is_enabled();
    let right = rect.right() - DYN_W;
    let ink = if enabled { t.text } else { t.text_dim };
    ui.painter().with_clip_rect(rect).text(egui::pos2(rect.left() + 2.0, rect.top() + 7.0), egui::Align2::LEFT_CENTER, tr(label), egui::FontId::proportional(11.5), ink);
    // valor (arrastar ou digitar)
    let vr = Rect::from_min_max(egui::pos2(right - 62.0, rect.top()), egui::pos2(right - 4.0, rect.top() + 15.0));
    let mut dv = *v;
    let r = ui.put(vr, egui::DragValue::new(&mut dv).range(lo..=hi).speed(((hi - lo) / 300.0).max(0.01)).max_decimals(decimals).suffix(suffix));
    e.notes.push((format!("tprop:{label}:value"), r.rect));
    if r.changed() {
        *v = dv;
    }
    // barra
    let br = Rect::from_min_max(egui::pos2(rect.left() + 2.0, rect.top() + 18.0), egui::pos2(right - 4.0, rect.top() + 23.0));
    let resp = ui.interact(br.expand2(egui::vec2(0.0, 4.0)), ui.id().with(("tprop_bar", label, e.salt)), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    e.notes.push((format!("tprop:{label}"), br));
    let to_f = |x: f32| if log { (x.max(lo.max(1e-3)) / lo.max(1e-3)).ln() / (hi / lo.max(1e-3)).ln() } else { (x - lo) / (hi - lo) };
    let from_f = |f: f32| if log { lo.max(1e-3) * (hi / lo.max(1e-3)).powf(f) } else { lo + f * (hi - lo) };
    if let Some(p) = resp.interact_pointer_pos() {
        let f = ((p.x - br.left()) / br.width()).clamp(0.0, 1.0);
        let nv = from_f(f);
        let q = 10f32.powi(decimals as i32);
        *v = (nv * q).round() / q;
    }
    let f = to_f(*v).clamp(0.0, 1.0);
    let p = ui.painter();
    p.rect_filled(br, 2.0, t.bg_widget);
    let fill = Rect::from_min_max(br.min, egui::pos2(br.left() + br.width() * f, br.bottom()));
    p.rect_filled(fill, 2.0, if enabled { t.accent } else { t.text_dim.gamma_multiply(0.5) });
    p.rect_stroke(br, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if d != Dyn::None {
        let ir = Rect::from_center_size(egui::pos2(rect.right() - DYN_W / 2.0, rect.center().y), egui::vec2(18.0, 18.0));
        dyn_button(ui, e, ir, d, label);
    }
}

/// Barra sem dinâmica para valores fora do pincel (borracha, largura da caneta vetorial).
#[allow(clippy::too_many_arguments)]
pub(crate) fn plain_bar(ui: &mut Ui, e: &mut Edit, label: &str, v: &mut f32, lo: f32, hi: f32, log: bool, decimals: usize, suffix: &str) {
    bar(ui, e, label, v, lo, hi, log, decimals, suffix, Dyn::None);
}

/// Ícone de pressão: aceso quando o parâmetro tem dinâmica; o clique abre a curva.
fn dyn_button(ui: &mut Ui, e: &mut Edit, ir: Rect, d: Dyn, label: &str) {
    let t = e.t;
    let active = match d {
        Dyn::Size => e.p.size_dyn.as_ref().map(dyn_active).unwrap_or(e.p.size_pressure_min < 0.999),
        Dyn::Opacity => e.p.opacity_dyn.as_ref().map(dyn_active).unwrap_or(e.p.opacity_pressure_min < 0.999),
        Dyn::Flow => e.p.flow_dyn.as_ref().map(dyn_active).unwrap_or(e.p.flow_pressure_min < 0.999),
        Dyn::Thickness => e.p.thickness_dyn.as_ref().is_some_and(dyn_active),
        Dyn::None => false,
    };
    let resp = ui.interact(ir, ui.id().with(("tprop_dyn", label, e.salt)), Sense::click());
    e.notes.push((format!("tprop:{label}:dyn"), ir));
    let p = ui.painter();
    if active {
        p.rect(ir.expand(1.0), 3.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(ir.expand(1.0), 3.0, t.bg_hover);
    }
    e.icons.paint(p, IconId::Pressure, ir.shrink(2.0), if active || resp.hovered() { t.text } else { t.text_dim });
    let resp = resp.on_hover_text(tr("tprop.dyn.tip"));
    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_width(250.0);
        ui.label(egui::RichText::new(format!("{} · {}", tr(label), tr("tprop.dyn"))).strong());
        let (dynamics, legacy) = match d {
            Dyn::Size => (&mut e.p.size_dyn, Some(&mut e.p.size_pressure_min)),
            Dyn::Opacity => (&mut e.p.opacity_dyn, Some(&mut e.p.opacity_pressure_min)),
            Dyn::Flow => (&mut e.p.flow_dyn, Some(&mut e.p.flow_pressure_min)),
            Dyn::Thickness => (&mut e.p.thickness_dyn, None),
            Dyn::None => return,
        };
        dyn_editor(ui, t, dynamics, legacy, label);
    });
}

fn dyn_active(d: &Dynamics) -> bool {
    d.pressure.as_ref().is_some_and(|p| p.min < 0.999) || d.velocity.is_some() || d.random.is_some() || d.tilt.is_some()
}

/// Editor da dinâmica: pressão (mínimo e curva), velocidade (mínimo e curva; de fábrica, rápido afina), inclinação da
/// caneta (mínimo e curva) e aleatório.
/// Sem dinâmica gravada, o mínimo antigo de pressão do preset vira o ponto de partida.
fn dyn_editor(ui: &mut Ui, t: Tokens, dynamics: &mut Option<Dynamics>, legacy: Option<&mut f32>, salt: &str) {
    let legacy_min = legacy.as_ref().map(|m| **m).unwrap_or(1.0);
    let mut d = dynamics.clone().unwrap_or_else(|| if legacy_min < 0.999 { Dynamics::pressure(legacy_min) } else { Dynamics::default() });
    let before = d.clone();
    // pressão
    let mut on = d.pressure.is_some();
    ui.checkbox(&mut on, tr("tprop.dyn.pressure"));
    if on != d.pressure.is_some() {
        d.pressure = on.then(|| DynInput::linear(0.2));
    }
    if let Some(p) = &mut d.pressure {
        min_slider(ui, &mut p.min);
        curve_editor(ui, t, &mut p.curve, false, (salt, "pressure"));
    }
    ui.separator();
    // velocidade
    let mut on = d.velocity.is_some();
    ui.checkbox(&mut on, tr("tprop.dyn.velocity")).on_hover_text(tr("tprop.dyn.velocity.tip"));
    if on != d.velocity.is_some() {
        d.velocity = on.then(|| DynInput::slower_is_bigger(0.5));
    }
    if let Some(v) = &mut d.velocity {
        min_slider(ui, &mut v.min);
        curve_editor(ui, t, &mut v.curve, true, (salt, "velocity"));
    }
    ui.separator();
    // inclinação da caneta (0 = em pé, 1 = deitada a 60°); com mouse não muda nada
    let mut on = d.tilt.is_some();
    ui.checkbox(&mut on, tr("tprop.dyn.tilt")).on_hover_text(tr("tprop.dyn.tilt.tip"));
    if on != d.tilt.is_some() {
        d.tilt = on.then(|| DynInput::linear(0.3));
    }
    if let Some(v) = &mut d.tilt {
        min_slider(ui, &mut v.min);
        curve_editor(ui, t, &mut v.curve, false, (salt, "tilt"));
    }
    ui.separator();
    // aleatório
    let mut on = d.random.is_some();
    ui.checkbox(&mut on, tr("tprop.dyn.random"));
    if on != d.random.is_some() {
        d.random = on.then_some(0.5);
    }
    if let Some(r) = &mut d.random {
        min_slider(ui, r);
    }
    ui.add_space(2.0);
    ui.label(egui::RichText::new(tr("tprop.dyn.note")).size(11.0).color(t.text_dim));
    if d != before {
        // a dinâmica gravada manda; o mínimo antigo acompanha a pressão (pincéis lidos por versões anteriores)
        if let Some(m) = legacy {
            *m = d.pressure.as_ref().map(|p| p.min).unwrap_or(1.0);
        }
        *dynamics = Some(d);
    }
}

fn min_slider(ui: &mut Ui, v: &mut f32) {
    ui.horizontal(|ui| {
        ui.label(tr("tprop.dyn.min"));
        let mut pct = *v * 100.0;
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            *v = (pct / 100.0).clamp(0.0, 1.0);
        }
    });
}

/// Curva de resposta (pontos [x, y] em 0..1): arrastar move o ponto, clique no vazio cria, duplo clique tira.
/// Vazia = linear (ou a descida padrão da velocidade, `descending`).
fn curve_editor(ui: &mut Ui, t: Tokens, pts: &mut Vec<[f32; 2]>, descending: bool, salt: (&str, &str)) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(240.0), 110.0), Sense::click_and_drag());
    let id = ui.id().with(("curve_drag", salt.0, salt.1));
    let to_screen = |p: [f32; 2]| egui::pos2(rect.left() + p[0] * rect.width(), rect.bottom() - p[1] * rect.height());
    let to_curve = |q: egui::Pos2| [((q.x - rect.left()) / rect.width()).clamp(0.0, 1.0), ((rect.bottom() - q.y) / rect.height()).clamp(0.0, 1.0)];
    if pts.is_empty() && (resp.drag_started() || resp.clicked()) {
        *pts = if descending { vec![[0.0, 1.0], [1.0, 0.0]] } else { vec![[0.0, 0.0], [1.0, 1.0]] };
    }
    let near = |q: egui::Pos2, pts: &[[f32; 2]]| pts.iter().position(|p| to_screen(*p).distance(q) < 8.0);
    if resp.drag_started() {
        let hit = resp.interact_pointer_pos().and_then(|q| near(q, pts));
        ui.data_mut(|d| d.insert_temp(id, hit));
    }
    let dragging: Option<usize> = ui.data(|d| d.get_temp(id)).flatten();
    if let (Some(i), Some(q)) = (dragging.filter(|_| resp.dragged()), resp.interact_pointer_pos()) {
        if i < pts.len() {
            let mut c = to_curve(q);
            let n = pts.len();
            // as pontas ficam em x = 0 e x = 1; os do meio entre os vizinhos
            c[0] = if i == 0 { 0.0 } else if i == n - 1 { 1.0 } else { c[0].clamp(pts[i - 1][0] + 0.01, pts[i + 1][0] - 0.01) };
            pts[i] = c;
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<usize>>(id));
    }
    if resp.double_clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|q| near(q, pts)) {
            if i > 0 && i + 1 < pts.len() {
                pts.remove(i);
            }
        }
    } else if resp.clicked() {
        if let Some(q) = resp.interact_pointer_pos().filter(|q| near(*q, pts).is_none()) {
            let c = to_curve(q);
            let at = pts.iter().position(|p| p[0] > c[0]).unwrap_or(pts.len());
            if at > 0 && at < pts.len() {
                pts.insert(at, c);
            }
        }
    }
    let p = ui.painter_at(rect);
    p.rect(rect, 2.0, t.bg_widget, Stroke::new(1.0, t.border), StrokeKind::Inside);
    for k in 1..4 {
        let f = k as f32 / 4.0;
        p.vline(rect.left() + rect.width() * f, rect.y_range(), Stroke::new(1.0, t.border.gamma_multiply(0.5)));
        p.hline(rect.x_range(), rect.top() + rect.height() * f, Stroke::new(1.0, t.border.gamma_multiply(0.5)));
    }
    let eval: Vec<[f32; 2]> = if pts.is_empty() && descending { vec![[0.0, 1.0], [1.0, 0.0]] } else { pts.clone() };
    let line: Vec<egui::Pos2> = (0..=48).map(|k| {
        let x = k as f32 / 48.0;
        to_screen([x, curve_eval(&eval, x)])
    }).collect();
    p.add(egui::Shape::line(line, Stroke::new(1.6, t.accent)));
    for q in pts.iter() {
        p.circle(to_screen(*q), 3.5, t.bg_panel, Stroke::new(1.4, t.text));
    }
    resp.on_hover_text(tr("tprop.curve.tip"));
}

/// Suavização de serrilhado em 4 ícones (nenhuma, fraca, média, forte). Sem nível gravado (pincel antigo) a borda de ~1 px
/// de sempre equivale à fraca.
fn aa_icons(ui: &mut Ui, e: &mut Edit) {
    let cur = match e.p.antialias {
        Some(l) => l,
        None if e.p.aliased => 0,
        None => 1,
    };
    let t = e.t;
    for (lvl, icon, key) in [(0u8, IconId::AaNone, "tprop.aa.none"), (1, IconId::AaWeak, "tprop.aa.weak"), (2, IconId::AaMid, "tprop.aa.mid"), (3, IconId::AaStrong, "tprop.aa.strong")] {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), Sense::click());
        e.notes.push((format!("tprop:aa:{lvl}"), r));
        let on = cur == lvl;
        let p = ui.painter();
        p.rect(r, 3.0, if on { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
        e.icons.paint(p, icon, r.shrink(3.0), if on { t.text } else { t.text_dim });
        if resp.on_hover_text(tr(key)).clicked() && ui.is_enabled() {
            e.p.antialias = Some(lvl);
            e.p.aliased = lvl == 0;
        }
    }
}

/// Várias pontas (Multi-Tip, auditoria 4.9d): cada forma ligada entra na roda de pontas do carimbo, com a de cima.
fn extra_tips_ui(ui: &mut Ui, e: &mut Edit) {
    ui.vertical(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            for (s, k) in TIP_SHAPES {
                let has = e.p.extra_tips.iter().any(|t| matches!(t, BrushTip::Shape { shape, .. } if *shape == s));
                let r = ui.selectable_label(has, egui::RichText::new(tr(k)).size(11.0));
                e.notes.push((format!("tprop:extra_tip:{k}"), r.rect));
                if r.clicked() {
                    if has {
                        e.p.extra_tips.retain(|t| !matches!(t, BrushTip::Shape { shape, .. } if *shape == s));
                    } else {
                        e.p.extra_tips.push(BrushTip::Shape { shape: s, aspect: 1.0 });
                    }
                }
            }
        });
        if !e.p.extra_tips.is_empty() {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut e.p.tip_order, aiai_paint::brush::TipOrder::Random, tr("tprop.tip_order.random"));
                ui.selectable_value(&mut e.p.tip_order, aiai_paint::brush::TipOrder::Sequence, tr("tprop.tip_order.sequence"));
            });
        }
    });
}

const TIP_SHAPES: [(TipShape, &str); 29] = [
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
    (TipShape::Note, "tipshape.note"),
    (TipShape::Book, "tipshape.book"),
    (TipShape::Building, "tipshape.building"),
    (TipShape::Button, "tipshape.button"),
    (TipShape::Bead, "tipshape.bead"),
    (TipShape::ChainLink, "tipshape.chain"),
    (TipShape::ChainLinkSide, "tipshape.chain_side"),
    (TipShape::ZipperA, "tipshape.zipper_a"),
    (TipShape::ZipperB, "tipshape.zipper_b"),
    (TipShape::BraidL, "tipshape.braid_l"),
    (TipShape::BraidR, "tipshape.braid_r"),
    (TipShape::Scallop, "tipshape.scallop"),
    (TipShape::Swirl, "tipshape.swirl"),
    (TipShape::Wave, "tipshape.wave"),
    (TipShape::Splat, "tipshape.splat"),
];

fn tip_combo(ui: &mut Ui, e: &mut Edit) {
    const SHAPES: [(TipShape, &str); 29] = TIP_SHAPES;
    let current = match &e.p.tip {
        BrushTip::Round { .. } => tr("tipshape.round").to_owned(),
        BrushTip::Shape { shape, .. } => SHAPES.iter().find(|(s, _)| s == shape).map(|(_, k)| tr(k).to_owned()).unwrap_or_default(),
        BrushTip::Image { .. } => tr("tipshape.image").to_owned(),
    };
    let mut next: Option<BrushTip> = None;
    egui::ComboBox::from_id_salt(("tprop_tip", e.salt)).width(ui.available_width().min(140.0)).selected_text(current).show_ui(ui, |ui| {
        if ui.selectable_label(matches!(e.p.tip, BrushTip::Round { .. }), tr("tipshape.round")).clicked() {
            next = Some(BrushTip::Round { hardness: 0.8, roundness: 1.0, angle: 0.0 });
        }
        for (s, k) in SHAPES {
            if ui.selectable_label(matches!(&e.p.tip, BrushTip::Shape { shape, .. } if *shape == s), tr(k)).clicked() {
                next = Some(BrushTip::Shape { shape: s, aspect: 1.0 });
            }
        }
    });
    if let Some(n) = next {
        e.p.tip = n;
    }
}

fn texture_combo(ui: &mut Ui, e: &mut Edit) {
    let key = |k: TextureKind| match k {
        TextureKind::Paper => "maker.tex_paper",
        TextureKind::Canvas => "maker.tex_canvas",
        TextureKind::Noise => "maker.tex_noise",
        TextureKind::Cells => "maker.tex_cells",
        TextureKind::Grain => "maker.tex_grain",
    };
    let cur = match &e.p.texture {
        None => tr("maker.tex_none").to_owned(),
        Some(x) if x.image.is_some() => tr("maker.tex_image").to_owned(),
        Some(x) => tr(key(x.kind)).to_owned(),
    };
    egui::ComboBox::from_id_salt(("tprop_tex", e.salt)).width(ui.available_width().min(140.0)).selected_text(cur).show_ui(ui, |ui| {
        if ui.selectable_label(e.p.texture.is_none(), tr("maker.tex_none")).clicked() {
            e.p.texture = None;
        }
        for k in [TextureKind::Paper, TextureKind::Canvas, TextureKind::Noise, TextureKind::Cells, TextureKind::Grain] {
            let sel = e.p.texture.as_ref().is_some_and(|x| x.image.is_none() && x.kind == k);
            if ui.selectable_label(sel, tr(key(k))).clicked() {
                e.p.texture = Some(BrushTexture { kind: k, scale: 4.0, depth: 0.6, mode: TextureMode::Multiply, seed: 7, invert: false, image: None, image_mask: None });
            }
        }
    });
    if !e.compact {
        if let Some(x) = &mut e.p.texture {
            ui.selectable_value(&mut x.mode, TextureMode::Multiply, tr("maker.tex_multiply"));
            ui.selectable_value(&mut x.mode, TextureMode::Subtract, tr("maker.tex_subtract"));
            ui.checkbox(&mut x.invert, tr("maker.invert"));
        }
    }
}

fn dual_ui(ui: &mut Ui, e: &mut Edit, label: &str) {
    labeled(ui, e, label, |ui, e| {
        let mut on = e.p.dual.is_some();
        if ui.checkbox(&mut on, "").on_hover_text(tr("tprop.dual.tip")).changed() {
            e.p.dual = on.then(|| DualBrush { size: (e.p.size * 0.5).max(2.0), spacing: 0.3, scatter: 0.4, hardness: 0.6 });
        }
    });
    if e.compact {
        return;
    }
    if let Some(mut dl) = e.p.dual.clone() {
        bar(ui, e, "tprop.dual.size", &mut dl.size, 1.0, 300.0, true, 1, " px", Dyn::None);
        let mut sp = dl.spacing * 100.0;
        bar(ui, e, "tprop.spacing", &mut sp, 2.0, 200.0, true, 0, "%", Dyn::None);
        dl.spacing = (sp / 100.0).clamp(0.02, 2.0);
        bar(ui, e, "tprop.scatter", &mut dl.scatter, 0.0, 2.0, false, 2, "", Dyn::None);
        let mut hd = dl.hardness * 100.0;
        bar(ui, e, "tprop.hardness", &mut hd, 0.0, 100.0, false, 0, "%", Dyn::None);
        dl.hardness = (hd / 100.0).clamp(0.0, 1.0);
        e.p.dual = Some(dl);
    }
}

/// Estabilização: a do pincel (0..100, "pela velocidade") quando ele tem; senão a do painel (média móvel ou corda),
/// que vale para todos os pincéis sem estabilização própria.
fn stabilizer_ui(ui: &mut Ui, e: &mut Edit, label: &str) {
    if let Some(mut s) = e.p.stabilization {
        let mut v = s.level as f32;
        bar(ui, e, label, &mut v, 0.0, 100.0, false, 0, "", Dyn::None);
        s.level = v.round().clamp(0.0, 100.0) as u8;
        if !e.compact {
            ui.checkbox(&mut s.by_speed, tr("tprop.stab.by_speed"));
            if ui.button(tr("tprop.stab.use_panel")).on_hover_text(tr("tprop.stab.use_panel.tip")).clicked() {
                e.p.stabilization = None;
                return;
            }
        }
        e.p.stabilization = Some(s);
        return;
    }
    let mut make_own = false;
    match e.o.as_deref_mut() {
        Some(o) => {
            let lbl = |m: StabMode| match m {
                StabMode::Off => "stab.off",
                StabMode::Average => "stab.average",
                StabMode::Lazy => "stab.lazy",
            };
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                let lw = if e.compact { 92.0 } else { 150.0 };
                let (r, _) = ui.allocate_exact_size(egui::vec2(lw, 20.0), Sense::hover());
                ui.painter().with_clip_rect(r).text(egui::pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, tr(label), egui::FontId::proportional(12.0), e.t.text);
                egui::ComboBox::from_id_salt(("tprop_stab", e.salt)).width(96.0).selected_text(tr(lbl(o.stab_mode))).show_ui(ui, |ui| {
                    for m in [StabMode::Off, StabMode::Average, StabMode::Lazy] {
                        ui.selectable_value(&mut o.stab_mode, m, tr(lbl(m)));
                    }
                });
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
        None => disabled_row(ui, e, label),
    }
    if !e.compact && ui.button(tr("tprop.stab.make_own")).on_hover_text(tr("tprop.stab.make_own.tip")).clicked() {
        make_own = true;
    }
    if make_own {
        e.p.stabilization = Some(Stabilization { level: 30, by_speed: true });
    }
}

/// Entrada e saída (afinamento das pontas do traço).
fn taper_ui(ui: &mut Ui, e: &mut Edit, label: &str) {
    let mut tp = e.p.taper.clone().unwrap_or(Taper { start: 0.0, end: 0.0, min: 0.0, size: true, opacity: false, by_speed: false });
    let before = tp.clone();
    bar(ui, e, "tprop.taper.start", &mut tp.start, 0.0, 400.0, false, 0, " px", Dyn::None);
    bar(ui, e, "tprop.taper.end", &mut tp.end, 0.0, 400.0, false, 0, " px", Dyn::None);
    if !e.compact {
        let mut m = tp.min * 100.0;
        bar(ui, e, "tprop.taper.min", &mut m, 0.0, 100.0, false, 0, "%", Dyn::None);
        tp.min = (m / 100.0).clamp(0.0, 1.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut tp.size, tr("tprop.taper.size"));
            ui.checkbox(&mut tp.opacity, tr("tprop.taper.opacity"));
            ui.checkbox(&mut tp.by_speed, tr("tprop.taper.by_speed"));
        });
    }
    let _ = label;
    if tp != before {
        e.p.taper = if tp.start <= 0.0 && tp.end <= 0.0 { None } else { Some(tp) };
    }
}

// ------------------------------------------------------------------------------------------------ janela

/// Abre as configurações avançadas do pincel em uso.
pub(crate) fn open(env: &mut Env) {
    if env.ed.draw_ui.detail.is_none() {
        env.ed.draw_ui.detail = Some(Detail::default());
    }
}

/// As categorias (lista à esquerda) e os parâmetros da escolhida (à direita), com o olho de cada um quando `eyes`
/// (chave da sub-ferramenta). Usada pela janela e pelo criador de pincel.
pub(crate) fn categories_ui(ui: &mut Ui, e: &mut Edit, cat: &mut Cat, eyes: Option<(&str, &mut crate::drawing_prefs::DrawingPrefs)>) -> bool {
    let mut changed = false;
    let mut eyes = eyes;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(150.0);
            for c in Cat::ALL {
                let r = ui.selectable_label(*cat == c, tr(c.title_key()));
                e.notes.push((format!("tdet:cat:{}", tr_key_tail(c.title_key())), r.rect));
                if r.clicked() {
                    *cat = c;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(300.0);
            for prm in PARAMS.iter().filter(|p| p.cat == *cat) {
                ui.horizontal(|ui| {
                    if let Some((key, prefs)) = eyes.as_mut() {
                        let on = prefs.is_shown(key, prm.key);
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
                        e.notes.push((format!("tdet:eye:{}", prm.key), r));
                        let p = ui.painter();
                        if resp.hovered() {
                            p.rect_filled(r, 3.0, e.t.bg_hover);
                        }
                        e.icons.paint(p, if on { IconId::Eye } else { IconId::EyeOff }, r.shrink(3.0), if on { e.t.text } else { e.t.text_dim.gamma_multiply(0.6) });
                        if resp.on_hover_text(tr("tdet.eye.tip")).clicked() {
                            prefs.set_shown(key, prm.key, !on);
                        }
                    }
                    ui.vertical(|ui| {
                        changed |= param_ui(ui, e, prm.key);
                    });
                });
            }
            if *cat == Cat::Erase {
                ui.label(egui::RichText::new(tr("tdet.erase.note")).size(11.0).color(e.t.text_dim));
            }
        });
    });
    changed
}

fn tr_key_tail(k: &str) -> &str {
    k.rsplit('.').next().unwrap_or(k)
}

/// A janela das configurações avançadas (desenhada uma vez por quadro, pelo painel Propriedades da ferramenta).
pub(crate) fn window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut d) = env.ed.draw_ui.detail.take() else { return };
    let o = &env.ed.opts;
    let Some(base) = env.ed.brushes.get(o.brush_preset).cloned() else { return };
    let usable = *env.tool == crate::workspace::ToolId::Brush || (*env.tool == crate::workspace::ToolId::Eraser && o.eraser_use_brush);
    let key = shelf::preset_key(&base);
    let mut w = o.brush_custom.clone().unwrap_or_else(|| base.clone());
    w.size = o.brush_size;
    w.opacity = o.brush_opacity;
    let mut open = true;
    let mut reset = false;
    let mut save_new = false;
    let t = env.t;
    let mut changed = false;
    // prévia pelo motor do pincel em edição
    if d.preview.as_ref().is_none_or(|(p, _)| *p != w) {
        let mut q = env.ed.brushes.with_masks(&w);
        q.size = q.size.clamp(2.0, 40.0);
        d.preview = Some((w.clone(), super::tool_props::stroke_preview(ctx, "tool-detail-preview", &q, t.bg_widget, t.text, 560, 110)));
    }
    let mut prefs = std::mem::take(&mut env.ed.draw_ui.prefs);
    // tamanho só muda quando o usuário arrasta a borda (auditoria M-02): o corpo rola dentro de uma altura fixa e o
    // rodapé fica sempre visível; a janela nunca passa de 90% da tela
    let max_h = (ctx.content_rect().height() * 0.9).max(360.0);
    let win = egui::Window::new(tr("tdet.title")).id(egui::Id::new("tool_detail")).open(&mut open).default_size([640.0, 520.0]).max_height(max_h).resizable(true).constrain(true).collapsible(false).show(ctx, |ui| {
        if let Some((_, tex)) = &d.preview {
            let wd = ui.available_width().min(560.0);
            let r = ui.add(egui::Image::new((tex.id(), egui::vec2(wd, wd * 110.0 / 560.0))));
            ui.painter().text(r.rect.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, &base.name, egui::FontId::proportional(12.0), t.text);
            env.notes.push(("tdet:preview".into(), r.rect));
        }
        if !usable {
            ui.label(egui::RichText::new(tr("tdet.not_brush")).color(t.text_dim));
        }
        ui.separator();
        let body = (ui.available_height() - 44.0).max(120.0);
        egui::ScrollArea::vertical().id_salt("tdet_scroll").auto_shrink([false, false]).max_height(body).min_scrolled_height(body).show(ui, |ui| {
            let mut e = Edit { p: &mut w, o: Some(&mut env.ed.opts), t, icons: env.icons, notes: env.notes, compact: false, salt: "detail" };
            changed = categories_ui(ui, &mut e, &mut d.cat, Some((&key, &mut prefs)));
        });
        ui.separator();
        ui.horizontal(|ui| {
            let b = ui.add(egui::Button::image_and_text(egui::Image::new((env.icons.texture(ui.ctx(), IconId::ToolReset, 14.0), egui::vec2(14.0, 14.0))).tint(t.text), tr("tdet.reset")));
            env.notes.push(("tdet:reset".into(), b.rect));
            reset = b.on_hover_text(tr("tdet.reset.tip")).clicked();
            let b = ui.button(tr("tdet.save_new"));
            env.notes.push(("tdet:save_new".into(), b.rect));
            save_new = b.on_hover_text(tr("tdet.save_new.tip")).clicked();
        });
    });
    env.ed.draw_ui.prefs = prefs;
    if let Some(r) = win.as_ref() {
        env.notes.push(("dialog:tool_detail".into(), r.response.rect));
    }
    if changed {
        commit_edit(&mut env.ed.opts, &base, w);
    }
    if reset {
        let i = env.ed.opts.brush_preset;
        super::brushes::reset_preset(env, i);
    }
    if save_new {
        super::brush_maker::open_maker(env, None);
    }
    if open {
        env.ed.draw_ui.detail = Some(d);
    }
}

/// Grava a cópia editada: tamanho e opacidade vão para as opções; o resto vira a cópia do pincel (`brush_custom`), que
/// some quando volta a ser igual ao da biblioteca.
pub(crate) fn commit_edit(o: &mut ToolOptions, base: &BrushPreset, w: BrushPreset) {
    o.brush_size = w.size.clamp(0.5, 2000.0);
    o.brush_opacity = w.opacity.clamp(0.0, 1.0);
    let mut cmp = w.clone();
    cmp.size = base.size;
    cmp.opacity = base.opacity;
    if cmp == *base {
        o.brush_custom = None;
    } else if w.validate().is_ok() {
        o.brush_custom = Some(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_parameter_has_a_unique_key_a_category_and_texts() {
        use crate::i18n::{Lang, tr_in};
        let keys: HashSet<_> = PARAMS.iter().map(|p| p.key).collect();
        assert_eq!(keys.len(), PARAMS.len());
        for c in Cat::ALL {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, c.title_key()), c.title_key());
            }
            if !matches!(c, Cat::AntiAlias) {
                assert!(PARAMS.iter().any(|p| p.cat == c), "{c:?} vazia");
            }
        }
        for p in PARAMS {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, p.label), p.label, "{}", p.key);
            }
        }
        // o conjunto padrão do olho existe entre os parâmetros
        for k in crate::drawing_prefs::DEFAULT_SHOWN {
            assert!(param(k).is_some(), "{k}");
        }
    }

    #[test]
    fn tip_specific_parameters_only_apply_to_their_tips() {
        let mut p = BrushPreset::round("x", 10.0, 0.5);
        assert!(applies(&p, "hardness") && !applies(&p, "aspect") && !applies(&p, "texture_depth"));
        p.tip = BrushTip::Shape { shape: TipShape::Leaf, aspect: 1.0 };
        assert!(!applies(&p, "hardness") && applies(&p, "aspect") && applies(&p, "two_tone"));
        assert!(applies(&p, "size") && applies(&p, "aa"));
    }

    #[test]
    fn editing_back_to_the_library_values_drops_the_custom_copy() {
        let base = BrushPreset::round("x", 10.0, 0.5);
        let mut o = ToolOptions::default();
        let mut w = base.clone();
        w.size = 33.0;
        w.opacity = 0.5;
        commit_edit(&mut o, &base, w.clone());
        assert_eq!((o.brush_size, o.brush_opacity), (33.0, 0.5));
        assert!(o.brush_custom.is_none(), "tamanho e opacidade não fazem cópia");
        w.spacing = 0.4;
        commit_edit(&mut o, &base, w.clone());
        assert!(o.brush_custom.is_some());
        w.spacing = base.spacing;
        commit_edit(&mut o, &base, w);
        assert!(o.brush_custom.is_none());
    }
}

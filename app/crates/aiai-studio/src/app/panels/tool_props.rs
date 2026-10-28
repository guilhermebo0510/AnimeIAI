//! Painel Propriedades da ferramenta (S25, no espírito do Clip Studio): em cima a prévia do traço com o nome da
//! sub-ferramenta; embaixo linhas compactas (rótulo pequeno, barra, valor e o ícone de pressão que abre a curva) só dos
//! parâmetros com o olho aceso nas configurações avançadas (por sub-ferramenta; padrão: tamanho, opacidade, suavização e
//! estabilização); no rodapé só ícones: "Redefinir a ferramenta" e a chave inglesa das configurações avançadas.
//! Ferramentas que não são pincel (balde, seleção, texto, régua, mover...) mostram as opções delas aqui.

use crate::app::editor::{EraserTip, ToolOptions};
use crate::app::shelf::{self, Sub};
use crate::app::ui_kit::{Env, body_ui};
use crate::i18n::tr;
use crate::icons::IconId;
use crate::workspace::ToolId;
use aiai_paint::brush::BrushPreset;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui};

#[derive(Default)]
pub(crate) struct PropsUi {
    preview: Option<(BrushPreset, egui::TextureHandle)>,
}

/// Prévia do traço pelo motor: um S com a pressão subindo e descendo (`w`×`h` px, fundo e tinta do tema).
pub(crate) fn stroke_preview(ctx: &egui::Context, name: &str, p: &BrushPreset, bg: egui::Color32, ink: egui::Color32, w: u32, h: u32) -> egui::TextureHandle {
    use aiai_paint::brush::{BrushPoint, paint_stroke};
    let c = |c: egui::Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0];
    let mut img = aiai_paint::Image::filled(w, h, c(bg));
    let path: Vec<BrushPoint> = (0..120)
        .map(|k| {
            let t = k as f32 / 119.0;
            BrushPoint { x: 0.06 * w as f32 + t * 0.88 * w as f32, y: h as f32 / 2.0 + 0.26 * h as f32 * (t * std::f32::consts::TAU).sin(), pressure: 0.15 + 0.85 * (t * std::f32::consts::PI).sin(), tilt: None }
        })
        .collect();
    paint_stroke(&mut img, p, &path, c(ink), 11);
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img.to_rgba8());
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

/// O pincel em edição: a cópia ajustada (ou o da biblioteca) com o tamanho e a opacidade em uso.
fn working(env: &Env) -> Option<(BrushPreset, BrushPreset)> {
    let o = &env.ed.opts;
    let base = env.ed.brushes.get(o.brush_preset).cloned()?;
    let mut w = o.brush_custom.clone().unwrap_or_else(|| base.clone());
    w.size = o.brush_size;
    w.opacity = o.brush_opacity;
    Some((base, w))
}

/// A sub-ferramenta em uso pinta com um pincel da biblioteca (pincel, ou borracha com pincel).
fn uses_brush(env: &Env, sub: &Option<Sub>) -> bool {
    match sub {
        Some(Sub::Preset(_)) | Some(Sub::EraserPreset(_)) | Some(Sub::EraserBrush) => true,
        None => *env.tool == ToolId::Brush,
        _ => false,
    }
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let t = env.t;
    let sub = shelf::current_sub(env);
    let brushy = uses_brush(env, &sub);
    // ---- cabeçalho: prévia do traço com o nome
    let name = match (&sub, brushy) {
        (_, true) => env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| p.name.clone()).unwrap_or_default(),
        (Some(s), false) => shelf::sub_label(env, s),
        (None, false) => tr(env.tool.title_key()).to_owned(),
    };
    let custom = brushy && env.ed.opts.brush_custom.is_some();
    let hw = ui.available_width();
    let (hr, _) = ui.allocate_exact_size(egui::vec2(hw, 34.0), Sense::hover());
    env.note("tprop:header", hr);
    ui.painter().rect(hr, 3.0, t.bg_widget, Stroke::new(1.0, t.border), StrokeKind::Inside);
    if brushy {
        if let Some((_, w)) = working(env) {
            let mut q = env.ed.brushes.with_masks(&w);
            q.size = q.size.clamp(2.0, 18.0);
            if env.ed.draw_ui.props.preview.as_ref().is_none_or(|(p, _)| *p != q) {
                let tex = stroke_preview(ui.ctx(), "tool-props-preview", &q, t.bg_widget, t.text, 480, 80);
                env.ed.draw_ui.props.preview = Some((q, tex));
            }
            if let Some((_, tex)) = &env.ed.draw_ui.props.preview {
                let r = Rect::from_center_size(hr.center(), egui::vec2(hr.height() * 6.0, hr.height()).min(hr.size()));
                ui.painter().with_clip_rect(hr.shrink(1.0)).image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
        }
    } else {
        let icon = env.ed.draw_ui.shelf.slot.map(|s| s.icon()).unwrap_or(IconId::ToolBrush);
        env.icons.paint(ui.painter(), icon, Rect::from_center_size(egui::pos2(hr.right() - 22.0, hr.center().y), egui::vec2(22.0, 22.0)), t.text_dim);
    }
    // nome sobre a prévia, numa plaquinha (como o rótulo do traço nos programas de pintura)
    let label = if custom { format!("{name} *") } else { name };
    let g = ui.painter().layout_no_wrap(label, egui::FontId::proportional(11.5), t.text);
    let plate = Rect::from_min_size(hr.min + egui::vec2(3.0, 3.0), g.size() + egui::vec2(8.0, 2.0));
    ui.painter().rect_filled(plate, 2.0, t.bg_panel.gamma_multiply(0.85));
    ui.painter().galley(plate.min + egui::vec2(4.0, 1.0), g, t.text);
    ui.add_space(4.0);
    // ---- linhas
    let footer_h = 28.0;
    let max_h = (ui.available_height() - footer_h).max(40.0);
    egui::ScrollArea::vertical().id_salt("tool_props_scroll").max_height(max_h).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        match (*env.tool, &sub) {
            (ToolId::Brush, _) => brush_rows(env, ui, &sub),
            (ToolId::Eraser, s) if brushy => {
                eraser_brush_rows(env, ui);
                let _ = s;
                brush_rows(env, ui, &sub);
            }
            (ToolId::Eraser, _) => eraser_rows(env, ui, &sub),
            // Figura (formas e linhas) e Degradê: as opções moram nos módulos deles
            (ToolId::Pen, Some(Sub::PenShape(m))) if *m != crate::app::editor::PenShape::Freehand => crate::app::figure::prop_rows(env, ui, *m),
            (ToolId::Gradient, _) => crate::app::gradient_tool::prop_rows(env, ui),
            (ToolId::Pen, _) => pen_rows(env, ui),
            (ToolId::Points, Some(Sub::Fix(_))) => crate::app::line_fix::options(env, ui, false),
            _ => super::properties::tool_options_panel(env, ui),
        }
    });
    // ---- rodapé: redefinir e configurações avançadas
    ui.add_space(2.0);
    // a faixa do rodapé tem a largura do cabeçalho (a rolagem com barra não empurra os ícones para fora do painel)
    let (fr, _) = ui.allocate_exact_size(egui::vec2(hw, 26.0), Sense::hover());
    let mut foot = ui.new_child(egui::UiBuilder::new().max_rect(fr).layout(egui::Layout::right_to_left(egui::Align::Center)));
    foot.spacing_mut().item_spacing.x = 2.0;
    let detail = env.icon_button(&mut foot, IconId::Wrench, if brushy { "tprop.detail" } else { "tprop.detail.none" }, env.ed.draw_ui.detail.is_some(), brushy, "tprop:detail").clicked();
    let reset = env.icon_button(&mut foot, IconId::ToolReset, "tprop.reset", false, true, "tprop:reset").clicked();
    if reset {
        reset_tool(env, &sub);
    }
    if detail {
        if env.ed.draw_ui.detail.is_some() {
            env.ed.draw_ui.detail = None;
        } else {
            super::tool_detail::open(env);
        }
    }
    super::tool_detail::window(env, ui.ctx());
    super::brush_maker::windows(env, ui.ctx());
}

/// Linhas do pincel: os parâmetros com o olho aceso que valem para ele, na ordem das categorias.
fn brush_rows(env: &mut Env, ui: &mut Ui, sub: &Option<Sub>) {
    // não passar da linha (auditoria 4.5m): só o pincel; no alto, visível mesmo com o painel baixo
    if *env.tool == ToolId::Brush {
        let o = &mut env.ed.opts;
        let r = ui.checkbox(&mut o.brush_no_overflow, tr("opt.brush_no_overflow")).on_hover_text(tr("opt.brush_no_overflow.tip"));
        env.notes.push(("opt:brush_no_overflow".into(), r.rect));
    }
    let Some((base, mut w)) = working(env) else { return };
    let key = sub.as_ref().filter(|s| matches!(s, Sub::Preset(_) | Sub::EraserPreset(_))).map(|s| s.key()).unwrap_or_else(|| shelf::preset_key(&base));
    let shown = env.ed.draw_ui.prefs.shown_for(&key);
    let t = env.t;
    let mut changed = false;
    {
        let mut e = super::tool_detail::Edit { p: &mut w, o: Some(&mut env.ed.opts), t, icons: env.icons, notes: env.notes, compact: true, salt: "panel" };
        for prm in super::tool_detail::PARAMS.iter().filter(|p| shown.iter().any(|s| s == p.key)) {
            if !super::tool_detail::applies(e.p, prm.key) {
                continue;
            }
            changed |= super::tool_detail::param_ui(ui, &mut e, prm.key);
        }
    }
    if changed {
        super::tool_detail::commit_edit(&mut env.ed.opts, &base, w);
    }
}

/// Barra simples (borracha, caneta vetorial) no mesmo desenho das do pincel.
fn plain_bar(env: &mut Env, ui: &mut Ui, label: &str, v: &mut f32, lo: f32, hi: f32, log: bool, decimals: usize, suffix: &str) -> bool {
    let mut dummy = BrushPreset::round("", 1.0, 1.0);
    dummy.size = *v;
    let before = *v;
    let t = env.t;
    let mut e = super::tool_detail::Edit { p: &mut dummy, o: None, t, icons: env.icons, notes: env.notes, compact: true, salt: "plain" };
    super::tool_detail::plain_bar(ui, &mut e, label, v, lo, hi, log, decimals, suffix);
    (*v - before).abs() > 1e-6
}

fn eraser_brush_rows(env: &mut Env, ui: &mut Ui) {
    let mut v = env.ed.opts.eraser_size;
    if plain_bar(env, ui, "tprop.size", &mut v, 1.0, 600.0, true, 0, " px") {
        env.ed.opts.eraser_size = v;
    }
}

fn eraser_rows(env: &mut Env, ui: &mut Ui, sub: &Option<Sub>) {
    let vector = matches!(sub, Some(Sub::EraserVector(_)));
    let mut v = env.ed.opts.eraser_size;
    if plain_bar(env, ui, "tprop.size", &mut v, 1.0, 600.0, true, 0, " px") {
        env.ed.opts.eraser_size = v;
    }
    if vector {
        ui.label(egui::RichText::new(tr("opt.eraser_hint")).size(11.5).color(env.t.text_dim));
        return;
    }
    let hard_on = env.ed.opts.eraser_tip == EraserTip::Brush;
    let mut h = env.ed.opts.eraser_hardness * 100.0;
    ui.add_enabled_ui(hard_on, |ui| {
        if plain_bar(env, ui, "tprop.hardness", &mut h, 0.0, 100.0, false, 0, "%") {
            env.ed.opts.eraser_hardness = (h / 100.0).clamp(0.0, 1.0);
        }
    });
    let mut op = env.ed.opts.eraser_opacity * 100.0;
    if plain_bar(env, ui, "tprop.opacity", &mut op, 1.0, 100.0, false, 0, "%") {
        env.ed.opts.eraser_opacity = (op / 100.0).clamp(0.01, 1.0);
    }
    let mut fl = env.ed.opts.eraser_flow * 100.0;
    if plain_bar(env, ui, "tprop.flow", &mut fl, 1.0, 100.0, false, 0, "%") {
        env.ed.opts.eraser_flow = (fl / 100.0).clamp(0.01, 1.0);
    }
    let o = &mut env.ed.opts;
    ui.horizontal(|ui| {
        ui.checkbox(&mut o.eraser_pressure_size, tr("tprop.pressure_size"));
        ui.checkbox(&mut o.eraser_pressure_opacity, tr("tprop.pressure_opacity"));
    });
}

/// Caneta vetorial: largura, suavização (o vetor só tem com e sem), estabilização e fechar o traço.
fn pen_rows(env: &mut Env, ui: &mut Ui) {
    let mut v = env.ed.opts.pen_width;
    if plain_bar(env, ui, "tprop.size", &mut v, 0.5, 200.0, true, 1, " px") {
        env.ed.opts.pen_width = v;
    }
    // Figura: o contorno da forma (a ponta vetorial) escolhido aqui mesmo, antes de desenhar (auditoria UX-01)
    if env.ed.opts.pen_shape != crate::app::editor::PenShape::Freehand {
        let t = env.t;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("tprop.outline")).size(12.0).color(t.text));
            let o = &mut env.ed.opts;
            let r = egui::ComboBox::from_id_salt("tprop_outline_tip").width(150.0).selected_text(format!("{} · {}", tr(crate::app::shelf::tip_key(o.pen_tip)), tr("tprop.vector"))).show_ui(ui, |ui| {
                for tip in aiai_core::VectorTip::ALL {
                    ui.selectable_value(&mut o.pen_tip, tip, tr(crate::app::shelf::tip_key(tip)));
                }
            });
            env.notes.push(("tprop:outline_tip".into(), r.response.rect));
        });
    }
    let t = env.t;
    ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        let (r, _) = ui.allocate_exact_size(egui::vec2(92.0, 20.0), Sense::hover());
        ui.painter().with_clip_rect(r).text(egui::pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, tr("tprop.aa"), egui::FontId::proportional(12.0), t.text);
        let cur = if env.ed.opts.pen_aliased { 0 } else { 1 };
        for (lvl, icon, key) in [(0u8, IconId::AaNone, "tprop.aa.none"), (1, IconId::AaWeak, "tprop.aa.weak"), (2, IconId::AaMid, "tprop.aa.vector_only"), (3, IconId::AaStrong, "tprop.aa.vector_only")] {
            let enabled = lvl <= 1;
            let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
            env.note(format!("tprop:aa:{lvl}"), r);
            let on = cur == lvl;
            let p = ui.painter();
            p.rect(r, 3.0, if on { t.accent_dim } else if resp.hovered() && enabled { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
            let ink = if !enabled { t.text_dim.gamma_multiply(0.4) } else if on { t.text } else { t.text_dim };
            env.icons.paint(p, icon, r.shrink(3.0), ink);
            if resp.on_hover_text(tr(key)).clicked() && enabled {
                env.ed.opts.pen_aliased = lvl == 0;
            }
        }
    });
    let mut dummy = BrushPreset::round("", 1.0, 1.0);
    let mut e = super::tool_detail::Edit { p: &mut dummy, o: Some(&mut env.ed.opts), t, icons: env.icons, notes: env.notes, compact: true, salt: "pen" };
    super::tool_detail::param_ui(ui, &mut e, "stabilizer");
    let o = &mut env.ed.opts;
    let r = ui.checkbox(&mut o.pen_autoclose, tr("opt.autoclose")).on_hover_text(tr("opt.autoclose.tip"));
    env.notes.push(("tprop:autoclose".into(), r.rect));
    // auditoria 1 (4.8a, 4.9b, 4.8d, 2.7): as mesmas opções da aba Propriedades, aqui no painel da ferramenta
    let mut v = env.ed.opts.pen_simplify;
    if plain_bar(env, ui, "opt.pen_simplify", &mut v, 0.0, 100.0, false, 0, "") {
        env.ed.opts.pen_simplify = v;
    }
    // Início e fim: afinar por comprimento, sem depender de pressão (mouse)
    let mut v = env.ed.opts.pen_taper_in;
    if plain_bar(env, ui, "opt.pen_taper_in", &mut v, 0.0, 400.0, false, 0, " px") {
        env.ed.opts.pen_taper_in = v;
    }
    let mut v = env.ed.opts.pen_taper_out;
    if plain_bar(env, ui, "opt.pen_taper_out", &mut v, 0.0, 400.0, false, 0, " px") {
        env.ed.opts.pen_taper_out = v;
    }
    let o = &mut env.ed.opts;
    let r = ui.checkbox(&mut o.pen_weld, tr("opt.pen_weld")).on_hover_text(tr("opt.pen_weld.tip"));
    env.notes.push(("tprop:pen_weld".into(), r.rect));
    let r = ui.checkbox(&mut o.pen_quickshape, tr("opt.pen_quickshape")).on_hover_text(tr("opt.pen_quickshape.tip"));
    env.notes.push(("tprop:pen_quickshape".into(), r.rect));
    let r = ui.checkbox(&mut o.pen_color_trace, tr("opt.pen_color_trace")).on_hover_text(tr("opt.pen_color_trace.tip"));
    env.notes.push(("tprop:pen_color_trace".into(), r.rect));
}

/// "Redefinir a ferramenta": o pincel volta ao inicial; as outras ferramentas voltam às opções de fábrica delas.
fn reset_tool(env: &mut Env, sub: &Option<Sub>) {
    let d = ToolOptions::default();
    let o = &mut env.ed.opts;
    match *env.tool {
        ToolId::Brush => {
            let i = o.brush_preset;
            super::brushes::reset_preset(env, i);
            return;
        }
        ToolId::Pen => (o.pen_width, o.pen_aliased, o.pen_autoclose, o.stab_mode, o.stab_window, o.stab_radius) = (d.pen_width, d.pen_aliased, d.pen_autoclose, d.stab_mode, d.stab_window, d.stab_radius),
        ToolId::Eraser => {
            (o.eraser_size, o.eraser_opacity, o.eraser_flow, o.eraser_pressure_size, o.eraser_pressure_opacity) = (d.eraser_size, d.eraser_opacity, d.eraser_flow, d.eraser_pressure_size, d.eraser_pressure_opacity);
            if let Some(Sub::EraserRaster(i)) = sub {
                o.eraser_hardness = shelf::ERASERS[*i as usize % shelf::ERASERS.len()].1;
            }
        }
        ToolId::Fill => {
            (o.fill_gap, o.fill_expand, o.fill_close_leftovers) = (d.fill_gap, d.fill_expand, d.fill_close_leftovers);
            (o.area.fill_tolerance, o.area.fill_brush) = (d.area.fill_tolerance, d.area.fill_brush);
        }
        ToolId::Lasso => {
            (o.select_op, o.sel_feather, o.wand_tolerance, o.wand_contiguous, o.wand_antialias) = (d.select_op, d.sel_feather, d.wand_tolerance, d.wand_contiguous, d.wand_antialias);
            // a referência da varinha é a sub-ferramenta: fica
            (o.area.wand_gap, o.area.wand_scale, o.area.sel_brush, o.area.sel_magnet) = (d.area.wand_gap, d.area.wand_scale, d.area.sel_brush, d.area.sel_magnet);
        }
        ToolId::Move => (o.move_auto, o.keep_aspect) = (d.move_auto, d.keep_aspect),
        // Corrigir linha: as opções voltam às de fábrica, a sub-ferramenta fica
        ToolId::Points => o.fix = crate::app::line_fix::FixOptions { tool: o.fix.tool, ..d.fix },
        _ => {}
    }
}

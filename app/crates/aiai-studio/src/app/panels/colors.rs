//! Painel Cores (S25: embaixo à esquerda nas áreas de desenho, com abas em ícones). Em todas as abas ficam as amostras
//! da cor principal, da secundária (clique troca) e da transparente (o pincel raster apaga). As abas:
//! - roda de matiz com o quadrado de saturação × valor;
//! - conjunto de cores: as paletas do projeto (amostras ligadas à paleta: trocar a cor na paleta recolore tudo que a usa,
//!   edição de nome/cor por `ReplaceData`, um passo de desfazer por ajuste) e os degradês;
//! - controles RGB/HSV;
//! - histórico das últimas cores usadas para desenhar;
//! - cor aproximada (grade em volta da cor atual) e cor intermediária (grade entre quatro cores escolhidas).

use crate::app::commands::{self as cmds, NodeId};
use crate::app::editor::{Editor, Gesture};
use crate::app::ui_kit::{Env, body_ui, color32_to_rgba, dim_label, rgba_to_color32};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::*;
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui};

/// Aba do painel Cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ColorTab {
    #[default]
    Wheel,
    Set,
    Sliders,
    History,
    Approx,
    Mix,
}

impl ColorTab {
    pub const ALL: [ColorTab; 6] = [ColorTab::Wheel, ColorTab::Set, ColorTab::Sliders, ColorTab::History, ColorTab::Approx, ColorTab::Mix];

    pub fn key(self) -> &'static str {
        match self {
            ColorTab::Wheel => "wheel",
            ColorTab::Set => "set",
            ColorTab::Sliders => "sliders",
            ColorTab::History => "history",
            ColorTab::Approx => "approx",
            ColorTab::Mix => "mix",
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            ColorTab::Wheel => IconId::ColorWheel,
            ColorTab::Set => IconId::ColorSet,
            ColorTab::Sliders => IconId::ColorSliders,
            ColorTab::History => IconId::ColorHistory,
            ColorTab::Approx => IconId::ColorApprox,
            ColorTab::Mix => IconId::ColorMix,
        }
    }

    pub fn tip_key(self) -> &'static str {
        match self {
            ColorTab::Wheel => "colors.tab.wheel",
            ColorTab::Set => "colors.tab.set",
            ColorTab::Sliders => "colors.tab.sliders",
            ColorTab::History => "colors.tab.history",
            ColorTab::Approx => "colors.tab.approx",
            ColorTab::Mix => "colors.tab.mix",
        }
    }
}

/// Estado do painel: aba, cor secundária, transparente, cantos da cor intermediária e passo da aproximada.
pub(crate) struct ColorUi {
    pub tab: ColorTab,
    /// A cor que não está em uso (secundária quando a principal está em uso, e vice-versa).
    pub other: ColorRef,
    /// A secundária é a que está em uso (`ed.color` é sempre a cor de desenho).
    pub sub_active: bool,
    /// Cor transparente: o pincel raster apaga (a caneta vetorial e o balde seguem com a cor em uso).
    pub transparent: bool,
    /// Última cor vista (a transparente desliga quando a cor muda por outro caminho: bridge, conta-gotas).
    seen: Option<ColorRef>,
    pub corners: [[u8; 3]; 4],
    pub approx_step: f32,
    pub sliders_hsv: bool,
}

impl Default for ColorUi {
    fn default() -> Self {
        ColorUi {
            tab: ColorTab::Wheel,
            other: ColorRef::Direct { rgba: Rgba::WHITE },
            sub_active: false,
            transparent: false,
            seen: None,
            corners: [[230, 80, 70], [250, 220, 120], [70, 110, 200], [30, 30, 40]],
            approx_step: 0.12,
            sliders_hsv: false,
        }
    }
}

/// A cada quadro: a cor usada num traço entra no histórico; a transparente desliga se a cor mudou por fora.
pub(crate) fn tick(core: &aiai_core::CoreSession, ed: &mut Editor) {
    if ed.draw_ui.colors.seen.as_ref() != Some(&ed.color) {
        if ed.draw_ui.colors.seen.is_some() {
            ed.draw_ui.colors.transparent = false;
        }
        ed.draw_ui.colors.seen = Some(ed.color.clone());
    }
    if matches!(ed.gesture, Gesture::Stroke(_)) && !ed.draw_ui.colors.transparent {
        let c = rgba_to_color32(cmds::resolve_color(&core.project, &ed.color));
        ed.draw_ui.prefs.push_color([c.r(), c.g(), c.b()]);
    }
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    // ---- amostras (sempre à vista) e as abas em ícones
    ui.horizontal(|ui| {
        swatch_cluster(env, ui);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                for tab in ColorTab::ALL {
                    let on = env.ed.draw_ui.colors.tab == tab;
                    if env.icon_button(ui, tab.icon(), tab.tip_key(), on, true, &format!("colors:tab:{}", tab.key())).clicked() {
                        env.ed.draw_ui.colors.tab = tab;
                    }
                }
            });
            let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
            let label = match &env.ed.color {
                _ if env.ed.draw_ui.colors.transparent => tr("colors.transparent").to_owned(),
                ColorRef::Direct { .. } => rgba.to_hex()[..7].to_uppercase(),
                ColorRef::Palette { palette, color } => {
                    let name = cmds::palette_data(&env.core.project, palette).and_then(|p| p.color(color)).map(|c| c.name.clone()).unwrap_or_default();
                    format!("{name}  {}", &rgba.to_hex()[..7].to_uppercase())
                }
            };
            let r = ui.label(egui::RichText::new(label).monospace().size(11.5).color(env.t.text_dim));
            if matches!(env.ed.color, ColorRef::Palette { .. }) {
                r.on_hover_text(tr("palette.linked.tip"));
            }
        });
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("colors_scroll").auto_shrink([false, false]).show(ui, |ui| match env.ed.draw_ui.colors.tab {
        ColorTab::Wheel => wheel_tab(env, ui),
        ColorTab::Set => set_tab(env, ui),
        ColorTab::Sliders => sliders_tab(env, ui),
        ColorTab::History => history_tab(env, ui),
        ColorTab::Approx => approx_tab(env, ui),
        ColorTab::Mix => mix_tab(env, ui),
    });
}

/// Cor em uso como `Color32` opaca.
fn current32(env: &Env) -> Color32 {
    let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
    rgba_to_color32(Rgba([rgba.0[0], rgba.0[1], rgba.0[2], 1.0]))
}

/// Escolhe uma cor direta (desliga a transparente).
fn set_direct(env: &mut Env, c: Color32) {
    env.ed.set_color_direct(color32_to_rgba(c));
    env.ed.draw_ui.colors.transparent = false;
}

fn checker(p: &egui::Painter, r: Rect, t: &crate::theme::Tokens) {
    let n = 4;
    let s = r.width() / n as f32;
    p.rect_filled(r, 3.0, Color32::from_gray(235));
    for y in 0..n {
        for x in 0..n {
            if (x + y) % 2 == 1 {
                p.rect_filled(Rect::from_min_size(r.min + egui::vec2(x as f32 * s, y as f32 * s), egui::vec2(s, s)), 0.0, Color32::from_gray(170));
            }
        }
    }
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
}

/// Principal atrás à esquerda, secundária na frente à direita (clique troca), o ícone de trocar e a transparente.
fn swatch_cluster(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let (area, _) = ui.allocate_exact_size(egui::vec2(58.0, 52.0), Sense::hover());
    let cu = &env.ed.draw_ui.colors;
    let cur = current32(env);
    let other = rgba_to_color32(cmds::resolve_color(&env.core.project, &cu.other)).to_opaque();
    let (main_c, sub_c) = if cu.sub_active { (other, cur) } else { (cur, other) };
    let main_r = Rect::from_min_size(area.min, egui::vec2(30.0, 30.0));
    let sub_r = Rect::from_min_size(area.min + egui::vec2(16.0, 14.0), egui::vec2(26.0, 26.0));
    let tr_r = Rect::from_min_size(egui::pos2(area.left(), area.bottom() - 12.0), egui::vec2(12.0, 12.0));
    let swap_r = Rect::from_min_size(egui::pos2(area.right() - 14.0, area.top()), egui::vec2(14.0, 14.0));
    let transparent = cu.transparent;
    let sub_active = cu.sub_active;
    // a amostra em uso fica por cima e com o contorno do acento
    let order: [(bool, Rect, Color32); 2] = if sub_active { [(false, main_r, main_c), (true, sub_r, sub_c)] } else { [(true, sub_r, sub_c), (false, main_r, main_c)] };
    let mut clicked_inactive = false;
    for (is_sub, r, c) in order {
        let active = is_sub == sub_active && !transparent;
        let resp = ui.interact(r, ui.id().with(("swatch", is_sub)), Sense::click());
        env.note(if is_sub { "colors:sub" } else { "colors:current" }, r);
        let p = ui.painter();
        p.rect_filled(r, 4.0, c);
        p.rect_stroke(r, 4.0, Stroke::new(if active { 2.0 } else { 1.0 }, if active { t.accent } else { t.border }), StrokeKind::Inside);
        if is_sub == sub_active && env.ed.grad.active {
            let g = env.ed.grad.gradient.resolve(|c| cmds::resolve_color(&env.core.project, c).0);
            super::gradient_edit::paint_strip(&ui.painter_at(r.shrink(3.0)), r.shrink(3.0), &g);
        }
        let resp = resp.on_hover_text(tr(if is_sub { "colors.sub.tip" } else { "colors.main.tip" }));
        if resp.clicked() {
            if is_sub != sub_active {
                clicked_inactive = true;
            }
            env.ed.draw_ui.colors.transparent = false;
        }
    }
    // trocar
    let sr = ui.interact(swap_r, ui.id().with("swatch_swap"), Sense::click());
    env.note("colors:swap", swap_r);
    env.icons.paint(ui.painter(), IconId::Swap, swap_r, if sr.hovered() { t.text } else { t.text_dim });
    let swap = sr.on_hover_text(tr("colors.swap.tip")).clicked();
    if clicked_inactive || swap {
        let cu = &mut env.ed.draw_ui.colors;
        std::mem::swap(&mut env.ed.color, &mut cu.other);
        cu.sub_active = !cu.sub_active;
        cu.seen = Some(env.ed.color.clone());
    }
    // transparente
    let tresp = ui.interact(tr_r, ui.id().with("swatch_transparent"), Sense::click());
    env.note("colors:transparent", tr_r);
    checker(ui.painter(), tr_r, &t);
    if transparent {
        ui.painter().rect_stroke(tr_r.expand(1.0), 3.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
    }
    if tresp.on_hover_text(tr("colors.transparent.tip")).clicked() {
        env.ed.draw_ui.colors.transparent = !transparent;
    }
}

// ------------------------------------------------------------------------------------------------ roda

/// Anel de matiz com o quadrado de saturação × valor dentro. A matiz fica guardada enquanto a cor for a mesma (no
/// cinza/preto a matiz não existe e o quadrado "pulava").
fn wheel_tab(env: &mut Env, ui: &mut Ui) {
    let c32 = current32(env);
    let mut hsv = hsv_of(env, c32);
    let before = hsv;
    // a roda cabe na largura e na altura do painel (com a linha HSV embaixo)
    let side = ui.available_width().min(ui.available_height() - 20.0).clamp(100.0, 260.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
    env.note("colors:wheel", rect);
    let c = rect.center();
    let outer = side / 2.0 - 2.0;
    let ring = (side * 0.09).clamp(10.0, 18.0);
    let inner = outer - ring;
    let half = inner * std::f32::consts::FRAC_1_SQRT_2 - 3.0;
    let sq = Rect::from_center_size(c, egui::vec2(half * 2.0, half * 2.0));
    env.note("colors:sv", sq);
    // o que o arrasto pega é decidido no começo (anel ou quadrado)
    let id = ui.id().with("wheel_grab");
    if resp.drag_started() || resp.clicked() {
        let part = pick_pos(ui, &resp).map(|p| if sq.expand(2.0).contains(p) { 2u8 } else if (p - c).length() >= inner - 2.0 { 1 } else { 0 });
        ui.data_mut(|d| d.insert_temp(id, part.unwrap_or(0)));
    }
    let part: u8 = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    if let Some(p) = pick_pos(ui, &resp) {
        match part {
            1 => {
                let a = (p - c).angle();
                hsv[0] = (a / std::f32::consts::TAU).rem_euclid(1.0);
            }
            2 => {
                hsv[1] = ((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0);
                hsv[2] = 1.0 - ((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0);
            }
            _ => {}
        }
    }
    let painter = ui.painter_at(rect);
    // anel
    let n = 72;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let f = i as f32 / n as f32;
        let a = f * std::f32::consts::TAU;
        let col = hsv_to32([f, 1.0, 1.0]);
        let d = egui::vec2(a.cos(), a.sin());
        mesh.colored_vertex(c + d * inner, col);
        mesh.colored_vertex(c + d * outer, col);
    }
    for i in 0..n as u32 {
        mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
        mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    let a = hsv[0] * std::f32::consts::TAU;
    marker(&painter, rect, c + egui::vec2(a.cos(), a.sin()) * (inner + ring / 2.0));
    // quadrado
    sv_mesh(&painter, sq, hsv[0]);
    painter.rect_stroke(sq, 0.0, Stroke::new(1.0, env.t.border), StrokeKind::Outside);
    marker(&painter, sq, egui::pos2(sq.left() + sq.width() * hsv[1], sq.top() + sq.height() * (1.0 - hsv[2])));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    ui.label(egui::RichText::new(format!("H {:.0}  S {:.0}  V {:.0}", hsv[0] * 360.0, hsv[1] * 100.0, hsv[2] * 100.0)).monospace().size(11.0).color(env.t.text_dim));
    apply_hsv(env, c32, before, hsv);
}

fn hsv_to32(h: [f32; 3]) -> Color32 {
    let c = egui::ecolor::Hsva::new(h[0], h[1], h[2], 1.0).to_srgba_unmultiplied();
    Color32::from_rgb(c[0], c[1], c[2])
}

fn hsv_of(env: &Env, c32: Color32) -> [f32; 3] {
    match env.ed.wheel_hsv {
        Some((c, h)) if c == c32 => h,
        _ => {
            let h = egui::ecolor::Hsva::from(c32);
            [h.h, h.s, h.v]
        }
    }
}

/// Grava a cor se o HSV mudou (e guarda a matiz junto da cor).
fn apply_hsv(env: &mut Env, c32: Color32, before: [f32; 3], hsv: [f32; 3]) {
    if hsv != before {
        let n = hsv_to32(hsv);
        set_direct(env, n);
        env.ed.wheel_hsv = Some((n, hsv));
    } else if env.ed.wheel_hsv.is_none_or(|(c, _)| c != c32) {
        env.ed.wheel_hsv = Some((c32, hsv));
    }
}

fn sv_mesh(painter: &egui::Painter, rect: Rect, hue: f32) {
    let n = 16;
    let mut mesh = egui::Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (s, v) = (i as f32 / n as f32, 1.0 - j as f32 / n as f32);
            mesh.colored_vertex(egui::pos2(rect.left() + rect.width() * s, rect.top() + rect.height() * (1.0 - v)), hsv_to32([hue, s, v]));
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let b = a + (n + 1) as u32;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

// ------------------------------------------------------------------------------------------------ conjunto de cores

fn set_tab(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    palette_picker(env, ui);
    match env.ed.palette.clone() {
        None => {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tr("panel.palette.empty")).color(t.text_dim));
        }
        Some(pal) => {
            swatches(env, ui, &pal);
            edit_selected(env, ui, &pal);
        }
    }
    // a cor em uso vira amostra da paleta (fica ligada a ela)
    let has_pal = env.ed.palette.is_some();
    let add = ui.add_enabled(has_pal, egui::Button::new(tr("palette.add_to")));
    env.note("colors:add_to_palette", add.rect);
    if add.on_disabled_hover_text(tr("palette.need_palette")).clicked() {
        if let Some(pal) = env.ed.palette.clone() {
            let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
            match cmds::add_palette_color(env.core, &pal, rgba) {
                Ok(id) => env.ed.color = ColorRef::Palette { palette: pal, color: id },
                Err(e) => env.ed.notify(e),
            }
        }
    }
    ui.add_space(4.0);
    super::gradient_edit::colors_section(env, ui);
}

// ------------------------------------------------------------------------------------------------ controles

fn sliders_tab(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let c32 = current32(env);
    let mut hsv = hsv_of(env, c32);
    let before = hsv;
    ui.horizontal(|ui| {
        let hsv_mode = &mut env.ed.draw_ui.colors.sliders_hsv;
        let r = ui.selectable_label(!*hsv_mode, "RGB");
        if r.clicked() {
            *hsv_mode = false;
        }
        env.notes.push(("colors:sliders:rgb".into(), r.rect));
        let r = ui.selectable_label(*hsv_mode, "HSV");
        if r.clicked() {
            *hsv_mode = true;
        }
        env.notes.push(("colors:sliders:hsv".into(), r.rect));
    });
    let w = ui.available_width().clamp(100.0, 300.0);
    if env.ed.draw_ui.colors.sliders_hsv {
        for (k, label) in ["H", "S", "V"].iter().enumerate() {
            let mut v = hsv[k];
            let stops: Vec<Color32> = (0..=12).map(|i| {
                let mut h = hsv;
                h[k] = i as f32 / 12.0;
                if k == 0 {
                    h[1] = h[1].max(0.6);
                    h[2] = h[2].max(0.6);
                }
                hsv_to32(h)
            }).collect();
            if channel(env, ui, label, &mut v, &stops, w, if k == 0 { 360.0 } else { 100.0 }) {
                hsv[k] = v;
            }
        }
        apply_hsv(env, c32, before, hsv);
    } else {
        let mut rgb = [c32.r() as f32 / 255.0, c32.g() as f32 / 255.0, c32.b() as f32 / 255.0];
        let mut changed = false;
        for (k, label) in ["R", "G", "B"].iter().enumerate() {
            let stops: Vec<Color32> = (0..=1).map(|i| {
                let mut c = rgb;
                c[k] = i as f32;
                Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
            }).collect();
            let mut v = rgb[k];
            if channel(env, ui, label, &mut v, &stops, w, 255.0) {
                rgb[k] = v;
                changed = true;
            }
        }
        if changed {
            set_direct(env, Color32::from_rgb((rgb[0] * 255.0).round() as u8, (rgb[1] * 255.0).round() as u8, (rgb[2] * 255.0).round() as u8));
        }
    }
    // hex
    ui.horizontal(|ui| {
        dim_label(ui, &t, "Hex");
        let c = current32(env);
        let mut hex = format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(80.0).font(egui::TextStyle::Monospace));
        env.note("colors:hex", r.rect);
        if r.changed() {
            if let Some(c) = Rgba::from_hex(hex.trim()) {
                let c = rgba_to_color32(c);
                set_direct(env, c);
            }
        }
    });
}

/// Um canal com a faixa em degradê, o marcador e o número (0..`scale`). Devolve se mudou.
fn channel(env: &mut Env, ui: &mut Ui, label: &str, v: &mut f32, stops: &[Color32], w: f32, scale: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).monospace().size(11.5));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2((w - 70.0).max(60.0), 14.0), Sense::click_and_drag());
        env.note(format!("colors:ch:{}", label.to_lowercase()), rect);
        if let Some(p) = pick_pos(ui, &resp) {
            *v = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            changed = true;
        }
        let mut mesh = egui::Mesh::default();
        let n = stops.len().max(2) - 1;
        for (i, c) in stops.iter().enumerate() {
            let x = rect.left() + rect.width() * i as f32 / n as f32;
            mesh.colored_vertex(egui::pos2(x, rect.top()), *c);
            mesh.colored_vertex(egui::pos2(x, rect.bottom()), *c);
        }
        for i in 0..n as u32 {
            mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
            mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
        }
        let p = ui.painter_at(rect.expand(3.0));
        p.add(egui::Shape::mesh(mesh));
        p.rect_stroke(rect, 2.0, Stroke::new(1.0, env.t.border), StrokeKind::Inside);
        let x = rect.left() + rect.width() * *v;
        let m = Rect::from_min_max(egui::pos2(x - 2.5, rect.top() - 1.0), egui::pos2(x + 2.5, rect.bottom() + 1.0));
        p.rect_stroke(m, 1.0, Stroke::new(2.5, Color32::from_black_alpha(170)), StrokeKind::Outside);
        p.rect_stroke(m, 1.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        let mut n = (*v * scale).round();
        let r = ui.add(egui::DragValue::new(&mut n).range(0.0..=scale).speed(1.0));
        if r.changed() {
            *v = (n / scale).clamp(0.0, 1.0);
            changed = true;
        }
    });
    changed
}

// ------------------------------------------------------------------------------------------------ histórico

fn history_tab(env: &mut Env, ui: &mut Ui) {
    let hist = env.ed.draw_ui.prefs.history_rgb();
    if hist.is_empty() {
        ui.label(egui::RichText::new(tr("colors.history.empty")).size(12.0).color(env.t.text_dim));
        return;
    }
    let pick = grid_of(env, ui, "history", &hist.iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])).collect::<Vec<_>>(), 20.0, None);
    if let Some(c) = pick {
        set_direct(env, c);
    }
}

/// Grade de amostras clicáveis (`cols` automáticas pela largura). Devolve a clicada.
fn grid_of(env: &mut Env, ui: &mut Ui, name: &str, cells: &[Color32], size: f32, cols: Option<usize>) -> Option<Color32> {
    let t = env.t;
    let cur = current32(env);
    let w = ui.available_width();
    let cols = cols.unwrap_or(((w + 2.0) / (size + 2.0)).floor().max(1.0) as usize);
    let cell = cols.max(1) as f32;
    let s = ((w + 2.0) / cell - 2.0).min(size * 1.6);
    let mut out = None;
    for (row, chunk) in cells.chunks(cols).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (k, c) in chunk.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(s, s.min(size)), Sense::click());
                env.note(format!("colors:{name}:{}", row * cols + k), r);
                let p = ui.painter();
                p.rect_filled(r, 2.0, *c);
                let on = *c == cur;
                p.rect_stroke(r, 2.0, Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.border }), StrokeKind::Inside);
                if resp.on_hover_text(format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())).clicked() {
                    out = Some(*c);
                }
            }
        });
        ui.add_space(-2.0);
    }
    out
}

// ------------------------------------------------------------------------------------------------ aproximada e intermediária

/// Grade de cores perto da atual: na horizontal a saturação, na vertical o valor (passo escolhido).
pub(crate) fn approx_cells(hsv: [f32; 3], step: f32, n: usize) -> Vec<[f32; 3]> {
    let mid = (n / 2) as f32;
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let s = (hsv[1] + (i as f32 - mid) * step).clamp(0.0, 1.0);
            let v = (hsv[2] - (j as f32 - mid) * step).clamp(0.0, 1.0);
            out.push([hsv[0], s, v]);
        }
    }
    out
}

/// Grade entre quatro cores (cantos: superior esquerdo, superior direito, inferior esquerdo, inferior direito).
pub(crate) fn mix_cells(corners: [[u8; 3]; 4], cols: usize, rows: usize) -> Vec<[u8; 3]> {
    let lerp = |a: [u8; 3], b: [u8; 3], f: f32| -> [f32; 3] { std::array::from_fn(|k| a[k] as f32 + (b[k] as f32 - a[k] as f32) * f) };
    let mut out = Vec::with_capacity(cols * rows);
    for j in 0..rows {
        let fy = if rows > 1 { j as f32 / (rows - 1) as f32 } else { 0.0 };
        for i in 0..cols {
            let fx = if cols > 1 { i as f32 / (cols - 1) as f32 } else { 0.0 };
            let top = lerp(corners[0], corners[1], fx);
            let bot = lerp(corners[2], corners[3], fx);
            out.push(std::array::from_fn(|k| (top[k] + (bot[k] - top[k]) * fy).round().clamp(0.0, 255.0) as u8));
        }
    }
    out
}

fn approx_tab(env: &mut Env, ui: &mut Ui) {
    let c32 = current32(env);
    let hsv = hsv_of(env, c32);
    ui.horizontal(|ui| {
        dim_label(ui, &env.t, tr("colors.approx.step"));
        let mut st = env.ed.draw_ui.colors.approx_step * 100.0;
        if ui.add(egui::Slider::new(&mut st, 3.0..=25.0).suffix("%").max_decimals(0)).changed() {
            env.ed.draw_ui.colors.approx_step = st / 100.0;
        }
    });
    let n = 9;
    let cells: Vec<Color32> = approx_cells(hsv, env.ed.draw_ui.colors.approx_step, n).into_iter().map(hsv_to32).collect();
    if let Some(c) = grid_of(env, ui, "approx", &cells, 22.0, Some(n)) {
        set_direct(env, c);
    }
}

fn mix_tab(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let corners = env.ed.draw_ui.colors.corners;
    // os quatro cantos: clique põe a cor em uso no canto
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("colors.mix.corners"));
        for (k, c) in corners.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            env.note(format!("colors:corner:{k}"), r);
            ui.painter().rect(r, 3.0, Color32::from_rgb(c[0], c[1], c[2]), Stroke::new(1.0, t.border), StrokeKind::Inside);
            if resp.on_hover_text(tr("colors.mix.corner.tip")).clicked() {
                let cur = current32(env);
                env.ed.draw_ui.colors.corners[k] = [cur.r(), cur.g(), cur.b()];
            }
        }
    });
    let (cols, rows) = (9, 7);
    let cells: Vec<Color32> = mix_cells(env.ed.draw_ui.colors.corners, cols, rows).into_iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])).collect();
    if let Some(c) = grid_of(env, ui, "mix", &cells, 22.0, Some(cols)) {
        set_direct(env, c);
    }
}

/// Onde o ponteiro pegou o controle: arrastando ou num clique rápido (pressionar e soltar entre dois quadros).
fn pick_pos(ui: &Ui, resp: &egui::Response) -> Option<egui::Pos2> {
    resp.interact_pointer_pos().or_else(|| if resp.clicked() || resp.is_pointer_button_down_on() { ui.ctx().input(|i| i.pointer.interact_pos()) } else { None })
}

/// Marcador pequeno em duas cores (aparece em fundo claro e escuro), sempre dentro da área.
fn marker(painter: &egui::Painter, area: Rect, p: egui::Pos2) {
    let p = p.clamp(area.min, area.max);
    painter.with_clip_rect(area.expand(1.0)).circle_stroke(p, 5.0, Stroke::new(3.0, Color32::from_black_alpha(170)));
    painter.with_clip_rect(area.expand(1.0)).circle_stroke(p, 5.0, Stroke::new(1.2, Color32::WHITE));
}


fn palette_picker(env: &mut Env, ui: &mut Ui) {
    let pals: Vec<(NodeId, String)> = env.core.project.roots.palettes.iter().filter_map(|id| env.core.project.node(id).map(|n| (id.clone(), n.name.clone()))).collect();
    if env.ed.palette.as_ref().is_none_or(|p| !pals.iter().any(|(id, _)| id == p)) {
        env.ed.palette = pals.first().map(|(id, _)| id.clone());
    }
    ui.horizontal(|ui| {
        let cur = env.ed.palette.as_ref().and_then(|p| pals.iter().find(|(id, _)| id == p)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("palette.none").to_owned());
        let combo = egui::ComboBox::from_id_salt("palette_pick").width(150.0).selected_text(cur).show_ui(ui, |ui| {
            for (id, name) in &pals {
                if ui.selectable_label(env.ed.palette.as_ref() == Some(id), name).clicked() {
                    env.ed.palette = Some(id.clone());
                }
            }
        });
        env.note("colors:palette_combo", combo.response.rect);
        let b = ui.button(tr("palette.new"));
        env.note("colors:new_palette", b.rect);
        if b.clicked() {
            match cmds::add_palette(env.core) {
                Ok(id) => env.ed.palette = Some(id),
                Err(e) => env.ed.notify(e),
            }
        }
    });
    // guardar a paleta (por exemplo a de settei de um personagem) na biblioteca para outros projetos
    if let Some(pal) = env.ed.palette.clone().filter(|p| cmds::palette_data(&env.core.project, p).is_some_and(|d| !d.colors.is_empty())) {
        let b = ui.small_button(tr("lib.save_palette"));
        env.note("colors:save_to_library", b.rect);
        if b.on_hover_text(tr("lib.save_palette.tip")).clicked() {
            crate::app::library::save_from_project(env, "palette", &pal);
        }
    }
}

fn swatches(env: &mut Env, ui: &mut Ui, pal: &NodeId) {
    let t = env.t;
    let Some(pd) = cmds::palette_data(&env.core.project, pal).cloned() else { return };
    ui.add_space(4.0);
    let size = 26.0;
    let mut rename_target: Option<NodeId> = None;
    let mut delete_target: Option<NodeId> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, c) in pd.colors.iter().enumerate() {
            let rgba = pd.resolve(&c.id).unwrap_or(c.rgba);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
            env.note(format!("colors:swatch:{i}"), rect);
            let selected = matches!(&env.ed.color, ColorRef::Palette { palette, color } if palette == pal && *color == c.id);
            let p = ui.painter();
            p.rect_filled(rect, 4.0, rgba_to_color32(Rgba([rgba.0[0], rgba.0[1], rgba.0[2], 1.0])));
            p.rect_stroke(rect, 4.0, Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { t.accent } else { t.border }), StrokeKind::Inside);
            let resp = resp.on_hover_text(format!("{}  {}", c.name, rgba.to_hex()[..7].to_uppercase()));
            if resp.clicked() {
                env.ed.color = ColorRef::Palette { palette: pal.clone(), color: c.id.clone() };
                env.ed.color_edit = None;
            }
            resp.context_menu(|ui| {
                if ui.button(tr("palette.rename")).clicked() {
                    rename_target = Some(c.id.clone());
                    ui.close();
                }
                if ui.button(tr("palette.delete_color")).clicked() {
                    delete_target = Some(c.id.clone());
                    ui.close();
                }
            });
        }
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
        env.note("colors:add_swatch", rect);
        let p = ui.painter();
        p.rect(rect, 4.0, if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(16.0), t.text_dim);
        if resp.on_hover_text(tr("palette.add_color")).clicked() {
            let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
            match cmds::add_palette_color(env.core, pal, rgba) {
                Ok(id) => env.ed.color = ColorRef::Palette { palette: pal.clone(), color: id },
                Err(e) => env.ed.notify(e),
            }
        }
    });
    if let Some(id) = rename_target {
        env.ed.color = ColorRef::Palette { palette: pal.clone(), color: id };
    }
    if let Some(id) = delete_target {
        let mut data = pd.clone();
        data.colors.retain(|c| c.id != id);
        match cmds::replace_palette(env.core, pal, data) {
            Ok(()) => {
                if matches!(&env.ed.color, ColorRef::Palette { color, .. } if *color == id) {
                    env.ed.color = ColorRef::Direct { rgba: Rgba::BLACK };
                }
            }
            Err(e) => env.ed.notify(e),
        }
    }
}

/// Nome e cor da amostra atualmente escolhida. A edição fica numa cópia local até o ponteiro soltar; aí vira um `ReplaceData`.
fn edit_selected(env: &mut Env, ui: &mut Ui, pal: &NodeId) {
    let t = env.t;
    let ColorRef::Palette { palette, color } = env.ed.color.clone() else { return };
    if &palette != pal {
        return;
    }
    let Some(pd) = cmds::palette_data(&env.core.project, pal).cloned() else { return };
    let Some(nc) = pd.color(&color).cloned() else { return };
    ui.add_space(2.0);
    let key = (pal.clone(), color.clone());
    let (mut name, mut c32) = match &env.ed.color_edit {
        Some((p, c, n, col)) if (p, c) == (&key.0, &key.1) => (n.clone(), *col),
        _ => (nc.name.clone(), rgba_to_color32(Rgba([nc.rgba.0[0], nc.rgba.0[1], nc.rgba.0[2], 1.0]))),
    };
    let mut touched = false;
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("palette.color_name"));
        let te = ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0));
        env.note("colors:name", te.rect);
        touched |= te.changed();
        let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque);
        env.note("colors:edit_picker", r.rect);
        touched |= r.changed();
        ui.label(egui::RichText::new(color32_to_rgba(c32).to_hex()[..7].to_uppercase()).monospace().size(12.0).color(t.text_dim));
    });
    if touched {
        env.ed.color_edit = Some((key.0.clone(), key.1.clone(), name.clone(), c32));
    }
    // aplica quando nada está sendo arrastrado nem digitado
    let busy = ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input();
    if !busy {
        if let Some((p, c, n, col)) = env.ed.color_edit.take() {
            if (&p, &c) == (&key.0, &key.1) {
                let rgba = color32_to_rgba(col);
                let changed = n.trim() != nc.name || (0..3).any(|i| (rgba.0[i] - nc.rgba.0[i]).abs() > 0.002);
                if changed {
                    let mut data = pd.clone();
                    if let Some(entry) = data.colors.iter_mut().find(|e| e.id == c) {
                        if !n.trim().is_empty() {
                            entry.name = n.trim().to_owned();
                        }
                        entry.rgba = Rgba([rgba.0[0], rgba.0[1], rgba.0[2], nc.rgba.0[3]]);
                    }
                    if let Err(e) = cmds::replace_palette(env.core, pal, data) {
                        env.ed.notify(e);
                    }
                }
            }
        }
    }
    let _: Option<Color32> = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_grid_is_centered_on_the_current_color() {
        let hsv = [0.3, 0.5, 0.5];
        let cells = approx_cells(hsv, 0.1, 9);
        assert_eq!(cells.len(), 81);
        assert_eq!(cells[4 * 9 + 4], hsv, "o centro é a cor atual");
        assert!(cells[4 * 9 + 8][1] > hsv[1] && cells[0][2] > hsv[2], "direita mais saturada, em cima mais clara");
        assert!(cells.iter().all(|c| c[0] == 0.3 && (0.0..=1.0).contains(&c[1]) && (0.0..=1.0).contains(&c[2])));
    }

    #[test]
    fn intermediate_grid_blends_the_four_corners() {
        let k = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        let g = mix_cells(k, 5, 3);
        assert_eq!(g.len(), 15);
        assert_eq!((g[0], g[4], g[10], g[14]), (k[0], k[1], k[2], k[3]), "os cantos são as cores escolhidas");
        assert_eq!(g[2], [128, 128, 0], "meio da borda de cima");
    }
}

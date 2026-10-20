//! Editor de degradê (gestão de cor): a barra com as paradas (clicar no vazio cria, arrastar move, arrastar para fora
//! da barra ou o botão tira), a cor da parada pelo seletor de sempre (ou "usar a cor atual", que pode ser uma cor de
//! paleta), opacidade e posição da parada, forma, ângulo, escala, repetição, espaço de mistura e os degradês prontos.
//!
//! O mesmo widget serve o painel Cores (o degradê atual, que pode virar a cor do pincel e do balde) e o painel Texto
//! (preenchimento, contornos, brilhos, sobreposição). Ele edita uma cópia; quem chama decide quando gravar (ver
//! `buffered`: um passo de desfazer por gesto).

use crate::app::commands as cmds;
use crate::app::ui_kit::{Env, color32_to_rgba, dim_label, rgba_to_color32};
use crate::i18n::tr;
use aiai_core::*;
use egui::{Color32, Id, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

/// Degradê atual do painel Cores e se ele é a "cor" do pincel e do balde.
#[derive(Clone)]
pub(crate) struct GradState {
    pub gradient: Gradient,
    /// Pincel (ao longo do traço) e balde (na caixa da região) pintam com o degradê no lugar da cor.
    pub active: bool,
    /// Pincel: px do traço para ir de uma ponta à outra do degradê.
    pub brush_len: f32,
}

impl Default for GradState {
    fn default() -> Self {
        GradState { gradient: aiai_core::paint::builtin_gradients()[1].1.clone(), active: false, brush_len: 400.0 }
    }
}

/// Edita `current` numa cópia guardada na memória do egui enquanto o ponteiro estiver apertado (ou um campo com o
/// teclado); devolve o valor novo só quando o gesto termina.
pub(crate) fn buffered<T: Clone + PartialEq + Send + Sync + 'static>(ui: &mut Ui, id: Id, current: &T, f: impl FnOnce(&mut Ui, &mut T)) -> Option<T> {
    let key = id.with("grad_buf");
    let mut v: T = ui.data(|d| d.get_temp::<T>(key)).unwrap_or_else(|| current.clone());
    f(ui, &mut v);
    let busy = ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input();
    if v == *current {
        ui.data_mut(|d| d.remove::<T>(key));
        return None;
    }
    if busy {
        ui.data_mut(|d| d.insert_temp(key, v));
        None
    } else {
        ui.data_mut(|d| d.remove::<T>(key));
        Some(v)
    }
}

fn resolve(env: &Env, g: &Gradient) -> ResolvedGradient {
    let p = &env.core.project;
    g.resolve(|c| cmds::resolve_color(p, c).0)
}

/// Fundo xadrez (para ver a transparência).
fn checker(painter: &egui::Painter, rect: Rect) {
    let s = 6.0;
    painter.rect_filled(rect, 2.0, Color32::from_gray(200));
    let (nx, ny) = ((rect.width() / s).ceil() as i32, (rect.height() / s).ceil() as i32);
    for j in 0..ny {
        for i in 0..nx {
            if (i + j) % 2 == 0 {
                let r = Rect::from_min_size(rect.min + Vec2::new(i as f32 * s, j as f32 * s), Vec2::splat(s)).intersect(rect);
                painter.rect_filled(r, 0.0, Color32::from_gray(150));
            }
        }
    }
}

/// Pinta a faixa do degradê (eixo 0..1 da esquerda para a direita) em `rect`.
pub(crate) fn paint_strip(painter: &egui::Painter, rect: Rect, g: &ResolvedGradient) {
    checker(painter, rect);
    let n = 96;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let c = g.color_at(t);
        let c32 = rgba_to_color32(Rgba(c));
        let x = rect.left() + rect.width() * t;
        mesh.colored_vertex(egui::pos2(x, rect.top()), c32);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), c32);
    }
    for i in 0..n as u32 {
        mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
        mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Amostra pequena do degradê (lista de degradês da paleta, botões).
pub(crate) fn swatch(env: &Env, ui: &mut Ui, g: &Gradient, size: Vec2, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    paint_strip(&painter, rect, &resolve(env, g));
    painter.rect_stroke(rect, 3.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { env.t.accent } else { env.t.border }), StrokeKind::Inside);
    resp
}

fn shape_key(s: GradientShape) -> &'static str {
    match s {
        GradientShape::Linear => "grad.shape.linear",
        GradientShape::Radial => "grad.shape.radial",
        GradientShape::Angular => "grad.shape.angular",
        GradientShape::Diamond => "grad.shape.diamond",
    }
}

fn extend_key(e: GradientExtend) -> &'static str {
    match e {
        GradientExtend::Pad => "grad.extend.pad",
        GradientExtend::Repeat => "grad.extend.repeat",
        GradientExtend::Reflect => "grad.extend.reflect",
    }
}

fn space_key(s: GradientSpace) -> &'static str {
    match s {
        GradientSpace::Srgb => "grad.space.srgb",
        GradientSpace::Linear => "grad.space.linear",
        GradientSpace::Oklab => "grad.space.oklab",
    }
}

/// O editor completo. `key` separa os editores da tela (paradas selecionadas, arrasto). Devolve se `g` mudou.
pub(crate) fn editor(env: &mut Env, ui: &mut Ui, key: &str, g: &mut Gradient) -> bool {
    let t = env.t;
    let before = g.clone();
    let id = Id::new(("grad_edit", key));
    let sel_id = id.with("sel");
    let drag_id = id.with("drag");
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    if g.stops.is_empty() {
        *g = Gradient::default();
    }
    sel = sel.min(g.stops.len() - 1);
    // ---- barra com as paradas
    let w = ui.available_width().clamp(120.0, 340.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click_and_drag());
    env.note(format!("grad:{key}:bar"), rect);
    let bar = Rect::from_min_max(rect.min + Vec2::new(6.0, 0.0), egui::pos2(rect.right() - 6.0, rect.top() + 20.0));
    let marks_y = bar.bottom() + 7.0;
    let to_t = |x: f32| ((x - bar.left()) / bar.width()).clamp(0.0, 1.0);
    let stop_x = |p: f32| bar.left() + bar.width() * p;
    let res = resolve(env, g);
    // começo do gesto: pegou uma parada ou o vazio?
    if resp.drag_started() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let near = g.stops.iter().enumerate().map(|(i, s)| (i, (stop_x(s.pos) - pos.x).abs())).filter(|(_, d)| *d < 7.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
            match near {
                Some(i) => {
                    sel = i;
                    ui.data_mut(|d| d.insert_temp(drag_id, i));
                }
                None if resp.clicked() || pos.y <= bar.bottom() + 12.0 => {
                    // parada nova com a cor que o degradê já tem ali
                    let tt = to_t(pos.x);
                    let c = res.color_at(tt);
                    g.stops.push(GradientStop { pos: tt, color: ColorRef::Direct { rgba: Rgba([c[0], c[1], c[2], 1.0]) }, opacity: c[3] });
                    sel = g.stops.len() - 1;
                    ui.data_mut(|d| d.insert_temp(drag_id, sel));
                }
                None => {}
            }
        }
    }
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    if let (Some(i), true) = (dragging, resp.dragged()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            if i < g.stops.len() {
                g.stops[i].pos = to_t(pos.x);
                // arrastar para longe da barra tira a parada (sobra no mínimo duas)
                let away = pos.y > rect.bottom() + 28.0 || pos.y < rect.top() - 28.0;
                if away && g.stops.len() > 2 {
                    g.stops.remove(i);
                    sel = 0;
                    ui.data_mut(|d| d.remove_temp::<usize>(drag_id));
                }
            }
        }
    }
    if resp.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| d.remove_temp::<usize>(drag_id));
    }
    // ordena mantendo a parada escolhida
    if g.stops.windows(2).any(|w| w[0].pos > w[1].pos) {
        let chosen = g.stops[sel.min(g.stops.len() - 1)].clone();
        let mut order: Vec<(usize, GradientStop)> = g.stops.drain(..).enumerate().collect();
        order.sort_by(|a, b| a.1.pos.total_cmp(&b.1.pos));
        let drag_now: Option<usize> = ui.data(|d| d.get_temp(drag_id));
        let mut new_sel = 0;
        let mut new_drag = None;
        for (ni, (oi, s)) in order.into_iter().enumerate() {
            if s == chosen {
                new_sel = ni;
            }
            if drag_now == Some(oi) {
                new_drag = Some(ni);
            }
            g.stops.push(s);
        }
        sel = new_sel;
        if let Some(d) = new_drag {
            ui.data_mut(|m| m.insert_temp(drag_id, d));
        }
    }
    let painter = ui.painter_at(rect.expand(2.0));
    paint_strip(&painter, bar, &resolve(env, g));
    painter.rect_stroke(bar, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    for (i, s) in g.stops.iter().enumerate() {
        let x = stop_x(s.pos);
        let c = cmds::resolve_color(&env.core.project, &s.color);
        let tri = vec![egui::pos2(x, bar.bottom() + 1.0), egui::pos2(x - 5.0, marks_y + 6.0), egui::pos2(x + 5.0, marks_y + 6.0)];
        let on = i == sel;
        painter.add(egui::Shape::convex_polygon(tri, rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], 1.0])), Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { Color32::from_black_alpha(200) })));
        if matches!(s.color, ColorRef::Palette { .. }) {
            painter.circle_filled(egui::pos2(x, marks_y + 9.5), 1.8, t.accent);
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tr("grad.bar.tip"));
    // ---- parada escolhida
    let n = g.stops.len();
    sel = sel.min(n - 1);
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("grad.stop"));
        let s = &mut g.stops[sel];
        let cur = cmds::resolve_color(&env.core.project, &s.color);
        let mut c32 = rgba_to_color32(Rgba([cur.0[0], cur.0[1], cur.0[2], 1.0]));
        let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque);
        env.note(format!("grad:{key}:stop_color"), r.rect);
        if r.changed() {
            let c = color32_to_rgba(c32);
            s.color = ColorRef::Direct { rgba: Rgba([c.0[0], c.0[1], c.0[2], 1.0]) };
        }
        let b = ui.small_button(tr("grad.use_current")).on_hover_text(tr("grad.use_current.tip"));
        env.note(format!("grad:{key}:use_current"), b.rect);
        if b.clicked() {
            s.color = env.ed.color.clone();
        }
        let mut pos = s.pos * 100.0;
        if ui.add(egui::DragValue::new(&mut pos).range(0.0..=100.0).speed(0.5).suffix(" %").max_decimals(1)).on_hover_text(tr("grad.pos")).changed() {
            s.pos = pos / 100.0;
        }
        let mut op = s.opacity * 100.0;
        if ui.add(egui::DragValue::new(&mut op).range(0.0..=100.0).speed(0.5).prefix("α ").suffix(" %").max_decimals(0)).on_hover_text(tr("grad.opacity")).changed() {
            s.opacity = op / 100.0;
        }
        let del = ui.add_enabled(n > 2, egui::Button::new("×").small()).on_hover_text(tr("grad.remove_stop"));
        env.note(format!("grad:{key}:remove_stop"), del.rect);
        if del.clicked() {
            g.stops.remove(sel);
            sel = sel.saturating_sub(1);
        }
    });
    // ---- forma e mapeamento
    ui.horizontal_wrapped(|ui| {
        let cb = egui::ComboBox::from_id_salt(id.with("shape")).width(84.0).selected_text(tr(shape_key(g.shape))).show_ui(ui, |ui| {
            for s in [GradientShape::Linear, GradientShape::Radial, GradientShape::Angular, GradientShape::Diamond] {
                ui.selectable_value(&mut g.shape, s, tr(shape_key(s)));
            }
        });
        env.note(format!("grad:{key}:shape"), cb.response.rect);
        ui.add(egui::DragValue::new(&mut g.angle).range(-360.0..=360.0).speed(1.0).suffix("°").max_decimals(0)).on_hover_text(tr("grad.angle"));
        let mut sc = g.scale * 100.0;
        if ui.add(egui::DragValue::new(&mut sc).range(1.0..=1000.0).speed(1.0).prefix("× ").suffix(" %").max_decimals(0)).on_hover_text(tr("grad.scale")).changed() {
            g.scale = sc / 100.0;
        }
        egui::ComboBox::from_id_salt(id.with("extend")).width(78.0).selected_text(tr(extend_key(g.extend))).show_ui(ui, |ui| {
            for e in [GradientExtend::Pad, GradientExtend::Repeat, GradientExtend::Reflect] {
                ui.selectable_value(&mut g.extend, e, tr(extend_key(e)));
            }
        });
        egui::ComboBox::from_id_salt(id.with("space")).width(78.0).selected_text(tr(space_key(g.space))).show_ui(ui, |ui| {
            for s in [GradientSpace::Srgb, GradientSpace::Linear, GradientSpace::Oklab] {
                ui.selectable_value(&mut g.space, s, tr(space_key(s)));
            }
        })
        .response
        .on_hover_text(tr("grad.space.tip"));
    });
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("grad.center"));
        let mut cx = g.center[0] * 100.0;
        let mut cy = g.center[1] * 100.0;
        if ui.add(egui::DragValue::new(&mut cx).range(-200.0..=300.0).speed(0.5).prefix("x ").suffix(" %").max_decimals(0)).changed() {
            g.center[0] = cx / 100.0;
        }
        if ui.add(egui::DragValue::new(&mut cy).range(-200.0..=300.0).speed(0.5).prefix("y ").suffix(" %").max_decimals(0)).changed() {
            g.center[1] = cy / 100.0;
        }
        if ui.small_button("⇄").on_hover_text(tr("grad.reverse")).clicked() {
            *g = g.reversed();
            sel = g.stops.len() - 1 - sel.min(g.stops.len() - 1);
        }
        let presets = aiai_core::paint::builtin_gradients();
        let cb = egui::ComboBox::from_id_salt(id.with("presets")).width(110.0).selected_text(tr("grad.presets")).show_ui(ui, |ui| {
            for (name, pg) in &presets {
                ui.horizontal(|ui| {
                    swatch(env, ui, pg, Vec2::new(48.0, 14.0), false);
                    if ui.selectable_label(false, *name).clicked() {
                        *g = pg.clone();
                        sel = 0;
                    }
                });
            }
        });
        env.note(format!("grad:{key}:presets"), cb.response.rect);
    });
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
    *g != before
}

/// Degradês da paleta: amostras (clicar = vira o degradê atual), "+" guarda o atual, botão direito apaga.
pub(crate) fn palette_gradients(env: &mut Env, ui: &mut Ui, pal: &cmds::NodeId) {
    let Some(pd) = cmds::palette_data(&env.core.project, pal).cloned() else { return };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        let mut remove: Option<aiai_core::Id> = None;
        for (i, ng) in pd.gradients.iter().enumerate() {
            let selected = env.ed.grad.gradient == ng.gradient;
            let r = swatch(env, ui, &ng.gradient, Vec2::new(54.0, 18.0), selected);
            env.note(format!("grad:pal:{i}"), r.rect);
            let r = r.on_hover_text(&ng.name);
            if r.clicked() {
                env.ed.grad.gradient = ng.gradient.clone();
            }
            r.context_menu(|ui| {
                if ui.button(tr("grad.delete")).clicked() {
                    remove = Some(ng.id.clone());
                    ui.close();
                }
            });
        }
        if let Some(rid) = remove {
            let mut data = pd.clone();
            data.gradients.retain(|x| x.id != rid);
            if let Err(e) = cmds::replace_palette(env.core, pal, data) {
                env.ed.notify(e);
            }
            return;
        }
        let b = ui.add(egui::Button::new("+").min_size(Vec2::new(24.0, 18.0))).on_hover_text(tr("grad.save_to_palette"));
        env.note("grad:save_to_palette", b.rect);
        if b.clicked() {
            let mut data = pd.clone();
            let id = env.core.ids.next();
            data.gradients.push(NamedGradient { id, name: format!("{} {}", tr("grad.name"), data.gradients.len() + 1), gradient: env.ed.grad.gradient.clone() });
            if let Err(e) = cmds::replace_palette(env.core, pal, data) {
                env.ed.notify(e);
            }
        }
    });
}

/// Seção do painel Cores: usar o degradê como cor, o editor, o comprimento no pincel e os degradês da paleta.
pub(crate) fn colors_section(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let r = egui::CollapsingHeader::new(tr("grad.section")).id_salt("colors_gradient").default_open(true).show(ui, |ui| {
        let mut on = env.ed.grad.active;
        let cb = ui.checkbox(&mut on, tr("grad.use_as_color")).on_hover_text(tr("grad.use_as_color.tip"));
        env.note("grad:use", cb.rect);
        if cb.changed() {
            env.ed.grad.active = on;
        }
        let mut g = env.ed.grad.gradient.clone();
        if editor(env, ui, "colors", &mut g) {
            env.ed.grad.gradient = g;
        }
        ui.horizontal_wrapped(|ui| {
            dim_label(ui, &t, tr("grad.brush_len"));
            ui.add(egui::DragValue::new(&mut env.ed.grad.brush_len).range(10.0..=10000.0).speed(2.0).suffix(" px").max_decimals(0)).on_hover_text(tr("grad.brush_len.tip"));
        });
        if let Some(pal) = env.ed.palette.clone() {
            dim_label(ui, &t, tr("grad.palette"));
            palette_gradients(env, ui, &pal);
        }
    });
    env.note("grad:section", r.header_response.rect);
}

/// O degradê atual pronto para o pincel (cores de paleta resolvidas).
pub(crate) fn brush_ramp(env: &Env) -> Option<aiai_paint::brush::ColorRamp> {
    if !env.ed.grad.active {
        return None;
    }
    let p = &env.core.project;
    Some(aiai_paint::brush::ColorRamp { gradient: env.ed.grad.gradient.with_direct_colors(|c| cmds::resolve_color(p, c).0), length: env.ed.grad.brush_len.max(1.0) })
}

/// O degradê atual para o balde (as cores de paleta ficam ligadas à paleta).
pub(crate) fn fill_gradient(env: &Env) -> Option<Gradient> {
    env.ed.grad.active.then(|| env.ed.grad.gradient.clone())
}

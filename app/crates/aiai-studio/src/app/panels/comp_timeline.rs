//! Painel Linha do tempo da composição (S24 3.2/3.3), como a Timeline do After Effects e diferente da linha do tempo de
//! exposição da Animação: uma linha por camada com as colunas cor de rótulo, olho, solo (só de visão), cadeado, nome,
//! pai (a pasta), modo de mistura e matte (alfa da camada abaixo ou máscara), e à direita a barra de duração (os quadros
//! com desenho exposto). A setinha (ou A P S R T E U) abre a camada: Transformar (Âncora, Posição, Escala, Rotação,
//! Opacidade) e Efeitos (cada parâmetro), cada propriedade com cronômetro, losango do quadro atual e valor; à direita,
//! as chaves como losangos (degrau = quadrado, com easing = círculo) que se selecionam (clique, Shift, caixa), arrastam,
//! copiam/colam (Ctrl+C/V), apagam (Delete) e mudam de interpolação (botões, clique direito, F9 = Easy Ease).
//!
//! Editor de gráfico (botão ou Shift+F3): a curva de valor da propriedade selecionada, chaves arrastáveis (tempo e
//! valor) e as alças da Bézier de cada trecho. Arrastar um efeito do painel Efeitos e predefinições para uma linha de
//! camada aplica o efeito nela.

use crate::app::comp::{self, CompState, GraphDrag, KeyDrag, KeySel, Owner, PropRef, Reveal};
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, paint_icon};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::keys::{Ease, KeyInterp};
use aiai_core::{LayerData, LayerType};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, UiBuilder};

const ROW_H: f32 = 20.0;
const HEAD_H: f32 = 22.0;
const BAR_H: f32 = 30.0;

/// Paleta das cores de rótulo (índice 1..=8; 0 = sem rótulo).
pub(crate) const LABELS: [Color32; 8] = [
    Color32::from_rgb(0xd9, 0x4a, 0x4a),
    Color32::from_rgb(0xe3, 0xc5, 0x3c),
    Color32::from_rgb(0x4f, 0xc3, 0xc9),
    Color32::from_rgb(0xe4, 0x7f, 0xb8),
    Color32::from_rgb(0xa7, 0x95, 0xdb),
    Color32::from_rgb(0xe8, 0xa8, 0x6c),
    Color32::from_rgb(0x7f, 0xc8, 0x8e),
    Color32::from_rgb(0x56, 0x80, 0xd8),
];

/// Cor das trilhas no editor de gráfico (x, y, z/azul, alfa).
const TRACK_COLORS: [Color32; 4] = [Color32::from_rgb(0xe0, 0x5a, 0x5a), Color32::from_rgb(0x5a, 0xc8, 0x6a), Color32::from_rgb(0x5a, 0x8a, 0xe8), Color32::from_rgb(0xd8, 0xd8, 0xd8)];

/// Uma linha da lista.
#[derive(Clone)]
enum Row {
    Layer { id: NodeId, depth: usize, l: Box<LayerData>, name: String },
    Group { layer: NodeId, depth: usize, title: &'static str },
    Effect { layer: NodeId, depth: usize, index: usize, l: Box<LayerData> },
    Prop { layer: NodeId, depth: usize, p: PropRef, l: Box<LayerData> },
}

/// Linhas visíveis: camadas e, nas abertas, as propriedades conforme o `Reveal`.
fn build_rows(env: &mut Env, cut: &NodeId) -> Vec<Row> {
    let p = &env.core.project;
    let st = &env.ed.comp;
    let mut rows = Vec::new();
    for r in cmds::layer_rows(p, cut) {
        let Some(l) = cmds::layer_data(p, &r.id) else { continue };
        let name = p.node(&r.id).map(|n| n.name.clone()).unwrap_or_default();
        rows.push(Row::Layer { id: r.id.clone(), depth: r.depth, l: Box::new(l.clone()), name });
        if !st.expanded.contains(&r.id) {
            continue;
        }
        let d = r.depth + 1;
        let tf: Vec<PropRef> = comp::TRANSFORM.iter().map(|n| PropRef::layer(&r.id, n)).filter(|pr| comp::shows(&st.reveal, l, pr)).collect();
        let grouped = !matches!(st.reveal, Reveal::Only(_));
        if !tf.is_empty() {
            if grouped {
                rows.push(Row::Group { layer: r.id.clone(), depth: d, title: "comp.group.transform" });
            }
            for pr in tf {
                rows.push(Row::Prop { layer: r.id.clone(), depth: if grouped { d + 1 } else { d }, p: pr, l: Box::new(l.clone()) });
            }
        }
        let fx = comp::stack(l);
        let show_fx = matches!(st.reveal, Reveal::All | Reveal::Effects | Reveal::Animated);
        if show_fx && !fx.is_empty() {
            let mut block = Vec::new();
            for (i, e) in fx.iter().enumerate() {
                let params: Vec<PropRef> = comp::effect_rows(&e.effect).iter().map(|n| PropRef::effect(&r.id, i, n)).filter(|pr| comp::shows(&st.reveal, l, pr)).collect();
                if matches!(st.reveal, Reveal::Animated) && params.is_empty() {
                    continue;
                }
                block.push(Row::Effect { layer: r.id.clone(), depth: d + 1, index: i, l: Box::new(l.clone()) });
                let open = st.fx_open.contains(&(r.id.clone(), i)) || matches!(st.reveal, Reveal::Animated);
                if open {
                    for pr in params {
                        block.push(Row::Prop { layer: r.id.clone(), depth: d + 2, p: pr, l: Box::new(l.clone()) });
                    }
                }
            }
            if !block.is_empty() {
                rows.push(Row::Group { layer: r.id.clone(), depth: d, title: "comp.group.effects" });
                rows.extend(block);
            }
        }
    }
    rows
}

/// Filho recortado num retângulo (célula da tabela).
fn cell(ui: &mut Ui, rect: Rect) -> Ui {
    let mut c = ui.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    c.set_clip_rect(rect.intersect(ui.clip_rect()));
    c
}

/// Forma do losango pela interpolação: linear = losango, degrau = quadrado, com easing = círculo.
fn paint_key(p: &egui::Painter, c: Pos2, ease: Ease, selected: bool, t: &crate::theme::Tokens) {
    let fill = if selected { t.accent } else { t.text_dim.gamma_multiply(0.85) };
    let stroke = Stroke::new(1.0, if selected { t.text } else { Color32::from_black_alpha(160) });
    match ease {
        Ease::Hold => {
            p.rect(Rect::from_center_size(c, egui::vec2(8.0, 8.0)), 1.0, fill, stroke, egui::StrokeKind::Middle);
        }
        Ease::Linear => {
            let r = 5.0;
            p.add(egui::Shape::convex_polygon(vec![c + egui::vec2(0.0, -r), c + egui::vec2(r, 0.0), c + egui::vec2(0.0, r), c + egui::vec2(-r, 0.0)], fill, stroke));
        }
        _ => {
            p.circle(c, 4.5, fill, stroke);
        }
    }
}

/// Intervalo das marcas da régua (em quadros) para ficarem a ≥ 44 px.
fn tick_step(px_per_frame: f32, fps: f32) -> u32 {
    let fps = fps.round().max(1.0) as u32;
    for s in [1, 2, 5, 10, fps / 2, fps, fps * 2, fps * 5, fps * 10, fps * 30, fps * 60] {
        if s > 0 && s as f32 * px_per_frame >= 44.0 {
            return s;
        }
    }
    fps * 120
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 4.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.label(egui::RichText::new(tr("panel.comp_timeline.empty")).color(t.text_dim));
        return;
    };
    let Some(cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
    let dur = cd.duration.max(1);
    let fps = env.core.project.settings.fps() as f32;
    let _ = env.ed.active_layer(env.core);
    if ui.ctx().input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F3)) && ui.ctx().memory(|m| m.focused().is_none()) {
        env.ed.comp.graph = !env.ed.comp.graph;
    }

    // ---- barra
    let full = ui.max_rect();
    let bar = Rect::from_min_size(full.min, egui::vec2(full.width(), BAR_H));
    toolbar(env, &mut cell(ui, bar), dur);
    let area = Rect::from_min_max(egui::pos2(full.left(), bar.bottom() + 2.0), full.max);
    if area.height() < HEAD_H + ROW_H || area.width() < 200.0 {
        return;
    }
    let left_w = (area.width() * 0.5).clamp(230.0, 470.0);
    let left = Rect::from_min_max(area.min, egui::pos2(area.left() + left_w, area.bottom()));
    let track = Rect::from_min_max(egui::pos2(left.right() + 1.0, area.top()), area.max);
    if env.ed.comp.px_per_frame <= 0.0 {
        env.ed.comp.px_per_frame = ((track.width() - 20.0) / dur as f32).clamp(0.5, 80.0);
    }
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 3.0, t.bg_widget.gamma_multiply(0.55));
    painter.vline(left.right() + 0.5, area.y_range(), Stroke::new(1.0, t.border));
    let cols = Columns::new(left, &t);
    // ---- cabeçalho: títulos das colunas e régua
    let head_l = Rect::from_min_size(left.min, egui::vec2(left.width(), HEAD_H));
    let head_r = Rect::from_min_max(track.min, egui::pos2(track.right(), track.top() + HEAD_H));
    cols.header(&painter, head_l, &t);
    ruler(env, ui, &painter, head_r, dur, fps);
    let list = Rect::from_min_max(egui::pos2(area.left(), area.top() + HEAD_H), area.max);
    let list_track = Rect::from_min_max(egui::pos2(track.left(), list.top()), track.max);
    let rows = build_rows(env, &cut);
    let bars = comp::duration_bars(env.core, &cut, env.ed);
    // ---- rolagem (roda = vertical; Shift = horizontal; Ctrl = zoom de tempo)
    let total_h = rows.len() as f32 * ROW_H + 8.0;
    if ui.rect_contains_pointer(list) {
        // eventos crus da roda (como o canvas), cada um com os modificadores dele: o delta suavizado pode ter sido
        // consumido por uma ScrollArea e o estado global de modificadores fica velho depois de um Alt de foco
        let wheel: Vec<(egui::Vec2, egui::Modifiers)> = ui.ctx().input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, modifiers, .. } => Some((
                        match unit {
                            egui::MouseWheelUnit::Line => *delta * 40.0,
                            egui::MouseWheelUnit::Page => *delta * 200.0,
                            egui::MouseWheelUnit::Point => *delta,
                        },
                        *modifiers,
                    )),
                    _ => None,
                })
                .collect()
        });
        for (d, mods) in wheel {
            if mods.command {
                if d.y != 0.0 {
                    let at = ui.ctx().pointer_hover_pos().map(|p| comp::x_frame(list_track, &env.ed.comp, p.x)).unwrap_or(0.0);
                    let st = &mut env.ed.comp;
                    let old = st.px_per_frame;
                    st.px_per_frame = (old * (1.0 + d.y * 0.004)).clamp(0.5, 80.0);
                    st.scroll_x = (st.scroll_x + at * (st.px_per_frame - old)).max(0.0);
                }
            } else if mods.shift {
                env.ed.comp.scroll_x = (env.ed.comp.scroll_x - d.y - d.x).max(0.0);
            } else {
                env.ed.comp.scroll_y = (env.ed.comp.scroll_y - d.y).clamp(0.0, (total_h - list.height()).max(0.0));
                env.ed.comp.scroll_x = (env.ed.comp.scroll_x - d.x).max(0.0);
            }
        }
    }
    let max_x = (dur as f32 * env.ed.comp.px_per_frame - list_track.width() + 40.0).max(0.0);
    env.ed.comp.scroll_x = env.ed.comp.scroll_x.min(max_x);
    env.ed.comp.scroll_y = env.ed.comp.scroll_y.min((total_h - list.height()).max(0.0));
    let sy = env.ed.comp.scroll_y;
    let lp = ui.painter().with_clip_rect(list);
    let mut hits: Vec<(Rect, KeySel)> = Vec::new();
    let graph = env.ed.comp.graph;
    for (i, row) in rows.iter().enumerate() {
        let y = list.top() + i as f32 * ROW_H - sy;
        // linha cortada em cima não é desenhada (os campos dela passariam por cima do cabeçalho)
        if y < list.top() - 0.5 || y > list.bottom() {
            continue;
        }
        let rl = Rect::from_min_size(egui::pos2(left.left(), y), egui::vec2(left.width(), ROW_H));
        let rt = Rect::from_min_size(egui::pos2(list_track.left(), y), egui::vec2(list_track.width(), ROW_H));
        draw_row(env, ui, &lp, &cols, row, rl, rt, &bars, graph, &mut hits, i);
    }
    // ---- editor de gráfico (no lugar das trilhas)
    if graph {
        graph_editor(env, ui, list_track, dur);
    } else {
        track_input(env, ui, list_track, &hits);
    }
    // cabeçote por cima de tudo
    let px = comp::frame_x(track, &env.ed.comp, env.ed.frame as f32 + 0.5);
    if px >= track.left() && px <= track.right() {
        painter.vline(px, track.y_range(), Stroke::new(1.0, t.accent));
    }
    env.note("comp_tl:list", list);
    env.note("comp_tl:track", list_track);
}

/// Posições das colunas da esquerda (as de modo/matte/pai somem quando falta largura).
struct Columns {
    twirl: f32,
    label: f32,
    eye: f32,
    solo: f32,
    lock: f32,
    name: f32,
    name_end: f32,
    parent: Option<f32>,
    blend: Option<f32>,
    matte: Option<f32>,
    right: f32,
    /// Onde começam os valores numa linha de propriedade.
    values: f32,
}

impl Columns {
    fn new(left: Rect, _t: &crate::theme::Tokens) -> Columns {
        let x = left.left() + 2.0;
        let (twirl, label, eye, solo, lock) = (x, x + 16.0, x + 30.0, x + 48.0, x + 66.0);
        let name = x + 86.0;
        let right = left.right() - 2.0;
        let room = right - name;
        let (mut parent, mut blend, mut matte) = (None, None, None);
        let mut end = right;
        if room > 330.0 {
            matte = Some(end - 82.0);
            end -= 84.0;
        }
        if room > 250.0 {
            blend = Some(end - 82.0);
            end -= 84.0;
        }
        if room > 410.0 {
            parent = Some(end - 66.0);
            end -= 68.0;
        }
        let values = (left.left() + left.width() * 0.52).max(name + 110.0).min(right - 90.0);
        Columns { twirl, label, eye, solo, lock, name, name_end: end - 2.0, parent, blend, matte, right, values }
    }

    fn header(&self, p: &egui::Painter, r: Rect, t: &crate::theme::Tokens) {
        p.rect_filled(r, 0.0, t.bg_header);
        let f = FontId::proportional(11.0);
        let y = r.center().y;
        let txt = |x: f32, k: &str| {
            p.text(egui::pos2(x, y), Align2::LEFT_CENTER, tr(k), f.clone(), t.text_dim);
        };
        txt(self.name, "comp.col.name");
        if let Some(x) = self.parent {
            txt(x, "comp.col.parent");
        }
        if let Some(x) = self.blend {
            txt(x, "comp.col.mode");
        }
        if let Some(x) = self.matte {
            txt(x, "comp.col.matte");
        }
        p.text(egui::pos2(self.eye + 2.0, y), Align2::LEFT_CENTER, "👁", FontId::proportional(10.0), t.text_dim);
        p.text(egui::pos2(self.solo + 3.0, y), Align2::LEFT_CENTER, "S", f.clone(), t.text_dim);
        p.text(egui::pos2(self.lock + 3.0, y), Align2::LEFT_CENTER, "🔒", FontId::proportional(10.0), t.text_dim);
        let _ = (self.twirl, self.label, self.right);
    }
}

fn toolbar(env: &mut Env, ui: &mut Ui, dur: u32) {
    let t = env.t;
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
    let playing = env.ed.playback.playing;
    if env.icon_button_sized(ui, IconId::StepBack, "timeline.step_back", false, true, "comp_tl:step_back", 22.0, false).clicked() {
        env.ed.playback.playing = false;
        env.ed.frame = env.ed.frame.saturating_sub(1);
    }
    if env.icon_button_sized(ui, if playing { IconId::Pause } else { IconId::Play }, if playing { "timeline.pause" } else { "timeline.play" }, playing, true, "comp_tl:play", 22.0, false).clicked() {
        super::timeline::toggle_play(env.ed, dur);
    }
    if env.icon_button_sized(ui, IconId::StepForward, "timeline.step_forward", false, true, "comp_tl:step_forward", 22.0, false).clicked() {
        env.ed.playback.playing = false;
        env.ed.frame = (env.ed.frame + 1).min(dur - 1);
    }
    let mut f1 = env.ed.frame + 1;
    let r = ui.add(egui::DragValue::new(&mut f1).range(1..=dur).speed(0.25).prefix(format!("{} ", tr("timeline.frame"))));
    env.note("comp_tl:frame", r.rect);
    if r.changed() {
        env.ed.playback.playing = false;
        env.ed.frame = f1 - 1;
    }
    ui.label(egui::RichText::new(format!("/ {dur}")).color(t.text_dim).size(12.0));
    ui.separator();
    // chave anterior / seguinte (J / K)
    let prev = ui.small_button("◀◆").on_hover_text(tr("comp.key_prev"));
    env.note("comp_tl:key_prev", prev.rect);
    if prev.clicked() {
        if let Some(f) = comp::neighbor_key(&env.core.project, env.ed, false) {
            env.ed.frame = f;
        }
    }
    let next = ui.small_button("◆▶").on_hover_text(tr("comp.key_next"));
    env.note("comp_tl:key_next", next.rect);
    if next.clicked() {
        if let Some(f) = comp::neighbor_key(&env.core.project, env.ed, true) {
            env.ed.frame = f;
        }
    }
    ui.separator();
    let has = !env.ed.comp.sel_keys.is_empty();
    for (k, i, note) in [("comp.interp.linear", KeyInterp::Linear, "linear"), ("comp.interp.hold", KeyInterp::Hold, "hold"), ("comp.interp.easy_ease", KeyInterp::EasyEase, "easy_ease")] {
        let b = ui.add_enabled(has, egui::Button::new(egui::RichText::new(tr(k)).size(12.0))).on_hover_text(tr(&format!("{k}.tip")));
        env.note(format!("comp_tl:{note}"), b.rect);
        if b.clicked() {
            comp::interp_selected(env.core, env.ed, i);
        }
    }
    ui.separator();
    let g = env.ed.comp.graph;
    let b = ui.selectable_label(g, egui::RichText::new(tr("comp.graph")).size(12.0)).on_hover_text(tr("comp.graph.tip"));
    env.note("comp_tl:graph", b.rect);
    if b.clicked() {
        env.ed.comp.graph = !g;
    }
    let fit = ui.small_button("⇔").on_hover_text(tr("comp.fit_time"));
    env.note("comp_tl:fit", fit.rect);
    if fit.clicked() {
        // ajustar ao corte: `show` recalcula pela largura da trilha
        env.ed.comp.scroll_x = 0.0;
        env.ed.comp.px_per_frame = 0.0;
    }
    if !env.ed.comp.solo.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new(tr("comp.solo_on")).size(11.5).color(t.accent)).on_hover_text(tr("comp.solo.tip"));
    }
}

fn ruler(env: &mut Env, ui: &mut Ui, p: &egui::Painter, r: Rect, dur: u32, fps: f32) {
    let t = env.t;
    p.rect_filled(r, 0.0, t.bg_header);
    let st = &env.ed.comp;
    let step = tick_step(st.px_per_frame, fps);
    let f0 = comp::x_frame(r, st, r.left()).floor().max(0.0) as u32;
    let f1 = (comp::x_frame(r, st, r.right()).ceil() as u32).min(dur);
    let mut f = f0 / step * step;
    while f <= f1 {
        let x = comp::frame_x(r, st, f as f32);
        if x >= r.left() - 1.0 {
            p.vline(x, egui::Rangef::new(r.bottom() - 7.0, r.bottom()), Stroke::new(1.0, t.text_dim));
            p.text(egui::pos2(x + 2.0, r.top() + 2.0), Align2::LEFT_TOP, format!("{}", f + 1), FontId::proportional(10.0), t.text_dim);
        }
        f += step;
    }
    // fim do corte
    let xe = comp::frame_x(r, st, dur as f32);
    if xe < r.right() {
        p.rect_filled(Rect::from_min_max(egui::pos2(xe, r.top()), r.max), 0.0, Color32::from_black_alpha(60));
    }
    let resp = ui.interact(r, ui.id().with("comp_ruler"), Sense::click_and_drag());
    env.note("comp_tl:ruler", r);
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let f = comp::x_frame(r, &env.ed.comp, pos.x).floor().clamp(0.0, (dur - 1) as f32) as u32;
            env.ed.playback.playing = false;
            env.ed.frame = f;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(env: &mut Env, ui: &mut Ui, p: &egui::Painter, cols: &Columns, row: &Row, rl: Rect, rt: Rect, bars: &std::collections::BTreeMap<NodeId, Option<(u32, u32)>>, graph: bool, hits: &mut Vec<(Rect, KeySel)>, idx: usize) {
    let t = env.t;
    let indent = |d: usize| d as f32 * 12.0;
    match row {
        Row::Layer { id, depth, l, name } => {
            let active = env.ed.layer.as_ref() == Some(id);
            let bg = if active { t.accent_dim } else if idx % 2 == 0 { t.bg_panel } else { t.bg_widget.gamma_multiply(0.8) };
            p.rect_filled(rl.union(rt), 0.0, bg);
            // setinha
            let open = env.ed.comp.expanded.contains(id);
            let tw = Rect::from_min_size(egui::pos2(cols.twirl, rl.top() + 3.0), egui::vec2(14.0, 14.0));
            let twr = ui.interact(tw, ui.id().with(("ctl_twirl", &id.0)), Sense::click());
            env.note(format!("comp_tl:layer:{}:twirl", id.0), tw);
            paint_icon(env.icons, p, if open { IconId::ChevronDown } else { IconId::ChevronRight }, tw, if twr.hovered() { t.text } else { t.text_dim }, false);
            if twr.clicked() {
                if open {
                    env.ed.comp.expanded.remove(id);
                } else {
                    env.ed.comp.expanded.insert(id.clone());
                    env.ed.comp.reveal = Reveal::All;
                }
            }
            // cor de rótulo
            let lab = Rect::from_min_size(egui::pos2(cols.label, rl.top() + 5.0), egui::vec2(10.0, 10.0));
            let lc = if l.label == 0 { t.border } else { LABELS[((l.label - 1) as usize).min(7)] };
            p.rect_filled(lab, 2.0, lc);
            let labr = ui.interact(lab.expand(2.0), ui.id().with(("ctl_label", &id.0)), Sense::click());
            env.note(format!("comp_tl:layer:{}:label", id.0), lab);
            let labr = labr.on_hover_text(tr("comp.label_color"));
            egui::Popup::menu(&labr).show(|ui| {
                for n in 0u8..=8 {
                    let c = if n == 0 { t.border } else { LABELS[(n - 1) as usize] };
                    let txt = if n == 0 { tr("comp.label_none").to_owned() } else { format!("{} {n}", tr("comp.label")) };
                    if ui.add(egui::Button::new(egui::RichText::new(format!("■ {txt}")).color(c))).clicked() {
                        if let Err(e) = cmds::set_layer_props(env.core, id, serde_json::json!({ "label": n })) {
                            env.ed.notify(e);
                        }
                    }
                }
            });
            // olho, solo, cadeado
            let toggles = [(cols.eye, "eye", l.visible), (cols.solo, "solo", env.ed.comp.solo.contains(id)), (cols.lock, "lock", l.locked)];
            for (x, what, on) in toggles {
                let r = Rect::from_min_size(egui::pos2(x, rl.top() + 1.0), egui::vec2(18.0, 18.0));
                let resp = ui.interact(r, ui.id().with(("ctl_tg", what, &id.0)), Sense::click());
                env.note(format!("comp_tl:layer:{}:{what}", id.0), r);
                let icon = match (what, on) {
                    ("eye", true) => IconId::Eye,
                    ("eye", false) => IconId::EyeOff,
                    ("lock", true) => IconId::Lock,
                    ("lock", false) => IconId::Unlock,
                    _ => IconId::Snap,
                };
                if what == "solo" {
                    let c = if on { t.accent } else { t.text_dim.gamma_multiply(0.5) };
                    p.circle(r.center(), 5.0, if on { c } else { Color32::TRANSPARENT }, Stroke::new(1.2, c));
                } else if on || what == "eye" || resp.hovered() {
                    paint_icon(env.icons, p, icon, r.shrink(3.0), if on { t.text } else { t.text_dim.gamma_multiply(0.5) }, false);
                }
                let tip = tr(match what {
                    "eye" => "comp.eye.tip",
                    "solo" => "comp.solo.tip",
                    _ => "comp.lock.tip",
                });
                if resp.on_hover_text(tip).clicked() {
                    match what {
                        "solo" => {
                            if !env.ed.comp.solo.remove(id) {
                                env.ed.comp.solo.insert(id.clone());
                            }
                        }
                        k => {
                            let field = if k == "eye" { "visible" } else { "locked" };
                            if let Err(e) = cmds::set_layer_props(env.core, id, serde_json::json!({ field: !on })) {
                                env.ed.notify(e);
                            }
                        }
                    }
                }
            }
            // nome (clique escolhe a camada)
            let nr = Rect::from_min_max(egui::pos2(cols.name + indent(*depth), rl.top()), egui::pos2(cols.name_end, rl.bottom()));
            let icon = super::layers::kind_icon(l.kind);
            let ir = Rect::from_min_size(egui::pos2(nr.left(), rl.top() + 3.0), egui::vec2(14.0, 14.0));
            paint_icon(env.icons, p, icon, ir, t.text_dim, false);
            let tp = p.with_clip_rect(nr.intersect(p.clip_rect()));
            tp.text(egui::pos2(ir.right() + 4.0, rl.center().y), Align2::LEFT_CENTER, name, FontId::proportional(12.5), if l.visible { t.text } else { t.text_dim });
            let nresp = ui.interact(nr, ui.id().with(("ctl_name", &id.0)), Sense::click());
            env.note(format!("comp_tl:layer:{}:name", id.0), nr);
            if nresp.clicked() {
                env.ed.layer = Some(id.clone());
            }
            if nresp.double_clicked() {
                if !env.ed.comp.expanded.remove(id) {
                    env.ed.comp.expanded.insert(id.clone());
                    env.ed.comp.reveal = Reveal::All;
                }
            }
            // pai (a pasta), modo, matte
            if let Some(x) = cols.parent {
                let parent = env.core.project.node(id).and_then(|n| n.parent.clone()).filter(|pid| cmds::layer_data(&env.core.project, pid).is_some_and(|pl| pl.kind == LayerType::Group));
                let txt = parent.and_then(|pid| env.core.project.node(&pid).map(|n| n.name.clone())).unwrap_or_else(|| tr("comp.parent_none").to_owned());
                let r = Rect::from_min_size(egui::pos2(x, rl.top()), egui::vec2(64.0, ROW_H));
                p.with_clip_rect(r).text(egui::pos2(x + 2.0, rl.center().y), Align2::LEFT_CENTER, txt, FontId::proportional(11.5), t.text_dim);
                ui.interact(r, ui.id().with(("ctl_parent", &id.0)), Sense::hover()).on_hover_text(tr("comp.parent.tip"));
            }
            if let Some(x) = cols.blend {
                let r = Rect::from_min_size(egui::pos2(x, rl.top() + 1.0), egui::vec2(80.0, ROW_H - 2.0));
                blend_combo(env, ui, r, id, l);
            }
            if let Some(x) = cols.matte {
                let r = Rect::from_min_size(egui::pos2(x, rl.top() + 1.0), egui::vec2(80.0, ROW_H - 2.0));
                matte_combo(env, ui, r, id, l);
            }
            // barra de duração
            if !graph {
                if let Some(Some((a, b))) = bars.get(id) {
                    let st = &env.ed.comp;
                    let (x0, x1) = (comp::frame_x(rt, st, *a as f32), comp::frame_x(rt, st, *b as f32));
                    let br = Rect::from_min_max(egui::pos2(x0.max(rt.left()), rt.top() + 4.0), egui::pos2(x1.min(rt.right()), rt.bottom() - 4.0));
                    if br.width() > 0.0 {
                        let c = if l.label == 0 { t.accent.gamma_multiply(0.45) } else { LABELS[((l.label - 1) as usize).min(7)].gamma_multiply(0.6) };
                        p.rect_filled(br, 3.0, c);
                    }
                }
            }
            // soltar efeito arrastado do navegador nesta camada
            let drop = ui.interact(rl.union(rt), ui.id().with(("ctl_drop", &id.0)), Sense::hover());
            if drop.dnd_hover_payload::<super::effects_browser::FxPayload>().is_some() {
                p.rect_stroke(rl.union(rt), 2.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            if let Some(pl) = drop.dnd_release_payload::<super::effects_browser::FxPayload>() {
                super::effects_browser::apply(env, id, &pl);
                env.ed.comp.expanded.insert(id.clone());
            }
        }
        Row::Group { depth, title, layer } => {
            p.rect_filled(rl.union(rt), 0.0, t.bg_panel.gamma_multiply(0.9));
            p.text(egui::pos2(cols.name + indent(*depth), rl.center().y), Align2::LEFT_CENTER, tr(title), FontId::proportional(12.0), t.text_dim);
            if *title == "comp.group.transform" {
                // redefinir a transformação
                let r = Rect::from_min_size(egui::pos2(cols.values, rl.top() + 1.0), egui::vec2(80.0, ROW_H - 2.0));
                let mut c = cell(ui, r);
                let b = c.add(egui::Button::new(egui::RichText::new(tr("comp.reset_transform")).size(11.0)).small());
                env.note(format!("comp_tl:layer:{}:reset", layer.0), b.rect);
                if b.clicked() {
                    comp::edit(env.core, env.ed, layer, "comp.undo.reset_transform", |l| {
                        for n in ["anchor_x", "anchor_y", "offset_x", "offset_y", "scale", "rotation", "opacity"] {
                            l.keys.0.remove(n);
                        }
                        l.offset = [0.0, 0.0];
                        l.anchor = [0.0, 0.0];
                        l.scale = 1.0;
                        l.rotation = 0.0;
                        l.opacity = 1.0;
                        Ok(())
                    });
                }
            }
        }
        Row::Effect { layer, depth, index, l } => {
            p.rect_filled(rl.union(rt), 0.0, t.bg_panel);
            let st = comp::stack(l);
            let Some(inst) = st.get(*index) else { return };
            let key = (layer.clone(), *index);
            let open = env.ed.comp.fx_open.contains(&key);
            let x = cols.name + indent(*depth) - 14.0;
            let tw = Rect::from_min_size(egui::pos2(x, rl.top() + 3.0), egui::vec2(14.0, 14.0));
            let twr = ui.interact(tw, ui.id().with(("ctl_fx", &layer.0, *index)), Sense::click());
            env.note(format!("comp_tl:fx:{}:{index}:twirl", layer.0), tw);
            paint_icon(env.icons, p, if open { IconId::ChevronDown } else { IconId::ChevronRight }, tw, t.text_dim, false);
            if twr.clicked() {
                if open {
                    env.ed.comp.fx_open.remove(&key);
                } else {
                    env.ed.comp.fx_open.insert(key);
                }
            }
            let ink = if inst.enabled { t.text } else { t.text_dim };
            p.text(egui::pos2(tw.right() + 3.0, rl.center().y), Align2::LEFT_CENTER, super::satsuei::effect_label(inst.effect.name()), FontId::proportional(12.0), ink);
            let r = Rect::from_min_size(egui::pos2(cols.values, rl.top()), egui::vec2(90.0, ROW_H));
            let mut c = cell(ui, r);
            let mut on = inst.enabled;
            let cb = c.checkbox(&mut on, egui::RichText::new(tr("comp.fx_on")).size(11.0));
            env.note(format!("comp_tl:fx:{}:{index}:enabled", layer.0), cb.rect);
            if on != inst.enabled {
                let r = crate::app::actions::run(env.core, "EFFECTS", serde_json::json!({ "op": "update", "layer": layer, "index": index, "enabled": on }));
                crate::app::actions::notify_err(env.ed, r);
            }
        }
        Row::Prop { layer, depth, p: prop, l } => {
            let selected = env.ed.comp.sel_prop.as_ref() == Some(prop);
            p.rect_filled(rl.union(rt), 0.0, if selected { t.accent_dim.gamma_multiply(0.6) } else { t.bg_widget.gamma_multiply(0.7) });
            let frame = env.ed.frame;
            let x = cols.name + indent(*depth) - 18.0;
            let swr = Rect::from_min_size(egui::pos2(x, rl.top() + 2.0), egui::vec2(16.0, 16.0));
            let anim = comp::animated(l, prop);
            let sw = ui.interact(swr, ui.id().with(("ctl_sw", &prop.owner, &prop.name)), Sense::click());
            let note = note_of(prop);
            env.note(format!("{note}:stopwatch"), swr);
            paint_icon(env.icons, p, IconId::Stopwatch, swr, if anim { t.accent } else if sw.hovered() { t.text } else { t.text_dim.gamma_multiply(0.6) }, false);
            if sw.on_hover_text(tr(if anim { "comp.stopwatch_off" } else { "comp.stopwatch_on" })).clicked() {
                comp::edit(env.core, env.ed, layer, "comp.undo.stopwatch", |l| comp::toggle_stopwatch(l, prop, frame).map(|_| ()));
            }
            let nr = Rect::from_min_max(egui::pos2(swr.right() + 4.0, rl.top()), egui::pos2(cols.values - 4.0, rl.bottom()));
            p.with_clip_rect(nr).text(egui::pos2(nr.left(), rl.center().y), Align2::LEFT_CENTER, super::effect_controls::prop_label(prop), FontId::proportional(12.0), t.text);
            let nresp = ui.interact(nr, ui.id().with(("ctl_pn", &prop.owner, &prop.name)), Sense::click());
            env.note(format!("{note}:name"), nr);
            if nresp.clicked() {
                env.ed.comp.sel_prop = Some(prop.clone());
                env.ed.layer = Some(layer.clone());
                // clique no nome seleciona as chaves da propriedade (como no After Effects)
                env.ed.comp.sel_keys = comp::key_frames(l, prop).into_iter().map(|f| KeySel { prop: prop.clone(), frame: f }).collect();
            }
            // losango do quadro atual (com o cronômetro ligado) e valores
            if anim {
                let here = comp::key_frames(l, prop).contains(&frame);
                let kr = Rect::from_min_size(egui::pos2(cols.values - 2.0, rl.top() + 2.0), egui::vec2(14.0, 16.0));
                let kresp = ui.interact(kr, ui.id().with(("ctl_kh", &prop.owner, &prop.name)), Sense::click());
                env.note(format!("{note}:key"), kr);
                super::effect_controls::paint_diamond(p, kr.center(), 4.5, if here { Some(t.accent) } else { None }, t.text_dim);
                if kresp.on_hover_text(tr(if here { "comp.key_remove" } else { "comp.key_add" })).clicked() {
                    comp::edit(env.core, env.ed, layer, "comp.undo.add_key", |l| comp::toggle_key_here(l, prop, frame));
                }
            }
            let vr = Rect::from_min_max(egui::pos2(cols.values + 14.0, rl.top()), egui::pos2(cols.right, rl.bottom()));
            env.note(format!("{note}:value"), vr);
            let mut c = cell(ui, vr);
            c.spacing_mut().interact_size.y = ROW_H - 4.0;
            if let Some(v) = super::effect_controls::value_widgets(&mut c, l, prop, frame, "tl", true) {
                comp::edit(env.core, env.ed, layer, "comp.undo.value", |l| comp::set_values(l, prop, frame, &v));
            }
            // chaves
            if !graph {
                let st = &env.ed.comp;
                let drag = st.key_drag.as_ref().map(|d| d.delta).unwrap_or(0);
                for f in comp::key_frames(l, prop) {
                    let ks = KeySel { prop: prop.clone(), frame: f };
                    let sel = st.sel_keys.contains(&ks);
                    let shown = if sel { (f as i64 + drag).max(0) as f32 } else { f as f32 };
                    let x = comp::frame_x(rt, st, shown + 0.5);
                    if x < rt.left() - 6.0 || x > rt.right() + 6.0 {
                        continue;
                    }
                    let c = egui::pos2(x, rt.center().y);
                    paint_key(p, c, comp::key_ease(l, prop, f).unwrap_or_default(), sel, &t);
                    hits.push((Rect::from_center_size(egui::pos2(comp::frame_x(rt, st, f as f32 + 0.5), rt.center().y), egui::vec2(12.0, 14.0)), ks));
                }
            }
        }
    }
}

fn note_of(p: &PropRef) -> String {
    match &p.owner {
        Owner::Layer(l) => format!("comp_tl:prop:{}:{}", l.0, p.name),
        Owner::Effect(l, i) => format!("comp_tl:prop:{}:fx{i}:{}", l.0, p.name),
    }
}

fn blend_combo(env: &mut Env, ui: &mut Ui, r: Rect, id: &NodeId, l: &LayerData) {
    let mut c = cell(ui, r);
    let mut pick = None;
    let resp = egui::ComboBox::from_id_salt(("ctl_blend", &id.0)).width(r.width() - 4.0).selected_text(egui::RichText::new(tr(cmds::blend_key(l.blend))).size(11.0)).show_ui(&mut c, |ui| {
        for m in cmds::BLEND_MODES {
            if m == aiai_core::BlendMode::PassThrough && l.kind != LayerType::Group {
                continue;
            }
            if ui.selectable_label(m == l.blend, tr(cmds::blend_key(m))).clicked() {
                pick = Some(m);
            }
        }
    });
    env.note(format!("comp_tl:layer:{}:blend", id.0), resp.response.rect);
    if let Some(m) = pick {
        let key = cmds::blend_key(m).rsplit('.').next().unwrap_or("normal");
        if let Err(e) = cmds::set_layer_props(env.core, id, serde_json::json!({ "blend": key })) {
            env.ed.notify(e);
        }
    }
}

/// Matte: nenhum, o alfa da camada abaixo (recorte) ou outra camada como máscara.
fn matte_combo(env: &mut Env, ui: &mut Ui, r: Rect, id: &NodeId, l: &LayerData) {
    let mut c = cell(ui, r);
    let Some(cut) = env.ed.cut.clone() else { return };
    let names: Vec<(NodeId, String)> = cmds::layer_rows(&env.core.project, &cut)
        .into_iter()
        .filter(|r| &r.id != id)
        .filter(|r| cmds::layer_data(&env.core.project, &r.id).is_some_and(|x| matches!(x.kind, LayerType::Raster | LayerType::Vector | LayerType::Fill)))
        .filter_map(|r| env.core.project.node(&r.id).map(|n| (r.id.clone(), n.name.clone())))
        .collect();
    let cur = if l.clip_to_below {
        tr("comp.matte.below").to_owned()
    } else if let Some(m) = &l.mask {
        let name = names.iter().find(|(i, _)| i == m).map(|(_, n)| n.clone()).unwrap_or_else(|| m.0.clone());
        let mode = match l.mask_mode {
            aiai_core::MaskMode::Alpha => tr("comp.matte.mode.alpha"),
            aiai_core::MaskMode::AlphaInverted => tr("comp.matte.mode.alpha_inv"),
            aiai_core::MaskMode::Luma => tr("comp.matte.mode.luma"),
            aiai_core::MaskMode::LumaInverted => tr("comp.matte.mode.luma_inv"),
        };
        format!("{name} · {mode}")
    } else {
        tr("comp.matte.none").to_owned()
    };
    let mut pick: Option<serde_json::Value> = None;
    let resp = egui::ComboBox::from_id_salt(("ctl_matte", &id.0)).width(r.width() - 4.0).selected_text(egui::RichText::new(cur).size(11.0)).show_ui(&mut c, |ui| {
        if ui.selectable_label(!l.clip_to_below && l.mask.is_none(), tr("comp.matte.none")).clicked() {
            pick = Some(serde_json::json!({ "clip_to_below": false, "mask": null }));
        }
        if ui.selectable_label(l.clip_to_below, tr("comp.matte.below")).on_hover_text(tr("comp.matte.below.tip")).clicked() {
            pick = Some(serde_json::json!({ "clip_to_below": true, "mask": null }));
        }
        ui.separator();
        for (mid, n) in &names {
            if ui.selectable_label(l.mask.as_ref() == Some(mid), format!("{} {n}", tr("comp.matte.mask"))).on_hover_text(tr("comp.matte.mask.tip")).clicked() {
                pick = Some(serde_json::json!({ "clip_to_below": false, "mask": mid }));
            }
        }
        // o que da camada-máscara vale (os quatro track mattes do After Effects)
        if l.mask.is_some() {
            ui.separator();
            for (m, k, key) in [
                (aiai_core::MaskMode::Alpha, "comp.matte.mode.alpha", "alpha"),
                (aiai_core::MaskMode::AlphaInverted, "comp.matte.mode.alpha_inv", "alpha_inverted"),
                (aiai_core::MaskMode::Luma, "comp.matte.mode.luma", "luma"),
                (aiai_core::MaskMode::LumaInverted, "comp.matte.mode.luma_inv", "luma_inverted"),
            ] {
                if ui.selectable_label(l.mask_mode == m, tr(k)).clicked() {
                    pick = Some(serde_json::json!({ "mask_mode": key }));
                }
            }
        }
    });
    env.note(format!("comp_tl:layer:{}:matte", id.0), resp.response.rect);
    if let Some(v) = pick {
        if let Err(e) = cmds::set_layer_props(env.core, id, v) {
            env.ed.notify(e);
        }
    }
}

/// Clique, Shift, arrasto e caixa de seleção das chaves; clique direito = interpolação.
fn track_input(env: &mut Env, ui: &mut Ui, area: Rect, hits: &[(Rect, KeySel)]) {
    let t = env.t;
    let resp = ui.interact(area, ui.id().with("comp_tl_track"), Sense::click_and_drag());
    let shift = ui.ctx().input(|i| i.modifiers.shift);
    let hit_at = |pos: Pos2| hits.iter().find(|(r, _)| r.contains(pos)).map(|(_, k)| k.clone());
    let press = resp.interact_pointer_pos();
    if resp.drag_started() {
        if let Some(pos) = press {
            match hit_at(pos) {
                Some(k) => {
                    if !env.ed.comp.sel_keys.contains(&k) {
                        if !shift {
                            env.ed.comp.sel_keys.clear();
                        }
                        env.ed.comp.sel_keys.insert(k.clone());
                    }
                    env.ed.comp.sel_prop = Some(k.prop.clone());
                    env.ed.comp.key_drag = Some(KeyDrag { start_x: pos.x, delta: 0 });
                }
                None => env.ed.comp.marquee = Some((pos, pos)),
            }
        }
    }
    if resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let ppf = env.ed.comp.px_per_frame;
            if let Some(d) = env.ed.comp.key_drag.as_mut() {
                d.delta = ((pos.x - d.start_x) / ppf).round() as i64;
            }
            if let Some((a, _)) = env.ed.comp.marquee {
                env.ed.comp.marquee = Some((a, pos));
            }
        }
    }
    if resp.drag_stopped() {
        if let Some(d) = env.ed.comp.key_drag.take() {
            comp::move_selected(env.core, env.ed, d.delta);
        }
        if let Some((a, b)) = env.ed.comp.marquee.take() {
            let r = Rect::from_two_pos(a, b);
            if !shift {
                env.ed.comp.sel_keys.clear();
            }
            for (hr, k) in hits {
                if r.intersects(*hr) {
                    env.ed.comp.sel_keys.insert(k.clone());
                }
            }
        }
    }
    if resp.clicked() {
        if let Some(pos) = press {
            match hit_at(pos) {
                Some(k) => {
                    if shift {
                        if !env.ed.comp.sel_keys.remove(&k) {
                            env.ed.comp.sel_keys.insert(k.clone());
                        }
                    } else {
                        env.ed.comp.sel_keys = [k.clone()].into_iter().collect();
                    }
                    env.ed.layer = Some(k.prop.owner.layer().clone());
                    env.ed.comp.sel_prop = Some(k.prop.clone());
                }
                None if !shift => env.ed.comp.sel_keys.clear(),
                None => {}
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(k) = press.and_then(hit_at) {
            if !env.ed.comp.sel_keys.contains(&k) {
                env.ed.comp.sel_keys = [k].into_iter().collect();
            }
        }
    }
    if let Some((a, b)) = env.ed.comp.marquee {
        let r = Rect::from_two_pos(a, b);
        ui.painter().rect(r, 0.0, t.accent.gamma_multiply(0.12), Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    }
    resp.context_menu(|ui| {
        let has = !env.ed.comp.sel_keys.is_empty();
        for (k, i) in [
            ("comp.interp.linear", KeyInterp::Linear),
            ("comp.interp.hold", KeyInterp::Hold),
            ("comp.interp.easy_ease", KeyInterp::EasyEase),
            ("comp.interp.easy_ease_in", KeyInterp::EasyEaseIn),
            ("comp.interp.easy_ease_out", KeyInterp::EasyEaseOut),
        ] {
            if ui.add_enabled(has, egui::Button::new(tr(k))).clicked() {
                comp::interp_selected(env.core, env.ed, i);
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(has, egui::Button::new(tr("comp.copy_keys"))).clicked() {
            comp::copy_selected(&env.core.project, env.ed);
            ui.close();
        }
        if ui.add_enabled(!env.ed.comp.clipboard.is_empty(), egui::Button::new(tr("comp.paste_keys"))).clicked() {
            comp::paste(env.core, env.ed);
            ui.close();
        }
        if ui.add_enabled(has, egui::Button::new(tr("comp.delete_keys"))).clicked() {
            comp::delete_selected(env.core, env.ed);
            ui.close();
        }
    });
}

// ------------------------------------------------------------------------------------------------ editor de gráfico

/// A propriedade do gráfico: a selecionada, ou a da primeira chave selecionada.
fn graph_prop(st: &CompState) -> Option<PropRef> {
    st.sel_prop.clone().or_else(|| st.sel_keys.iter().next().map(|k| k.prop.clone()))
}

fn graph_editor(env: &mut Env, ui: &mut Ui, area: Rect, dur: u32) {
    let t = env.t;
    let p = ui.painter().with_clip_rect(area);
    p.rect_filled(area, 0.0, t.bg_panel);
    let Some(prop) = graph_prop(&env.ed.comp) else {
        p.text(area.center(), Align2::CENTER_CENTER, tr("comp.graph_pick"), FontId::proportional(12.5), t.text_dim);
        return;
    };
    let layer = prop.owner.layer().clone();
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    let tracks = comp::tracks(&l, &prop);
    if tracks.is_empty() || !comp::animated(&l, &prop) {
        p.text(area.center(), Align2::CENTER_CENTER, tr("comp.graph_needs_keys"), FontId::proportional(12.5), t.text_dim);
        return;
    }
    let scale = if comp::unit(&prop) == comp::Unit::Percent { 100.0 } else { 1.0 };
    let st = &env.ed.comp;
    // faixa de valores: chaves e curva amostrada (fixa durante o arrasto)
    let (lo, hi) = st.graph_range.unwrap_or_else(|| {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for f in 0..dur {
            for v in comp::values(&l, &prop, f) {
                lo = lo.min(v * scale);
                hi = hi.max(v * scale);
            }
        }
        if (hi - lo).abs() < 1e-3 {
            lo -= 1.0;
            hi += 1.0;
        }
        let pad = (hi - lo) * 0.12;
        (lo - pad, hi + pad)
    });
    let y_of = |v: f32| area.bottom() - 10.0 - (v * scale - lo) / (hi - lo) * (area.height() - 20.0);
    let v_of = |y: f32| (lo + (area.bottom() - 10.0 - y) / (area.height() - 20.0) * (hi - lo)) / scale;
    // grade horizontal com valores
    for k in 0..5 {
        let v = lo + (hi - lo) * k as f32 / 4.0;
        let y = y_of(v / scale);
        p.hline(area.x_range(), y, Stroke::new(1.0, t.border.gamma_multiply(0.6)));
        p.text(egui::pos2(area.left() + 4.0, y - 1.0), Align2::LEFT_BOTTOM, format!("{v:.1}"), FontId::proportional(10.0), t.text_dim);
    }
    p.text(egui::pos2(area.right() - 6.0, area.top() + 4.0), Align2::RIGHT_TOP, super::effect_controls::prop_label(&prop), FontId::proportional(12.0), t.text);
    let mut handles: Vec<(Pos2, GraphDrag)> = Vec::new();
    let mut points: Vec<(Pos2, GraphDrag)> = Vec::new();
    for (ti, tr_name) in tracks.iter().enumerate() {
        let color = TRACK_COLORS[ti.min(3)];
        let keys = comp::track_keys(&l, &prop, tr_name);
        let owner_keys = l.clone();
        // curva amostrada a cada meio quadro
        let mut pts = Vec::new();
        let steps = (dur * 2).max(2);
        for s in 0..=steps {
            let f = s as f32 / 2.0;
            let x = comp::frame_x(area, st, f + 0.5);
            if x < area.left() - 2.0 || x > area.right() + 2.0 {
                continue;
            }
            let v = aiai_core::keys::eval_keys(&keys, f).unwrap_or_else(|| comp::static_value(&owner_keys, &prop.owner, tr_name));
            pts.push(egui::pos2(x, y_of(v)));
        }
        p.add(egui::Shape::line(pts, Stroke::new(1.6, color)));
        for (ki, k) in keys.iter().enumerate() {
            let sel = st.sel_keys.contains(&KeySel { prop: prop.clone(), frame: k.frame });
            let c = egui::pos2(comp::frame_x(area, st, k.frame as f32 + 0.5), y_of(k.value));
            p.rect(Rect::from_center_size(c, egui::vec2(7.0, 7.0)), 1.0, if sel { t.accent } else { color }, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Middle);
            points.push((c, GraphDrag::Key { track: tr_name.clone(), frame: k.frame, df: 0, value: k.value }));
            // alças da Bézier do trecho que começa nesta chave (só na 1ª trilha, que manda no easing de todas)
            if ti == 0 {
                if let (Some(b), Some(next)) = (aiai_core::keys::ease_to_bezier(k.ease), keys.get(ki + 1)) {
                    if (next.value - k.value).abs() > 1e-6 && !matches!(k.ease, Ease::Linear) {
                        let (fa, fb) = (k.frame as f32 + 0.5, next.frame as f32 + 0.5);
                        let h1 = egui::pos2(comp::frame_x(area, st, fa + b[0] * (fb - fa)), y_of(k.value + b[1] * (next.value - k.value)));
                        let h2 = egui::pos2(comp::frame_x(area, st, fa + b[2] * (fb - fa)), y_of(k.value + b[3] * (next.value - k.value)));
                        let n2 = egui::pos2(comp::frame_x(area, st, fb), y_of(next.value));
                        p.line_segment([c, h1], Stroke::new(1.0, t.text_dim));
                        p.line_segment([n2, h2], Stroke::new(1.0, t.text_dim));
                        p.circle_filled(h1, 3.5, t.text);
                        p.circle_filled(h2, 3.5, t.text);
                        handles.push((h1, GraphDrag::Handle { frame: k.frame, out: true, b }));
                        handles.push((h2, GraphDrag::Handle { frame: k.frame, out: false, b }));
                    }
                }
            }
        }
    }
    // entrada: alças primeiro (ficam por cima), depois chaves
    let resp = ui.interact(area, ui.id().with("comp_graph"), Sense::click_and_drag());
    env.note("comp_tl:graph_area", area);
    let near = |pos: Pos2| handles.iter().chain(points.iter()).find(|(c, _)| (*c - pos).length() < 7.0).map(|(_, g)| g.clone());
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            env.ed.comp.graph_drag = near(pos);
            env.ed.comp.graph_range = Some((lo, hi));
        }
    }
    if resp.dragged() {
        if let (Some(pos), Some(g)) = (ui.ctx().pointer_interact_pos(), env.ed.comp.graph_drag.clone()) {
            let st = &env.ed.comp;
            let g2 = match g {
                GraphDrag::Key { track, frame, value: _, .. } => {
                    let f = comp::x_frame(area, st, pos.x) - 0.5;
                    GraphDrag::Key { track, frame, df: (f - frame as f32).round() as i64, value: v_of(pos.y) }
                }
                GraphDrag::Handle { frame, out, mut b } => {
                    let keys = comp::track_keys(&l, &prop, &tracks[0]);
                    if let Some(i) = keys.iter().position(|k| k.frame == frame) {
                        if let Some(next) = keys.get(i + 1) {
                            let (fa, fb) = (keys[i].frame as f32 + 0.5, next.frame as f32 + 0.5);
                            let x = ((comp::x_frame(area, st, pos.x) - fa) / (fb - fa)).clamp(0.0, 1.0);
                            let y = (v_of(pos.y) - keys[i].value) / (next.value - keys[i].value);
                            if out {
                                b[0] = x;
                                b[1] = y;
                            } else {
                                b[2] = x;
                                b[3] = y;
                            }
                        }
                    }
                    GraphDrag::Handle { frame, out, b }
                }
            };
            env.ed.comp.graph_drag = Some(g2);
        }
    }
    // prévia do arrasto
    if let Some(g) = &env.ed.comp.graph_drag {
        match g {
            GraphDrag::Key { frame, df, value, .. } => {
                let c = egui::pos2(comp::frame_x(area, &env.ed.comp, (*frame as i64 + df).max(0) as f32 + 0.5), y_of(*value));
                p.circle_stroke(c, 6.0, Stroke::new(1.5, t.accent));
            }
            GraphDrag::Handle { .. } => {}
        }
    }
    if resp.drag_stopped() {
        env.ed.comp.graph_range = None;
        if let Some(g) = env.ed.comp.graph_drag.take() {
            match g {
                GraphDrag::Key { track, frame, df, value } => {
                    comp::edit(env.core, env.ed, &layer, "comp.undo.graph", |l| {
                        comp::set_key_value(l, &prop, &track, frame, value)?;
                        if df != 0 {
                            comp::move_keys(l, &prop, &[frame], df)?;
                        }
                        Ok(())
                    });
                }
                GraphDrag::Handle { frame, b, .. } => {
                    let ease = if (b[0] - b[1]).abs() < 1e-4 && (b[2] - b[3]).abs() < 1e-4 { Ease::Linear } else { Ease::Bezier { x1: b[0], y1: b[1], x2: b[2], y2: b[3] } };
                    comp::edit(env.core, env.ed, &layer, "comp.undo.graph", |l| comp::set_segment_ease(l, &prop, frame, ease));
                }
            }
        }
    }
    if resp.clicked() {
        if let Some(GraphDrag::Key { frame, .. }) = resp.interact_pointer_pos().and_then(near) {
            env.ed.comp.sel_keys = [KeySel { prop: prop.clone(), frame }].into_iter().collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_ticks_stay_readable() {
        assert_eq!(tick_step(50.0, 24.0), 1);
        assert_eq!(tick_step(10.0, 24.0), 5);
        assert_eq!(tick_step(1.0, 24.0), 48);
        assert!(tick_step(0.01, 24.0) >= 24 * 60);
    }
}

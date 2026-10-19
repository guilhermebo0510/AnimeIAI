//! Painel Ferramenta (como o de subferramentas dos programas de pintura), igual para todas as ferramentas: à esquerda
//! a busca e as categorias, à direita os itens da categoria com miniatura, e embaixo só as propriedades do item
//! escolhido. Pincel: a biblioteca (fábrica + usuário) com o traço de verdade de cada pincel; caneta: as penas e as
//! formas; borracha: as raster (dura, macia) e os modos vetoriais; balde: os jeitos de preencher; seleção: os modos.
//! Escolher uma ferramenta traz este painel para a frente.

use crate::app::brushes::{CATEGORIES, cat_label, cat_of};
use crate::app::editor::{PenShape, PointsMode, SelMode};
use crate::app::ui_kit::{Env, body_ui, section};
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::VectorTip;
use aiai_tools::erase::EraseMode;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

#[derive(Default)]
pub(crate) struct BrushPanelState {
    /// Categoria aberta no pincel (`None` = todas).
    pub cat: Option<String>,
    /// Categoria aberta nas outras ferramentas.
    cat_other: std::collections::HashMap<&'static str, String>,
    pub search: String,
    /// Formulário "salvar como pincel novo": (nome, categoria).
    pub saving: Option<(String, String)>,
    thumbs: Vec<egui::TextureHandle>,
    thumbs_key: (u64, bool),
    /// Miniaturas feitas pelo motor para as penas e as borrachas raster (refeitas com o tema).
    extra: std::collections::HashMap<String, egui::TextureHandle>,
    extra_dark: Option<bool>,
    /// Altura da lista escolhida arrastando a divisória (`None` = automática: as propriedades ficam com o que
    /// precisam e a lista com o resto). Dois cliques na divisória voltam ao automático.
    pub list_h: Option<f32>,
    /// Altura que as propriedades e a lista (itens e categorias) ocuparam no quadro anterior (divisória automática).
    props_h: f32,
    list_need: f32,
}

/// Cartão de item: largura mínima, altura, espaço entre cartões e altura da miniatura (proporção fixa 5:1).
const CARD_MIN_W: f32 = 100.0;
const CARD_H: f32 = 38.0;
const CARD_GAP: f32 = 3.0;
const THUMB_H: f32 = 20.0;
const THUMB_ASPECT: f32 = 5.0;

/// O que escolher um item faz.
#[derive(Clone, Copy, PartialEq)]
enum Act {
    Brush(usize),
    PenTip(VectorTip),
    PenShape(PenShape),
    EraserRaster(u8),
    EraserVector(EraseMode),
    Fill(u8),
    Select(SelMode),
    Points(PointsMode),
    Only,
}

/// Como desenhar a miniatura.
#[derive(Clone, Copy)]
enum Thumb {
    Brush(usize),
    Tex(&'static str),
    Pen(PenShape),
    Erase(EraseMode),
    Fill(u8),
    Select(SelMode),
    Points(PointsMode),
    Icon(ToolId),
}

struct Item {
    name: String,
    cat: String,
    act: Act,
    thumb: Thumb,
    /// Pincel do usuário (pode apagar).
    user: bool,
}

/// Borrachas raster (dureza) e jeitos do balde (vão, expandir, fechar sobras).
const ERASERS: [(&str, f32); 2] = [("eraser.hard", 0.9), ("eraser.soft", 0.0)];
const FILLS: [(&str, f32, u32, bool); 4] = [("fill.normal", 3.0, 2, false), ("fill.wide", 8.0, 2, false), ("fill.exact", 0.0, 0, false), ("fill.leftovers", 3.0, 2, true)];

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let tool = *env.tool;
    ensure_thumbs(env, ui);
    ui.label(egui::RichText::new(tr(tool.title_key())).strong().size(13.0));
    ui.add_space(2.0);
    let items = items_for(env, tool);
    let cats = categories(&items, tool);
    // categoria aberta (o pincel começa em "todas"; as outras ferramentas na do item em uso)
    let key = tool.key();
    let mut cur_cat: Option<String> = if tool == ToolId::Brush {
        env.ed.brush_panel.cat.clone()
    } else {
        env.ed.brush_panel.cat_other.get(key).cloned().or_else(|| items.iter().find(|it| is_selected(env, it.act)).map(|it| it.cat.clone())).or_else(|| cats.first().cloned())
    };
    // ---- em cima: categorias à esquerda, itens à direita; a divisória embaixo regula a altura
    let avail_h = ui.available_height();
    let max_list = (avail_h - 70.0).max(80.0);
    let below = env.ed.brush_panel.props_h + if tool == ToolId::Brush { 64.0 } else { 36.0 };
    let need = env.ed.brush_panel.list_need.max(60.0);
    // automático: cabendo tudo, a lista fica do tamanho dos itens; senão, os itens até ~metade do painel e as
    // propriedades com o resto (rolando)
    let auto = if need + below + 10.0 <= avail_h { need } else { need.min(avail_h * 0.5).max(avail_h - below - 10.0) };
    let list_h = match env.ed.brush_panel.list_h {
        Some(h) => h,
        None => auto,
    }
    .min(max_list)
    .max(80.0f32.min(max_list));
    let total_w = ui.available_width();
    let left_w = (total_w * 0.36).clamp(92.0, 170.0);
    let side = tool == ToolId::Brush || cats.len() > 1;
    let mut pick: Option<Act> = None;
    let mut delete: Option<usize> = None;
    let (mut cats_need, mut items_need) = (0.0f32, 0.0f32);
    // a linha de cima tem altura fixa (o separador vertical não pode esticar até o fim do painel)
    ui.allocate_ui_with_layout(egui::vec2(total_w, list_h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_max_height(list_h);
        // uma categoria só (balde, seleção, editar pontos...): sem a coluna da esquerda, a grade usa a largura toda
        if side {
        ui.allocate_ui_with_layout(egui::vec2(left_w, list_h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(left_w);
            let r = ui.add(egui::TextEdit::singleline(&mut env.ed.brush_panel.search).hint_text(tr(if tool == ToolId::Brush { "brushes.search" } else { "tool.search" })).desired_width(left_w));
            env.note("brushes:search", r.rect);
            let cats_out = egui::ScrollArea::vertical().id_salt(("tool_cats", key)).max_height(list_h - 28.0).auto_shrink([false, true]).show(ui, |ui| {
                if tool == ToolId::Brush {
                    let r = ui.selectable_label(cur_cat.is_none(), egui::RichText::new(format!("{} ({})", tr("brushes.all"), items.len())).size(12.0));
                    env.note("brushes:cat:all", r.rect);
                    if r.clicked() {
                        cur_cat = None;
                    }
                }
                for c in &cats {
                    let n = items.iter().filter(|it| it.cat == *c).count();
                    let r = ui.selectable_label(cur_cat.as_deref() == Some(c.as_str()), egui::RichText::new(format!("{} ({n})", cat_label_any(c))).size(12.0));
                    env.note(format!("brushes:cat:{c}"), r.rect);
                    if r.clicked() {
                        cur_cat = Some(c.clone());
                    }
                }
            });
            cats_need = cats_out.content_size.y + 30.0;
        });
        let (sep, _) = ui.allocate_exact_size(egui::vec2(9.0, list_h), Sense::hover());
        ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, env.t.border));
        }
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), list_h), egui::Layout::top_down(egui::Align::Min), |ui| {
            let items_out = egui::ScrollArea::vertical().id_salt(("tool_items", key)).max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
                let needle = env.ed.brush_panel.search.trim().to_lowercase();
                let shown: Vec<usize> = (0..items.len())
                    .filter(|&i| cur_cat.as_ref().is_none_or(|c| items[i].cat == *c))
                    .filter(|&i| needle.is_empty() || items[i].name.to_lowercase().contains(&needle))
                    .collect();
                // grade: cartões de tamanho fixo lado a lado; colunas conforme a largura (estreito = uma coluna, a
                // miniatura é cortada, nunca achatada)
                let avail_w = ui.available_width();
                let cols = (((avail_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP)).floor() as usize).max(1);
                let card_w = ((avail_w - CARD_GAP * (cols - 1) as f32) / cols as f32).floor().max(24.0);
                for row in shown.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = CARD_GAP;
                        for &i in row {
                            let it = &items[i];
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(card_w, CARD_H), Sense::click());
                            let note = match it.act {
                                Act::Brush(b) => format!("brushes:item:{b}"),
                                _ => format!("tool_item:{i}"),
                            };
                            env.note(note, rect);
                            let sel = is_selected(env, it.act);
                            let t = env.t;
                            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                            let fill = if sel { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget };
                            painter.rect(rect, 3.0, fill, Stroke::new(1.0, if sel { t.accent } else { t.border }), StrokeKind::Inside);
                            let name = if it.user { format!("{} ·", it.name) } else { it.name.clone() };
                            let g = painter.layout_no_wrap(name, egui::FontId::proportional(10.5), t.text);
                            painter.with_clip_rect(rect.shrink(2.0).intersect(ui.clip_rect())).galley(egui::pos2(rect.left() + 4.0, rect.top() + 2.0), g, t.text);
                            let tr_rect = Rect::from_min_max(egui::pos2(rect.left() + 3.0, rect.bottom() - THUMB_H - 3.0), egui::pos2(rect.right() - 3.0, rect.bottom() - 3.0));
                            paint_thumb(env, &painter, tr_rect, it.thumb);
                            if resp.clicked() {
                                pick = Some(it.act);
                            }
                            let (user, idx) = (it.user, if let Act::Brush(b) = it.act { Some(b) } else { None });
                            let r = resp.on_hover_text(it.name.clone());
                            if let Some(b) = idx {
                                r.context_menu(|ui| {
                                    if ui.add_enabled(user, egui::Button::new(tr("brushes.delete"))).clicked() {
                                        delete = Some(b);
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(CARD_GAP);
                }
            });
            items_need = items_out.content_size.y;
        });
    });
    env.ed.brush_panel.list_need = items_need.max(cats_need);
    if tool == ToolId::Brush {
        env.ed.brush_panel.cat = cur_cat;
    } else if let Some(c) = cur_cat {
        env.ed.brush_panel.cat_other.insert(key, c);
    }
    if let Some(a) = pick {
        apply(env, a);
    }
    if let Some(i) = delete {
        if let Err(e) = env.ed.brushes.remove_user(i) {
            env.ed.notify(e);
        }
        if env.ed.opts.brush_preset >= env.ed.brushes.items.len() || env.ed.opts.brush_preset == i {
            env.ed.opts.brush_preset = 1;
            env.ed.opts.brush_custom = None;
        }
    }
    // divisória: arrastar sobe/desce o limite entre a lista e as propriedades; dois cliques = automático
    let (div, dr) = ui.allocate_exact_size(egui::vec2(total_w, 9.0), Sense::click_and_drag());
    env.note("brushes:divider", div);
    let hot = dr.hovered() || dr.dragged();
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let lc = if hot { env.t.accent } else { env.t.border };
    ui.painter().hline(div.x_range(), div.center().y, Stroke::new(1.0, lc));
    for k in -1..=1 {
        ui.painter().circle_filled(div.center() + egui::vec2(k as f32 * 6.0, 0.0), 1.6, if hot { env.t.accent } else { env.t.text_dim });
    }
    if dr.dragged() {
        env.ed.brush_panel.list_h = Some((list_h + dr.drag_delta().y).min(max_list).max(80.0));
    }
    if dr.double_clicked() {
        env.ed.brush_panel.list_h = None;
    }
    dr.on_hover_text(tr("brushes.divider.tip"));
    if tool == ToolId::Brush {
        save_form(env, ui);
    }
    // ---- embaixo: as propriedades do item escolhido
    let out = egui::ScrollArea::vertical().id_salt("brush_props").auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &env.t, "brushes.props");
        super::properties::tool_options_panel(env, ui);
    });
    env.ed.brush_panel.props_h = out.content_size.y;
}

fn cat_label_any(c: &str) -> String {
    if c.starts_with("tc.") { tr(c).to_owned() } else { cat_label(c) }
}

fn categories(items: &[Item], tool: ToolId) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if tool == ToolId::Brush {
        out.extend(CATEGORIES.iter().map(|(k, _)| (*k).to_owned()));
    }
    for it in items {
        if !out.contains(&it.cat) {
            out.push(it.cat.clone());
        }
    }
    out.retain(|c| items.iter().any(|it| it.cat == *c));
    out
}

fn items_for(env: &Env, tool: ToolId) -> Vec<Item> {
    let it = |name: String, cat: &str, act: Act, thumb: Thumb| Item { name, cat: cat.to_owned(), act, thumb, user: false };
    match tool {
        ToolId::Brush => env.ed.brushes.items.iter().enumerate().map(|(i, b)| Item { name: b.preset.name.clone(), cat: cat_of(&b.preset), act: Act::Brush(i), thumb: Thumb::Brush(i), user: b.user }).collect(),
        ToolId::Pen => {
            let mut v: Vec<Item> = [(VectorTip::GPen, "tip.g_pen", "pen:g"), (VectorTip::MaruPen, "tip.maru_pen", "pen:maru"), (VectorTip::Marker, "tip.marker", "pen:marker"), (VectorTip::Pencil, "tip.pencil", "pen:pencil")]
                .into_iter()
                .map(|(t, k, tex)| it(tr(k).to_owned(), "tc.pens", Act::PenTip(t), Thumb::Tex(tex)))
                .collect();
            v.extend([(PenShape::Freehand, "pen.freehand"), (PenShape::Line, "pen.line"), (PenShape::Curve, "pen.curve"), (PenShape::Rect, "pen.rect"), (PenShape::Ellipse, "pen.ellipse")].into_iter().map(|(s, k)| it(tr(k).to_owned(), "tc.shapes", Act::PenShape(s), Thumb::Pen(s))));
            v
        }
        ToolId::Eraser => {
            let mut v: Vec<Item> = ERASERS.iter().enumerate().map(|(i, (k, _))| it(tr(k).to_owned(), "tc.raster", Act::EraserRaster(i as u8), Thumb::Tex(if i == 0 { "erase:hard" } else { "erase:soft" }))).collect();
            v.extend([(EraseMode::Segment, "opt.erase_segment"), (EraseMode::Intersection, "opt.erase_intersection"), (EraseMode::Whole, "opt.erase_whole")].into_iter().map(|(m, k)| it(tr(k).to_owned(), "tc.vector", Act::EraserVector(m), Thumb::Erase(m))));
            v
        }
        ToolId::Fill => FILLS.iter().enumerate().map(|(i, (k, ..))| it(tr(k).to_owned(), "tc.fill", Act::Fill(i as u8), Thumb::Fill(i as u8))).collect(),
        ToolId::Lasso => [(SelMode::Rect, "sel.rect"), (SelMode::Ellipse, "sel.ellipse"), (SelMode::Lasso, "sel.lasso"), (SelMode::Wand, "sel.wand")].into_iter().map(|(m, k)| it(tr(k).to_owned(), "tc.select", Act::Select(m), Thumb::Select(m))).collect(),
        ToolId::Points => super::properties::POINTS_MODES.iter().map(|&m| it(tr(super::properties::points_mode_key(m)).to_owned(), "tc.points", Act::Points(m), Thumb::Points(m))).collect(),
        other => vec![it(tr(other.title_key()).to_owned(), "tc.tool", Act::Only, Thumb::Icon(other))],
    }
}

fn is_selected(env: &Env, a: Act) -> bool {
    let o = &env.ed.opts;
    match a {
        Act::Brush(i) => o.brush_preset == i,
        Act::PenTip(t) => o.pen_tip == t,
        Act::PenShape(s) => o.pen_shape == s,
        Act::EraserRaster(i) => (o.eraser_hardness - ERASERS[i as usize].1).abs() < 1e-3,
        Act::EraserVector(m) => o.eraser_mode == m,
        Act::Fill(i) => {
            let (_, gap, expand, left) = FILLS[i as usize];
            (o.fill_gap - gap).abs() < 1e-3 && o.fill_expand == expand && o.fill_close_leftovers == left
        }
        Act::Select(m) => o.select_mode == m,
        Act::Points(m) => o.points_mode == m,
        Act::Only => true,
    }
}

fn apply(env: &mut Env, a: Act) {
    match a {
        Act::Brush(i) => {
            if let Some(p) = env.ed.brushes.get(i).cloned() {
                let o = &mut env.ed.opts;
                o.brush_preset = i;
                o.brush_size = p.size;
                o.brush_opacity = p.opacity;
                o.brush_custom = None;
            }
        }
        Act::PenTip(t) => env.ed.opts.pen_tip = t,
        Act::PenShape(s) => env.ed.opts.pen_shape = s,
        Act::EraserRaster(i) => env.ed.opts.eraser_hardness = ERASERS[i as usize].1,
        Act::EraserVector(m) => env.ed.opts.eraser_mode = m,
        Act::Fill(i) => {
            let (_, gap, expand, left) = FILLS[i as usize];
            let o = &mut env.ed.opts;
            o.fill_gap = gap;
            o.fill_expand = expand;
            o.fill_close_leftovers = left;
        }
        Act::Select(m) => env.ed.opts.select_mode = m,
        Act::Points(m) => env.ed.opts.points_mode = m,
        Act::Only => {}
    }
}

// ------------------------------------------------------------------------------------------------ miniaturas

fn ensure_thumbs(env: &mut Env, ui: &Ui) {
    let t = env.t;
    let dark = ui.ctx().theme() == egui::Theme::Dark;
    let key = (env.ed.brushes.version, dark);
    if env.ed.brush_panel.thumbs_key != key || env.ed.brush_panel.thumbs.len() != env.ed.brushes.items.len() {
        let presets: Vec<_> = env.ed.brushes.items.iter().map(|b| b.preset.clone()).collect();
        env.ed.brush_panel.thumbs = super::properties::brush_thumbnails(ui.ctx(), &presets, t.bg_widget, t.text);
        env.ed.brush_panel.thumbs_key = key;
    }
    if env.ed.brush_panel.extra_dark == Some(dark) {
        return;
    }
    let mut m = std::collections::HashMap::new();
    let (w, h) = (280u32, 56u32);
    let c = |c: Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0];
    let (bg, ink) = (c(t.bg_widget), c(t.text));
    // penas vetoriais: o traço rasterizado pelo próprio motor (largura com pressão e afinamento de cada pena)
    for (tip, name) in [(VectorTip::GPen, "pen:g"), (VectorTip::MaruPen, "pen:maru"), (VectorTip::Marker, "pen:marker"), (VectorTip::Pencil, "pen:pencil")] {
        let pts: Vec<aiai_core::StrokePoint> = (0..40)
            .map(|k| {
                let f = k as f32 / 39.0;
                aiai_core::StrokePoint { x: 18.0 + f * (w as f32 - 36.0), y: h as f32 / 2.0 + 13.0 * (f * std::f32::consts::TAU).sin(), pressure: 0.3 + 0.7 * (f * std::f32::consts::PI).sin(), tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
            })
            .collect();
        let s = aiai_core::Stroke { id: aiai_core::Id(String::new()), points: pts, geometry: aiai_core::StrokeGeometry::Spline, tension: 0.0, tip, width: 9.0, profile: Default::default(), color: aiai_core::ColorRef::default(), opacity: 1.0, closed: false, aliased: false, color_trace: false };
        let mut img = aiai_paint::Image::filled(w, h, bg);
        if let Some(cov) = aiai_paint::stroke_raster::rasterize_stroke(&aiai_paint::sample_stroke(&s), false, w, h) {
            for yy in 0..cov.height {
                for xx in 0..cov.width {
                    let a = cov.cov[(yy * cov.width + xx) as usize];
                    if a > 0.0 {
                        let i = img.idx(cov.x0 as u32 + xx, cov.y0 as u32 + yy);
                        aiai_paint::Image::over(&mut img.data[i], [ink[0] * a, ink[1] * a, ink[2] * a, a], 1.0);
                    }
                }
            }
        }
        m.insert(name.to_owned(), tex(ui, name, &img));
    }
    // borrachas raster: uma faixa da cor do texto apagada pelo próprio pincel de borracha
    for (i, name) in ["erase:hard", "erase:soft"].iter().enumerate() {
        let mut img = aiai_paint::Image::new(w, h);
        for y in 12..44 {
            for x in 10..w - 10 {
                let k = img.idx(x, y);
                img.data[k] = ink;
            }
        }
        let mut p = aiai_paint::brush::BrushPreset::round("borracha", 26.0, ERASERS[i].1);
        p.mode = aiai_paint::brush::BrushMode::Erase;
        let path: Vec<aiai_paint::brush::BrushPoint> = (0..40)
            .map(|k| {
                let f = k as f32 / 39.0;
                aiai_paint::brush::BrushPoint { x: 24.0 + f * (w as f32 - 48.0), y: h as f32 / 2.0 + 9.0 * (f * std::f32::consts::TAU).sin(), pressure: 1.0 }
            })
            .collect();
        aiai_paint::brush::paint_stroke(&mut img, &p, &path, [0.0, 0.0, 0.0, 1.0], 3);
        // o que a borracha tirou mostra o fundo do cartão
        for px in img.data.iter_mut() {
            let a = px[3];
            for ch in 0..3 {
                px[ch] += bg[ch] * (1.0 - a);
            }
            px[3] = 1.0;
        }
        m.insert((*name).to_owned(), tex(ui, name, &img));
    }
    env.ed.brush_panel.extra = m;
    env.ed.brush_panel.extra_dark = Some(dark);
}

fn tex(ui: &Ui, name: &str, img: &aiai_paint::Image) -> egui::TextureHandle {
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_rgba8());
    ui.ctx().load_texture(format!("tool-thumb-{name}"), ci, egui::TextureOptions::LINEAR)
}

fn paint_thumb(env: &mut Env, p: &egui::Painter, cell: Rect, th: Thumb) {
    let t = env.t;
    // proporção fixa: sobra = centrada; falta = cortada (nunca achatada)
    let r = Rect::from_center_size(cell.center(), egui::vec2(cell.height() * THUMB_ASPECT, cell.height()));
    let p = &p.with_clip_rect(cell.intersect(p.clip_rect()));
    let ink = Stroke::new(1.6, t.text);
    let dim = Stroke::new(1.2, t.text_dim);
    let at = |fx: f32, fy: f32| Pos2::new(r.left() + r.width() * fx, r.top() + r.height() * fy);
    let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
    let ellipse = |c: Pos2| -> Vec<Pos2> {
        (0..=40)
            .map(|k| {
                let a = k as f32 / 40.0 * std::f32::consts::TAU;
                c + Vec2::new(a.cos() * r.width() * 0.28, a.sin() * r.height() * 0.36)
            })
            .collect()
    };
    match th {
        Thumb::Brush(i) => {
            if let Some(x) = env.ed.brush_panel.thumbs.get(i) {
                p.image(x.id(), r, uv, Color32::WHITE);
            }
        }
        Thumb::Tex(name) => {
            if let Some(x) = env.ed.brush_panel.extra.get(name) {
                p.image(x.id(), r, uv, Color32::WHITE);
            }
        }
        Thumb::Pen(s) => match s {
            PenShape::Freehand => {
                let pts: Vec<Pos2> = (0..24).map(|k| at(0.08 + 0.84 * k as f32 / 23.0, 0.5 + 0.3 * (k as f32 / 23.0 * 6.0).sin())).collect();
                p.add(egui::Shape::line(pts, ink));
            }
            PenShape::Line => {
                p.line_segment([at(0.1, 0.8), at(0.9, 0.2)], ink);
            }
            PenShape::Curve => {
                let pts: Vec<Pos2> = (0..24).map(|k| {
                    let f = k as f32 / 23.0;
                    at(0.1 + 0.8 * f, 0.8 - 0.6 * (f * std::f32::consts::PI).sin())
                }).collect();
                p.add(egui::Shape::line(pts, ink));
                for (fx, fy) in [(0.1, 0.8), (0.5, 0.2), (0.9, 0.8)] {
                    p.rect_filled(Rect::from_center_size(at(fx, fy), egui::vec2(4.0, 4.0)), 0.0, t.accent);
                }
            }
            PenShape::Rect => {
                p.rect_stroke(Rect::from_min_max(at(0.25, 0.18), at(0.75, 0.82)), 0.0, ink, StrokeKind::Inside);
            }
            PenShape::Ellipse => {
                p.add(egui::Shape::line(ellipse(r.center()), ink));
            }
        },
        Thumb::Erase(m) => {
            // dois traços que se cruzam; o círculo é a borracha e some o que o modo apaga
            let (a0, a1) = (at(0.08, 0.7), at(0.92, 0.3));
            let (b0, b1) = (at(0.35, 0.1), at(0.6, 0.95));
            let cut = at(0.3, 0.6);
            p.line_segment([b0, b1], dim);
            match m {
                EraseMode::Whole => {}
                EraseMode::Segment => {
                    p.line_segment([a0, a0 + (cut - a0) * 0.75], ink);
                    p.line_segment([cut + (cut - a0) * 0.6, a1], ink);
                }
                EraseMode::Intersection => {
                    p.line_segment([at(0.45, 0.52), a1], ink);
                }
            }
            p.circle_stroke(cut, r.height() * 0.32, Stroke::new(1.0, t.accent));
        }
        Thumb::Fill(i) => {
            // contorno (com fresta no "fechar frestas largas"); o balde enche ou não conforme o jeito
            let rr = Rect::from_min_max(at(0.18, 0.15), at(0.82, 0.85));
            if i != 2 {
                p.rect_filled(rr.shrink(2.0), 2.0, t.accent.gamma_multiply(0.55));
            }
            if i == 1 {
                p.line_segment([rr.left_top(), rr.right_top()], ink);
                p.line_segment([rr.right_top(), rr.right_bottom()], ink);
                p.line_segment([rr.left_top(), rr.left_bottom()], ink);
                p.line_segment([rr.left_bottom(), rr.left_bottom() + Vec2::new(rr.width() * 0.35, 0.0)], ink);
                p.line_segment([rr.right_bottom(), rr.right_bottom() - Vec2::new(rr.width() * 0.35, 0.0)], ink);
            } else {
                p.rect_stroke(rr, 2.0, ink, StrokeKind::Inside);
            }
            if i == 3 {
                p.circle_filled(at(0.7, 0.32), 3.0, t.text);
            }
        }
        Thumb::Select(m) => {
            let dash = Stroke::new(1.2, t.text);
            match m {
                SelMode::Rect => {
                    let rr = Rect::from_min_max(at(0.25, 0.15), at(0.75, 0.85));
                    let pts = vec![rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
                    p.extend(egui::Shape::dashed_line(&pts, dash, 4.0, 3.0));
                }
                SelMode::Ellipse => {
                    p.extend(egui::Shape::dashed_line(&ellipse(r.center()), dash, 4.0, 3.0));
                }
                SelMode::Lasso => {
                    let pts: Vec<Pos2> = [(0.2, 0.5), (0.35, 0.15), (0.6, 0.25), (0.8, 0.2), (0.75, 0.6), (0.55, 0.85), (0.3, 0.75), (0.2, 0.5)].iter().map(|(x, y)| at(*x, *y)).collect();
                    p.extend(egui::Shape::dashed_line(&pts, dash, 4.0, 3.0));
                }
                SelMode::Wand => {
                    p.line_segment([at(0.25, 0.85), at(0.6, 0.35)], ink);
                    let c = at(0.66, 0.27);
                    for k in 0..4 {
                        let a = k as f32 * std::f32::consts::FRAC_PI_2 + 0.4;
                        p.line_segment([c, c + Vec2::new(a.cos(), a.sin()) * 6.0], Stroke::new(1.2, t.accent));
                    }
                }
            }
        }
        Thumb::Points(m) => {
            // curva com três pontos; o do meio mostra o que o modo faz
            let (a, mid, b) = (at(0.1, 0.75), at(0.5, 0.3), at(0.9, 0.75));
            let curve: Vec<Pos2> = (0..=24)
                .map(|k| {
                    let f = k as f32 / 24.0;
                    if m == PointsMode::Corner {
                        if f < 0.5 { a + (mid - a) * (f * 2.0) } else { mid + (b - mid) * (f * 2.0 - 1.0) }
                    } else {
                        let u = 1.0 - f;
                        let c = mid + (mid - (a + (b - a) * 0.5)) * 0.9;
                        Pos2::new(u * u * a.x + 2.0 * u * f * c.x + f * f * b.x, u * u * a.y + 2.0 * u * f * c.y + f * f * b.y)
                    }
                })
                .collect();
            if m == PointsMode::Width {
                // traço que engrossa no ponto
                for (k, q) in curve.iter().enumerate() {
                    let f = k as f32 / 24.0;
                    p.circle_filled(*q, 0.8 + 3.2 * (1.0 - (f - 0.5).abs() * 2.0), t.text);
                }
            } else {
                p.add(egui::Shape::line(curve, ink));
            }
            for q in [a, b] {
                p.circle(q, 2.4, Color32::WHITE, Stroke::new(1.0, t.accent));
            }
            match m {
                PointsMode::Move => {
                    p.circle(mid, 3.0, t.accent, Stroke::new(1.0, Color32::WHITE));
                    p.arrow(mid + Vec2::new(4.0, -1.0), Vec2::new(10.0, -5.0), Stroke::new(1.2, t.accent));
                }
                PointsMode::Add => {
                    p.circle(mid, 3.0, t.accent, Stroke::new(1.0, Color32::WHITE));
                    let c = mid + Vec2::new(10.0, -2.0);
                    p.line_segment([c - Vec2::new(3.5, 0.0), c + Vec2::new(3.5, 0.0)], Stroke::new(1.4, t.accent));
                    p.line_segment([c - Vec2::new(0.0, 3.5), c + Vec2::new(0.0, 3.5)], Stroke::new(1.4, t.accent));
                }
                PointsMode::Delete => {
                    let k = 3.0;
                    p.line_segment([mid - Vec2::new(k, k), mid + Vec2::new(k, k)], Stroke::new(1.6, t.accent));
                    p.line_segment([mid - Vec2::new(k, -k), mid + Vec2::new(k, -k)], Stroke::new(1.6, t.accent));
                }
                PointsMode::Corner => {
                    p.rect(Rect::from_center_size(mid, egui::vec2(6.0, 6.0)), 0.0, t.accent, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
                }
                PointsMode::Width => {
                    p.circle(mid, 2.6, t.accent, Stroke::new(1.0, Color32::WHITE));
                }
            }
        }
        Thumb::Icon(tool) => {
            let side = r.height().min(r.width());
            let ir = Rect::from_center_size(r.center(), egui::vec2(side, side));
            env.icons.paint(p, tool.icon(), ir, t.text);
        }
    }
}

/// "Salvar como pincel novo" (fora da rolagem da lista, sempre à vista).
fn save_form(env: &mut Env, ui: &mut Ui) {
    match env.ed.brush_panel.saving.clone() {
        None => {
            let b = ui.button(tr("brushes.save_new"));
            env.note("brushes:save_new", b.rect);
            if b.on_hover_text(tr("brushes.save_new.tip")).clicked() {
                let base = env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| (p.name.clone(), cat_of(p))).unwrap_or_default();
                env.ed.brush_panel.saving = Some((format!("{} 2", base.0), base.1));
            }
        }
        Some((mut name, mut cat)) => {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(tr("brushes.save_new")).strong());
                egui::Grid::new("brush_save").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(tr("brushes.name"));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
                    env.note("brushes:save_name", r.rect);
                    ui.end_row();
                    ui.label(tr("brushes.category"));
                    egui::ComboBox::from_id_salt("brush_save_cat").selected_text(cat_label(&cat)).show_ui(ui, |ui| {
                        for (k, _) in CATEGORIES {
                            ui.selectable_value(&mut cat, k.to_owned(), cat_label(k));
                        }
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("brushes.save")));
                    env.note("brushes:save", ok.rect);
                    let cancel = ui.button(tr("confirm.cancel"));
                    if ok.clicked() {
                        let o = &env.ed.opts;
                        let mut p = o.brush_custom.clone().or_else(|| env.ed.brushes.get(o.brush_preset).cloned()).unwrap_or_else(|| aiai_paint::brush::BrushPreset::round("Pincel", 24.0, 0.9));
                        p.size = o.brush_size;
                        p.opacity = o.brush_opacity;
                        match env.ed.brushes.add_user(p, &name, &cat) {
                            Ok(i) => {
                                env.ed.opts.brush_preset = i;
                                env.ed.opts.brush_custom = None;
                                env.ed.brush_panel.saving = None;
                                env.ed.brush_panel.cat = Some(cat.clone());
                            }
                            Err(e) => env.ed.notify(e),
                        }
                        return;
                    }
                    if cancel.clicked() {
                        env.ed.brush_panel.saving = None;
                        return;
                    }
                    env.ed.brush_panel.saving = Some((name.clone(), cat.clone()));
                });
            });
        }
    }
}

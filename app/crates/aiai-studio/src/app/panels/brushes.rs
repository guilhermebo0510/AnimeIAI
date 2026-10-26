//! Painel Grupo de ferramentas (S25, no espírito do painel de sub-ferramentas do Clip Studio): as abas de grupo da
//! ferramenta escolhida na prateleira (ícone e nome curto: Caneta | Marcador, Lápis, Pincel...), a lista das
//! sub-ferramentas com o traço de verdade de cada uma e o nome, e no rodapé só ícones: adicionar da Biblioteca,
//! importar (.aiaibrush/.aiaibrushes), duplicar e apagar. O botão direito num pincel renomeia, duplica, exporta, volta ao
//! inicial ou apaga. As propriedades ficam no painel Propriedades da ferramenta (`tool_props`).

use crate::app::brushes::cat_of;
use crate::app::editor::{EraserTip, PenShape, PointsMode};
use crate::app::selection::SelTool;
use crate::app::shelf::{self, Slot, Sub};
use crate::app::ui_kit::{Env, body_ui};
use crate::i18n::tr;
use crate::icons::IconId;
use crate::workspace::ToolId;
use aiai_core::VectorTip;
use aiai_tools::erase::EraseMode;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

#[derive(Default)]
pub(crate) struct BrushPanelState {
    thumbs: Vec<egui::TextureHandle>,
    thumbs_key: (u64, bool),
    /// Miniaturas feitas pelo motor para as penas e as borrachas raster (refeitas com o tema).
    extra: std::collections::HashMap<String, egui::TextureHandle>,
    extra_dark: Option<bool>,
    /// Janelas "Criar pincel", "Importar pincéis" e "Exportar pincel / categoria".
    pub maker: Option<super::brush_maker::Maker>,
    pub importer: Option<super::brush_maker::Importer>,
    pub export: Option<super::brush_maker::ExportDialog>,
    /// Categoria aberta (o criador ainda grava a categoria escolhida ao salvar).
    pub cat: Option<String>,
}

/// Altura de uma linha da lista e proporção fixa da miniatura do traço (5:1, nunca achatada).
const ROW_H: f32 = 30.0;
const THUMB_ASPECT: f32 = 5.0;

/// Como desenhar a miniatura.
#[derive(Clone, Copy)]
enum Thumb {
    Brush(usize),
    Tex(&'static str),
    Pen(PenShape),
    Erase(EraseMode),
    Fill(u8),
    Select(SelTool),
    Points(PointsMode),
    Icon(ToolId),
}

/// O que um pedido do menu ou do rodapé faz depois de desenhar a lista.
enum Req {
    Pick(Sub),
    Duplicate(usize),
    Delete(usize),
    Reset(usize),
    Export(usize),
    ExportCat(String),
    Rename(usize),
    Library,
    Import,
    Maker,
    Rescan,
    OpenDir,
}

fn thumb_of(env: &Env, s: &Sub) -> Thumb {
    match s {
        Sub::Preset(k) | Sub::EraserPreset(k) => shelf::preset_index(env, k).map(Thumb::Brush).unwrap_or(Thumb::Icon(ToolId::Brush)),
        Sub::PenTip(t) => Thumb::Tex(match t {
            VectorTip::GPen => "pen:g",
            VectorTip::MaruPen => "pen:maru",
            VectorTip::Marker => "pen:marker",
            VectorTip::Pencil => "pen:pencil",
        }),
        Sub::PenShape(m) => Thumb::Pen(*m),
        Sub::EraserRaster(i) => Thumb::Tex(if *i == 0 { "erase:hard" } else { "erase:soft" }),
        Sub::EraserTip(EraserTip::Pencil) => Thumb::Tex("erase:pencil"),
        Sub::EraserTip(_) => Thumb::Tex("erase:block"),
        Sub::EraserBrush => Thumb::Brush(env.ed.opts.brush_preset),
        Sub::EraserVector(m) => Thumb::Erase(*m),
        Sub::Fill(i) => Thumb::Fill(*i),
        Sub::Select(m) => Thumb::Select(*m),
        Sub::Points(m) => Thumb::Points(*m),
        Sub::Ruler(_) => Thumb::Icon(ToolId::Ruler),
        Sub::Engine(t) => Thumb::Icon(*t),
    }
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    ensure_thumbs(env, ui);
    let Some(slot) = env.ed.draw_ui.shelf.slot.filter(|_| !shelf::slots_for(env.ws).is_empty()) else {
        ui.label(egui::RichText::new(tr("subtool.none")).color(env.t.text_dim));
        return;
    };
    let t = env.t;
    // ---- abas de grupo: ícone + nome curto
    let cur_tab = shelf::current_tab(env, slot);
    let mut pick_tab = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let tabs = slot.tabs();
        let w = ((ui.available_width() - 3.0 * (tabs.len() as f32 - 1.0)) / tabs.len() as f32).min(150.0);
        for tb in tabs {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
            env.note(format!("subtool:tab:{}", tb.key), rect);
            let on = tb.key == cur_tab.key;
            let p = ui.painter();
            p.rect(rect, 4.0, if on { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
            let ir = Rect::from_min_size(rect.min + egui::vec2(5.0, 4.0), egui::vec2(16.0, 16.0));
            env.icons.paint(p, tb.icon, ir, if on { t.text } else { t.text_dim });
            let g = p.layout_no_wrap(tr(tb.title_key).to_owned(), egui::FontId::proportional(12.0), if on { t.text } else { t.text_dim });
            p.with_clip_rect(rect.shrink(2.0)).galley(egui::pos2(ir.right() + 5.0, rect.center().y - g.size().y / 2.0), g, t.text);
            if resp.on_hover_text(tr(tb.title_key)).clicked() {
                pick_tab = Some(tb.key);
            }
        }
    });
    if let Some(k) = pick_tab {
        env.ed.draw_ui.shelf.tab.insert(slot, k);
    }
    let cur_tab = shelf::current_tab(env, slot);
    ui.add_space(3.0);
    // ---- lista: traço + nome
    let items = shelf::subs(env, slot, cur_tab);
    // rodapé (26) + o respiro antes dele + o espaço entre itens: a lista nunca empurra os ícones para fora do painel
    let footer_h = 36.0;
    let list_h = (ui.available_height() - footer_h).max(ROW_H);
    let mut req: Option<Req> = None;
    let rename = env.ed.draw_ui.rename.clone();
    egui::ScrollArea::vertical().id_salt(("subtool_list", slot.key(), cur_tab.key)).max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if items.is_empty() {
            let msg = if slot.available() { tr("subtool.empty") } else { tr("shelf.soon") };
            ui.label(egui::RichText::new(msg).size(12.0).color(t.text_dim));
        }
        for (n, s) in items.iter().enumerate() {
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROW_H), Sense::click());
            let lib_i = match s {
                Sub::Preset(k) | Sub::EraserPreset(k) => shelf::preset_index(env, k),
                _ => None,
            };
            env.note(match lib_i {
                Some(i) => format!("brushes:item:{i}"),
                None => format!("subtool:item:{n}"),
            }, rect);
            let sel = shelf::is_current(env, s);
            let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            let fill = if sel { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget };
            p.rect(rect, 3.0, fill, Stroke::new(1.0, if sel { t.accent } else { t.border }), StrokeKind::Inside);
            let user = lib_i.is_some_and(|i| env.ed.brushes.items.get(i).is_some_and(|b| b.user));
            // nome à direita (com o selo de vetor e o ponto dos pincéis do usuário); o traço ocupa o resto
            let name = shelf::sub_label(env, s);
            let renaming = rename.as_ref().is_some_and(|(ri, _)| Some(*ri) == lib_i);
            let g = p.layout_no_wrap(name.clone(), egui::FontId::proportional(12.0), t.text);
            let name_w = g.size().x.min(w * 0.5);
            let badge = if s.is_vector() { 14.0 } else { 0.0 };
            let thumb_r = Rect::from_min_max(egui::pos2(rect.left() + 4.0, rect.top() + 4.0), egui::pos2((rect.right() - name_w - badge - 14.0).max(rect.left() + 40.0), rect.bottom() - 4.0));
            paint_thumb(env, &p, thumb_r, thumb_of(env, s));
            if !renaming {
                let x = rect.right() - 6.0 - name_w;
                p.with_clip_rect(Rect::from_min_max(egui::pos2(x, rect.top()), rect.max)).galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, t.text);
                if s.is_vector() {
                    env.icons.paint(&p, IconId::LayerVector, Rect::from_center_size(egui::pos2(x - 9.0, rect.center().y), egui::vec2(12.0, 12.0)), t.text_dim);
                }
                if user {
                    p.circle_filled(egui::pos2(rect.right() - 3.5, rect.top() + 4.0), 1.8, t.accent);
                }
            }
            let resp = match lib_i {
                Some(i) => {
                    let lib = &env.ed.brushes;
                    resp.on_hover_ui(|ui| brush_tooltip(ui, lib, i))
                }
                None if s.is_vector() => resp.on_hover_text(format!("{name}\n{}", tr("subtool.vector.tip"))),
                None => resp.on_hover_text(name.clone()),
            };
            if resp.clicked() {
                req = Some(Req::Pick(s.clone()));
            }
            if let Some(i) = lib_i {
                if renaming {
                    let mut text = rename.as_ref().map(|r| r.1.clone()).unwrap_or_default();
                    let er = Rect::from_min_max(egui::pos2(rect.right() - w * 0.5, rect.top() + 3.0), egui::pos2(rect.right() - 3.0, rect.bottom() - 3.0));
                    let te = ui.put(er, egui::TextEdit::singleline(&mut text).font(egui::FontId::proportional(12.0)));
                    env.note("subtool:rename", te.rect);
                    te.request_focus();
                    if te.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            keep_selection(env, |env| env.ed.brushes.rename_user(i, &text).map(|_| ()));
                        }
                        env.ed.draw_ui.rename = None;
                    } else {
                        env.ed.draw_ui.rename = Some((i, text));
                    }
                }
                let cat_user = user;
                let cat = env.ed.brushes.get(i).map(cat_of).unwrap_or_default();
                let cat_has_user = env.ed.brushes.category_has_user(&cat);
                resp.context_menu(|ui| {
                    let mut item = |ui: &mut Ui, key: &str, on: bool, r: Req| {
                        if ui.add_enabled(on, egui::Button::new(tr(key))).on_disabled_hover_text(tr("subtool.factory.tip")).clicked() {
                            req = Some(r);
                            ui.close();
                        }
                    };
                    item(ui, "subtool.rename", cat_user, Req::Rename(i));
                    item(ui, "subtool.duplicate", true, Req::Duplicate(i));
                    item(ui, "subtool.export", cat_user, Req::Export(i));
                    item(ui, "subtool.export_cat", cat_has_user, Req::ExportCat(cat.clone()));
                    item(ui, "subtool.reset", true, Req::Reset(i));
                    ui.separator();
                    item(ui, "subtool.delete", cat_user, Req::Delete(i));
                });
            }
        }
    });
    // ---- rodapé: só ícones
    let cur_lib = match shelf::current_sub(env) {
        Some(Sub::Preset(k)) | Some(Sub::EraserPreset(k)) => shelf::preset_index(env, &k),
        _ => None,
    };
    let cur_user = cur_lib.is_some_and(|i| env.ed.brushes.items.get(i).is_some_and(|b| b.user));
    let brushy = cur_tab.group.is_some();
    ui.add_space(2.0);
    let (fr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(fr).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let more = env.icon_button(ui, IconId::ChevronDown, "subtool.more", false, true, "subtool:more");
        egui::Popup::menu(&more).show(|ui| {
            for (key, r) in [("subtool.maker", Req::Maker), ("subtool.rescan", Req::Rescan), ("subtool.open_dir", Req::OpenDir)] {
                let on = !matches!(r, Req::Rescan | Req::OpenDir) || env.ed.brushes.dir().is_some();
                if ui.add_enabled(on, egui::Button::new(tr(key))).clicked() {
                    req = Some(r);
                }
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if env.icon_button(ui, IconId::Trash, "subtool.delete", false, cur_user, "subtool:delete").clicked() {
                req = cur_lib.map(Req::Delete);
            }
            if env.icon_button(ui, IconId::Copy, "subtool.duplicate", false, cur_lib.is_some(), "subtool:duplicate").clicked() {
                req = cur_lib.map(Req::Duplicate);
            }
            if env.icon_button(ui, IconId::Import, "subtool.import", false, brushy, "subtool:import").clicked() {
                req = Some(Req::Import);
            }
            if env.icon_button(ui, IconId::Plus, "subtool.add", false, brushy, "subtool:add").clicked() {
                req = Some(Req::Library);
            }
        });
    });
    if let Some(r) = req {
        run(env, ui.ctx(), slot, r);
    }
    super::brush_maker::windows(env, ui.ctx());
}

fn run(env: &mut Env, ctx: &egui::Context, slot: Slot, r: Req) {
    match r {
        Req::Pick(s) => shelf::apply_sub(env, slot, &s),
        Req::Duplicate(i) => duplicate(env, slot, i),
        Req::Delete(i) => {
            keep_selection(env, |env| env.ed.brushes.remove_user(i));
            // a ferramenta volta para a primeira sub-ferramenta que sobrou
            env.ed.draw_ui.shelf.last.remove(&slot);
            shelf::activate(env, slot);
        }
        Req::Reset(i) => reset_preset(env, i),
        Req::Export(i) => super::brush_maker::open_export(env, super::brush_maker::ExportTarget::Brush(i)),
        Req::ExportCat(c) => super::brush_maker::open_export(env, super::brush_maker::ExportTarget::Category(c)),
        Req::Rename(i) => env.ed.draw_ui.rename = env.ed.brushes.get(i).map(|p| (i, p.name.clone())),
        Req::Library => {
            env.ed.library.kind = Some(aiai_assets::kind::Kind::Brush);
            env.ed.draw_ui.insert_panel = Some(crate::layout::PanelKind::Library);
        }
        Req::Import => super::brush_maker::pick_import(env, ctx),
        Req::Maker => super::brush_maker::open_maker(env, None),
        Req::Rescan => rescan(env),
        Req::OpenDir => {
            if let Some(d) = env.ed.brushes.dir() {
                let _ = std::fs::create_dir_all(d);
                let _ = std::process::Command::new("explorer").arg(d).spawn();
            }
        }
    }
}

/// Duplica o pincel (com o ajuste em uso, se é ele) como pincel do usuário e passa a usar a cópia.
fn duplicate(env: &mut Env, slot: Slot, i: usize) {
    let o = &env.ed.opts;
    let Some(mut p) = env.ed.brushes.get(i).cloned() else { return };
    if o.brush_preset == i {
        if let Some(c) = o.brush_custom.clone() {
            p = c;
        }
        p.size = o.brush_size;
        p.opacity = o.brush_opacity;
    }
    let name = format!("{} 2", p.name);
    let cat = match cat_of(&p).as_str() {
        "importados" | "" => "meus".to_owned(),
        c => c.to_owned(),
    };
    let cur = env.ed.brushes.get(env.ed.opts.brush_preset).map(shelf::preset_key);
    match env.ed.brushes.add_user(p, &name, &cat) {
        Ok(n) => {
            if let Some(i) = cur.and_then(|k| shelf::preset_index(env, &k)) {
                env.ed.opts.brush_preset = i;
            }
            if let Some(k) = env.ed.brushes.get(n).map(shelf::preset_key) {
                let sub = if slot == Slot::Eraser { Sub::EraserPreset(k) } else { Sub::Preset(k) };
                shelf::apply_sub(env, slot, &sub);
            }
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Volta o pincel ao inicial: esquece o tamanho, a opacidade e os ajustes finos feitos nele.
pub(crate) fn reset_preset(env: &mut Env, i: usize) {
    let Some(p) = env.ed.brushes.get(i).cloned() else { return };
    env.ed.draw_ui.shelf.tuned.remove(&shelf::preset_key(&p));
    if env.ed.opts.brush_preset == i {
        let o = &mut env.ed.opts;
        (o.brush_size, o.brush_opacity, o.brush_custom) = (p.size, p.opacity, None);
    }
}

/// Mexe na biblioteca (apagar, renomear) sem perder o pincel em uso (os índices andam); o pincel que sumiu dá lugar ao
/// primeiro de fábrica que sobrou.
fn keep_selection(env: &mut Env, f: impl FnOnce(&mut Env) -> Result<(), String>) {
    let cur = env.ed.brushes.get(env.ed.opts.brush_preset).map(shelf::preset_key);
    if let Err(e) = f(env) {
        env.ed.notify(e);
    }
    match cur.and_then(|k| shelf::preset_index(env, &k)) {
        Some(i) => env.ed.opts.brush_preset = i,
        None => {
            env.ed.opts.brush_preset = 1.min(env.ed.brushes.items.len().saturating_sub(1));
            env.ed.opts.brush_custom = None;
        }
    }
}

/// Relê a pasta de pincéis e mantém o pincel escolhido (os índices mudam).
fn rescan(env: &mut Env) {
    let cur = env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| (p.id.clone(), p.name.clone()));
    let n = env.ed.brushes.rescan();
    let bad = env.ed.brushes.problems.len();
    if let Some((id, name)) = cur {
        env.ed.opts.brush_preset = env.ed.brushes.index_of(&id, &name).unwrap_or(1);
    }
    if env.ed.opts.brush_preset >= env.ed.brushes.items.len() {
        env.ed.opts.brush_preset = 1;
    }
    env.ed.opts.brush_custom = None;
    let mut msg = tr("brushes.rescan.done").replace("{n}", &n.to_string()).replace("{bad}", &bad.to_string());
    // quais arquivos deram problema (os primeiros); o motivo de cada um está no registro (log)
    let names: Vec<String> = env.ed.brushes.problems.iter().take(3).filter_map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    if !names.is_empty() {
        msg = format!("{msg}: {}", names.join(", "));
    }
    env.ed.notify(msg);
    // o aviso detalhado só vale na abertura; aqui o resumo acima basta
    let _ = env.ed.brushes.take_warning();
}

/// O que a dica do cartão do pincel mostra: nome, autor, licença, fonte, versão e etiquetas.
fn brush_tooltip(ui: &mut Ui, lib: &crate::app::brushes::BrushLibrary, i: usize) {
    let Some(b) = lib.items.get(i) else { return };
    let p = &b.preset;
    ui.label(egui::RichText::new(&p.name).strong());
    if !b.user {
        ui.label(tr("brush.tip.factory"));
        return;
    }
    let row = |ui: &mut Ui, key: &str, v: String| {
        if !v.trim().is_empty() {
            ui.label(format!("{}: {v}", tr(key)));
        }
    };
    row(ui, "brushes.author", [p.author.name.clone(), p.author.email.clone(), p.author.url.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · "));
    row(ui, "brushes.license", p.license.clone());
    let imported = b.manifest.as_ref().map(|m| m.source.imported_from.clone()).filter(|f| !f.is_empty());
    let source = match imported {
        Some(f) if p.source.is_empty() => f,
        Some(f) => format!("{} ({f})", p.source),
        None => p.source.clone(),
    };
    row(ui, "brushes.source", source);
    row(ui, "brush.tip.version", p.version.clone());
    row(ui, "brushes.tags", p.tags.join(", "));
}

// ------------------------------------------------------------------------------------------------ miniaturas

fn ensure_thumbs(env: &mut Env, ui: &Ui) {
    let t = env.t;
    let dark = ui.ctx().theme() == egui::Theme::Dark;
    let key = (env.ed.brushes.version, dark);
    if env.ed.brush_panel.thumbs_key != key || env.ed.brush_panel.thumbs.len() != env.ed.brushes.items.len() {
        let presets: Vec<_> = env.ed.brushes.items.iter().map(|b| env.ed.brushes.with_masks(&b.preset)).collect();
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
    // borrachas raster: uma faixa da cor do texto apagada pelo próprio preset da borracha (dura, macia, lápis, bloco)
    let eraser = |tip: EraserTip, hardness: f32| {
        let o = crate::app::editor::ToolOptions { eraser_size: 26.0, eraser_hardness: hardness, eraser_tip: tip, ..Default::default() };
        crate::app::tools::eraser_preset(&o, None)
    };
    let kinds = [("erase:hard", eraser(EraserTip::Brush, shelf::ERASERS[0].1)), ("erase:soft", eraser(EraserTip::Brush, shelf::ERASERS[1].1)), ("erase:pencil", eraser(EraserTip::Pencil, 1.0)), ("erase:block", eraser(EraserTip::Block, 1.0))];
    for (name, p) in kinds.iter() {
        let mut img = aiai_paint::Image::new(w, h);
        for y in 12..44 {
            for x in 10..w - 10 {
                let k = img.idx(x, y);
                img.data[k] = ink;
            }
        }
        let path: Vec<aiai_paint::brush::BrushPoint> = (0..40)
            .map(|k| {
                let f = k as f32 / 39.0;
                aiai_paint::brush::BrushPoint { x: 24.0 + f * (w as f32 - 48.0), y: h as f32 / 2.0 + 9.0 * (f * std::f32::consts::TAU).sin(), pressure: 1.0 }
            })
            .collect();
        aiai_paint::brush::paint_stroke(&mut img, p, &path, [0.0, 0.0, 0.0, 1.0], 3);
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
                SelTool::Rect => {
                    let rr = Rect::from_min_max(at(0.25, 0.15), at(0.75, 0.85));
                    let pts = vec![rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
                    p.extend(egui::Shape::dashed_line(&pts, dash, 4.0, 3.0));
                }
                SelTool::Ellipse => {
                    p.extend(egui::Shape::dashed_line(&ellipse(r.center()), dash, 4.0, 3.0));
                }
                SelTool::Polygon => {
                    let pts: Vec<Pos2> = [(0.2, 0.75), (0.32, 0.18), (0.62, 0.32), (0.82, 0.15), (0.74, 0.82), (0.2, 0.75)].iter().map(|(x, y)| at(*x, *y)).collect();
                    p.extend(egui::Shape::dashed_line(&pts, dash, 4.0, 3.0));
                    for q in &pts[..5] {
                        p.circle_filled(*q, 1.8, t.accent);
                    }
                }
                SelTool::Strokes => {
                    // dois traços; o de cima escolhido (destacado), com a seta do cursor
                    let a: Vec<Pos2> = (0..=16).map(|k| { let f = k as f32 / 16.0; at(0.12 + 0.6 * f, 0.62 - 0.3 * (f * std::f32::consts::PI).sin()) }).collect();
                    let b: Vec<Pos2> = (0..=16).map(|k| { let f = k as f32 / 16.0; at(0.15 + 0.6 * f, 0.85 - 0.12 * (f * std::f32::consts::PI).sin()) }).collect();
                    p.add(egui::Shape::line(a.clone(), Stroke::new(3.2, t.accent.gamma_multiply(0.6))));
                    p.add(egui::Shape::line(a, Stroke::new(1.4, ink.color)));
                    p.add(egui::Shape::line(b, Stroke::new(1.4, ink.color)));
                    let c = at(0.78, 0.3);
                    p.add(egui::Shape::convex_polygon(vec![c, c + Vec2::new(0.0, 11.0), c + Vec2::new(3.0, 8.0), c + Vec2::new(7.5, 8.0)], t.text, Stroke::NONE));
                }
                SelTool::Lasso => {
                    let pts: Vec<Pos2> = [(0.2, 0.5), (0.35, 0.15), (0.6, 0.25), (0.8, 0.2), (0.75, 0.6), (0.55, 0.85), (0.3, 0.75), (0.2, 0.5)].iter().map(|(x, y)| at(*x, *y)).collect();
                    p.extend(egui::Shape::dashed_line(&pts, dash, 4.0, 3.0));
                }
                SelTool::Wand => {
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


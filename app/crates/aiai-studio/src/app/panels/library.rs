//! Painéis da Biblioteca de recursos (S24 2.7–2.8): `Biblioteca` (árvore de tipos e etiquetas à esquerda; busca, ordem,
//! tamanho e grade de miniaturas à direita; importar e salvar do projeto) e `Prévia do recurso` (turntable do modelo 3D,
//! teste de repetição 3×3 da textura, amostras da paleta; ações e edição de nome, etiquetas, autor, licença e fonte).
//! Valem em qualquer workspace (Exibir > Painéis). Os dados e as ações com o documento ficam em `app/library.rs`.

use crate::app::library::{self, LibDrag, ModelState};
use crate::app::ui_kit::{Env, body_ui, dim_label, paint_icon, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_assets::index::{Entry, Sort};
use aiai_assets::kind::{IMAGE_EXTS, ext_of};
use aiai_assets::palette::Palette;
use aiai_assets::store::MetaEdit;
use aiai_assets::{Kind, store};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::path::PathBuf;

/// Extensões de pincel (o importador de pincéis cuida delas, com a licença à vista antes de instalar).
const BRUSH_EXTS: [&str; 9] = ["aiaibrush", "aiaibrushes", "abr", "brush", "brushset", "bundle", "gbr", "gih", "sut"];

/// Modelos de licença da lista do campo Licença.
/// Lado da célula das amostras da paleta na Prévia.
const PAL_CELL: f32 = 46.0;

const LICENSES: [&str; 8] = ["CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "CC-BY-NC-4.0", "MIT", "Apache-2.0", aiai_assets::envelope::DEFAULT_LICENSE, aiai_assets::envelope::UNKNOWN_LICENSE];

pub(crate) fn kind_icon(k: Kind) -> IconId {
    match k {
        Kind::Model3d => IconId::LayerModel3d,
        Kind::Material => IconId::LayerFill,
        Kind::Texture => IconId::LayerRaster,
        Kind::Palette => IconId::ToolEyedropper,
        Kind::Pose => IconId::NodeCharacter,
        Kind::Brush => IconId::ToolBrush,
    }
}

fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Model3d => tr("lib.kind.model3d"),
        Kind::Material => tr("lib.kind.material"),
        Kind::Texture => tr("lib.kind.texture"),
        Kind::Palette => tr("lib.kind.palette"),
        Kind::Pose => tr("lib.kind.pose"),
        Kind::Brush => tr("lib.kind.brush"),
    }
}

/// Milhares com ponto (12.345).
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn human_bytes(b: u64) -> String {
    match b {
        0..=1023 => format!("{b} B"),
        1024..=1_048_575 => format!("{:.1} KB", b as f64 / 1024.0),
        _ => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

/// Texto de uma linha que termina em reticências quando não cabe na largura.
fn elided(p: &egui::Painter, text: &str, font: FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    p.layout_job(job)
}

/// Linha curta da célula da grade (a Prévia mostra a completa).
fn stats_short(e: &Entry) -> String {
    match (e.kind, e.stats.triangles) {
        (Kind::Model3d, Some(t)) => format!("{} {}", thousands(t), tr("lib.tris_short")),
        _ => stats_line(e),
    }
}

/// Segunda linha da célula: o que o tipo tem de mais útil de saber à primeira vista.
fn stats_line(e: &Entry) -> String {
    let s = &e.stats;
    match e.kind {
        Kind::Model3d => match (s.triangles, s.size_m) {
            (Some(t), Some(d)) => format!("{} {} · {:.1}×{:.1}×{:.1} m", thousands(t), tr("lib.tris"), d[0], d[1], d[2]),
            (Some(t), None) => format!("{} {}", thousands(t), tr("lib.tris")),
            _ => human_bytes(e.bytes),
        },
        Kind::Texture | Kind::Material => match (s.width, s.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => kind_label(e.kind).to_owned(),
        },
        Kind::Palette => format!("{} {}", s.colors.unwrap_or(0), tr("lib.colors")),
        Kind::Pose | Kind::Brush => kind_label(e.kind).to_owned(),
    }
}

fn license_missing(e: &Entry) -> bool {
    let l = e.env.license_text();
    l.trim().is_empty() || l.trim() == aiai_assets::envelope::UNKNOWN_LICENSE
}

fn hex_color(h: &str) -> Option<Color32> {
    aiai_assets::palette::parse_hex(h).map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
}

pub(crate) fn palette_of(e: &Entry) -> Palette {
    Palette::from_payload(&e.env.payload).unwrap_or_default()
}

/// Material toon (cores no payload, sem imagem)?
pub(crate) fn is_toon_material(e: &Entry) -> bool {
    e.kind == Kind::Material && !IMAGE_EXTS.contains(&ext_of(&e.path).as_str())
}

// ================================================================================================ painel Biblioteca

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let ctx = ui.ctx().clone();
    let bv = env.ed.brushes.version;
    env.ed.library.sync(&ctx, bv);
    if std::mem::take(&mut env.ed.library.brush_rescan) {
        // a pasta de pincéis mudou por aqui: o painel Pincéis relê, mantendo o pincel escolhido
        let keep = env.ed.brushes.get(env.ed.opts.brush_preset).map(|p| (p.id.clone(), p.name.clone()));
        env.ed.brushes.rescan();
        if let Some((id, name)) = keep {
            if let Some(i) = env.ed.brushes.index_of(&id, &name) {
                env.ed.opts.brush_preset = i;
            }
        }
    }
    env.ed.library.drop_rect = Some((body, ctx.cumulative_pass_nr()));
    super::brush_maker::windows(env, &ctx);
    take_brush_files(env, &ctx);
    import_window(env, &ctx);

    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    if !env.ed.library.enabled() {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tr("lib.disabled")).size(12.5).color(t.text_dim));
        return;
    }
    let area = ui.max_rect();
    let left_w = (area.width() * 0.27).clamp(112.0, 176.0);
    let left = Rect::from_min_size(area.min, vec2(left_w, area.height()));
    let right = Rect::from_min_max(pos2(area.min.x + left_w + 8.0, area.min.y), area.max);
    let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left));
    lui.set_clip_rect(left);
    tree(env, &mut lui);
    ui.painter().vline(left.right() + 4.0, area.y_range(), Stroke::new(1.0, t.border));
    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right));
    rui.set_clip_rect(right);
    grid_side(env, &mut rui, right);
}

/// Arquivos de pincel que chegaram (diálogo ou arrastados do sistema) vão para o importador de pincéis.
fn take_brush_files(env: &mut Env, ctx: &egui::Context) {
    let Some(dlg) = env.ed.library.import.as_mut() else { return };
    let (brush, rest): (Vec<PathBuf>, Vec<PathBuf>) = std::mem::take(&mut dlg.files).into_iter().partition(|f| BRUSH_EXTS.contains(&ext_of(f).as_str()));
    dlg.files = rest;
    if dlg.files.is_empty() {
        env.ed.library.import = None;
    }
    if !brush.is_empty() {
        super::brush_maker::import_files(env, ctx, brush);
    }
}

fn tree(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    egui::ScrollArea::vertical().id_salt("lib_tree").auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &t, "lib.kinds");
        let total = env.ed.library.index.count(None);
        let rows: Vec<(Option<Kind>, usize)> = std::iter::once((None, total)).chain(Kind::ALL.iter().map(|k| (Some(*k), env.ed.library.index.count(Some(*k))))).collect();
        for (k, n) in rows {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            env.note(format!("lib:kind:{}", k.map(Kind::key).unwrap_or("all")), rect);
            let on = env.ed.library.kind == k;
            if on {
                ui.painter().rect_filled(rect, 4.0, t.accent_dim);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, t.bg_hover);
            }
            let icon = k.map(kind_icon).unwrap_or(IconId::Grid);
            paint_icon(env.icons, ui.painter(), icon, Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(16.0, 16.0)), if on { t.text } else { t.text_dim }, false);
            let label = k.map(kind_label).unwrap_or(tr("lib.kind.all"));
            ui.painter().text(pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), t.text);
            ui.painter().text(pos2(rect.max.x - 6.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), FontId::proportional(11.5), t.text_dim);
            if resp.clicked() {
                env.ed.library.kind = k;
                env.ed.library.tag = None;
            }
        }
        section(ui, &t, "lib.tags");
        let tags = env.ed.library.index.tag_counts(env.ed.library.kind);
        if tags.is_empty() {
            dim_label(ui, &t, tr("lib.no_tags"));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (tag, n) in tags.into_iter().take(80) {
                let on = env.ed.library.tag.as_deref().is_some_and(|x| aiai_assets::envelope::fold(x) == aiai_assets::envelope::fold(&tag));
                let r = ui.selectable_label(on, format!("{tag} {n}"));
                env.note(format!("lib:tag:{tag}"), r.rect);
                if r.clicked() {
                    env.ed.library.tag = if on { None } else { Some(tag) };
                }
            }
        });
        ui.add_space(8.0);
        if let Some(root) = env.ed.library.root().map(|p| p.to_path_buf()) {
            dim_label(ui, &t, tr("lib.folder"));
            ui.label(egui::RichText::new(root.display().to_string()).size(10.5).color(t.text_dim));
            let b = ui.button(tr("lib.open_folder"));
            env.note("lib:open_folder", b.rect);
            if b.clicked() {
                library::open_folder(&root);
            }
        }
    });
}

fn grid_side(env: &mut Env, ui: &mut Ui, area: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let q = env.ed.library.query();
    let hits: Vec<Entry> = env.ed.library.index.search(&q).into_iter().cloned().collect();
    let total = env.ed.library.index.count(None);
    // ---- barra numa linha só (quebra quando a largura não dá): busca, ordem, tamanho, importar, salvar, reler, contagem
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
        let mut q = env.ed.library.query.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(tr("lib.search_hint")).desired_width((area.width() - 8.0).clamp(80.0, 180.0)));
        env.note("lib:search", r.rect);
        env.ed.library.query = q;
        let cur = match env.ed.library.sort {
            Sort::Name => tr("lib.sort.name"),
            Sort::Newest => tr("lib.sort.newest"),
            Sort::Size => tr("lib.sort.size"),
            Sort::Kind => tr("lib.sort.kind"),
        };
        let c = egui::ComboBox::from_id_salt("lib_sort").width(104.0).selected_text(cur).show_ui(ui, |ui| {
            for (s, k) in [(Sort::Name, "lib.sort.name"), (Sort::Newest, "lib.sort.newest"), (Sort::Size, "lib.sort.size"), (Sort::Kind, "lib.sort.kind")] {
                if ui.selectable_label(env.ed.library.sort == s, tr(k)).clicked() {
                    env.ed.library.sort = s;
                }
            }
        });
        env.note("lib:sort", c.response.rect);
        let mut size = env.ed.library.size;
        let sl = ui.add(egui::Slider::new(&mut size, 64.0..=192.0).show_value(false).step_by(4.0));
        env.note("lib:size", sl.rect);
        sl.on_hover_text(tr("lib.size"));
        env.ed.library.size = size;
        let b = ui.button(tr("lib.import"));
        env.note("lib:import", b.rect);
        if b.on_hover_text(tr("lib.import.tip")).clicked() {
            pick_files(env);
        }
        save_menu(env, ui);
        let r = ui.button(tr("lib.rescan"));
        env.note("lib:rescan", r.rect);
        if r.on_hover_text(tr("lib.rescan.tip")).clicked() {
            env.ed.library.rescan();
        }
        if env.ed.library.busy > 0 {
            ui.spinner();
        }
        let count = if hits.len() == total { tr("lib.count").replace("{n}", &total.to_string()) } else { tr("lib.count_filtered").replace("{n}", &hits.len().to_string()).replace("{total}", &total.to_string()) };
        dim_label(ui, &t, &count);
    });
    if let Some(m) = env.ed.library.msg.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&m).size(11.5).color(t.text_dim));
            if ui.small_button("x").clicked() {
                env.ed.library.msg = None;
            }
        });
    }
    problems(env, ui);
    if hits.is_empty() {
        ui.add_space(8.0);
        let key = if total == 0 { "lib.empty_folder" } else { "lib.no_match" };
        ui.label(egui::RichText::new(tr(key)).size(12.5).color(t.text_dim));
        return;
    }
    // ---- grade
    let side = env.ed.library.size;
    let (cell_w, cell_h) = (side + 12.0, side + 12.0 + 32.0);
    let avail = area.width() - 16.0;
    let cols = ((avail / cell_w).floor() as usize).max(1);
    let rows = hits.len().div_ceil(cols);
    env.ed.library.begin_visible();
    let top = ui.cursor().min.y;
    let _ = (area, top);
    egui::ScrollArea::vertical().id_salt("lib_grid").auto_shrink([false, false]).show_rows(ui, cell_h, rows, |ui, range| {
        env.note("lib:grid", ui.max_rect());
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for e in hits.iter().skip(row * cols).take(cols) {
                    cell(env, ui, &ctx, e, vec2(cell_w, cell_h), side);
                }
            });
        }
    });
}

fn problems(env: &mut Env, ui: &mut Ui) {
    let n = env.ed.library.index.problems.len();
    if n == 0 {
        return;
    }
    let t = env.t;
    let label = tr("lib.problems").replace("{n}", &n.to_string());
    let r = ui.add(egui::Label::new(egui::RichText::new(label).size(11.5).color(t.danger)).sense(Sense::click()));
    env.note("lib:problems", r.rect);
    if r.clicked() {
        env.ed.library.show_problems = !env.ed.library.show_problems;
    }
    if env.ed.library.show_problems {
        for p in env.ed.library.index.problems.iter().take(30) {
            let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ui.label(egui::RichText::new(format!("{name}: {}", p.reason)).size(10.5).color(t.text_dim)).on_hover_text(p.path.display().to_string());
        }
    }
}

/// "Salvar do projeto": paletas com cores e modelos 3D que o projeto já tem.
fn save_menu(env: &mut Env, ui: &mut Ui) {
    use aiai_core::NodeKind;
    let mut pals: Vec<(aiai_core::Id, String)> = Vec::new();
    let mut models: Vec<(aiai_core::Id, String)> = Vec::new();
    // paleta do figurino: o nome diz de quem é
    let mut outfit: std::collections::HashMap<aiai_core::Id, String> = std::collections::HashMap::new();
    for (_, n) in &env.core.project.nodes {
        if let NodeKind::Character(c) = &n.data {
            for m in &c.materials {
                if let Some(p) = &m.palette {
                    outfit.insert(p.clone(), format!("{} · {}", n.name, m.name));
                }
            }
        }
    }
    for (id, n) in &env.core.project.nodes {
        match &n.data {
            NodeKind::Palette(p) if !p.colors.is_empty() => pals.push((id.clone(), outfit.get(id).map(|o| if n.name.contains(o.as_str()) { n.name.clone() } else { format!("{} ({})", n.name, o) }).unwrap_or_else(|| n.name.clone()))),
            NodeKind::Asset(a) if a.asset == aiai_core::AssetType::Model3d => models.push((id.clone(), n.name.clone())),
            _ => {}
        }
    }
    let r = ui.menu_button(tr("lib.save_from_project"), |ui| {
        if pals.is_empty() && models.is_empty() {
            ui.label(tr("lib.save.none"));
        }
        if !pals.is_empty() {
            ui.label(egui::RichText::new(tr("lib.save.palettes")).size(11.0).color(env.t.text_dim));
            for (id, name) in &pals {
                if ui.button(name).clicked() {
                    library::save_from_project(env, "palette", id);
                    ui.close();
                }
            }
        }
        if !models.is_empty() {
            ui.label(egui::RichText::new(tr("lib.save.models")).size(11.0).color(env.t.text_dim));
            for (id, name) in &models {
                if ui.button(name).clicked() {
                    library::save_from_project(env, "model", id);
                    ui.close();
                }
            }
        }
    });
    env.note("lib:save_menu", r.response.rect);
    r.response.on_hover_text(tr("lib.save_from_project.tip"));
}

fn pick_files(env: &mut Env) {
    let all: Vec<&str> = ["glb", "gltf", "obj", "png", "jpg", "jpeg", "webp", "bmp", "gpl", "ase", "aiaipal", "json", "aiaipose", "aiaimat"].into_iter().chain(BRUSH_EXTS).collect();
    let files = rfd::FileDialog::new()
        .set_title(tr("lib.import_title"))
        .add_filter(tr("lib.import.filter_all"), &all)
        .add_filter(tr("lib.kind.model3d"), &["glb", "gltf", "obj"])
        .add_filter(tr("lib.import.filter_images"), &["png", "jpg", "jpeg", "webp", "bmp"])
        .add_filter(tr("lib.kind.palette"), &["gpl", "ase", "aiaipal", "json"])
        .add_filter(tr("lib.kind.brush"), &BRUSH_EXTS)
        .pick_files()
        .unwrap_or_default();
    env.ed.library.begin_import(files);
}

/// Uma célula da grade: miniatura, nome e a linha de números; clique seleciona, duplo clique usa, arrastar leva ao canvas.
fn cell(env: &mut Env, ui: &mut Ui, ctx: &egui::Context, e: &Entry, size: egui::Vec2, side: f32) {
    let t = env.t;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return;
    }
    env.note(format!("lib:item:{}", e.id), rect);
    let on = env.ed.library.selected.as_deref() == Some(e.id.as_str());
    let p = ui.painter();
    if on {
        p.rect(rect.shrink(1.0), 6.0, t.accent_dim, Stroke::new(1.5, t.accent), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 6.0, t.bg_hover);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(side, side));
    p.rect_filled(thumb, 4.0, t.bg_widget);
    let tex = env.ed.library.thumb(ctx, e);
    match (&tex, e.kind) {
        (Some(tx), _) => {
            let s = tx.size_vec2();
            let k = (thumb.width() / s.x).min(thumb.height() / s.y);
            let r = Rect::from_center_size(thumb.center(), s * k);
            p.image(tx.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (None, Kind::Palette) => swatches(p, thumb, &palette_of(e).swatches()),
        (None, Kind::Material) if is_toon_material(e) => toon_ball(p, thumb, e),
        (None, k) => {
            let icon = Rect::from_center_size(thumb.center(), vec2(side * 0.4, side * 0.4));
            paint_icon(env.icons, p, kind_icon(k), icon, if env.ed.library.thumb_pending(e) { t.text_dim.gamma_multiply(0.5) } else { t.text_dim }, false);
        }
    }
    // selo do tipo (no "Todos" ajuda a distinguir) e aviso de licença
    if env.ed.library.kind.is_none() {
        let b = Rect::from_min_size(thumb.min + vec2(3.0, 3.0), vec2(14.0, 14.0));
        p.rect_filled(b.expand(2.0), 3.0, t.bg_panel.gamma_multiply(0.85));
        paint_icon(env.icons, p, kind_icon(e.kind), b, t.text_dim, false);
    }
    if license_missing(e) && e.kind != Kind::Pose {
        let c = pos2(thumb.max.x - 8.0, thumb.min.y + 8.0);
        p.circle_filled(c, 5.0, t.st_review);
        p.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(9.0), t.text);
    }
    let name_pos = pos2(rect.min.x + 6.0, thumb.max.y + 4.0);
    let label_clip = Rect::from_min_size(name_pos, vec2(rect.width() - 10.0, 30.0));
    let lp = p.with_clip_rect(label_clip);
    let w = rect.width() - 12.0;
    lp.galley(name_pos, elided(&lp, &e.env.name, FontId::proportional(11.5), t.text, w), t.text);
    lp.galley(name_pos + vec2(0.0, 14.0), elided(&lp, &stats_short(e), FontId::proportional(10.0), t.text_dim, w), t.text_dim);

    // interação
    let resp = resp.on_hover_text(format!("{}\n{}", e.env.name, e.path.display()));
    if resp.clicked() {
        env.ed.library.select(Some(e.id.clone()));
    }
    if resp.double_clicked() {
        env.ed.library.select(Some(e.id.clone()));
        library::use_entry(env, &e.id, true);
    }
    if resp.drag_started() {
        env.ed.library.select(Some(e.id.clone()));
        egui::DragAndDrop::set_payload(ctx, LibDrag { id: e.id.clone() });
    }
    if resp.dragged() {
        if let Some(pt) = ctx.pointer_interact_pos() {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("lib_drag")));
            let g = painter.layout_no_wrap(format!("+ {}", e.env.name), FontId::proportional(12.5), t.text);
            let r = Rect::from_min_size(pt + vec2(14.0, 6.0), g.size() + vec2(10.0, 6.0));
            painter.rect_filled(r, 4.0, t.bg_panel);
            painter.rect_stroke(r, 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            painter.galley(r.min + vec2(5.0, 3.0), g, t.text);
        }
    }
    let id = e.id.clone();
    resp.context_menu(|ui| entry_menu(env, ui, &id));
}

fn entry_menu(env: &mut Env, ui: &mut Ui, id: &str) {
    let Some(e) = env.ed.library.index.get(id).cloned() else { return };
    env.ed.library.select(Some(id.to_owned()));
    use_buttons(env, ui, &e);
    ui.separator();
    manage_buttons(env, ui, &e);
    ui.close();
}

pub(crate) fn swatches(p: &egui::Painter, area: Rect, colors: &[[u8; 4]]) {
    let n = colors.len().min(16);
    if n == 0 {
        return;
    }
    let cols = if n <= 4 { n } else if n <= 9 { 3.max(n.div_ceil(3)).min(4) } else { 4 };
    let rows = n.div_ceil(cols);
    let (w, h) = (area.width() / cols as f32, area.height() / rows as f32);
    for (i, c) in colors.iter().take(n).enumerate() {
        let r = Rect::from_min_size(area.min + vec2((i % cols) as f32 * w, (i / cols) as f32 * h), vec2(w, h)).shrink(1.0);
        p.rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
    }
}

/// Esfera toon (base + sombra) para o material de cores.
pub(crate) fn toon_ball(p: &egui::Painter, area: Rect, e: &Entry) {
    let col = |k: &str, d: Color32| e.env.payload.get(k).and_then(|v| v.as_str()).and_then(hex_color).unwrap_or(d);
    let base = col("base_color", Color32::from_gray(190));
    let shade = col("shade_color", base.gamma_multiply(0.55));
    let c = area.center();
    let r = area.width().min(area.height()) * 0.36;
    p.circle_filled(c, r, shade);
    p.circle_filled(c + vec2(-r * 0.18, -r * 0.2), r * 0.82, base);
    p.circle_stroke(c, r, Stroke::new(1.5, col("outline_color", Color32::from_gray(40))));
}

// ================================================================================================ importar

fn import_window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut dlg) = env.ed.library.import.clone() else { return };
    let t = env.t;
    let mut open = true;
    let (mut go, mut cancel) = (false, false);
    let center = ctx.content_rect().center();
    egui::Window::new(tr("lib.imp.title")).id(egui::Id::new("lib_import")).open(&mut open).collapsible(false).resizable(false).default_width(420.0).default_pos(center - vec2(210.0, 190.0)).show(ctx, |ui| {
        dim_label(ui, &t, tr("lib.imp.files"));
        egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
            let mut remove = None;
            for (i, f) in dlg.files.iter().enumerate() {
                let kind = aiai_assets::import::kind_of_file(f).map(kind_label).unwrap_or("?");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} · {kind}", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).size(12.0));
                    if ui.small_button("x").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                dlg.files.remove(i);
            }
        });
        if dlg.files.iter().any(|f| IMAGE_EXTS.contains(&ext_of(f).as_str())) {
            ui.horizontal(|ui| {
                ui.label(tr("lib.imp.images_as"));
                ui.radio_value(&mut dlg.as_material, false, tr("lib.kind.texture"));
                ui.radio_value(&mut dlg.as_material, true, tr("lib.kind.material"));
            });
        }
        ui.add_space(4.0);
        dim_label(ui, &t, tr("lib.imp.license"));
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut dlg.license).desired_width(260.0).hint_text(tr("lib.imp.license_hint")));
            env.note("lib:imp:license", r.rect);
            egui::ComboBox::from_id_salt("lib_imp_lic").width(90.0).selected_text(tr("lib.meta.presets")).show_ui(ui, |ui| {
                for l in LICENSES {
                    if ui.selectable_label(dlg.license == l, l).clicked() {
                        dlg.license = l.to_owned();
                    }
                }
            });
        });
        for (label, name, field) in [("lib.imp.author", "author", &mut dlg.author), ("lib.imp.source", "source", &mut dlg.source), ("lib.imp.tags", "tags", &mut dlg.tags)] {
            dim_label(ui, &t, tr(label));
            let r = ui.add(egui::TextEdit::singleline(field).desired_width(360.0));
            env.note(format!("lib:imp:{name}"), r.rect);
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr("lib.imp.note")).size(11.0).color(t.text_dim));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let ok = ui.add_enabled(!dlg.files.is_empty(), egui::Button::new(tr("lib.imp.go")));
            env.note("lib:import_go", ok.rect);
            go = ok.clicked();
            let c = ui.button(tr("lib.imp.cancel"));
            env.note("lib:import_cancel", c.rect);
            cancel = c.clicked();
        });
    });
    if go {
        env.ed.library.import = None;
        env.ed.library.run_import(ctx, dlg);
    } else if cancel || !open {
        env.ed.library.import = None;
    } else {
        env.ed.library.import = Some(dlg);
    }
}

// ================================================================================================ ações

/// Botão de ação (registra o retângulo para o harness) que diz se foi clicado.
fn action_button(env: &mut Env, ui: &mut Ui, key: &str, name: &str) -> bool {
    let r = ui.button(tr(key));
    env.note(format!("lib:{name}"), r.rect);
    r.clicked()
}

fn use_buttons(env: &mut Env, ui: &mut Ui, e: &Entry) {
    let id = e.id.as_str();
    match e.kind {
        Kind::Model3d | Kind::Texture => {
            if action_button(env, ui, "lib.insert_layer", "insert_layer") {
                library::use_entry(env, id, true);
            }
            if action_button(env, ui, "lib.use_project", "use_project") {
                library::use_entry(env, id, false);
            }
        }
        Kind::Material if is_toon_material(e) => {
            if action_button(env, ui, "lib.apply_3d", "apply_3d") {
                library::use_entry(env, id, true);
            }
        }
        Kind::Material => {
            if action_button(env, ui, "lib.insert_layer", "insert_layer") {
                library::use_entry(env, id, true);
            }
            if action_button(env, ui, "lib.use_project", "use_project") {
                library::use_entry(env, id, false);
            }
        }
        Kind::Palette => {
            if action_button(env, ui, "lib.add_palette", "add_palette") {
                library::use_entry(env, id, false);
            }
        }
        Kind::Brush => {
            if action_button(env, ui, "lib.use_brush", "use_brush") {
                library::use_entry(env, id, false);
            }
        }
        Kind::Pose => {
            ui.add_enabled(false, egui::Button::new(tr("lib.apply_pose"))).on_disabled_hover_text(tr("lib.apply_pose.tip"));
        }
    }
}

fn manage_buttons(env: &mut Env, ui: &mut Ui, e: &Entry) {
    let t = env.t;
    let Some(root) = env.ed.library.root().map(|p| p.to_path_buf()) else { return };
    if ui.button(tr("lib.duplicate")).clicked() {
        match store::duplicate(e) {
            Ok(p) => {
                env.ed.library.msg = Some(tr("lib.duplicated").replace("{name}", &e.env.name));
                env.ed.library.changed();
                // seleciona a cópia assim que entrar no índice
                if let Some(c) = env.ed.library.index.entries.iter().find(|x| x.path == p).map(|x| x.id.clone()) {
                    env.ed.library.select(Some(c));
                }
            }
            Err(m) => env.ed.library.msg = Some(m),
        }
    }
    let del = ui.add(egui::Button::new(egui::RichText::new(tr("lib.delete")).color(t.danger)));
    env.note("lib:delete", del.rect);
    if del.on_hover_text(tr("lib.delete.tip")).clicked() {
        match store::trash(&root, e) {
            Ok(_) => {
                env.ed.library.msg = Some(tr("lib.deleted").replace("{name}", &e.env.name));
                env.ed.library.select(None);
                env.ed.library.changed();
            }
            Err(m) => env.ed.library.msg = Some(m),
        }
    }
    if ui.button(tr("lib.reveal")).clicked() {
        library::open_folder(e.path.parent().unwrap_or(&root));
    }
}

// ================================================================================================ painel Prévia

pub(crate) fn preview(env: &mut Env, ui: &mut Ui, body: Rect) {
    let ctx = ui.ctx().clone();
    let bv = env.ed.brushes.version;
    env.ed.library.sync(&ctx, bv);
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(e) = env.ed.library.selected_entry().cloned() else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("panel.library_preview.empty")).size(12.5).color(t.text_dim));
        return;
    };
    env.ed.library.ensure_preview(&ctx);
    egui::ScrollArea::vertical().id_salt("lib_preview_scroll").auto_shrink([false, false]).show(ui, |ui| {
        // folga para a barra de rolagem: os campos não passam da borda do painel
        let w = ui.available_width() - 12.0;
        // a paleta pede só a altura das amostras; o resto é uma área quase quadrada
        let h = if e.kind == Kind::Palette {
            let cols = ((w - 12.0) / PAL_CELL).floor().max(1.0);
            ((e.stats.colors.unwrap_or(1) as f32 / cols).ceil().clamp(1.0, 12.0) * PAL_CELL + 12.0).max(70.0)
        } else {
            (w * 0.78).clamp(120.0, 300.0)
        };
        let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        env.note("lib:preview", rect);
        ui.painter().rect_filled(rect, 6.0, t.canvas_bg);
        preview_area(env, ui, &ctx, &e, rect);
        ui.add_space(4.0);
        preview_controls(env, ui, &e);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            use_buttons(env, ui, &e);
            manage_buttons(env, ui, &e);
        });
        palette_update(env, ui, &e);
        section(ui, &t, "lib.info");
        info(env, ui, &e);
        section(ui, &t, "lib.meta");
        metadata(env, ui, &e);
    });
}

fn preview_area(env: &mut Env, ui: &mut Ui, ctx: &egui::Context, e: &Entry, rect: Rect) {
    let t = env.t;
    let p = ui.painter().with_clip_rect(rect);
    match e.kind {
        Kind::Model3d => turntable(env, ctx, e, rect, &p),
        Kind::Texture | Kind::Material if is_toon_material(e) => toon_ball(&p, rect.shrink(18.0), e),
        Kind::Texture | Kind::Material => {
            let pv = &env.ed.library.preview;
            if let Some(tex) = &pv.image {
                let s = tex.size_vec2();
                if pv.tile3 {
                    // 3×3: a mesma imagem repetida (a costura aparece se a textura não repete bem)
                    let r0 = rect.shrink(4.0);
                    let k = (r0.width() / (3.0 * s.x)).min(r0.height() / (3.0 * s.y));
                    let r = Rect::from_center_size(rect.center(), s * (3.0 * k));
                    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(3.0, 3.0)), Color32::WHITE);
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
                } else {
                    let r0 = rect.shrink(4.0);
                    let k = (r0.width() / s.x).min(r0.height() / s.y);
                    let r = Rect::from_center_size(rect.center(), s * k);
                    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            } else if let Some(err) = &pv.image_err {
                p.text(rect.center(), Align2::CENTER_CENTER, err, FontId::proportional(11.0), t.danger);
            } else {
                p.text(rect.center(), Align2::CENTER_CENTER, tr("lib.preview.loading"), FontId::proportional(12.0), t.text_dim);
            }
        }
        Kind::Palette => {
            let pal = palette_of(e);
            let cols = ((rect.width() - 12.0) / PAL_CELL).floor().max(1.0) as usize;
            for (i, c) in pal.colors.iter().enumerate().take(cols * 12) {
                let r = Rect::from_min_size(rect.min + vec2(6.0 + (i % cols) as f32 * PAL_CELL, 6.0 + (i / cols) as f32 * PAL_CELL), vec2(PAL_CELL - 4.0, PAL_CELL - 4.0));
                if let Some(col) = hex_color(&c.hex) {
                    p.rect_filled(r, 4.0, col);
                    p.rect_stroke(r, 4.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
                }
                let tip = format!("{}  {}{}", c.name, c.hex, if c.group.is_empty() { String::new() } else { format!("  ({})", c.group) });
                let resp = ui.interact(r, egui::Id::new(("lib_pal_swatch", i)), Sense::click()).on_hover_text(tip);
                if resp.clicked() {
                    ctx.copy_text(c.hex.clone());
                    env.ed.notify(tr("lib.preview.hex_copied").replace("{hex}", &c.hex));
                }
            }
        }
        Kind::Brush => {
            let tex = env.ed.library.thumb(ctx, e);
            match tex {
                Some(tx) => {
                    let side = rect.width().min(rect.height()) - 12.0;
                    p.image(tx.id(), Rect::from_center_size(rect.center(), vec2(side, side)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    p.text(rect.center(), Align2::CENTER_CENTER, tr("lib.preview.loading"), FontId::proportional(12.0), t.text_dim);
                }
            }
        }
        Kind::Pose => {
            let text = serde_json::to_string_pretty(&e.env.payload).unwrap_or_default();
            let g = p.layout(text.chars().take(600).collect(), FontId::monospace(10.0), t.text_dim, rect.width() - 12.0);
            p.galley(rect.min + vec2(6.0, 6.0), g, t.text_dim);
        }
    }
}

/// Modelo 3D girando: o serviço da GPU do canvas desenha os quadros (CPU quando não há GPU); um quadro pedido por vez.
fn turntable(env: &mut Env, ctx: &egui::Context, e: &Entry, rect: Rect, p: &egui::Painter) {
    let t = env.t;
    let pv = &mut env.ed.library.preview;
    // chega o quadro pedido
    if let Some((rx, _)) = &pv.pending {
        if let Ok(r) = rx.try_recv() {
            pv.pending = None;
            match r {
                Ok(rgba) => {
                    let (w, h) = (rect.width().round().max(8.0) as usize, rect.height().round().max(8.0) as usize);
                    if rgba.len() == w * h * 4 {
                        let img = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
                        match &mut pv.frame {
                            Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                            None => pv.frame = Some(ctx.load_texture(format!("lib-turn-{}", e.id), img, egui::TextureOptions::LINEAR)),
                        }
                    }
                }
                Err(m) => pv.model_err = Some(m),
            }
        }
    }
    match &pv.model {
        ModelState::Failed(m) => {
            p.text(rect.center(), Align2::CENTER_CENTER, m, FontId::proportional(11.0), t.danger);
            return;
        }
        ModelState::None | ModelState::Loading => {
            p.text(rect.center(), Align2::CENTER_CENTER, tr("lib.preview.loading"), FontId::proportional(12.0), t.text_dim);
            return;
        }
        ModelState::Ready(..) => {}
    }
    if let Some(tex) = &pv.frame {
        p.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if let Some(m) = &pv.model_err {
        p.text(rect.center(), Align2::CENTER_CENTER, m, FontId::proportional(11.0), t.danger);
    }
    // gira: 36 graus por segundo; pausado, o quadro fica
    let now = std::time::Instant::now();
    let started = *pv.started.get_or_insert(now);
    if !pv.paused {
        pv.yaw = (now.duration_since(started).as_secs_f32() * 36.0) % 360.0;
    }
    let want = (rect.width().round().max(8.0) as u32, rect.height().round().max(8.0) as u32);
    let ModelState::Ready(model, hash) = &pv.model else { return };
    if pv.pending.is_none() && (!pv.paused || pv.frame.is_none()) {
        let st = aiai_assets::thumb::model_state(model, hash, pv.yaw, want.0 as f32 / want.1 as f32);
        pv.pending = Some((request_frame(model.clone(), hash.clone(), st, want), want));
    }
    if !pv.paused {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    } else if pv.pending.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Pede um quadro à GPU do canvas (ou ao rasterizador CPU numa thread) e devolve o canal da resposta (RGBA pré-multiplicado).
fn request_frame(model: std::sync::Arc<aiai_3d_model::Model>, hash: aiai_core::BlobHash, st: aiai_core::model3d::Model3dState, (w, h): (u32, u32)) -> crossbeam_channel::Receiver<Result<Vec<u8>, String>> {
    if let Some(rx) = crate::app::model3d::preview_request(hash.clone(), model.clone(), st.clone(), w, h) {
        return rx;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let px = aiai_3d_model::raster::render_cpu(&model, &st, w, h);
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let _ = tx.send(Ok(px.iter().flat_map(|p| [b(p[0]), b(p[1]), b(p[2]), b(p[3])]).collect()));
    });
    rx
}

fn preview_controls(env: &mut Env, ui: &mut Ui, e: &Entry) {
    let ctx = ui.ctx().clone();
    match e.kind {
        Kind::Model3d => {
            ui.horizontal(|ui| {
                let paused = env.ed.library.preview.paused;
                let b = ui.button(tr(if paused { "lib.preview.turn" } else { "lib.preview.pause" }));
                env.note("lib:turn_toggle", b.rect);
                if b.clicked() {
                    env.ed.library.preview.paused = !paused;
                }
                let gpu = crate::app::model3d::gpu_name().is_some();
                dim_label(ui, &env.t, tr(if gpu { "lib.preview.gpu" } else { "lib.preview.cpu" }));
            });
        }
        Kind::Texture | Kind::Material if !is_toon_material(e) => {
            ui.horizontal(|ui| {
                let mut tile = env.ed.library.preview.tile3;
                let c = ui.checkbox(&mut tile, tr("lib.preview.tile3"));
                env.note("lib:tile3", c.rect);
                env.ed.library.preview.tile3 = tile;
            });
            let b = ui.button(tr("lib.preview.extract"));
            env.note("lib:extract", b.rect);
            if b.on_hover_text(tr("lib.preview.extract.tip")).clicked() {
                env.ed.library.extract_palette(&ctx, e);
            }
        }
        _ => {}
    }
}

/// Paleta: "Atualizar com a paleta do projeto" grava as cores novas nesta paleta da biblioteca (versão +1).
fn palette_update(env: &mut Env, ui: &mut Ui, e: &Entry) {
    if e.kind != Kind::Palette {
        return;
    }
    let pals: Vec<(aiai_core::Id, String)> = env
        .core
        .project
        .roots
        .palettes
        .iter()
        .filter_map(|id| match &env.core.project.node(id)?.data {
            aiai_core::NodeKind::Palette(p) if !p.colors.is_empty() => Some((id.clone(), env.core.project.node(id)?.name.clone())),
            _ => None,
        })
        .collect();
    if pals.is_empty() {
        return;
    }
    ui.add_space(2.0);
    let r = ui.menu_button(tr("lib.update_palette"), |ui| {
        for (id, name) in &pals {
            if ui.button(name).clicked() {
                library::update_palette_from_project(env, &e.id, id);
                ui.close();
            }
        }
    });
    env.note("lib:update_palette", r.response.rect);
    r.response.on_hover_text(tr("lib.update_palette.tip"));
}

fn info(env: &mut Env, ui: &mut Ui, e: &Entry) {
    let t = env.t;
    let row = |ui: &mut Ui, k: &str, v: String| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{}:", tr(k))).size(11.5).color(t.text_dim));
            ui.label(egui::RichText::new(v).size(11.5));
        });
    };
    row(ui, "lib.info.kind", kind_label(e.kind).to_owned());
    row(ui, "lib.info.file", e.file_name());
    row(ui, "lib.info.size", human_bytes(e.bytes));
    let s = stats_line(e);
    if !s.is_empty() && s != kind_label(e.kind) {
        row(ui, "lib.info.data", s);
    }
    row(ui, "lib.info.version", format!("{} · {}", e.env.version, e.env.modified.split('T').next().unwrap_or("")));
    if license_missing(e) && e.kind != Kind::Pose {
        ui.label(egui::RichText::new(tr("lib.meta.license_warn")).size(11.0).color(t.st_retake));
    }
    if e.inferred {
        let side = aiai_assets::index::sidecar_for(&e.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ui.label(egui::RichText::new(tr("lib.meta.inferred").replace("{file}", &side)).size(11.0).color(t.text_dim));
    }
}

/// Campo de texto de metadado; diz se perdeu o foco (é quando o manifesto é gravado).
fn meta_field(ui: &mut Ui, env: &mut Env, key: &str, name: &str, text: &mut String, hint: &str, multi: bool) -> bool {
    dim_label(ui, &env.t, tr(key));
    let w = (ui.available_width() - 12.0).max(60.0);
    let r = if multi { ui.add(egui::TextEdit::multiline(text).desired_width(w).desired_rows(2).hint_text(hint)) } else { ui.add(egui::TextEdit::singleline(text).desired_width(w).hint_text(hint)) };
    env.note(format!("lib:meta:{name}"), r.rect);
    r.lost_focus()
}

/// Edição de metadados: grava no manifesto quando o campo perde o foco.
fn metadata(env: &mut Env, ui: &mut Ui, e: &Entry) {
    let t = env.t;
    let mut d = env.ed.library.draft.clone();
    if d.id != e.id {
        d = library::Draft::of(e);
    }
    let mut left = false;
    left |= meta_field(ui, env, "lib.meta.name", "name", &mut d.name, "", false);
    left |= meta_field(ui, env, "lib.meta.tags", "tags", &mut d.tags, tr("lib.meta.tags_hint"), false);
    left |= meta_field(ui, env, "lib.meta.author", "author", &mut d.author, "", false);
    dim_label(ui, &t, tr("lib.meta.license"));
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 96.0).max(60.0);
        let r = ui.add(egui::TextEdit::singleline(&mut d.license).desired_width(w));
        env.note("lib:meta:license", r.rect);
        if r.lost_focus() {
            left = true;
        }
        egui::ComboBox::from_id_salt("lib_meta_lic").width(76.0).selected_text(tr("lib.meta.presets")).show_ui(ui, |ui| {
            for l in LICENSES {
                if ui.selectable_label(d.license == l, l).clicked() {
                    d.license = l.to_owned();
                    left = true;
                }
            }
        });
    });
    left |= meta_field(ui, env, "lib.meta.source", "source", &mut d.source, "https://...", false);
    left |= meta_field(ui, env, "lib.meta.description", "description", &mut d.description, "", true);
    env.ed.library.draft = d.clone();
    if left {
        let base = library::Draft::of(e);
        let edit = MetaEdit {
            name: (d.name.trim() != base.name && !d.name.trim().is_empty()).then(|| d.name.clone()),
            tags: (d.tags != base.tags).then(|| aiai_assets::parse_tags(&d.tags)),
            author: (d.author != base.author).then(|| d.author.clone()),
            license: (d.license != base.license).then(|| d.license.clone()),
            source: (d.source != base.source).then(|| d.source.clone()),
            description: (d.description != base.description).then(|| d.description.clone()),
            category: None,
        };
        if d.name.trim().is_empty() {
            env.ed.library.draft.name = base.name.clone();
        }
        if !edit.is_empty() {
            match store::update_meta(e, &edit) {
                Ok(_) => {
                    env.ed.library.msg = Some(tr("lib.meta.saved").to_owned());
                    env.ed.library.draft.id = String::new();
                    env.ed.library.changed();
                }
                Err(m) => env.ed.library.msg = Some(m),
            }
        }
    }
}

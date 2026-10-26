//! Barra de materiais (S25): uma coluna de ícones entre o canvas e a coluna da direita, um ícone por categoria da
//! biblioteca (texturas, padrões, imagens, materiais 3D, modelos 3D, poses, pincéis, paletas e degradês) e os
//! personagens do projeto. Clicar num ícone abre o painel da categoria encostado na barra, tirando espaço do canvas
//! (busca, etiquetas e miniaturas; arrastar para o canvas ou duplo clique usa o recurso pelo mesmo caminho da
//! Biblioteca); clicar de novo recolhe e o canvas ganha o espaço de volta.
//!
//! A barra vive dentro do painel do canvas (lado direito): vai junto quando o canvas é maximizado ou solto numa janela,
//! e o layout do dock não muda ao abrir/fechar uma categoria.

use super::library::{self, LibDrag};
use super::ui_kit::{Env, paint_icon, rgba_to_color32};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_assets::envelope::fold;
use aiai_assets::index::{Entry, Query, Sort};
use aiai_assets::Kind;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// Largura da coluna de ícones.
pub(crate) const BAR_W: f32 = 34.0;
const PANEL_MIN: f32 = 190.0;
const PANEL_MAX: f32 = 460.0;

/// Categoria da barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MatCat {
    Textures,
    Patterns,
    Images,
    Materials,
    Models,
    Poses,
    Brushes,
    Palettes,
    Characters,
}

/// Palavras (sem acento, minúsculas) que marcam uma textura como padrão (repete) ou como imagem (foto, referência), em
/// etiquetas, categoria ou subpasta do recurso.
const PATTERN_WORDS: [&str; 7] = ["padrao", "padroes", "pattern", "tile", "seamless", "repet", "trama"];
const IMAGE_WORDS: [&str; 7] = ["imagem", "imagens", "image", "foto", "photo", "referencia", "reference"];

fn marked(e: &Entry, words: &[&str]) -> bool {
    let hay = std::iter::once(fold(&e.env.category)).chain(std::iter::once(fold(&e.folder))).chain(e.env.tags.iter().map(|t| fold(t)));
    hay.into_iter().any(|h| words.iter().any(|w| h.contains(w)))
}

impl MatCat {
    pub(crate) const ALL: [MatCat; 9] = [MatCat::Textures, MatCat::Patterns, MatCat::Images, MatCat::Materials, MatCat::Models, MatCat::Poses, MatCat::Brushes, MatCat::Palettes, MatCat::Characters];

    pub(crate) fn id(self) -> &'static str {
        match self {
            MatCat::Textures => "textures",
            MatCat::Patterns => "patterns",
            MatCat::Images => "images",
            MatCat::Materials => "materials",
            MatCat::Models => "models",
            MatCat::Poses => "poses",
            MatCat::Brushes => "brushes",
            MatCat::Palettes => "palettes",
            MatCat::Characters => "characters",
        }
    }

    pub(crate) fn icon(self) -> IconId {
        match self {
            MatCat::Textures => IconId::Texture,
            MatCat::Patterns => IconId::Pattern,
            MatCat::Images => IconId::Image,
            MatCat::Materials => IconId::Material,
            MatCat::Models => IconId::LayerModel3d,
            MatCat::Poses => IconId::Pose,
            MatCat::Brushes => IconId::ToolBrush,
            MatCat::Palettes => IconId::Palette,
            MatCat::Characters => IconId::NodeCharacter,
        }
    }

    pub(crate) fn title_key(self) -> &'static str {
        match self {
            MatCat::Textures => "mat.textures",
            MatCat::Patterns => "mat.patterns",
            MatCat::Images => "mat.images",
            MatCat::Materials => "mat.materials",
            MatCat::Models => "mat.models",
            MatCat::Poses => "mat.poses",
            MatCat::Brushes => "mat.brushes",
            MatCat::Palettes => "mat.palettes",
            MatCat::Characters => "mat.characters",
        }
    }

    /// Tipo da biblioteca por trás da categoria (`None` = personagens do projeto).
    pub(crate) fn kind(self) -> Option<Kind> {
        match self {
            MatCat::Textures | MatCat::Patterns | MatCat::Images => Some(Kind::Texture),
            MatCat::Materials => Some(Kind::Material),
            MatCat::Models => Some(Kind::Model3d),
            MatCat::Poses => Some(Kind::Pose),
            MatCat::Brushes => Some(Kind::Brush),
            MatCat::Palettes => Some(Kind::Palette),
            MatCat::Characters => None,
        }
    }

    /// O recurso entra nesta categoria? Texturas = as que não são padrão nem imagem.
    pub(crate) fn matches(self, e: &Entry) -> bool {
        if self.kind() != Some(e.kind) {
            return false;
        }
        match self {
            MatCat::Patterns => marked(e, &PATTERN_WORDS),
            MatCat::Images => !marked(e, &PATTERN_WORDS) && marked(e, &IMAGE_WORDS),
            MatCat::Textures => !marked(e, &PATTERN_WORDS) && !marked(e, &IMAGE_WORDS),
            _ => true,
        }
    }
}

/// Estado da barra (não vai para o documento).
#[derive(Debug, Clone)]
pub(crate) struct MaterialsState {
    /// Categoria aberta (`None` = recolhida: só a coluna de ícones).
    pub open: Option<MatCat>,
    pub query: String,
    pub tag: Option<String>,
    /// Largura do painel aberto (pontos); arrastar a borda esquerda muda.
    pub width: f32,
    /// Lado das miniaturas.
    pub size: f32,
    pub selected: Option<String>,
}

impl Default for MaterialsState {
    fn default() -> Self {
        MaterialsState { open: None, query: String::new(), tag: None, width: 250.0, size: 72.0, selected: None }
    }
}

impl MaterialsState {
    /// Clicar no ícone: abre a categoria; clicar na aberta recolhe.
    pub(crate) fn toggle(&mut self, c: MatCat) {
        if self.open == Some(c) {
            self.open = None;
        } else {
            self.open = Some(c);
            self.tag = None;
            self.selected = None;
        }
    }

    /// Largura que a barra tira do canvas.
    pub(crate) fn reserved(&self) -> f32 {
        BAR_W + if self.open.is_some() { self.width.clamp(PANEL_MIN, PANEL_MAX) + 1.0 } else { 0.0 }
    }

    /// Recursos da categoria aberta, filtrados pela busca e pela etiqueta.
    pub(crate) fn hits<'a>(&self, index: &'a aiai_assets::Index, c: MatCat) -> Vec<&'a Entry> {
        let q = Query { kind: c.kind(), text: self.query.clone(), tags: self.tag.iter().cloned().collect(), sort: Sort::Name };
        index.search(&q).into_iter().filter(|e| c.matches(e)).collect()
    }
}

/// Desenha a barra (e o painel aberto) na borda direita de `body` e devolve o que sobra para o canvas.
pub(crate) fn strip(env: &mut Env, ui: &mut Ui, body: Rect) -> Rect {
    let t = env.t;
    let w = env.ed.workbar.materials.reserved().min(body.width() * 0.6);
    if body.width() < 260.0 {
        return body;
    }
    let bar = Rect::from_min_max(pos2(body.max.x - BAR_W, body.min.y), body.max);
    let p = ui.painter().with_clip_rect(body);
    p.rect_filled(bar, 0.0, t.bg_panel);
    p.vline(bar.min.x, bar.y_range(), Stroke::new(1.0, t.border));
    env.note("matbar", bar);
    // ícones
    let mut y = bar.min.y + 6.0;
    for c in MatCat::ALL {
        let r = Rect::from_min_size(pos2(bar.min.x + 4.0, y), vec2(BAR_W - 8.0, BAR_W - 8.0));
        let id = ui.id().with(("matbar", c.id()));
        let resp = ui.interact(r, id, Sense::click());
        env.note(format!("matbar:{}", c.id()), r);
        let on = env.ed.workbar.materials.open == Some(c);
        let p = ui.painter();
        if on {
            p.rect(r, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(r, 4.0, t.bg_hover);
        }
        paint_icon(env.icons, p, c.icon(), r.shrink(5.0), if on || resp.hovered() { t.text } else { t.text_dim }, false);
        if resp.on_hover_text(tr(c.title_key())).clicked() {
            env.ed.workbar.materials.toggle(c);
        }
        y += BAR_W - 4.0;
        if matches!(c, MatCat::Images | MatCat::Poses) {
            // separadores entre os grupos (imagens | 3D | ferramentas e cor)
            ui.painter().hline((bar.min.x + 8.0)..=(bar.max.x - 8.0), y + 1.0, Stroke::new(1.0, t.border));
            y += 6.0;
        }
    }
    let Some(cat) = env.ed.workbar.materials.open else {
        return Rect::from_min_max(body.min, pos2(bar.min.x, body.max.y));
    };
    let panel = Rect::from_min_max(pos2(body.max.x - w, body.min.y), pos2(bar.min.x, body.max.y));
    ui.painter().rect_filled(panel, 0.0, t.bg_panel);
    ui.painter().vline(panel.min.x, panel.y_range(), Stroke::new(1.0, t.border));
    env.note("matpanel", panel);
    // borda esquerda arrastável (largura do painel)
    let edge = Rect::from_min_max(pos2(panel.min.x - 3.0, panel.min.y), pos2(panel.min.x + 3.0, panel.max.y));
    let er = ui.interact(edge, ui.id().with("matpanel_edge"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if er.dragged() {
        let m = &mut env.ed.workbar.materials;
        m.width = (m.width - er.drag_delta().x).clamp(PANEL_MIN, PANEL_MAX);
    }
    let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(panel.shrink(6.0)));
    pui.set_clip_rect(panel);
    panel_ui(env, &mut pui, cat);
    Rect::from_min_max(body.min, pos2(panel.min.x, body.max.y))
}

fn panel_ui(env: &mut Env, ui: &mut Ui, cat: MatCat) {
    let t = env.t;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        paint_icon(env.icons, ui.painter(), cat.icon(), r, t.text_dim, false);
        ui.label(egui::RichText::new(tr(cat.title_key())).strong().size(12.5));
    });
    if cat == MatCat::Characters {
        return characters(env, ui);
    }
    let ctx = ui.ctx().clone();
    let bv = env.ed.brushes.version;
    env.ed.library.sync(&ctx, bv);
    if !env.ed.library.enabled() {
        ui.label(egui::RichText::new(tr("lib.disabled")).size(12.0).color(t.text_dim));
        return;
    }
    let m = &mut env.ed.workbar.materials;
    let r = ui.add(egui::TextEdit::singleline(&mut m.query).hint_text(tr("lib.search_hint")).desired_width(ui.available_width()));
    env.note("matpanel:search", r.rect);
    // etiquetas da categoria (as mais usadas)
    let hits_all: Vec<Entry> = env.ed.workbar.materials.hits(&env.ed.library.index, cat).into_iter().cloned().collect();
    let mut tags: std::collections::BTreeMap<String, usize> = Default::default();
    for e in &hits_all {
        for tg in &e.env.tags {
            *tags.entry(tg.clone()).or_default() += 1;
        }
    }
    let mut tags: Vec<(String, usize)> = tags.into_iter().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if !tags.is_empty() || env.ed.workbar.materials.tag.is_some() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for (tag, n) in tags.into_iter().take(12) {
                let on = env.ed.workbar.materials.tag.as_deref().is_some_and(|x| fold(x) == fold(&tag));
                let r = ui.selectable_label(on, egui::RichText::new(format!("{tag} {n}")).size(11.0));
                env.note(format!("matpanel:tag:{tag}"), r.rect);
                if r.clicked() {
                    env.ed.workbar.materials.tag = if on { None } else { Some(tag) };
                }
            }
        });
    }
    ui.add_space(2.0);
    if hits_all.is_empty() {
        ui.label(egui::RichText::new(tr("mat.empty")).size(12.0).color(t.text_dim));
        return;
    }
    // grade de miniaturas (o nome embaixo), com o tamanho no rodapé
    let foot = 22.0;
    let grid_h = (ui.available_height() - foot).max(40.0);
    let side = env.ed.workbar.materials.size;
    let (cw, ch) = (side + 8.0, side + 8.0 + 16.0);
    let cols = ((ui.available_width() / cw).floor() as usize).max(1);
    let rows = hits_all.len().div_ceil(cols);
    egui::ScrollArea::vertical().id_salt(("matpanel_grid", cat.id())).max_height(grid_h).auto_shrink([false, false]).show_rows(ui, ch, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for e in hits_all.iter().skip(row * cols).take(cols) {
                    cell(env, ui, &ctx, e, vec2(cw, ch), side);
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{}", hits_all.len())).size(11.0).color(t.text_dim));
        let mut s = env.ed.workbar.materials.size;
        let sl = ui.add(egui::Slider::new(&mut s, 48.0..=140.0).show_value(false).step_by(4.0));
        env.note("matpanel:size", sl.rect);
        sl.on_hover_text(tr("lib.size"));
        env.ed.workbar.materials.size = s;
    });
}

fn cell(env: &mut Env, ui: &mut Ui, ctx: &egui::Context, e: &Entry, size: egui::Vec2, side: f32) {
    let t = env.t;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return;
    }
    env.note(format!("matpanel:item:{}", e.id), rect);
    let on = env.ed.workbar.materials.selected.as_deref() == Some(e.id.as_str());
    let p = ui.painter();
    if on {
        p.rect(rect.shrink(1.0), 5.0, t.accent_dim, Stroke::new(1.5, t.accent), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 5.0, t.bg_hover);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(side, side));
    p.rect_filled(thumb, 3.0, t.bg_widget);
    match (env.ed.library.thumb(ctx, e), e.kind) {
        (Some(tx), _) => {
            let s = tx.size_vec2();
            let k = (thumb.width() / s.x).min(thumb.height() / s.y);
            p.image(tx.id(), Rect::from_center_size(thumb.center(), s * k), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (None, Kind::Palette) => super::panels::library::swatches(p, thumb, &super::panels::library::palette_of(e).swatches()),
        (None, Kind::Material) if super::panels::library::is_toon_material(e) => super::panels::library::toon_ball(p, thumb, e),
        (None, _) => {
            let icon = Rect::from_center_size(thumb.center(), vec2(side * 0.4, side * 0.4));
            paint_icon(env.icons, p, super::panels::library::kind_icon(e.kind), icon, t.text_dim, false);
        }
    }
    let name_r = Rect::from_min_size(pos2(rect.min.x + 4.0, thumb.max.y + 2.0), vec2(rect.width() - 8.0, 14.0));
    p.with_clip_rect(name_r).text(name_r.left_center(), Align2::LEFT_CENTER, &e.env.name, FontId::proportional(10.5), t.text);
    let resp = resp.on_hover_text(format!("{}\n{}", e.env.name, tr("mat.use_tip")));
    if resp.clicked() {
        env.ed.workbar.materials.selected = Some(e.id.clone());
    }
    if resp.double_clicked() {
        library::use_entry(env, &e.id, true);
    }
    if resp.drag_started() {
        env.ed.workbar.materials.selected = Some(e.id.clone());
        egui::DragAndDrop::set_payload(ctx, LibDrag { id: e.id.clone() });
    }
    if resp.dragged() {
        if let Some(pt) = ctx.pointer_interact_pos() {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("mat_drag")));
            let g = painter.layout_no_wrap(format!("+ {}", e.env.name), FontId::proportional(12.5), t.text);
            let r = Rect::from_min_size(pt + vec2(14.0, 6.0), g.size() + vec2(10.0, 6.0));
            painter.rect_filled(r, 4.0, t.bg_panel);
            painter.rect_stroke(r, 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            painter.galley(r.min + vec2(5.0, 3.0), g, t.text);
        }
    }
}

/// Personagens do projeto: nome e a paleta de settei; duplo clique deixa a paleta dele ativa (Cores) e mostra as
/// referências.
fn characters(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let ids = env.core.project.roots.characters.clone();
    if ids.is_empty() {
        ui.label(egui::RichText::new(tr("mat.no_characters")).size(12.0).color(t.text_dim));
        return;
    }
    egui::ScrollArea::vertical().id_salt("matpanel_chars").auto_shrink([false, false]).show(ui, |ui| {
        for id in ids {
            let p = &env.core.project;
            let Some(node) = p.node(&id) else { continue };
            let name = node.name.clone();
            let pal = super::panels::characters::char_data(p, &id).and_then(|c| c.palette.clone());
            let colors: Vec<Color32> = pal.as_ref().and_then(|pl| super::commands::palette_data(p, pl)).map(|pd| pd.colors.iter().take(14).map(|c| rgba_to_color32(c.rgba)).collect()).unwrap_or_default();
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
            env.note(format!("matpanel:char:{name}"), rect);
            let on = env.ed.palette.is_some() && env.ed.palette == pal;
            let pt = ui.painter();
            if on {
                pt.rect(rect, 5.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            } else if resp.hovered() {
                pt.rect_filled(rect, 5.0, t.bg_hover);
            }
            paint_icon(env.icons, pt, IconId::NodeCharacter, Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(16.0, 16.0)), t.text_dim, false);
            pt.text(pos2(rect.min.x + 28.0, rect.min.y + 14.0), Align2::LEFT_CENTER, &name, FontId::proportional(12.5), t.text);
            let sw = 12.0;
            for (i, c) in colors.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.min.x + 28.0 + i as f32 * (sw + 2.0), rect.min.y + 26.0), vec2(sw, sw));
                if r.max.x > rect.max.x - 4.0 {
                    break;
                }
                pt.rect_filled(r, 2.0, *c);
            }
            if resp.on_hover_text(tr("mat.char_tip")).double_clicked() {
                match pal {
                    Some(pl) => {
                        env.ed.palette = Some(pl);
                        env.ed.anim.char_ref.sel = Some(id.clone());
                        env.ed.want_panel = Some(crate::layout::PanelKind::Palette);
                        env.ed.notify(tr("mat.char_palette").replace("{name}", &name));
                    }
                    None => env.ed.notify(tr("mat.char_no_palette").replace("{name}", &name)),
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_assets::envelope::Envelope;

    fn entry(kind: Kind, tags: &[&str], folder: &str) -> Entry {
        let mut env: Envelope = serde_json::from_value(serde_json::json!({ "format": "aiai-asset", "format_version": 1, "kind": kind.key(), "id": format!("{}-{}", kind.key(), tags.join("-")), "name": "x" })).unwrap();
        env.tags = tags.iter().map(|s| s.to_string()).collect();
        Entry { id: env.id.clone(), kind, path: std::path::PathBuf::from("x.png"), storage: aiai_assets::Storage::Embedded, env, inferred: false, bytes: 1, mtime: 0, folder: folder.into(), stats: Default::default() }
    }

    #[test]
    fn textures_split_into_patterns_images_and_plain_textures() {
        let plain = entry(Kind::Texture, &["papel"], "");
        let pat = entry(Kind::Texture, &["Padrão"], "");
        let tiled = entry(Kind::Texture, &[], "Tiles/madeira");
        let folder = entry(Kind::Texture, &[], "Padrões");
        assert!(MatCat::Patterns.matches(&folder), "pasta Padrões");
        let photo = entry(Kind::Texture, &["foto", "céu"], "");
        assert!(MatCat::Textures.matches(&plain) && !MatCat::Patterns.matches(&plain) && !MatCat::Images.matches(&plain));
        assert!(MatCat::Patterns.matches(&pat) && !MatCat::Textures.matches(&pat));
        assert!(MatCat::Patterns.matches(&tiled), "subpasta conta");
        assert!(MatCat::Images.matches(&photo) && !MatCat::Textures.matches(&photo));
        // outros tipos só na sua categoria
        let pal = entry(Kind::Palette, &[], "");
        assert!(MatCat::Palettes.matches(&pal) && !MatCat::Textures.matches(&pal));
        assert!(!MatCat::Characters.matches(&pal));
        // toda categoria tem texto nos dois idiomas e ícone próprio
        let mut icons = std::collections::HashSet::new();
        for c in MatCat::ALL {
            assert!(icons.insert(c.icon()), "{c:?}");
            for lang in [crate::i18n::Lang::Pt, crate::i18n::Lang::En] {
                assert_ne!(crate::i18n::tr_in(lang, c.title_key()), c.title_key());
            }
        }
    }

    #[test]
    fn clicking_the_open_category_collapses_and_the_canvas_gets_the_space_back() {
        let mut m = MaterialsState::default();
        assert_eq!(m.open, None);
        assert_eq!(m.reserved(), BAR_W);
        m.toggle(MatCat::Textures);
        assert_eq!(m.open, Some(MatCat::Textures));
        assert!(m.reserved() > BAR_W + PANEL_MIN - 1.0);
        // outra categoria troca sem fechar (e limpa a etiqueta escolhida)
        m.tag = Some("x".into());
        m.toggle(MatCat::Palettes);
        assert_eq!((m.open, m.tag.clone()), (Some(MatCat::Palettes), None));
        m.toggle(MatCat::Palettes);
        assert_eq!(m.open, None);
        assert_eq!(m.reserved(), BAR_W);
        // largura fora dos limites é contida
        m.width = 5000.0;
        m.toggle(MatCat::Models);
        assert_eq!(m.reserved(), BAR_W + PANEL_MAX + 1.0);
    }
}

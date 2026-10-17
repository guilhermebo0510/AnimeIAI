//! Painel Prancha (Storyboard): grade de miniaturas dos painéis do storyboard aberto, com o nº do corte, o enquadramento, os
//! segundos e a ação embaixo de cada uma. A miniatura é o desenho do painel (`aiai_tools::preprod::panel_image`, o mesmo
//! render do animatic e da folha de e-conte) numa textura em cache: só se renderiza de novo quando o DESENHO do painel muda
//! (assinatura do conteúdo do corte), no máximo [`RENDER_BUDGET`] painéis por quadro, nunca todos a cada quadro.
//! A barra de cima gera o animatic (`ANIMATIC`) e os cortes do storyboard (`CUTS_FROM_STORYBOARD`).

use crate::app::actions;
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_core::{CoreSession, Project};
use aiai_tools::preprod::{self, Panel};
use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::json;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Largura da textura da miniatura (o canvas padrão do painel tem 480).
pub(crate) const THUMB_W: u32 = 480;
/// Painéis renderizados por quadro (os outros mostram a miniatura antiga até a vez deles).
pub(crate) const RENDER_BUDGET: u32 = 2;

/// Tamanho da textura de um painel de canvas `[w, h]`: largura `THUMB_W` (ou o canvas, se menor) e a mesma proporção.
pub(crate) fn thumb_size(canvas: [u32; 2]) -> [u32; 2] {
    let (cw, ch) = (canvas[0].max(1), canvas[1].max(1));
    let w = cw.min(THUMB_W);
    [w, ((w as f32 * ch as f32 / cw as f32).round() as u32).max(1)]
}

/// Assinatura do desenho de um painel: hash do conteúdo de todos os nós do corte. Muda quando o desenho muda (também no
/// desfazer, que devolve conteúdo antigo) e não muda com edições que não tocam o corte (roteiro, outros painéis).
pub(crate) fn drawing_signature(p: &Project, cut: &NodeId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for id in p.subtree(cut) {
        if let Some(n) = p.node(&id) {
            id.0.hash(&mut h);
            if let Ok(bytes) = serde_json::to_vec(&n.data) {
                bytes.hash(&mut h);
            }
        }
    }
    h.finish()
}

struct Thumb {
    /// `change_counter` em que a assinatura foi conferida pela última vez.
    checked: u64,
    sig: u64,
    size: [u32; 2],
    /// `None`: o render falhou (tenta de novo só quando o desenho mudar).
    tex: Option<egui::TextureHandle>,
}

/// Texturas das miniaturas, por painel.
#[derive(Default)]
pub(crate) struct ThumbCache {
    map: HashMap<String, Thumb>,
    /// Quantos renders de painel já aconteceram (testes e diagnóstico).
    pub renders: u64,
}

impl ThumbCache {
    /// Textura do painel; renderiza se o desenho mudou e ainda houver `budget`, senão devolve a antiga (ou nada).
    pub(crate) fn get(&mut self, ctx: &egui::Context, core: &CoreSession, panel: &Panel, canvas: [u32; 2], budget: &mut u32) -> Option<egui::TextureId> {
        let size = thumb_size(canvas);
        let counter = core.change_counter;
        let sig = panel.drawing_cut.as_ref().map(|c| drawing_signature(&core.project, c)).unwrap_or(0);
        if let Some(e) = self.map.get_mut(&panel.id.0) {
            if e.size == size && (e.checked == counter || e.sig == sig) {
                e.checked = counter;
                return e.tex.as_ref().map(|t| t.id());
            }
        }
        if *budget == 0 {
            ctx.request_repaint();
            return self.map.get(&panel.id.0).and_then(|e| e.tex.as_ref().map(|t| t.id()));
        }
        *budget -= 1;
        self.renders += 1;
        let tex = match preprod::panel_image(&core.project, &core.blobs, panel, size[0], size[1], Some([1.0, 1.0, 1.0])) {
            Ok(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_rgba8());
                Some(ctx.load_texture(format!("storyboard-panel-{}", panel.id.0), color, egui::TextureOptions::LINEAR))
            }
            Err(_) => None,
        };
        let id = tex.as_ref().map(|t| t.id());
        self.map.insert(panel.id.0.clone(), Thumb { checked: counter, sig, size, tex });
        id
    }

    /// Esquece as miniaturas de painéis que não existem mais.
    pub(crate) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.map.retain(|k, _| keep(k));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
}

/// Abre o corte de desenho do painel no workspace Layout (corte ativo + troca de workspace no fim do quadro).
pub(crate) fn open_drawing(env: &mut Env, p: &Panel) {
    let Some(cut) = p.drawing_cut.clone().filter(|c| env.core.project.node(c).is_some()) else {
        env.ed.notify(tr("board.no_drawing"));
        return;
    };
    env.ed.set_cut(cut.clone());
    env.ed.sel_node = Some(cut);
    env.ed.want_workspace = Some(WorkspaceId::Layout);
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some((sb, doc)) = actions::sync_story_sel(env.ed, env.core) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(tr("panel.board.empty")).color(t.text_dim));
            ui.add_space(8.0);
            let b = ui.button(tr("shots.new_storyboard"));
            env.note("board:new_storyboard", b.rect);
            if b.clicked() {
                super::shots::new_storyboard(env);
            }
        });
        return;
    };

    // ---- barra: animatic, cortes e abrir desenho
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(!doc.panels.is_empty(), egui::Button::new(tr("board.animatic")));
        env.note("board:animatic", b.rect);
        if b.on_hover_text(tr("board.animatic.tip")).clicked() {
            make_animatic(env, &sb);
        }
        let b = ui.add_enabled(!doc.panels.is_empty(), egui::Button::new(tr("board.cuts")));
        env.note("board:cuts", b.rect);
        if b.on_hover_text(tr("board.cuts.tip")).clicked() {
            make_cuts(env, &sb, doc.panels.iter().any(|p| p.scene.is_some()));
        }
        let sel = env.ed.sel_panel.as_ref().and_then(|id| doc.panel(id)).cloned();
        let b = ui.add_enabled(sel.as_ref().is_some_and(|p| p.drawing_cut.is_some()), egui::Button::new(tr("board.open_drawing")));
        env.note("board:open_drawing", b.rect);
        if let (true, Some(p)) = (b.on_hover_text(tr("board.open_drawing.tip")).clicked(), &sel) {
            open_drawing(env, p);
        }
        dim_label(ui, &t, &format!("{} {} · {:.1} s", doc.panels.len(), tr("shots.panels"), doc.total_seconds()));
    });
    ui.add_space(2.0);

    // ---- grade de miniaturas
    let ctx = ui.ctx().clone();
    let mut budget = RENDER_BUDGET;
    egui::ScrollArea::vertical().id_salt("board_scroll").auto_shrink([false, false]).show(ui, |ui| {
        if doc.panels.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("board.no_panels")).color(t.text_dim));
            return;
        }
        ui.add_space(3.0);
        let gap = 10.0;
        let avail = (ui.available_width() - 12.0).max(100.0);
        let cols = (((avail + gap) / (190.0 + gap)).floor() as usize).clamp(1, 6);
        let cw = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let ih = (cw * doc.canvas[1].max(1) as f32 / doc.canvas[0].max(1) as f32).round();
        let ch = ih + 8.0 + 46.0;
        ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
        for (row, chunk) in doc.panels.chunks(cols).enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(3.0);
                for (k, p) in chunk.iter().enumerate() {
                    let i = row * cols + k;
                    let tex = env.ed.thumbs.get(&ctx, env.core, p, doc.canvas, &mut budget);
                    cell(env, ui, i, p, tex, egui::vec2(cw, ch), ih);
                }
            });
        }
    });
    let alive: std::collections::HashSet<&str> = doc.panels.iter().map(|p| p.id.0.as_str()).collect();
    env.ed.thumbs.retain(|k| alive.contains(k));
}

fn cell(env: &mut Env, ui: &mut Ui, i: usize, p: &Panel, tex: Option<egui::TextureId>, size: egui::Vec2, ih: f32) {
    let t = env.t;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    env.note(format!("board:panel:{i}"), rect);
    let selected = env.ed.sel_panel.as_ref() == Some(&p.id);
    let painter = ui.painter();
    let thumb = Rect::from_min_size(rect.min, egui::vec2(size.x, ih));
    painter.rect_filled(thumb, 2.0, Color32::WHITE);
    match tex {
        Some(id) => {
            painter.image(id, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(thumb.center(), egui::Align2::CENTER_CENTER, if p.drawing_cut.is_some() { "…" } else { "—" }, FontId::proportional(18.0), Color32::from_gray(170));
        }
    }
    if selected {
        painter.rect_stroke(thumb.expand(1.0), 2.0, Stroke::new(2.5, t.accent), StrokeKind::Outside);
    } else {
        painter.rect_stroke(thumb, 2.0, Stroke::new(1.0, if resp.hovered() { t.text_dim } else { t.border }), StrokeKind::Inside);
    }
    let clip = Rect::from_min_max(egui::pos2(rect.left(), thumb.bottom()), rect.max);
    let painter = painter.with_clip_rect(clip);
    let head = format!("{}   {}   {:.1} s", p.cut_number, p.shot.code(), p.seconds);
    let g = painter.layout_no_wrap(head, FontId::proportional(12.5), t.text);
    painter.galley(egui::pos2(rect.left() + 1.0, thumb.bottom() + 5.0), g, t.text);
    if !p.action.trim().is_empty() {
        let mut job = egui::text::LayoutJob::simple(p.action.clone(), FontId::proportional(11.5), t.text_dim, (size.x - 2.0).max(20.0));
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = false;
        let g = painter.layout_job(job);
        painter.galley(egui::pos2(rect.left() + 1.0, thumb.bottom() + 22.0), g, t.text_dim);
    }
    if resp.clicked() {
        env.ed.sel_panel = Some(p.id.clone());
    }
    if resp.double_clicked() {
        open_drawing(env, p);
    }
}

/// Animatic (MP4) em `<pasta do projeto>/saida/animatic.mp4`: a pasta do projeto é a que contém a pasta `.aiai`.
fn make_animatic(env: &mut Env, sb: &NodeId) {
    let Some(path) = env.core.path.clone() else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or(path);
    let out = dir.join("saida").join("animatic.mp4");
    let r = actions::run(env.core, "ANIMATIC", json!({ "storyboard": sb, "out": out.display().to_string() }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        let secs = v.get("seconds").and_then(|s| s.as_f64()).unwrap_or(0.0);
        env.ed.notify(format!("{}: {} ({secs:.1} s)", tr("board.animatic_done"), out.display()));
    }
}

/// Gera Cena/Corte no primeiro episódio a partir dos painéis (um passo de desfazer).
fn make_cuts(env: &mut Env, sb: &NodeId, by_group: bool) {
    let Some(ep) = env.core.project.roots.episodes.first().cloned() else {
        env.ed.notify(tr("board.need_episode"));
        return;
    };
    let mut params = json!({ "storyboard": sb, "episode": ep, "scene_per_panel_group": by_group });
    if let Some(s) = env.ed.sel_script.clone() {
        params["script"] = json!(s);
    }
    let r = actions::run(env.core, "CUTS_FROM_STORYBOARD", params);
    if let Some(v) = actions::notify_err(env.ed, r) {
        let n = v.get("cuts").and_then(|c| c.as_array()).map(|c| c.len()).unwrap_or(0);
        env.ed.notify(format!("{n} {}", tr("board.cuts_done")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;
    use aiai_core::{Command, NodeKind};

    fn session_with_panel() -> (CoreSession, NodeId, Panel) {
        let mut core = CoreSession::new(Project::new("t"));
        core.author = AUTHOR.into();
        let v = actions::storyboard_op(&mut core, "create", json!({ "name": "Storyboard", "title": "t" })).unwrap();
        let sb = aiai_core::Id(v["storyboard"].as_str().unwrap().to_owned());
        actions::storyboard_op(&mut core, "panel_add", json!({ "storyboard": sb, "spec": { "action": "Hana entra." } })).unwrap();
        let panel = preprod::storyboard_read(&core.project, &sb).unwrap().panels.remove(0);
        (core, sb, panel)
    }

    #[test]
    fn thumbnail_size_keeps_the_canvas_aspect() {
        assert_eq!(thumb_size([480, 270]), [480, 270]);
        assert_eq!(thumb_size([1920, 1080]), [480, 270]);
        assert_eq!(thumb_size([240, 240]), [240, 240]);
        assert_eq!(thumb_size([0, 0]), [1, 1]);
    }

    #[test]
    fn signature_follows_the_drawing_and_ignores_other_edits() {
        let (mut core, sb, panel) = session_with_panel();
        let cut = panel.drawing_cut.clone().unwrap();
        let s0 = drawing_signature(&core.project, &cut);
        assert_eq!(s0, drawing_signature(&core.project, &cut));
        // editar o roteiro/outro painel não muda o desenho deste painel
        actions::storyboard_op(&mut core, "panel_add", json!({ "storyboard": sb, "spec": {} })).unwrap();
        assert_eq!(s0, drawing_signature(&core.project, &cut));
        // mexer no corte muda
        let NodeKind::Cut(mut cd) = core.project.node(&cut).unwrap().data.clone() else { panic!() };
        cd.duration += 5;
        core.apply_as(Command::ReplaceData { id: cut.clone(), data: NodeKind::Cut(cd) }, "t", None).unwrap();
        assert_ne!(s0, drawing_signature(&core.project, &cut));
    }

    #[test]
    fn cache_renders_once_per_drawing_and_respects_the_budget() {
        let (mut core, sb, panel) = session_with_panel();
        let ctx = egui::Context::default();
        let mut cache = ThumbCache::default();
        let mut budget = RENDER_BUDGET;
        assert!(cache.get(&ctx, &core, &panel, [480, 270], &mut budget).is_some());
        assert_eq!((cache.renders, budget), (1, RENDER_BUDGET - 1));
        // mesmo documento: nenhum render novo, nem gasto de orçamento
        assert!(cache.get(&ctx, &core, &panel, [480, 270], &mut budget).is_some());
        assert_eq!((cache.renders, budget), (1, RENDER_BUDGET - 1));
        // o documento mudou (outro painel), mas o desenho deste não: continua sem render
        actions::storyboard_op(&mut core, "panel_add", json!({ "storyboard": sb, "spec": {} })).unwrap();
        assert!(cache.get(&ctx, &core, &panel, [480, 270], &mut budget).is_some());
        assert_eq!(cache.renders, 1);
        // painel novo sem orçamento: sem miniatura ainda
        let other = preprod::storyboard_read(&core.project, &sb).unwrap().panels.remove(1);
        let mut none = 0;
        assert!(cache.get(&ctx, &core, &other, [480, 270], &mut none).is_none());
        assert_eq!(cache.renders, 1);
        // o desenho mudou: renderiza de novo
        let cut = panel.drawing_cut.clone().unwrap();
        let NodeKind::Cut(mut cd) = core.project.node(&cut).unwrap().data.clone() else { panic!() };
        cd.duration += 5;
        core.apply_as(Command::ReplaceData { id: cut, data: NodeKind::Cut(cd) }, "t", None).unwrap();
        let mut budget = RENDER_BUDGET;
        assert!(cache.get(&ctx, &core, &panel, [480, 270], &mut budget).is_some());
        assert_eq!(cache.renders, 2);
        cache.retain(|k| k == panel.id.0.as_str());
        assert_eq!(cache.len(), 1);
    }
}

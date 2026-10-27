//! Painel Linha do tempo: régua de quadros com cabeçote (clicar/arrastar rola o quadro), uma linha por camada com os blocos
//! de exposição do timesheet (número do cel e a marca do papel: ● chave, ◐ breakdown, ○ intercalação), clique na célula
//! = expõe o cel escolhido naquele quadro (cria a trilha e o cel se faltarem; Shift = quadro vazio; Ctrl/Alt = segurar),
//! menu de contexto (exposição, papel do cel, trava do douga), laço, duração e reprodução na taxa do projeto (os quadros
//! são renderizados à frente em segundo plano; os que não dão tempo são pulados, e o fps real aparece).
//!
//! Densidade compacta (padrão na Animação): linhas de 14 px, cabeçalho de 110 px e nomes cortados com "…". Embaixo da
//! régua ficam a trilha-resumo das marcas de papel de todas as camadas (clique leva à marca) e, se o corte tem som, uma
//! linha fina com a forma de onda dos clipes. Pastas viram uma linha de resumo que recolhe as filhas.

use crate::app::anim::{self, Density};
use crate::app::commands::{self as cmds, Exposure, NodeId};
use crate::app::editor::*;
use crate::app::panels::commit_on_release;
use crate::app::panels::layers::kind_icon;
use crate::app::ui_kit::{Env, body_ui, paint_icon};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::*;
use egui::{Align2, Color32, FontId, Id, Rect, Sense, Stroke, Ui};
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

pub(crate) const HEADER_W: f32 = 150.0;
pub(crate) const RULER_H: f32 = 22.0;
pub(crate) const ROW_H: f32 = 22.0;
/// Medidas da densidade compacta.
pub(crate) const HEADER_W_COMPACT: f32 = 110.0;
pub(crate) const RULER_H_COMPACT: f32 = 18.0;
pub(crate) const ROW_H_COMPACT: f32 = 14.0;

/// Blocos de exposição de uma trilha: (quadro inicial, quadro final exclusivo, o que aparece). Entradas repetidas que não
/// mudam nada se fundem; o último bloco vai até o fim do corte.
pub(crate) fn exposure_runs(track: &Track, duration: u32) -> Vec<(u32, u32, CellRef)> {
    let mut runs: Vec<(u32, u32, CellRef)> = Vec::new();
    for (&f, &c) in track.cells.range(..duration) {
        match runs.last_mut() {
            Some(last) if last.2 == c => {}
            Some(last) => {
                last.1 = f;
                runs.push((f, duration, c));
            }
            None => runs.push((f, duration, c)),
        }
    }
    runs
}

/// Reprodução: avança o quadro pelo relógio (na taxa do projeto) dentro do laço.
pub(crate) fn tick(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    if !ed.playback.playing {
        return;
    }
    let Some(cut) = ed.cut.clone() else {
        ed.playback.playing = false;
        return;
    };
    let dur = cmds::cut_data(&core.project, &cut).map(|c| c.duration).unwrap_or(1);
    let fps = core.project.settings.fps().max(1.0);
    let (lo, hi) = ed.playback.range(dur);
    let len = (hi - lo + 1) as u64;
    let n = (ed.playback.started.elapsed().as_secs_f64() * fps).floor() as u64;
    let rel = ed.playback.start_pos.clamp(lo, hi).saturating_sub(lo) as u64 + n;
    if !ed.playback.looping && rel >= len {
        ed.frame = hi;
        ed.playback.playing = false;
    } else {
        ed.frame = lo + (rel % len) as u32;
    }
    ctx.request_repaint();
}

pub(crate) fn toggle_play(ed: &mut Editor, dur: u32) {
    let pb = &mut ed.playback;
    if pb.playing {
        pb.playing = false;
        return;
    }
    let (lo, hi) = pb.range(dur);
    pb.start_pos = if ed.frame >= hi && !pb.looping { lo } else { ed.frame.clamp(lo, hi) };
    pb.started = Instant::now();
    pb.playing = true;
    pb.shown_times.clear();
    pb.fps_real = 0.0;
    pb.last_shown = None;
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 4.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("timeline.no_cut")).color(t.text_dim));
        return;
    };
    let Some(cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
    let dur = cd.duration;
    let fps = env.core.project.settings.fps();
    let compact = env.ed.anim.density(env.ws) == Density::Compact;

    // ---- barra de transporte
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
        let isz = if compact { 20.0 } else { 24.0 };
        if env.icon_button_sized(ui, IconId::StepBack, "timeline.step_back", false, true, "timeline:step_back", isz, false).clicked() {
            env.ed.playback.playing = false;
            env.ed.frame = env.ed.frame.saturating_sub(1);
        }
        let playing = env.ed.playback.playing;
        let play = env.icon_button_sized(ui, if playing { IconId::Pause } else { IconId::Play }, if playing { "timeline.pause" } else { "timeline.play" }, playing, true, "timeline:play", isz, false);
        if play.clicked() {
            toggle_play(env.ed, dur);
        }
        if env.icon_button_sized(ui, IconId::StepForward, "timeline.step_forward", false, true, "timeline:step_forward", isz, false).clicked() {
            env.ed.playback.playing = false;
            env.ed.frame = (env.ed.frame + 1).min(dur - 1);
        }
        let looping = env.ed.playback.looping;
        if env.icon_button_sized(ui, IconId::Loop, "timeline.loop", looping, true, "timeline:loop", isz, false).clicked() {
            env.ed.playback.looping = !looping;
        }
        let mut f1 = env.ed.frame + 1;
        let r = ui.add(egui::DragValue::new(&mut f1).range(1..=dur).speed(0.25).prefix(format!("{} ", tr("timeline.frame"))));
        env.note("timeline:frame", r.rect);
        if r.changed() {
            env.ed.playback.playing = false;
            env.ed.frame = f1 - 1;
        }
        ui.label(egui::RichText::new(format!("/ {dur}")).color(t.text_dim));
        if let Some(v) = commit_on_release(ui, Id::new("cut_duration"), dur, |ui, v| ui.add(egui::DragValue::new(v).range(1..=36000).speed(0.25).prefix(format!("{} ", tr("timeline.duration"))).suffix(format!(" {}", tr("unit.frames_short"))))) {
            if let Err(e) = cmds::set_cut_props(env.core, &cut, serde_json::json!({ "duration": v })) {
                env.ed.notify(e);
            }
        }
        // laço (1-based; "fim" 0 = até o fim do corte)
        let (lo, hi) = env.ed.playback.range(dur);
        let (mut lin, mut lout) = (lo + 1, hi + 1);
        ui.label(egui::RichText::new(tr("timeline.loop_range")).size(12.0).color(t.text_dim));
        let a = ui.add(egui::DragValue::new(&mut lin).range(1..=dur).speed(0.25));
        let b = ui.add(egui::DragValue::new(&mut lout).range(1..=dur).speed(0.25));
        env.note("timeline:loop_in", a.rect);
        env.note("timeline:loop_out", b.rect);
        if a.changed() {
            env.ed.playback.loop_in = lin - 1;
        }
        if b.changed() {
            env.ed.playback.loop_out = if lout >= dur { 0 } else { lout - 1 };
        }
        // o que a imagem mostra neste quadro e o cel escolhido para a próxima exposição são coisas diferentes
        // (auditoria V2: "Cel escolhido: 2" com o quadro 1 mostrando o Cel 1 confundia)
        let active = env.ed.active_layer(env.core);
        let shown = active.as_ref().and_then(|l| cmds::exposed_number(&env.core.project, &cut, l, env.ed.frame));
        let shown_s = shown.map(|n| format!("{} {n}", tr("layers.cel"))).unwrap_or_else(|| tr("timeline.no_cel").to_owned());
        let r = ui.label(egui::RichText::new(format!("{} {shown_s}", tr("timeline.exposed_cel"))).size(12.0).color(t.text)).on_hover_text(tr("timeline.exposed_cel.tip"));
        env.note("timeline:exposed_cel", r.rect);
        if let Some(n) = env.ed.cel_number.filter(|n| Some(*n) != shown) {
            let r = ui.label(egui::RichText::new(format!("{} {} {n}", tr("timeline.selected_cel"), tr("layers.cel"))).size(12.0).color(t.accent)).on_hover_text(tr("timeline.selected_cel.tip"));
            env.note("timeline:selected_cel", r.rect);
        }
        let pb = &env.ed.playback;
        if pb.playing {
            ui.label(egui::RichText::new(format!("{:.1} / {:.2} fps", pb.fps_real, fps)).monospace().size(12.0).color(t.accent));
        } else {
            ui.label(egui::RichText::new(format!("{fps:.3} fps")).monospace().size(12.0).color(t.text_dim));
        }
        // densidade: compacta = linhas de 14 px (o padrão da Animação)
        let key = env.ed.keys.label(crate::keymap::Action::TimelineDensity);
        let r = ui.selectable_label(compact, tr("timeline.compact")).on_hover_text(format!("{} ({key})", tr("timeline.compact_tip")));
        env.note("timeline:density", r.rect);
        if r.clicked() {
            env.ed.anim.toggle_density(env.ws);
        }
    });

    // ---- régua + linhas
    let area = Rect::from_min_max(egui::pos2(ui.min_rect().left(), ui.cursor().top() + 2.0), ui.max_rect().max);
    let header_w = if compact { HEADER_W_COMPACT } else { HEADER_W };
    if area.width() < header_w + 40.0 || area.height() < RULER_H + 10.0 {
        return;
    }
    env.note("timeline:area", area);
    grid(env, ui, area, &cut, &cd, compact);
}

/// Uma linha da grade: camada de desenho ou pasta (que mostra o resumo das filhas e recolhe).
struct GridRow {
    id: NodeId,
    depth: usize,
    l: LayerData,
    name: String,
    group: bool,
    collapsed: bool,
    /// Índice entre as linhas de camada de desenho (o `timeline:row:N` da bridge não conta as pastas).
    layer_idx: usize,
}

/// Linhas visíveis: camadas que guardam desenho (fora texto) e as pastas que têm alguma; filhas de pasta recolhida somem.
fn grid_rows(p: &Project, cut: &NodeId, collapsed: &HashSet<NodeId>) -> Vec<GridRow> {
    let is_drawing = |l: &LayerData| l.kind.holds_cels() && l.kind != LayerType::Text;
    let mut out = Vec::new();
    let mut skip: Option<usize> = None;
    let mut li = 0;
    for r in cmds::layer_rows(p, cut) {
        if let Some(d) = skip {
            if r.depth > d {
                continue;
            }
            skip = None;
        }
        let Some(l) = cmds::layer_data(p, &r.id).cloned() else { continue };
        let name = p.node(&r.id).map(|n| n.name.clone()).unwrap_or_default();
        if l.kind == LayerType::Group {
            let has = p.subtree(&r.id).iter().skip(1).any(|d| cmds::layer_data(p, d).is_some_and(is_drawing));
            if !has {
                continue;
            }
            let c = collapsed.contains(&r.id);
            if c {
                skip = Some(r.depth);
            }
            out.push(GridRow { id: r.id, depth: r.depth, l, name, group: true, collapsed: c, layer_idx: usize::MAX });
        } else if is_drawing(&l) {
            out.push(GridRow { id: r.id, depth: r.depth, l, name, group: false, collapsed: false, layer_idx: li });
            li += 1;
        }
    }
    out
}

/// Marcas de papel da trilha-resumo: em cada quadro onde algum desenho começa, o papel mais forte (chave > breakdown >
/// intercalação).
pub(crate) fn key_summary(p: &Project, cut: &NodeId) -> BTreeMap<u32, CelRole> {
    let rank = |r: CelRole| match r {
        CelRole::Key => 0,
        CelRole::Breakdown => 1,
        CelRole::Inbetween => 2,
    };
    let mut out: BTreeMap<u32, CelRole> = BTreeMap::new();
    for r in cmds::layer_rows(p, cut) {
        if !cmds::layer_data(p, &r.id).is_some_and(|l| l.kind.holds_cels() && l.kind != LayerType::Text && !l.kind.is_non_rendering()) {
            continue;
        }
        for (f, _, role) in anim::drawing_starts(p, cut, &r.id) {
            let e = out.entry(f).or_insert(role);
            if rank(role) < rank(*e) {
                *e = role;
            }
        }
    }
    out
}

/// Corta o nome com "…" para caber em `max_w` pontos.
fn elide(painter: &egui::Painter, text: &str, font: &FontId, max_w: f32) -> String {
    let fits = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x <= max_w;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (1..chars.len()).rev() {
        let s: String = chars[..n].iter().collect::<String>() + "…";
        if fits(&s) {
            return s;
        }
    }
    "…".to_owned()
}

fn grid(env: &mut Env, ui: &mut Ui, area: Rect, cut: &NodeId, cd: &CutData, compact: bool) {
    let t = env.t;
    let dur = cd.duration;
    let fps = env.core.project.settings.fps().round().max(1.0) as u32;
    let fps_f = env.core.project.settings.fps().max(1.0) as f32;
    let (header_w, ruler_h, row_h) = if compact { (HEADER_W_COMPACT, RULER_H_COMPACT, ROW_H_COMPACT) } else { (HEADER_W, RULER_H, ROW_H) };
    env.ed.anim.tl = anim::TlMetrics { header_w, ruler_h, row_h };
    let (font_name, font_cell) = if compact { (10.5, 10.0) } else { (12.0, 11.5) };
    let rows = grid_rows(&env.core.project, cut, &env.ed.anim.collapsed);
    let summary = key_summary(&env.core.project, cut);
    let douga = anim::cut_phase(&env.core.project, cut) == CutPhase::Douga;
    // faixa fixa embaixo da régua: trilha-resumo de chaves e (se o corte tem som) a linha fina do áudio
    let keys_h = if compact { 12.0 } else { 16.0 };
    let audio_h = if cd.audio.is_empty() { 0.0 } else if compact { 12.0 } else { 18.0 };
    let band_h = keys_h + audio_h;
    let active = env.ed.active_layer(env.core);
    let cw = env.ed.tl_cell_w.clamp(4.0, 48.0);
    let x0 = area.min.x + header_w;
    let view_w = area.width() - header_w;
    let rows_top = area.min.y + ruler_h + band_h;
    let view_h = (area.max.y - rows_top).max(0.0);
    let content_h = rows.len() as f32 * row_h;
    let resp = ui.interact(area, ui.id().with("tl_area"), Sense::click_and_drag());
    let hovered = resp.hovered();

    // ---- rolagem e zoom pela roda
    if hovered {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        for e in events {
            if let egui::Event::MouseWheel { unit, delta, .. } = e {
                let k = match unit {
                    egui::MouseWheelUnit::Line => 40.0,
                    egui::MouseWheelUnit::Page => 400.0,
                    egui::MouseWheelUnit::Point => 1.0,
                };
                if mods.command {
                    let f = 1.12f32.powf(delta.y * k / 40.0);
                    let at = ui.input(|i| i.pointer.hover_pos()).map(|p| p.x).unwrap_or(x0);
                    let frame_under = (at - x0 + env.ed.tl_scroll.x) / cw;
                    let ncw = (cw * f).clamp(4.0, 48.0);
                    env.ed.tl_cell_w = ncw;
                    env.ed.tl_scroll.x = frame_under * ncw - (at - x0);
                } else if mods.shift || delta.x.abs() > delta.y.abs() {
                    env.ed.tl_scroll.x -= (if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y }) * k;
                } else {
                    env.ed.tl_scroll.y -= delta.y * k * (row_h / ROW_H);
                }
            }
        }
    }
    let cw = env.ed.tl_cell_w.clamp(4.0, 48.0);
    // acompanha o cabeçote quando o quadro muda (reprodução, setas)
    let head_x = env.ed.frame as f32 * cw;
    let last_seen: u32 = ui.data(|d| d.get_temp(Id::new("tl_last_frame"))).unwrap_or(u32::MAX);
    if last_seen != env.ed.frame {
        ui.data_mut(|d| d.insert_temp(Id::new("tl_last_frame"), env.ed.frame));
        if head_x < env.ed.tl_scroll.x {
            env.ed.tl_scroll.x = (head_x - 20.0).max(0.0);
        } else if head_x + cw > env.ed.tl_scroll.x + view_w {
            env.ed.tl_scroll.x = head_x + cw - view_w + 40.0;
        }
    }
    env.ed.tl_scroll.x = env.ed.tl_scroll.x.clamp(0.0, (dur as f32 * cw - view_w + 40.0).max(0.0));
    env.ed.tl_scroll.y = env.ed.tl_scroll.y.clamp(0.0, (content_h - view_h).max(0.0));
    let (sx, sy) = (env.ed.tl_scroll.x, env.ed.tl_scroll.y);

    let frame_at = |x: f32| -> u32 { (((x - x0 + sx) / cw).floor().max(0.0) as u32).min(dur - 1) };
    let row_at = |y: f32| -> Option<usize> {
        let r = ((y - rows_top + sy) / row_h).floor();
        (y >= rows_top && r >= 0.0 && (r as usize) < rows.len()).then_some(r as usize)
    };
    // linha de camada (não pasta) sob o ponteiro
    let layer_row_at = |y: f32| row_at(y).filter(|r| !rows[*r].group);

    // ---- interação
    let press = ui.input(|i| i.pointer.press_origin());
    let ptr = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
    let in_cells = |p: egui::Pos2| p.x >= x0 && p.y >= rows_top;
    let in_ruler = |p: egui::Pos2| p.x >= x0 && p.y < area.min.y + ruler_h && p.y >= area.min.y;
    let in_band = |p: egui::Pos2| p.x >= x0 && p.y >= area.min.y + ruler_h && p.y < rows_top;
    if resp.dragged_by(egui::PointerButton::Primary) {
        // arrastar a régua, a faixa ou as células rola o quadro; arrastar o cabeçalho não faz nada
        if let (Some(p), Some(o)) = (ptr, press) {
            if (in_ruler(o) || in_band(o) || in_cells(o)) && p.x >= area.min.x {
                env.ed.playback.playing = false;
                env.ed.frame = frame_at(p.x.max(x0));
                if let Some(r) = layer_row_at(o.y) {
                    if in_cells(o) && env.ed.layer.as_ref() != Some(&rows[r].id) {
                        env.ed.layer = Some(rows[r].id.clone());
                        env.ed.cel_number = None;
                    }
                }
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(p) = ptr {
            env.ed.tl_ctx_cell = layer_row_at(p.y).filter(|_| in_cells(p)).map(|r| (rows[r].id.clone(), frame_at(p.x)));
        }
    }
    if resp.clicked() {
        if let Some(p) = ptr {
            let mods = ui.input(|i| i.modifiers);
            if in_ruler(p) {
                env.ed.playback.playing = false;
                env.ed.frame = frame_at(p.x);
            } else if in_band(p) {
                // trilha-resumo: o clique perto de uma marca vai para ela
                let f = frame_at(p.x);
                let near = summary.range(f.saturating_sub(1)..=f + 1).min_by_key(|(k, _)| (**k as i64 - f as i64).abs()).map(|(k, _)| *k);
                env.ed.playback.playing = false;
                env.ed.frame = near.unwrap_or(f);
            } else if let Some(r) = row_at(p.y).filter(|r| rows[*r].group) {
                // pasta: o triângulo (ou o cabeçalho) recolhe/abre; nas células só leva o cabeçote
                if p.x < x0 {
                    let id = rows[r].id.clone();
                    if !env.ed.anim.collapsed.remove(&id) {
                        env.ed.anim.collapsed.insert(id);
                    }
                } else {
                    env.ed.playback.playing = false;
                    env.ed.frame = frame_at(p.x);
                }
            } else if in_cells(p) {
                if let Some(r) = layer_row_at(p.y) {
                    let lid = rows[r].id.clone();
                    let f = frame_at(p.x);
                    let was_active = active.as_ref() == Some(&lid);
                    env.ed.playback.playing = false;
                    env.ed.frame = f;
                    env.ed.layer = Some(lid.clone());
                    if !was_active {
                        env.ed.cel_number = None;
                    } else {
                        let what = if mods.shift {
                            Exposure::Empty
                        } else if mods.command || mods.alt {
                            Exposure::Hold
                        } else {
                            let n = env.ed.cel_number.or_else(|| cmds::exposed_number(&env.core.project, cut, &lid, f)).or_else(|| cmds::layer_cels(&env.core.project, &lid).first().map(|(n, _)| *n)).unwrap_or(1);
                            Exposure::Cel(n)
                        };
                        if let Err(e) = cmds::set_exposure(env.core, cut, &lid, f, what) {
                            env.ed.notify(e);
                        } else if let Exposure::Cel(n) = what {
                            env.ed.cel_number = Some(n);
                        }
                    }
                }
            } else if let Some(r) = layer_row_at(p.y) {
                if env.ed.layer.as_ref() != Some(&rows[r].id) {
                    env.ed.cel_number = None;
                }
                env.ed.layer = Some(rows[r].id.clone());
            }
        }
    }
    let ctx_cell = env.ed.tl_ctx_cell.clone();
    resp.context_menu(|ui| {
        let Some((lid, f)) = ctx_cell.clone() else {
            ui.label(tr("timeline.ctx.none"));
            return;
        };
        let n = env.ed.cel_number.or_else(|| cmds::exposed_number(&env.core.project, cut, &lid, f)).unwrap_or(1);
        let mut act: Option<Exposure> = None;
        let b = ui.button(format!("{} {n} {} {}", tr("timeline.ctx.expose"), tr("timeline.ctx.at"), f + 1));
        env.note("timeline:ctx.expose", b.rect);
        if b.clicked() {
            act = Some(Exposure::Cel(n));
            ui.close();
        }
        let b = ui.button(tr("timeline.ctx.hold"));
        env.note("timeline:ctx.hold", b.rect);
        if b.clicked() {
            act = Some(Exposure::Hold);
            ui.close();
        }
        let b = ui.button(tr("timeline.ctx.clear"));
        env.note("timeline:ctx.clear", b.rect);
        if b.clicked() {
            act = Some(Exposure::Empty);
            ui.close();
        }
        // Intercalar até a próxima chave (auditoria 4.6a): os desenhos entre este cel e o próximo da camada, em 2s
        if let Some(plan) = crate::app::anim::inbetween_plan(&env.core.project, cut, &lid, f) {
            ui.separator();
            let b = ui.button(format!("{} ({})", tr("timeline.ctx.inbetween"), plan.count));
            env.note("timeline:ctx.inbetween", b.rect);
            if b.on_hover_text(tr("timeline.ctx.inbetween.tip")).clicked() {
                crate::app::anim::run_inbetween(env, cut, &lid, &plan);
                ui.close();
            }
        }
        // papel do cel exposto nesta célula (chave, breakdown, intercalação) e a trava do douga
        if let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &lid, f) {
            ui.separator();
            let role = cmds::cel_data(&env.core.project, &cel).map(|c| c.role).unwrap_or_default();
            for (r, name) in [(CelRole::Key, "key"), (CelRole::Breakdown, "breakdown"), (CelRole::Inbetween, "inbetween")] {
                let b = ui.selectable_label(role == r, tr(cmds::cel_role_key(r)));
                env.note(format!("timeline:ctx.role.{name}"), b.rect);
                if b.clicked() {
                    if let Err(e) = cmds::set_cel_role(env.core, &cel, r) {
                        env.ed.notify(e);
                    }
                    ui.close();
                }
            }
            if douga && cmds::cel_data(&env.core.project, &cel).is_some_and(|c| c.role == CelRole::Key) {
                let locked = !env.ed.anim.unlocked.contains(&cel);
                let b = ui.button(tr(if locked { "anim.lock.unlock" } else { "anim.lock.relock" }));
                env.note("timeline:ctx.lock", b.rect);
                if b.clicked() {
                    if locked {
                        env.ed.anim.unlocked.insert(cel.clone());
                    } else {
                        env.ed.anim.unlocked.remove(&cel);
                    }
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button(tr("timeline.ctx.loop_in")).clicked() {
            env.ed.playback.loop_in = f;
            ui.close();
        }
        if ui.button(tr("timeline.ctx.loop_out")).clicked() {
            env.ed.playback.loop_out = if f + 1 >= dur { 0 } else { f };
            ui.close();
        }
        if let Some(a) = act {
            env.ed.layer = Some(lid.clone());
            if let Err(e) = cmds::set_exposure(env.core, cut, &lid, f, a) {
                env.ed.notify(e);
            }
        }
    });
    // espaço toca/pausa com o ponteiro sobre a linha do tempo
    if hovered && !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
        toggle_play(env.ed, dur);
    }

    // ---- pintura
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 4.0, t.bg_panel);
    // régua
    let ruler = Rect::from_min_size(area.min, egui::vec2(area.width(), ruler_h));
    painter.rect_filled(ruler, 0.0, t.bg_header);
    let f_lo = (sx / cw).floor().max(0.0) as u32;
    let f_hi = (((sx + view_w) / cw).ceil() as u32 + 1).min(dur);
    let label_step = [1u32, 2, 5, 10, 12, 24, 48, 96].into_iter().find(|s| *s as f32 * cw >= 30.0).unwrap_or(96);
    let cells_clip = Rect::from_min_max(egui::pos2(x0, area.min.y), area.max);
    let cp = painter.with_clip_rect(cells_clip);
    let (pb_lo, pb_hi) = env.ed.playback.range(dur);
    if env.ed.playback.looping && (pb_lo > 0 || pb_hi + 1 < dur) {
        let band = Rect::from_min_max(egui::pos2(x0 - sx + pb_lo as f32 * cw, ruler.top()), egui::pos2(x0 - sx + (pb_hi + 1) as f32 * cw, ruler.bottom()));
        cp.rect_filled(band, 0.0, t.accent_dim);
    }
    for f in f_lo..f_hi {
        let x = x0 - sx + f as f32 * cw;
        let major = f % fps == 0;
        cp.vline(x, (ruler.bottom() - if major { 9.0 } else { 5.0 }).max(ruler.top())..=ruler.bottom(), Stroke::new(1.0, t.border));
        if f % label_step == 0 {
            cp.text(egui::pos2(x + 3.0, ruler.top() + 1.0), Align2::LEFT_TOP, (f + 1).to_string(), FontId::proportional(if compact { 9.5 } else { 10.5 }), t.text_dim);
        }
    }
    painter.hline(area.x_range(), ruler.bottom(), Stroke::new(1.0, t.border));

    // faixa: trilha-resumo das marcas de papel (todas as camadas)
    let keys_r = Rect::from_min_size(egui::pos2(area.min.x, ruler.bottom()), egui::vec2(area.width(), keys_h));
    env.note("timeline:keys", Rect::from_min_max(egui::pos2(x0, keys_r.top()), keys_r.max));
    painter.rect_filled(keys_r, 0.0, t.bg_header.gamma_multiply(0.6));
    painter.text(egui::pos2(area.min.x + 6.0, keys_r.center().y), Align2::LEFT_CENTER, tr("timeline.keys_track"), FontId::proportional(if compact { 9.5 } else { 10.5 }), t.text_dim);
    let mr = (keys_h * 0.3).min(cw * 0.42).max(2.0);
    for (&f, &role) in summary.range(f_lo..f_hi) {
        let c = egui::pos2(x0 - sx + (f as f32 + 0.5) * cw, keys_r.center().y);
        anim::paint_role_mark(&cp, c, mr, role);
    }
    // faixa: áudio do corte numa linha fina com a forma de onda
    if audio_h > 0.0 {
        let ar = Rect::from_min_size(egui::pos2(area.min.x, keys_r.bottom()), egui::vec2(area.width(), audio_h));
        env.note("timeline:audio", Rect::from_min_max(egui::pos2(x0, ar.top()), ar.max));
        painter.text(egui::pos2(area.min.x + 6.0, ar.center().y), Align2::LEFT_CENTER, tr("timeline.audio_track"), FontId::proportional(if compact { 9.5 } else { 10.5 }), t.text_dim);
        for a in &cd.audio {
            let secs = super::audio::clip_seconds(&mut env.ed.audio, env.core, a);
            let (bx0, bx1) = (x0 - sx + a.start_frame as f32 * cw, x0 - sx + (a.start_frame as f32 + secs * fps_f) * cw);
            if bx1 < x0 || bx0 > area.max.x {
                continue;
            }
            let col = match a.track {
                AudioTrack::Dialogue => Color32::from_rgb(90, 160, 230),
                AudioTrack::Sfx => Color32::from_rgb(230, 160, 70),
                AudioTrack::Bgm => Color32::from_rgb(150, 110, 220),
                AudioTrack::Ambience => Color32::from_rgb(110, 190, 130),
            };
            let bar = Rect::from_min_max(egui::pos2(bx0, ar.top() + 1.5), egui::pos2(bx1.max(bx0 + 2.0), ar.bottom() - 1.5));
            cp.rect_filled(bar, 2.0, col.gamma_multiply(0.30));
            if let Some(peaks) = super::audio::wave_peaks(&mut env.ed.audio, env.core, &a.asset) {
                let mid = bar.center().y;
                let half = bar.height() / 2.0;
                let (from, to) = (bar.left().max(x0).floor() as i32, bar.right().min(area.max.x).ceil() as i32);
                for px in from..to {
                    let tsec = (px as f32 - bx0) / cw / fps_f + a.trim_start;
                    let v = peaks.get((tsec * 100.0).max(0.0) as usize).copied().unwrap_or(0.0);
                    if v > 0.01 {
                        cp.vline(px as f32 + 0.5, (mid - v * half)..=(mid + v * half), Stroke::new(1.0, col));
                    }
                }
            }
        }
    }
    painter.hline(area.x_range(), rows_top, Stroke::new(1.0, t.border.gamma_multiply(0.8)));

    // linhas
    let rows_clip = Rect::from_min_max(egui::pos2(area.min.x, rows_top), area.max);
    let rp = painter.with_clip_rect(rows_clip);
    let rcp = painter.with_clip_rect(Rect::from_min_max(egui::pos2(x0, rows_top), area.max));
    let selected_cel = env.ed.cel_number;
    let name_font = FontId::proportional(font_name);
    for (i, row) in rows.iter().enumerate() {
        let y = rows_top + i as f32 * row_h - sy;
        if y + row_h < rows_top || y > area.max.y {
            continue;
        }
        let l = &row.l;
        let row_rect = Rect::from_min_max(egui::pos2(area.min.x, y), egui::pos2(area.max.x, y + row_h));
        let cells_rect = Rect::from_min_max(egui::pos2(x0, y), egui::pos2(area.max.x, y + row_h));
        if row.group {
            env.note(format!("timeline:group:{i}"), row_rect);
        } else {
            env.note(format!("timeline:row:{}", row.layer_idx), cells_rect);
        }
        let is_active = active.as_ref() == Some(&row.id);
        if is_active {
            rp.rect_filled(row_rect, 0.0, t.accent_dim.gamma_multiply(0.45));
        } else if row.group {
            rp.rect_filled(row_rect, 0.0, t.bg_header.gamma_multiply(0.7));
        } else if i % 2 == 1 {
            rp.rect_filled(row_rect, 0.0, t.bg_header.gamma_multiply(0.45));
        }
        // quadros: linhas finas e segundos
        for f in f_lo..f_hi {
            let x = x0 - sx + f as f32 * cw;
            if cw >= 9.0 || f % fps == 0 {
                rcp.vline(x, y..=y + row_h, Stroke::new(1.0, if f % fps == 0 { t.border } else { t.border.gamma_multiply(0.45) }));
            }
        }
        let pad = if compact { 1.0 } else { 2.0 };
        let block = |from: u32, to: u32| Rect::from_min_max(egui::pos2(x0 - sx + from as f32 * cw + 1.0, y + pad), egui::pos2(x0 - sx + to as f32 * cw - 1.0, y + row_h - pad));
        if row.group {
            // resumo da pasta: um tique onde algum desenho das filhas começa, na cor do papel
            let p = &env.core.project;
            for d in p.subtree(&row.id).iter().skip(1) {
                if !cmds::layer_data(p, d).is_some_and(|l| l.kind.holds_cels() && l.kind != LayerType::Text) {
                    continue;
                }
                for (f, _, role) in anim::drawing_starts(p, cut, d) {
                    if f < f_lo || f >= f_hi {
                        continue;
                    }
                    let x = x0 - sx + f as f32 * cw;
                    rcp.rect_filled(Rect::from_min_max(egui::pos2(x + 1.0, y + row_h * 0.3), egui::pos2(x + (cw - 1.0).max(2.0), y + row_h * 0.7)), 1.0, anim::role_color(role).gamma_multiply(0.75));
                }
            }
        } else {
            // blocos de exposição com a marca do papel (● chave, ◐ breakdown, ○ intercalação)
            let cels = cmds::layer_cels(&env.core.project, &row.id);
            match cd.xsheet.track_for_layer(&row.id) {
                Some(track) => {
                    for (a, b, c) in exposure_runs(track, dur) {
                        if b <= f_lo || a >= f_hi {
                            continue;
                        }
                        let r = block(a, b);
                        match c {
                            CellRef::Cel { number } => {
                                let sel = is_active && selected_cel == Some(number);
                                rcp.rect(r, 3.0, if sel { t.accent_dim } else { t.bg_widget }, Stroke::new(1.0, if sel { t.accent } else { t.border }), egui::StrokeKind::Inside);
                                let cel = env.core.project.cel_by_number(&row.id, number).map(|n| n.id.clone());
                                let role = cel.as_ref().and_then(|c| cmds::cel_data(&env.core.project, c)).map(|c| c.role);
                                let mut tx = r.left().max(x0) + 4.0;
                                if let Some(role) = role.filter(|_| r.width() >= row_h * 0.9) {
                                    let mr = (row_h * 0.22).max(2.5);
                                    anim::paint_role_mark(&rcp, egui::pos2(tx + mr, y + row_h / 2.0), mr, role);
                                    tx += mr * 2.0 + 3.0;
                                }
                                if r.right() - tx > 6.0 {
                                    rcp.with_clip_rect(r.intersect(rcp.clip_rect())).text(egui::pos2(tx, y + row_h / 2.0), Align2::LEFT_CENTER, number.to_string(), FontId::proportional(font_cell), t.text);
                                }
                                // douga: o cel-chave com desenho está travado
                                if douga && r.width() >= row_h * 3.0 && cel.as_ref().is_some_and(|c| anim::key_cel_locked(&env.core.project, &env.ed.anim.unlocked, c)) {
                                    let s = (row_h - 4.0).max(9.0);
                                    paint_icon(env.icons, &rcp, IconId::Lock, Rect::from_center_size(egui::pos2(r.right() - s / 2.0 - 2.0, y + row_h / 2.0), egui::vec2(s, s)), t.text_dim, false);
                                }
                            }
                            CellRef::Empty => {
                                rcp.rect(r, 3.0, Color32::TRANSPARENT, Stroke::new(1.0, t.text_dim.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                                let mut x = r.left() - row_h;
                                while x < r.right() {
                                    let (p0, p1) = (egui::pos2(x, r.bottom()), egui::pos2(x + row_h - 4.0, r.top()));
                                    rcp.with_clip_rect(r).line_segment([p0, p1], Stroke::new(1.0, t.text_dim.gamma_multiply(0.35)));
                                    x += 7.0;
                                }
                            }
                        }
                    }
                }
                None => {
                    if let Some((n, _)) = cels.first() {
                        let r = block(0, dur);
                        rcp.rect(r, 3.0, t.bg_widget.gamma_multiply(0.6), Stroke::new(1.0, t.border.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                        rcp.text(egui::pos2(r.left().max(x0) + 4.0, y + row_h / 2.0), Align2::LEFT_CENTER, format!("{n}  ({})", tr("timeline.static")), FontId::proportional(font_cell - 0.5), t.text_dim);
                    }
                }
            }
        }
        // cabeçalho da linha
        let hdr = Rect::from_min_max(egui::pos2(area.min.x, y), egui::pos2(x0, y + row_h));
        let hp = rp.with_clip_rect(hdr.intersect(rows_clip));
        hp.rect_filled(hdr, 0.0, if is_active { t.accent_dim } else { t.bg_header });
        let ind = row.depth as f32 * if compact { 8.0 } else { 10.0 };
        let isz = if compact { 11.0 } else { 13.0 };
        let mut nx = hdr.left() + 6.0 + ind;
        if row.group {
            let chev = if row.collapsed { IconId::ChevronRight } else { IconId::ChevronDown };
            paint_icon(env.icons, &hp, chev, Rect::from_center_size(egui::pos2(nx + isz / 2.0, y + row_h / 2.0), egui::vec2(isz, isz)), t.text_dim, false);
            nx += isz + 2.0;
        }
        paint_icon(env.icons, &hp, kind_icon(l.kind), Rect::from_center_size(egui::pos2(nx + isz / 2.0, y + row_h / 2.0), egui::vec2(isz, isz)), if is_active { t.accent } else { t.text_dim }, false);
        nx += isz + 4.0;
        let shown = elide(&hp, &row.name, &name_font, (x0 - nx - 4.0).max(8.0));
        hp.text(egui::pos2(nx, y + row_h / 2.0), Align2::LEFT_CENTER, shown, name_font.clone(), if l.visible { t.text } else { t.text_dim });
        rp.hline(area.x_range(), y + row_h, Stroke::new(1.0, t.border.gamma_multiply(0.6)));
    }
    // nome inteiro no cabeçalho cortado
    if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| p.x < x0 && p.y >= rows_top && area.contains(*p)) {
        if let Some(r) = row_at(p.y) {
            resp.clone().on_hover_text_at_pointer(&rows[r].name);
        }
    }
    painter.vline(x0, ruler.top()..=area.max.y, Stroke::new(1.0, t.border));
    // cabeçote
    let hx = x0 - sx + (env.ed.frame as f32 + 0.5) * cw;
    if hx >= x0 && hx <= area.max.x {
        let hl = painter.with_clip_rect(cells_clip);
        hl.rect_filled(Rect::from_min_max(egui::pos2(hx - cw / 2.0, ruler.bottom()), egui::pos2(hx + cw / 2.0, area.max.y)), 0.0, t.accent.gamma_multiply(0.12));
        hl.vline(hx, ruler.top()..=area.max.y, Stroke::new(1.5, t.accent));
        hl.add(egui::Shape::convex_polygon(vec![egui::pos2(hx - 5.0, ruler.top()), egui::pos2(hx + 5.0, ruler.top()), egui::pos2(hx, ruler.top() + 8.0)], t.accent, Stroke::NONE));
    }
    painter.rect_stroke(area, 4.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_merge_repeated_entries_and_extend_to_the_end() {
        let mut t = Track::new(NodeId::from("t"), "A", TrackKind::Drawing, None);
        t.cells.insert(0, CellRef::Cel { number: 1 });
        t.cells.insert(2, CellRef::Cel { number: 1 });
        t.cells.insert(4, CellRef::Cel { number: 2 });
        t.cells.insert(8, CellRef::Empty);
        let runs = exposure_runs(&t, 12);
        assert_eq!(runs, vec![(0, 4, CellRef::Cel { number: 1 }), (4, 8, CellRef::Cel { number: 2 }), (8, 12, CellRef::Empty)]);
        // entradas depois do fim do corte não contam
        t.cells.insert(20, CellRef::Cel { number: 3 });
        assert_eq!(exposure_runs(&t, 12).len(), 3);
        assert!(exposure_runs(&Track::new(NodeId::from("x"), "B", TrackKind::Drawing, None), 12).is_empty());
    }
}

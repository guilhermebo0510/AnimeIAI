//! Painéis da Entrega (S24 4.8–4.12): Fila de exportação (progresso por quadro, ETA, pausar/retomar/cancelar, reordenar,
//! tentar de novo com o erro inteiro), Destinos (pasta, modelo de nome, versão automática, cópias, pacote de entrega),
//! Entregáveis (biblioteca do que já foi exportado, com miniatura), Pôster (quadro e título sobre a imagem) e Verificação
//! final (semáforo com link para corrigir). O estado e a execução estão em [`super::deliver`].

use super::deliver::{self, PickFor, PosterEd, cancel_job, now_ms, open_path, pick_folder, preset_label, request_export, reveal, set_paused, start_preflight};
use crate::app::editor::Editor;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_tools::delivery::formats::Kind;
use aiai_tools::delivery::names::{self, TOKENS};
use aiai_tools::delivery::preflight::{self, Level};
use aiai_tools::delivery::store::{Dest, JobRecord, Status, Store, fmt_duration, fmt_size};
use aiai_tools::delivery::poster;
use egui::{Color32, FontId, Rect, Sense, TextureId, Ui};
use std::path::Path;
use std::time::{Duration, Instant};

const PASS: Color32 = Color32::from_rgb(0x4c, 0xc0, 0x6a);
const WARN: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x3a);

fn level_color(t: &crate::theme::Tokens, l: Level) -> Color32 {
    match l {
        Level::Pass => PASS,
        Level::Warn => WARN,
        Level::Fail => t.danger,
    }
}

/// Textura de um PNG da pasta de miniaturas (carrega uma vez; arquivo ausente guarda `None`).
fn thumb(ed: &mut Editor, ctx: &egui::Context, name: &str) -> Option<(TextureId, [f32; 2])> {
    let st = &mut ed.deliver;
    let dir = st.loaded.as_ref()?.dir.clone();
    let path = Store::thumbs_dir(&dir).join(name);
    let key = path.display().to_string();
    if !st.thumbs.contains_key(&key) {
        let tex = aiai_paint::Image::load_png(&path).ok().map(|img| {
            let (w, h) = (img.width as usize, img.height as usize);
            let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], &img.to_rgba8());
            ctx.load_texture(format!("deliver-thumb-{key}"), ci, egui::TextureOptions::LINEAR)
        });
        st.thumbs.insert(key.clone(), tex);
    }
    st.thumbs.get(&key).and_then(|t| t.as_ref()).map(|t| (t.id(), t.size_vec2().into()))
}

/// Pinta a miniatura ajustada ao retângulo (centralizada, sem distorcer) ou um bloco liso com a inicial do formato.
fn paint_thumb(ui: &Ui, rect: Rect, tex: Option<(TextureId, [f32; 2])>, letter: &str, t: &crate::theme::Tokens) {
    let p = ui.painter();
    p.rect_filled(rect, 3.0, t.bg_frame);
    match tex {
        Some((id, [w, h])) => {
            let s = (rect.width() / w).min(rect.height() / h);
            let r = Rect::from_center_size(rect.center(), egui::vec2(w * s, h * s));
            p.image(id, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, letter, FontId::proportional((rect.height() * 0.4).clamp(10.0, 28.0)), t.text_dim);
        }
    }
}

fn small_button(ui: &mut Ui, env: &mut Env, label: &str, tip: &str, note: String, enabled: bool) -> bool {
    let b = ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(label).size(12.0)).min_size(egui::vec2(22.0, 20.0)));
    env.note(note, b.rect);
    b.on_hover_text(tip).clicked()
}

fn kind_letter(preset: &str) -> &'static str {
    match aiai_tools::delivery::formats::preset(preset).map(|p| p.kind) {
        Some(Kind::Wav | Kind::Stems) => "♪",
        Some(Kind::PngSequence | Kind::ContactSheet) => "▦",
        _ => "▶",
    }
}

// ------------------------------------------------------------------------------------------------ Fila

pub(crate) fn queue(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    if env.ed.deliver.loaded.is_none() {
        ui.label(egui::RichText::new(tr("board.save_first")).color(t.text_dim));
        return;
    }
    let paused = env.ed.deliver.store().map(|s| s.paused).unwrap_or(false);
    let running_id = env.ed.deliver.running.as_ref().map(|r| r.id);
    let pending = env.ed.deliver.store().map(|s| s.pending()).unwrap_or(0);
    let preset = env.ed.deliver.preset();
    let n = env.ed.deliver.matrix.len();
    // barra de ações
    ui.horizontal(|ui| {
        let usable = env.ed.deliver.source.is_some() && (preset.availability().is_ok() || n > 0);
        let label = if n > 1 { tr("deliver.export_n").replace("{0}", &n.to_string()) } else { tr("deliver.export").to_owned() };
        let b = ui.add_enabled(usable && !env.ed.deliver.pf.is_running(), egui::Button::new(label));
        env.note("deliver:export", b.rect);
        if b.on_hover_text(tr("deliver.export.tip")).clicked() {
            request_export(env.core, env.ed, &ctx);
        }
        let b = ui.add_enabled(running_id.is_some() || pending > 0 || paused, egui::Button::new(tr(if paused { "deliver.resume" } else { "deliver.pause" })));
        env.note("deliver:pause", b.rect);
        if b.on_hover_text(tr("deliver.pause.tip")).clicked() {
            set_paused(&mut env.ed.deliver, !paused);
        }
        let any_done = env.ed.deliver.store().is_some_and(|s| s.jobs.iter().any(|j| matches!(j.status, Status::Done | Status::Failed | Status::Canceled)));
        let b = ui.add_enabled(any_done, egui::Button::new(tr("deliver.clear")));
        env.note("deliver:clear", b.rect);
        if b.clicked() {
            if let Some(s) = env.ed.deliver.store_mut() {
                s.clear_finished();
            }
        }
        if env.ed.deliver.pf.is_running() {
            ui.label(egui::RichText::new(tr("deliver.verifying")).color(t.text_dim).size(12.0));
        }
    });
    // avisos: fila parada com itens da última sessão; verificação final com falha
    if paused && pending > 0 && running_id.is_none() {
        let resumed = env.ed.deliver.store().map(|s| s.jobs.iter().filter(|j| j.resumed && j.status == Status::Queued).count()).unwrap_or(0);
        ui.add_space(3.0);
        egui::Frame::new().fill(t.st_review).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.label(egui::RichText::new(tr(if resumed > 0 { "deliver.resume_banner" } else { "deliver.paused_banner" }).replace("{0}", &pending.to_string())).size(12.0));
            ui.horizontal(|ui| {
                if small_button(ui, env, tr("deliver.resume"), tr("deliver.pause.tip"), "deliver:banner:resume".into(), true) {
                    set_paused(&mut env.ed.deliver, false);
                }
                if small_button(ui, env, &tr("deliver.discard").to_owned(), tr("deliver.discard.tip"), "deliver:banner:discard".into(), true) {
                    let ids: Vec<u64> = env.ed.deliver.store().map(|s| s.jobs.iter().filter(|j| j.status == Status::Queued).map(|j| j.spec.id).collect()).unwrap_or_default();
                    if let Some(s) = env.ed.deliver.store_mut() {
                        for id in ids {
                            s.cancel_queued(id);
                        }
                    }
                }
            });
        });
    }
    if env.ed.deliver.pf.blocked {
        let fails = env.ed.deliver.pf.report.as_ref().map(|(_, r)| r.count(Level::Fail)).unwrap_or(0);
        ui.add_space(3.0);
        egui::Frame::new().fill(t.st_retake).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.label(egui::RichText::new(tr("deliver.blocked").replace("{0}", &fails.to_string())).size(12.0));
            ui.horizontal(|ui| {
                if small_button(ui, env, &tr("deliver.export_anyway").to_owned(), tr("deliver.export_anyway.tip"), "deliver:anyway".into(), true) {
                    deliver::enqueue_now(env.core, env.ed);
                }
                if small_button(ui, env, &tr("deliver.dismiss").to_owned(), tr("deliver.dismiss"), "deliver:dismiss".into(), true) {
                    env.ed.deliver.pf.blocked = false;
                }
            });
        });
    }
    ui.add_space(4.0);
    let jobs: Vec<JobRecord> = env.ed.deliver.store().map(|s| s.jobs.clone()).unwrap_or_default();
    if jobs.is_empty() {
        ui.label(egui::RichText::new(tr("panel.queue.empty")).color(t.text_dim));
        return;
    }
    let mut rows: Vec<(u64, Rect)> = Vec::new();
    let mut action: Option<(u64, &'static str)> = None;
    egui::ScrollArea::vertical().id_salt("deliver_queue").auto_shrink([false, false]).show(ui, |ui| {
        for (i, j) in jobs.iter().enumerate() {
            let id = j.spec.id;
            let ctl = env.ed.deliver.running.as_ref().filter(|r| r.id == id).map(|r| r.ctl.clone());
            let thumb_name = env.ed.deliver.store().and_then(|s| s.deliverable(id)).and_then(|d| d.result.thumbnail.clone());
            // o erro vem inteiro do motor; no cartão cabem 4 linhas e o resto aparece ao passar o mouse
            let err_galley = j.error.as_ref().filter(|_| matches!(j.status, Status::Failed)).map(|e| {
                let mut job = egui::text::LayoutJob::simple(e.clone(), FontId::proportional(11.0), t.danger, (ui.available_width() - 290.0).max(60.0));
                job.wrap.max_rows = 4;
                ui.painter().layout_job(job)
            });
            let extra = err_galley.as_ref().map(|g| g.size().y + 4.0).unwrap_or(0.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 66.0 + extra), Sense::click_and_drag());
            env.note(format!("deliver:job:{i}"), rect);
            rows.push((id, rect));
            let queued = j.status == Status::Queued;
            let painter = ui.painter().clone();
            painter.rect_filled(rect, 4.0, if env.ed.deliver.drag == Some(id) { t.bg_hover } else { t.bg_widget });
            let (stripe, status_txt) = match j.status {
                Status::Queued => (t.bg_hover, tr(if j.resumed { "deliver.status.resumed" } else { "deliver.status.queued" }).to_owned()),
                Status::Running => (t.st_progress, tr(if ctl.as_ref().is_some_and(|c| c.is_paused()) { "deliver.status.paused" } else { "deliver.status.running" }).to_owned()),
                Status::Done => (t.st_ok, format!("{} · {}", tr("deliver.status.done"), fmt_duration(j.secs as f64))),
                Status::Failed => (t.st_retake, tr("deliver.status.failed").to_owned()),
                Status::Canceled => (t.bg_hover, tr("deliver.status.canceled").to_owned()),
            };
            painter.rect_filled(Rect::from_min_size(rect.left_top(), egui::vec2(4.0, rect.height())), 2.0, stripe);
            // pega para reordenar (só os que esperam na fila)
            let grip = Rect::from_min_size(rect.left_top() + egui::vec2(6.0, 0.0), egui::vec2(14.0, rect.height()));
            if queued {
                // pegador: duas colunas de três pontos
                for dx in [-2.5f32, 2.5] {
                    for dy in [-5.0f32, 0.0, 5.0] {
                        painter.circle_filled(grip.center() + egui::vec2(dx, dy), 1.3, t.text_dim);
                    }
                }
                if resp.dragged() && ui.rect_contains_pointer(grip) && env.ed.deliver.drag.is_none() {
                    env.ed.deliver.drag = Some(id);
                }
            }
            // miniatura
            let tr_rect = Rect::from_min_size(rect.left_top() + egui::vec2(24.0, 6.0), egui::vec2(88.0, 49.5));
            let tex = thumb_name.as_deref().and_then(|n| thumb(env.ed, &ctx, n));
            paint_thumb(ui, tr_rect, tex, kind_letter(&j.spec.preset), &t);
            let x0 = tr_rect.right() + 8.0;
            let right_w = 150.0;
            let text_clip = painter.with_clip_rect(Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(rect.right() - right_w, rect.bottom())));
            let title = format!("{} · {}", j.spec.source_name, preset_label(&j.spec.preset));
            text_clip.text(egui::pos2(x0, rect.top() + 6.0), egui::Align2::LEFT_TOP, title, FontId::proportional(13.0), t.text);
            let file = Path::new(&j.spec.out).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            text_clip.text(egui::pos2(x0, rect.top() + 23.0), egui::Align2::LEFT_TOP, file, FontId::proportional(11.0), t.text_dim);
            text_clip.text(egui::pos2(x0, rect.top() + 38.0), egui::Align2::LEFT_TOP, &status_txt, FontId::proportional(11.0), t.text);
            if let Some(c) = &ctl {
                // barra por quadro, velocidade e ETA
                let (done, total) = c.frames();
                let bar = Rect::from_min_max(egui::pos2(x0, rect.top() + 53.0), egui::pos2(rect.right() - right_w, rect.top() + 59.0));
                painter.rect_filled(bar, 3.0, t.bg_frame);
                let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                painter.rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac, bar.height())), 3.0, t.accent);
                let phase = match c.phase() {
                    aiai_bridge::export_ctl::Phase::Preparing => tr("deliver.phase.preparing").to_owned(),
                    aiai_bridge::export_ctl::Phase::Audio => tr("deliver.phase.audio").to_owned(),
                    aiai_bridge::export_ctl::Phase::Finishing => tr("deliver.phase.finishing").to_owned(),
                    aiai_bridge::export_ctl::Phase::Frames => {
                        let mut s = tr("deliver.frames_of").replace("{0}", &done.to_string()).replace("{1}", &total.to_string());
                        if let Some(sp) = c.speed() {
                            s.push_str(&format!(" · {}", tr("deliver.speed").replace("{0}", &format!("{sp:.1}"))));
                        }
                        if let Some(eta) = c.eta_secs() {
                            s.push_str(&format!(" · {}", tr("deliver.eta").replace("{0}", &fmt_duration(eta))));
                        }
                        s
                    }
                };
                text_clip.text(egui::pos2(x0 + 150.0, rect.top() + 38.0), egui::Align2::LEFT_TOP, phase, FontId::proportional(11.0), t.text_dim);
            }
            if let Some(g) = err_galley {
                let pos = egui::pos2(x0, rect.top() + 62.0);
                painter.with_clip_rect(Rect::from_min_max(pos, egui::pos2(rect.right() - right_w, rect.bottom()))).galley(pos, g, t.danger);
            }
            // botões à direita
            let btns = Rect::from_min_max(egui::pos2(rect.right() - right_w + 4.0, rect.top() + 6.0), egui::pos2(rect.right() - 6.0, rect.top() + 28.0));
            let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(btns).layout(egui::Layout::right_to_left(egui::Align::Center)));
            match j.status {
                Status::Queued => {
                    if small_button(&mut bui, env, "×", tr("deliver.cancel"), format!("deliver:job:{i}:cancel"), true) {
                        action = Some((id, "cancel"));
                    }
                    if small_button(&mut bui, env, "▼", tr("deliver.down"), format!("deliver:job:{i}:down"), true) {
                        action = Some((id, "down"));
                    }
                    if small_button(&mut bui, env, "▲", tr("deliver.up"), format!("deliver:job:{i}:up"), true) {
                        action = Some((id, "up"));
                    }
                }
                Status::Running => {
                    if small_button(&mut bui, env, "×", tr("deliver.cancel"), format!("deliver:job:{i}:cancel"), true) {
                        action = Some((id, "cancel"));
                    }
                    let is_paused = paused || ctl.as_ref().is_some_and(|c| c.is_paused());
                    if small_button(&mut bui, env, if is_paused { "▶" } else { "⏸" }, tr(if is_paused { "deliver.resume" } else { "deliver.pause" }), format!("deliver:job:{i}:pause"), true) {
                        action = Some((id, if is_paused { "resume" } else { "pause" }));
                    }
                }
                Status::Failed | Status::Canceled => {
                    if small_button(&mut bui, env, "×", tr("deliver.remove"), format!("deliver:job:{i}:remove"), true) {
                        action = Some((id, "remove"));
                    }
                    if small_button(&mut bui, env, tr("deliver.retry.short"), tr("deliver.retry"), format!("deliver:job:{i}:retry"), true) {
                        action = Some((id, "retry"));
                    }
                }
                Status::Done => {
                    if small_button(&mut bui, env, "×", tr("deliver.remove"), format!("deliver:job:{i}:remove"), true) {
                        action = Some((id, "remove"));
                    }
                    if small_button(&mut bui, env, tr("deliver.folder.short"), tr("deliver.open_folder"), format!("deliver:job:{i}:folder"), true) {
                        action = Some((id, "folder"));
                    }
                }
            }
            if matches!(j.status, Status::Running) {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            // clique no item concluído seleciona o entregável
            if resp.clicked() && j.status == Status::Done && !ui.rect_contains_pointer(btns) {
                env.ed.deliver.sel = Some(id);
            }
            if let (Some(e), Status::Failed) = (&j.error, j.status) {
                resp.on_hover_text(e);
            }
        }
    });
    // arrasto: o item solta na posição da linha sob o ponteiro
    if let Some(id) = env.ed.deliver.drag {
        let (pos, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        if let Some(p) = pos {
            if let Some((_, r)) = rows.iter().find(|(_, r)| r.y_range().contains(p.y)) {
                ui.painter().hline(r.x_range(), if p.y < r.center().y { r.top() } else { r.bottom() }, egui::Stroke::new(2.0, t.accent));
            }
        }
        if !down {
            env.ed.deliver.drag = None;
            if let Some(p) = pos {
                if let Some(to) = rows.iter().position(|(_, r)| r.y_range().contains(p.y)) {
                    if let Some(s) = env.ed.deliver.store_mut() {
                        let _ = s.move_job(id, to);
                    }
                }
            }
        }
    }
    if let Some((id, what)) = action {
        match what {
            "cancel" => cancel_job(&mut env.ed.deliver, id),
            "pause" => set_paused(&mut env.ed.deliver, true),
            "resume" => set_paused(&mut env.ed.deliver, false),
            "up" | "down" => {
                if let Some(s) = env.ed.deliver.store_mut() {
                    let _ = s.nudge_job(id, if what == "up" { -1 } else { 1 });
                }
            }
            "remove" => {
                if let Some(s) = env.ed.deliver.store_mut() {
                    let _ = s.remove_job(id);
                }
            }
            "retry" => {
                if let Some(s) = env.ed.deliver.store_mut() {
                    let _ = s.retry(id);
                }
                // volta a andar sozinha (o usuário pediu para tentar de novo)
                set_paused(&mut env.ed.deliver, false);
            }
            "folder" => {
                let out = env.ed.deliver.store().and_then(|s| s.job(id)).map(|j| j.spec.out.clone());
                if let Some(out) = out {
                    let p = Path::new(&out);
                    let target = if out.contains("%05d") { p.parent().map(Path::to_path_buf).unwrap_or_default() } else { p.to_path_buf() };
                    if let Err(e) = reveal(&target) {
                        env.ed.notify(e);
                    }
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------------ Destinos

pub(crate) fn destinations(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(l) = env.ed.deliver.loaded.as_ref() else {
        ui.label(egui::RichText::new(tr("board.save_first")).color(t.text_dim));
        return;
    };
    let (dir, name) = (l.dir.clone(), env.core.project.name.clone());
    let settings = l.store.settings.clone();
    let out_dir = settings.out_dir(&dir, &name);
    let mut next = settings.clone();
    egui::ScrollArea::vertical().id_salt("deliver_dest").auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &t, "deliver.dest.folder");
        let mut choice = next.dest;
        for (d, key) in [(Dest::Project, "deliver.dest.project"), (Dest::Documents, "deliver.dest.documents"), (Dest::Custom, "deliver.dest.custom")] {
            let r = ui.radio_value(&mut choice, d, tr(key));
            env.note(format!("deliver:dest:{key}"), r.rect);
        }
        if choice != next.dest {
            next.dest = choice;
        }
        ui.horizontal(|ui| {
            if small_button(ui, env, &tr("deliver.dest.choose").to_owned(), tr("deliver.dest.pick"), "deliver:dest:choose".into(), env.ed.deliver.picker.is_none()) {
                pick_folder(&mut env.ed.deliver, PickFor::Custom, Some(out_dir.clone()));
            }
            if small_button(ui, env, &tr("deliver.open_folder").to_owned(), tr("deliver.open_folder"), "deliver:dest:open".into(), true) {
                let _ = std::fs::create_dir_all(&out_dir);
                if let Err(e) = open_path(&out_dir) {
                    env.ed.notify(e);
                }
            }
        });
        dim_label(ui, &t, &out_dir.display().to_string());
        if next.dest == Dest::Custom && next.custom_dir.trim().is_empty() {
            dim_label(ui, &t, tr("deliver.dest.custom_empty"));
        }

        section(ui, &t, "deliver.dest.template");
        let buf = env.ed.deliver.template_buf.get_or_insert_with(|| settings.name_template.clone());
        let r = ui.add(egui::TextEdit::singleline(buf).desired_width((ui.available_width() - 8.0).max(80.0)));
        let typed = buf.clone();
        env.note("deliver:dest:template", r.rect);
        match names::validate_template(&typed) {
            Ok(()) => {
                if typed != next.name_template {
                    next.name_template = typed.clone();
                }
            }
            Err(e) => {
                ui.label(egui::RichText::new(e).color(t.danger).size(11.5));
            }
        }
        ui.horizontal_wrapped(|ui| {
            for tok in TOKENS {
                let b = ui.add(egui::Button::new(egui::RichText::new(tok).size(11.0)).small());
                env.note(format!("deliver:dest:token:{tok}"), b.rect);
                if b.on_hover_text(tr("deliver.dest.token_tip")).clicked() {
                    if let Some(b) = env.ed.deliver.template_buf.as_mut() {
                        b.push_str(tok);
                    }
                }
            }
        });
        // exemplo com o pedido atual
        let sample = env.ed.deliver.source.clone().and_then(|s| {
            let preset = env.ed.deliver.preset();
            let mut store = env.ed.deliver.store()?.clone();
            store.settings = next.clone();
            store.plan(&env.core.project, &dir, &s, preset, None, now_ms()).ok()
        });
        if let Some(spec) = sample {
            dim_label(ui, &t, &format!("{} {}", tr("deliver.dest.example"), Path::new(&spec.out).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        }
        let r = ui.checkbox(&mut next.auto_version, tr("deliver.dest.auto_version"));
        env.note("deliver:dest:auto_version", r.rect);
        r.on_hover_text(tr("deliver.dest.auto_version.tip"));

        section(ui, &t, "deliver.dest.extra");
        let mut remove = None;
        for (i, d) in next.extra_dirs.iter().enumerate() {
            ui.horizontal(|ui| {
                if small_button(ui, env, "×", tr("deliver.remove"), format!("deliver:dest:extra:{i}:remove"), true) {
                    remove = Some(i);
                }
                ui.label(egui::RichText::new(d).size(12.0));
            });
        }
        if let Some(i) = remove {
            next.extra_dirs.remove(i);
        }
        if small_button(ui, env, &tr("deliver.dest.add_extra").to_owned(), tr("deliver.dest.extra_tip"), "deliver:dest:extra:add".into(), env.ed.deliver.picker.is_none()) {
            pick_folder(&mut env.ed.deliver, PickFor::Extra, None);
        }

        section(ui, &t, "deliver.dest.package");
        let r = ui.checkbox(&mut next.package, tr("deliver.dest.package.on"));
        env.note("deliver:dest:package", r.rect);
        dim_label(ui, &t, tr("deliver.dest.package.what"));
        ui.add_space(6.0);
        let r = ui.checkbox(&mut env.ed.deliver.verify_first, tr("deliver.verify_first"));
        env.note("deliver:dest:verify_first", r.rect);
    });
    if next != settings {
        if let Some(s) = env.ed.deliver.store_mut() {
            s.settings = next;
        }
        // `Dest` mudou por fora (diálogo de pasta): o texto em edição segue o valor guardado
        env.ed.deliver.template_buf = None;
    }
}

// ------------------------------------------------------------------------------------------------ Entregáveis

fn date_text(ms: u64) -> String {
    let (d, t) = names::local_date_time_of(ms);
    format!("{d} {}:{}", &t[..2], &t[2..])
}

pub(crate) fn deliverables(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(l) = env.ed.deliver.loaded.as_ref() else {
        ui.label(egui::RichText::new(tr("board.save_first")).color(t.text_dim));
        return;
    };
    if l.store.deliverables.is_empty() {
        ui.label(egui::RichText::new(tr("panel.deliverables.empty")).color(t.text_dim));
        return;
    }
    let mut items: Vec<_> = l.store.deliverables.clone();
    items.reverse(); // os mais recentes primeiro
    let dir = l.dir.clone();
    let sel = env.ed.deliver.sel.filter(|id| items.iter().any(|d| d.id() == *id));
    env.ed.deliver.sel = sel;
    // ações do selecionado
    let cur = sel.and_then(|id| items.iter().find(|d| d.id() == id)).cloned();
    ui.horizontal_wrapped(|ui| {
        let on = cur.is_some();
        let missing = cur.as_ref().is_some_and(|d| d.missing);
        if small_button(ui, env, &tr("deliver.open_file").to_owned(), tr("deliver.open_file"), "deliver:lib:open".into(), on && !missing) {
            if let Some(f) = cur.as_ref().and_then(|d| d.main_file()) {
                if let Err(e) = open_path(Path::new(f)) {
                    env.ed.notify(e);
                }
            }
        }
        if small_button(ui, env, &tr("deliver.open_folder").to_owned(), tr("deliver.open_folder"), "deliver:lib:folder".into(), on) {
            if let Some(d) = &cur {
                let target = d.folder().filter(|f| f.exists()).or_else(|| d.folder().and_then(|f| f.parent().map(Path::to_path_buf)));
                if let Some(f) = target {
                    let r = if d.result.is_sequence || missing { open_path(&f) } else { reveal(Path::new(d.main_file().unwrap_or(""))) };
                    if let Err(e) = r {
                        env.ed.notify(e);
                    }
                }
            }
        }
        if small_button(ui, env, &tr("deliver.reexport").to_owned(), tr("deliver.reexport.tip"), "deliver:lib:reexport".into(), on) {
            if let Some(d) = &cur {
                reexport(env, d.id());
            }
        }
        if small_button(ui, env, &tr("deliver.package").to_owned(), tr("deliver.package.tip"), "deliver:lib:package".into(), on && !missing) {
            if let Some(d) = &cur {
                make_package(env, d.id());
            }
        }
        if small_button(ui, env, &tr("deliver.delete").to_owned(), tr("deliver.delete.tip"), "deliver:lib:delete".into(), on) {
            env.ed.deliver.confirm_delete = sel;
        }
    });
    if let Some(id) = env.ed.deliver.confirm_delete {
        egui::Frame::new().fill(t.st_review).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.label(egui::RichText::new(tr("deliver.delete.confirm")).size(12.0));
            ui.horizontal(|ui| {
                if small_button(ui, env, &tr("deliver.delete.yes").to_owned(), tr("deliver.delete.tip"), "deliver:lib:delete_yes".into(), true) {
                    let now = now_ms();
                    let res = env.ed.deliver.store_mut().map(|s| s.trash_deliverable(id, &dir, now));
                    match res {
                        Some(Ok(moved)) => env.ed.notify(format!("{} ({})", tr("deliver.deleted"), moved.len())),
                        Some(Err(e)) => env.ed.notify(e),
                        None => {}
                    }
                    env.ed.deliver.confirm_delete = None;
                    env.ed.deliver.sel = None;
                }
                if small_button(ui, env, &tr("deliver.dismiss").to_owned(), tr("deliver.dismiss"), "deliver:lib:delete_no".into(), true) {
                    env.ed.deliver.confirm_delete = None;
                }
            });
        });
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("deliver_lib").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for (i, d) in items.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(204.0, 172.0), Sense::click());
                env.note(format!("deliver:lib:{i}"), rect);
                let on = env.ed.deliver.sel == Some(d.id());
                let painter = ui.painter().clone();
                painter.rect_filled(rect, 4.0, if on { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget });
                let tr_rect = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 4.0), egui::vec2(196.0, 110.0));
                let tex = d.result.thumbnail.as_deref().and_then(|n| thumb(env.ed, &ctx, n));
                paint_thumb(ui, tr_rect, tex, kind_letter(&d.spec.preset), &t);
                let clip = painter.with_clip_rect(rect.shrink(4.0));
                let title = format!("{} · {}", d.spec.source_name, preset_label(&d.spec.preset));
                clip.text(rect.left_top() + egui::vec2(6.0, 118.0), egui::Align2::LEFT_TOP, title, FontId::proportional(12.0), t.text);
                let mut meta: Vec<String> = Vec::new();
                if d.result.width > 0 {
                    meta.push(format!("{}×{}", d.result.width, d.result.height));
                }
                if d.result.duration_s > 0.0 {
                    meta.push(fmt_duration(d.result.duration_s));
                }
                meta.push(fmt_size(d.result.size_bytes));
                if let Some(l) = d.result.lufs {
                    meta.push(format!("{l:.1} LUFS"));
                }
                clip.text(rect.left_top() + egui::vec2(6.0, 134.0), egui::Align2::LEFT_TOP, meta.join(" · "), FontId::proportional(10.5), t.text_dim);
                let third = format!("{} · {} {}", date_text(d.finished_ms), tr("deliver.lib.rev"), d.spec.revision);
                clip.text(rect.left_top() + egui::vec2(6.0, 148.0), egui::Align2::LEFT_TOP, third, FontId::proportional(10.5), t.text_dim);
                let mut badges = Vec::new();
                if d.missing {
                    badges.push(tr("deliver.lib.missing").to_owned());
                }
                if d.result.package.is_some() {
                    badges.push("zip".to_owned());
                }
                if !d.result.warnings.is_empty() {
                    badges.push(format!("⚠ {}", d.result.warnings.len()));
                }
                if !d.result.copies.is_empty() {
                    badges.push(tr("deliver.lib.copied").to_owned());
                }
                if !badges.is_empty() {
                    clip.text(rect.left_top() + egui::vec2(6.0, 161.0), egui::Align2::LEFT_TOP, badges.join(" · "), FontId::proportional(10.5), if d.missing { t.danger } else { t.text_dim });
                }
                if resp.clicked() {
                    env.ed.deliver.sel = Some(d.id());
                }
                if resp.double_clicked() && !d.missing {
                    if let Some(f) = d.main_file() {
                        if let Err(e) = open_path(Path::new(f)) {
                            env.ed.notify(e);
                        }
                    }
                }
                let mut tip = d.result.files.join("\n");
                for w in &d.result.warnings {
                    tip.push_str(&format!("\n⚠ {w}"));
                }
                resp.on_hover_text(tip);
            }
        });
    });
}

fn reexport(env: &mut Env, id: u64) {
    let (Some(d), Some(dir)) = (env.ed.deliver.store().and_then(|s| s.deliverable(id)).cloned(), env.ed.deliver.loaded.as_ref().map(|l| l.dir.clone())) else { return };
    let Some(preset) = aiai_tools::delivery::formats::preset(&d.spec.preset) else { return };
    let planned = env.ed.deliver.store().map(|s| s.plan(&env.core.project, &dir, &d.spec.source, preset, d.spec.crf, now_ms()));
    match planned {
        Some(Ok(mut spec)) => {
            spec.poster_frame = d.spec.poster_frame;
            spec.extra_dirs = d.spec.extra_dirs.clone();
            spec.package = d.spec.package;
            if let Some(s) = env.ed.deliver.store_mut() {
                s.add_job(spec);
            }
            env.ed.notify(tr("deliver.reexport.queued"));
        }
        Some(Err(e)) => env.ed.notify(e.to_string()),
        None => {}
    }
}

fn make_package(env: &mut Env, id: u64) {
    let Some(l) = env.ed.deliver.loaded.as_mut() else { return };
    let res = aiai_bridge::delivery_tools::handle_with_store(env.core, &mut l.store, &serde_json::json!({ "op": "package", "id": id }), now_ms());
    match res {
        Ok(o) => {
            if let Some(l) = env.ed.deliver.loaded.as_mut() {
                l.dirty = true;
            }
            let zip = o.value["package"].as_str().unwrap_or("").to_owned();
            env.ed.notify(format!("{}: {zip}", tr("deliver.package.done")));
        }
        Err(e) => env.ed.notify(e.message),
    }
}

// ------------------------------------------------------------------------------------------------ Pôster

/// Recolhe a prévia do pôster que a thread terminou.
pub(crate) fn poll_poster(ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver.poster;
    let Some(rx) = &st.rx else { return };
    match rx.try_recv() {
        Ok(Ok(img)) => {
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_rgba8());
            st.tex = Some(ctx.load_texture("deliver-poster", ci, egui::TextureOptions::LINEAR));
            st.shown = st.asked.clone();
            st.rx = None;
        }
        Ok(Err(e)) => {
            st.rx = None;
            st.shown = st.asked.clone();
            st.tex = None;
            ed.toast = Some(format!("{}: {e}", tr("deliver.poster.failed")));
        }
        Err(crossbeam_channel::TryRecvError::Empty) => {}
        Err(_) => st.rx = None,
    }
}

pub(crate) fn poster_panel(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(id) = env.ed.deliver.sel else {
        ui.label(egui::RichText::new(tr("panel.poster.empty")).color(t.text_dim));
        return;
    };
    let Some(d) = env.ed.deliver.store().and_then(|s| s.deliverable(id)).cloned() else {
        ui.label(egui::RichText::new(tr("panel.poster.empty")).color(t.text_dim));
        return;
    };
    let total = match d.spec.source.total_frames(&env.core.project) {
        Ok(n) if n > 0 => n,
        _ => {
            ui.label(egui::RichText::new(tr("deliver.poster.no_source")).color(t.danger));
            return;
        }
    };
    // trocou de entregável: o editor começa do pôster guardado (ou do quadro do meio) e do título dele
    if env.ed.deliver.poster.id != Some(id) {
        let (frame, title) = d.poster.as_ref().map(|p| (p.frame, p.title.clone())).unwrap_or_else(|| (d.spec.poster_frame.unwrap_or(total / 2), String::new()));
        env.ed.deliver.poster = PosterEd { id: Some(id), frame: frame.min(total - 1), title, ..Default::default() };
    }
    let (mut frame, mut title, mut sub) = (env.ed.deliver.poster.frame, env.ed.deliver.poster.title.clone(), env.ed.deliver.poster.subtitle.clone());
    let before = (frame, title.clone(), sub.clone());
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("deliver.poster.frame"));
        let r = ui.add(egui::Slider::new(&mut frame, 0..=total - 1).show_value(true));
        env.note("deliver:poster:frame", r.rect);
        if small_button_plain(ui, tr("deliver.poster.middle"), tr("deliver.poster.middle")) {
            frame = total / 2;
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("deliver.poster.title"));
        let r = ui.add(egui::TextEdit::singleline(&mut title).desired_width((ui.available_width() - 8.0).max(60.0)));
        env.note("deliver:poster:title", r.rect);
    });
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("deliver.poster.subtitle"));
        let r = ui.add(egui::TextEdit::singleline(&mut sub).desired_width((ui.available_width() - 8.0).max(60.0)));
        env.note("deliver:poster:subtitle", r.rect);
    });
    let ed_state = &mut env.ed.deliver.poster;
    if before != (frame, title.clone(), sub.clone()) {
        ed_state.changed_at = Some(Instant::now());
    }
    (ed_state.frame, ed_state.title, ed_state.subtitle) = (frame, title, sub);
    // prévia: pede ao fim de 250 ms sem mexer e só uma por vez
    let want = (id, ed_state.frame, ed_state.title.clone(), ed_state.subtitle.clone());
    let settled = ed_state.changed_at.is_none_or(|t0| t0.elapsed() > Duration::from_millis(250));
    if ed_state.shown.as_ref() != Some(&want) && ed_state.asked.as_ref() != Some(&want) && ed_state.rx.is_none() && settled {
        let (project, blobs, src) = (env.core.project.clone(), env.core.blobs.clone(), d.spec.source.clone());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (frame, title, sub) = (want.1, want.2.clone(), want.3.clone());
        let c2 = ctx.clone();
        let spawned = std::thread::Builder::new().name("aiai-poster".into()).spawn(move || {
            let r = poster::poster(&project, &blobs, &src, frame, 800, &title, &sub).map_err(|e| e.to_string());
            let _ = tx.send(r);
            c2.request_repaint();
        });
        if spawned.is_ok() {
            ed_state.rx = Some(rx);
            ed_state.asked = Some(want.clone());
        }
    } else if ed_state.shown.as_ref() != Some(&want) {
        ctx.request_repaint_after(Duration::from_millis(120));
    }
    // salvar/abrir
    let saved = d.poster.clone();
    let mut save = false;
    let mut open_saved = false;
    ui.horizontal(|ui| {
        let r = ui.add(egui::Button::new(tr("deliver.poster.save")));
        env.note("deliver:poster:save", r.rect);
        save = r.on_hover_text(tr("deliver.poster.save.tip")).clicked();
        let r = ui.add_enabled(saved.is_some(), egui::Button::new(tr("deliver.poster.open")));
        env.note("deliver:poster:open", r.rect);
        open_saved = r.clicked();
    });
    if let Some(p) = &saved {
        dim_label(ui, &t, &format!("{} {} · {}", tr("deliver.poster.saved"), Path::new(&p.file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p.frame));
    }
    let (frame, title, sub) = (env.ed.deliver.poster.frame, env.ed.deliver.poster.title.clone(), env.ed.deliver.poster.subtitle.clone());
    if save {
        let dir = env.ed.deliver.loaded.as_ref().map(|l| l.dir.clone());
        if let Some(dir) = dir {
            match aiai_bridge::delivery_tools::write_poster(env.core, &dir, id, &d.spec.source, frame, &title, &sub) {
                Ok(info) => {
                    if let Some(x) = env.ed.deliver.store_mut().and_then(|s| s.deliverable_mut(id)) {
                        x.poster = Some(info);
                    }
                    env.ed.notify(tr("deliver.poster.done"));
                }
                Err(e) => env.ed.notify(e),
            }
        }
    }
    if open_saved {
        if let Some(p) = &saved {
            if let Err(e) = open_path(Path::new(&p.file)) {
                env.ed.notify(e);
            }
        }
    }
    // a prévia ocupa o resto do painel, ajustada sem distorcer
    let avail = ui.available_rect_before_wrap();
    if avail.height() > 20.0 {
        let tex = env.ed.deliver.poster.tex.as_ref().map(|t| (t.id(), t.size_vec2()));
        let (rect, _) = ui.allocate_exact_size(avail.size(), Sense::hover());
        env.note("deliver:poster:preview", rect);
        ui.painter().rect_filled(rect, 3.0, t.bg_frame);
        if let Some((tid, sz)) = tex {
            let s = (rect.width() / sz.x).min(rect.height() / sz.y);
            let r = Rect::from_center_size(rect.center(), egui::vec2(sz.x * s, sz.y * s));
            ui.painter().image(tid, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}

fn small_button_plain(ui: &mut Ui, label: &str, tip: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).size(12.0))).on_hover_text(tip).clicked()
}

// ------------------------------------------------------------------------------------------------ Verificação final

pub(crate) fn check_text(c: &preflight::Check) -> String {
    let key = format!("deliver.pf.{}", c.code);
    let tpl = tr(&key);
    if tpl == key { c.message.clone() } else { preflight::fill(tpl, &c.args) }
}

pub(crate) fn preflight_panel(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    if env.ed.deliver.loaded.is_none() {
        ui.label(egui::RichText::new(tr("board.save_first")).color(t.text_dim));
        return;
    }
    let key = env.ed.deliver.key(&env.core.project);
    let running = env.ed.deliver.pf.is_running();
    ui.horizontal(|ui| {
        let b = ui.add_enabled(!running && key.is_some(), egui::Button::new(tr("deliver.verify")));
        env.note("deliver:pf:run", b.rect);
        if b.on_hover_text(tr("deliver.verify.tip")).clicked() {
            start_preflight(env.core, env.ed, &ctx);
        }
        if running {
            ui.label(egui::RichText::new(tr("deliver.verifying")).color(t.text_dim).size(12.0));
        }
    });
    ui.label(egui::RichText::new(format!("{} · {}", tr(env.ed.deliver.preset().key), env.ed.deliver.source.as_ref().and_then(|s| s.info(&env.core.project).ok()).map(|i| i.name).unwrap_or_default())).size(11.5).color(t.text_dim));
    let Some((rk, rep)) = env.ed.deliver.pf.report.clone() else {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("panel.preflight.empty")).color(t.text_dim));
        return;
    };
    let stale = key.as_ref() != Some(&rk);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (l, n) in [(Level::Pass, rep.count(Level::Pass)), (Level::Warn, rep.count(Level::Warn)), (Level::Fail, rep.count(Level::Fail))] {
            ui.label(egui::RichText::new("●").color(level_color(&t, l)));
            ui.label(egui::RichText::new(format!("{n} {}", tr(match l {
                Level::Pass => "deliver.pf.pass",
                Level::Warn => "deliver.pf.warn",
                Level::Fail => "deliver.pf.fail",
            }))).size(12.0));
            ui.add_space(6.0);
        }
    });
    if stale {
        ui.label(egui::RichText::new(tr("deliver.pf.stale")).color(WARN).size(11.5));
    }
    ui.add_space(2.0);
    let mut goto = None;
    egui::ScrollArea::vertical().id_salt("deliver_pf").auto_shrink([false, false]).show(ui, |ui| {
        for (i, c) in rep.checks.iter().enumerate() {
            ui.horizontal_top(|ui| {
                let color = level_color(&t, c.level).gamma_multiply(if stale { 0.5 } else { 1.0 });
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), Sense::hover());
                env.note(format!("deliver:pf:{i}"), r);
                ui.painter().circle_filled(r.center(), 5.0, color);
                let w = (ui.available_width() - 74.0).max(60.0);
                let text = check_text(c);
                ui.scope(|ui| {
                    ui.set_max_width(w);
                    ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(if stale { t.text_dim } else { t.text })).wrap());
                });
                if let Some(f) = &c.fix {
                    let b = ui.add(egui::Button::new(egui::RichText::new(tr("deliver.pf.fix")).size(11.5)));
                    env.note(format!("deliver:pf:{i}:fix"), b.rect);
                    if b.on_hover_text(tr("deliver.pf.fix.tip")).clicked() {
                        goto = WorkspaceId::from_key(f.workspace);
                    }
                }
            });
            ui.add_space(2.0);
        }
        if let Some(l) = &rep.loudness {
            if let Some(v) = l.lufs {
                dim_label(ui, &t, &format!("{}: {v:.1} LUFS{}", tr("deliver.loudness"), l.true_peak_dbtp.map(|p| format!(" · {p:.1} dBTP")).unwrap_or_default()));
            }
        }
    });
    if let Some(w) = goto {
        env.ed.want_workspace = Some(w);
    }
}


//! Workspace Entrega: painel Presets (formatos de entrega prontos: TV com ARIB −24 LKFS e SRT ao lado, web −14 LUFS,
//! H.264 na GPU, ProRes, sequência PNG), painel Fila (exportações pedidas nesta sessão, uma por vez, com estado, tempo e
//! resultado) e, no painel Propriedades, o pedido: o que exportar (edição, episódio ou corte), o arquivo e os ajustes.
//!
//! A exportação é a ação `EXPORT_VIDEO` da bridge (a mesma do MCP), rodada numa thread com uma CÓPIA da sessão (projeto
//! e blobs): a ação só lê o documento, então o resultado é o mesmo, e a interface continua respondendo durante o render.

use crate::app::actions::{self, DocKind};
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_core::{CoreSession, NodeKind, Project};
use egui::{FontId, Rect, Sense, Ui};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Preset {
    pub key: &'static str,
    pub tip: &'static str,
    /// Valor de `codec` do EXPORT_VIDEO.
    pub codec: &'static str,
    pub crf: Option<u32>,
    /// `arib` | `ebu` | `streaming`; `None` = sem áudio.
    pub loudness: Option<&'static str>,
    pub ext: &'static str,
    pub sidecar_srt: bool,
}

pub(crate) const PRESETS: [Preset; 5] = [
    Preset { key: "deliver.p.tv", tip: "deliver.p.tv.tip", codec: "h264", crf: Some(18), loudness: Some("arib"), ext: "mp4", sidecar_srt: true },
    Preset { key: "deliver.p.web", tip: "deliver.p.web.tip", codec: "h264", crf: Some(20), loudness: Some("streaming"), ext: "mp4", sidecar_srt: true },
    Preset { key: "deliver.p.nvenc", tip: "deliver.p.nvenc.tip", codec: "h264_nvenc", crf: Some(19), loudness: Some("arib"), ext: "mp4", sidecar_srt: false },
    Preset { key: "deliver.p.prores", tip: "deliver.p.prores.tip", codec: "prores", crf: None, loudness: Some("ebu"), ext: "mov", sidecar_srt: false },
    Preset { key: "deliver.p.png", tip: "deliver.p.png.tip", codec: "png_sequence", crf: None, loudness: None, ext: "png", sidecar_srt: false },
];

/// O que exportar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Source {
    Edit(NodeId),
    Episode(NodeId),
    Cut(NodeId),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JobState {
    Queued,
    Running(Instant),
    Done { secs: f32, summary: String },
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct Job {
    pub label: String,
    pub params: Value,
    pub out: String,
    pub state: JobState,
}

#[derive(Default)]
pub(crate) struct DeliverState {
    pub preset: usize,
    pub source: Option<Source>,
    /// Nome do arquivo (sem pasta e sem extensão); vazio = nome da fonte.
    pub name: String,
    pub crf: Option<u32>,
    pub jobs: Vec<Job>,
    running: Option<(usize, crossbeam_channel::Receiver<Result<Value, String>>)>,
}

/// Todas as fontes possíveis: edições, episódios e cortes, com o rótulo para a lista.
pub(crate) fn sources(p: &Project) -> Vec<(Source, String)> {
    let mut out: Vec<(Source, String)> = actions::docs_of(p, DocKind::Edit).into_iter().map(|(id, n)| (Source::Edit(id), format!("{}: {n}", tr("deliver.src.edit")))).collect();
    for ep in &p.roots.episodes {
        if let Some(n) = p.node(ep) {
            out.push((Source::Episode(ep.clone()), format!("{}: {}", tr("deliver.src.episode"), n.name)));
        }
    }
    for (cut, code, scene, _) in super::edit::project_cuts(p) {
        out.push((Source::Cut(cut), format!("{}: {code} · {scene}", tr("deliver.src.cut"))));
    }
    out
}

/// Nome de arquivo seguro a partir do nome de um nó (sem separadores nem caracteres proibidos no Windows).
pub(crate) fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
    let t = cleaned.trim().trim_end_matches('.').trim();
    if t.is_empty() { "entrega".into() } else { t.chars().take(80).collect() }
}

fn source_name(p: &Project, s: &Source) -> String {
    let id = match s {
        Source::Edit(i) | Source::Episode(i) | Source::Cut(i) => i,
    };
    match p.node(id).map(|n| (&n.data, n.name.clone())) {
        Some((NodeKind::Cut(cd), _)) => format!("C{:03}", cd.number),
        Some((_, name)) => name,
        None => "entrega".into(),
    }
}

/// Pasta de saída: `<pasta do projeto>/saida`.
fn out_dir(core: &CoreSession) -> Option<PathBuf> {
    let path = core.path.clone()?;
    Some(path.parent().map(|d| d.to_path_buf()).unwrap_or(path).join("saida"))
}

/// Parâmetros do EXPORT_VIDEO para a fonte, o preset e o arquivo.
pub(crate) fn export_params(src: &Source, preset: &Preset, out: &str, crf: Option<u32>) -> Value {
    let mut v = match src {
        Source::Edit(i) => json!({ "edit": i }),
        Source::Episode(i) => json!({ "episode": i }),
        Source::Cut(i) => json!({ "cut": i }),
    };
    v["out"] = json!(out);
    v["codec"] = json!(preset.codec);
    if let Some(c) = crf.or(preset.crf) {
        v["crf"] = json!(c);
    }
    match preset.loudness {
        Some(l) => v["preset"] = json!(l),
        None => v["audio"] = json!(false),
    }
    if preset.sidecar_srt {
        v["sidecar_srt"] = json!(true);
    }
    v
}

/// Resumo de uma exportação concluída para a fila.
pub(crate) fn summarize(v: &Value) -> String {
    let frames = v.get("frames").and_then(Value::as_u64).unwrap_or(0);
    let size = v.get("size").and_then(Value::as_array).map(|a| format!("{}x{}", a.first().and_then(Value::as_u64).unwrap_or(0), a.get(1).and_then(Value::as_u64).unwrap_or(0))).unwrap_or_default();
    let lufs = v.pointer("/audio/loudness/integrated_lufs").or_else(|| v.pointer("/audio/integrated_lufs")).and_then(Value::as_f64);
    let mut s = format!("{frames} {} · {size}", tr("unit.frames"));
    if let Some(l) = lufs {
        s.push_str(&format!(" · {l:.1} LUFS"));
    }
    s
}

/// Recolhe a exportação pronta e começa a próxima da fila (uma por vez).
pub(crate) fn poll(env: &mut Env, ctx: &egui::Context) {
    let st = &mut env.ed.deliver;
    if let Some((k, rx)) = &st.running {
        match rx.try_recv() {
            Ok(res) => {
                let k = *k;
                st.running = None;
                let secs = match st.jobs[k].state {
                    JobState::Running(t0) => t0.elapsed().as_secs_f32(),
                    _ => 0.0,
                };
                let msg;
                st.jobs[k].state = match res {
                    Ok(v) => {
                        msg = format!("{}: {}", tr("deliver.done"), st.jobs[k].out);
                        JobState::Done { secs, summary: summarize(&v) }
                    }
                    Err(e) => {
                        msg = format!("{}: {e}", tr("deliver.failed"));
                        JobState::Failed(e)
                    }
                };
                env.ed.notify(msg);
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(250)),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                let k = *k;
                st.running = None;
                st.jobs[k].state = JobState::Failed(tr("deliver.lost").to_owned());
            }
        }
    }
    let st = &mut env.ed.deliver;
    if st.running.is_some() {
        return;
    }
    let Some(k) = st.jobs.iter().position(|j| j.state == JobState::Queued) else { return };
    // cópia da sessão: o export só lê o documento
    let mut copy = CoreSession::new(env.core.project.clone());
    copy.blobs = env.core.blobs.clone();
    copy.path = env.core.path.clone();
    let params = st.jobs[k].params.clone();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let spawned = std::thread::Builder::new().name("aiai-export".into()).spawn(move || {
        let _ = tx.send(actions::run(&mut copy, "EXPORT_VIDEO", params));
    });
    match spawned {
        Ok(_) => {
            st.jobs[k].state = JobState::Running(Instant::now());
            st.running = Some((k, rx));
            ctx.request_repaint();
        }
        Err(e) => st.jobs[k].state = JobState::Failed(e.to_string()),
    }
}

/// Põe o pedido atual na fila.
fn enqueue(env: &mut Env) {
    let Some(src) = env.ed.deliver.source.clone() else {
        env.ed.notify(tr("deliver.no_source"));
        return;
    };
    let Some(dir) = out_dir(env.core) else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let preset = PRESETS[env.ed.deliver.preset.min(PRESETS.len() - 1)];
    let base = if env.ed.deliver.name.trim().is_empty() { source_name(&env.core.project, &src) } else { env.ed.deliver.name.trim().to_owned() };
    let base = safe_name(&base);
    let out = if preset.codec == "png_sequence" { dir.join(&base).join(format!("{base}_%05d.png")) } else { dir.join(format!("{base}.{}", preset.ext)) };
    let out = out.display().to_string();
    let params = export_params(&src, &preset, &out, env.ed.deliver.crf);
    let label = format!("{} · {}", source_name(&env.core.project, &src), tr(preset.key));
    env.ed.deliver.jobs.push(Job { label, params, out, state: JobState::Queued });
}

// ------------------------------------------------------------------------------------------------ Presets

pub(crate) fn presets(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    poll(env, &ctx);
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    ui.spacing_mut().item_spacing.y = 3.0;
    for (i, p) in PRESETS.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
        env.note(format!("deliver:preset:{i}"), rect);
        let on = env.ed.deliver.preset == i;
        let painter = ui.painter();
        if on {
            painter.rect_filled(rect, 4.0, t.accent_dim);
            painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, 38.0)), 1.5, t.accent);
        } else if resp.hovered() {
            painter.rect_filled(rect, 4.0, t.bg_hover);
        }
        let clip = painter.with_clip_rect(rect.shrink2(egui::vec2(8.0, 0.0)));
        clip.text(rect.left_top() + egui::vec2(10.0, 6.0), egui::Align2::LEFT_TOP, tr(p.key), FontId::proportional(13.0), t.text);
        let mut job = egui::text::LayoutJob::simple(tr(p.tip).to_owned(), FontId::proportional(11.0), t.text_dim, (rect.width() - 20.0).max(20.0));
        job.wrap.max_rows = 1;
        let g = clip.layout_job(job);
        clip.galley(rect.left_top() + egui::vec2(10.0, 25.0), g, t.text_dim);
        if resp.on_hover_text(tr(p.tip)).clicked() {
            env.ed.deliver.preset = i;
            env.ed.deliver.crf = None;
        }
    }
}

// ------------------------------------------------------------------------------------------------ Fila

pub(crate) fn queue(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    poll(env, &ctx);
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    ui.horizontal(|ui| {
        let b = ui.add_enabled(env.ed.deliver.source.is_some(), egui::Button::new(tr("deliver.export")));
        env.note("deliver:export", b.rect);
        if b.on_hover_text(tr("deliver.export.tip")).clicked() {
            enqueue(env);
            poll(env, &ctx);
        }
        let done = env.ed.deliver.jobs.iter().any(|j| matches!(j.state, JobState::Done { .. } | JobState::Failed(_)));
        let b = ui.add_enabled(done, egui::Button::new(tr("deliver.clear")));
        env.note("deliver:clear", b.rect);
        if b.clicked() {
            // limpa só as terminadas; os índices da que está rodando mudam junto
            let running = env.ed.deliver.running.as_ref().map(|(k, _)| *k);
            let mut keep_idx = None;
            let mut kept = Vec::new();
            for (i, j) in env.ed.deliver.jobs.drain(..).enumerate() {
                if matches!(j.state, JobState::Queued | JobState::Running(_)) {
                    if Some(i) == running {
                        keep_idx = Some(kept.len());
                    }
                    kept.push(j);
                }
            }
            env.ed.deliver.jobs = kept;
            if let (Some(n), Some((k, _))) = (keep_idx, env.ed.deliver.running.as_mut()) {
                *k = n;
            }
        }
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("deliver_queue").auto_shrink([false, false]).show(ui, |ui| {
        if env.ed.deliver.jobs.is_empty() {
            ui.label(egui::RichText::new(tr("panel.queue.empty")).color(t.text_dim));
            return;
        }
        for (i, j) in env.ed.deliver.jobs.clone().iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), Sense::hover());
            env.note(format!("deliver:job:{i}"), rect);
            let painter = ui.painter();
            painter.rect_filled(rect, 4.0, t.bg_widget);
            let (status, fill) = match &j.state {
                JobState::Queued => (tr("deliver.queued").to_owned(), t.bg_hover),
                JobState::Running(t0) => (format!("{} · {:.0} s", tr("deliver.running"), t0.elapsed().as_secs_f32()), t.st_progress),
                JobState::Done { secs, summary } => (format!("{} · {secs:.0} s · {summary}", tr("deliver.ok")), t.st_ok),
                JobState::Failed(e) => (format!("{}: {}", tr("deliver.failed"), actions::excerpt(e, 140)), t.st_retake),
            };
            painter.rect_filled(Rect::from_min_size(rect.left_top(), egui::vec2(4.0, rect.height())), 2.0, fill);
            let clip = painter.with_clip_rect(rect.shrink2(egui::vec2(10.0, 0.0)));
            clip.text(rect.left_top() + egui::vec2(12.0, 5.0), egui::Align2::LEFT_TOP, &j.label, FontId::proportional(13.0), t.text);
            clip.text(rect.left_top() + egui::vec2(12.0, 22.0), egui::Align2::LEFT_TOP, &j.out, FontId::proportional(11.0), t.text_dim);
            clip.text(rect.left_top() + egui::vec2(12.0, 36.0), egui::Align2::LEFT_TOP, status, FontId::proportional(11.0), t.text);
            if matches!(j.state, JobState::Running(_)) {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ Propriedades

pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let all = sources(&env.core.project);
    let st = &mut env.ed.deliver;
    if st.source.as_ref().is_none_or(|s| !all.iter().any(|(x, _)| x == s)) {
        st.source = all.first().map(|(s, _)| s.clone());
    }
    let preset = PRESETS[st.preset.min(PRESETS.len() - 1)];
    section(ui, &t, "deliver.request");
    ui.label(egui::RichText::new(tr(preset.key)).strong());
    dim_label(ui, &t, tr("deliver.source"));
    let current = st.source.as_ref().and_then(|s| all.iter().find(|(x, _)| x == s)).map(|(_, l)| l.clone()).unwrap_or_else(|| tr("deliver.no_source").to_owned());
    let mut pick = None;
    let r = egui::ComboBox::from_id_salt("deliver_src").width((ui.available_width() - 8.0).max(80.0)).selected_text(current).show_ui(ui, |ui| {
        for (s, l) in &all {
            if ui.selectable_label(env.ed.deliver.source.as_ref() == Some(s), l).clicked() {
                pick = Some(s.clone());
            }
        }
    });
    env.note("deliver:source", r.response.rect);
    if let Some(s) = pick {
        env.ed.deliver.source = Some(s);
        env.ed.deliver.name.clear();
    }
    dim_label(ui, &t, tr("deliver.file_name"));
    let hint = env.ed.deliver.source.as_ref().map(|s| safe_name(&source_name(&env.core.project, s))).unwrap_or_default();
    let r = ui.add(egui::TextEdit::singleline(&mut env.ed.deliver.name).desired_width((ui.available_width() - 8.0).max(80.0)).hint_text(hint));
    env.note("deliver:name", r.rect);
    match out_dir(env.core) {
        Some(d) => dim_label(ui, &t, &format!("{} {}", tr("deliver.folder"), d.display())),
        None => dim_label(ui, &t, tr("board.save_first")),
    }
    egui::Grid::new("deliver_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("deliver.codec"));
        ui.label(preset.codec);
        ui.end_row();
        if let Some(def) = preset.crf {
            dim_label(ui, &t, tr("deliver.crf"));
            let mut v = env.ed.deliver.crf.unwrap_or(def);
            let r = ui.add(egui::DragValue::new(&mut v).range(10..=35).speed(0.1));
            env.note("deliver:crf", r.rect);
            if v != env.ed.deliver.crf.unwrap_or(def) {
                env.ed.deliver.crf = Some(v);
            }
            ui.end_row();
        }
        dim_label(ui, &t, tr("deliver.loudness"));
        ui.label(match preset.loudness {
            Some("arib") => "ARIB TR-B32 · −24 LKFS",
            Some("ebu") => "EBU R128 · −23 LUFS",
            Some("streaming") => "−14 LUFS",
            _ => tr("deliver.no_audio"),
        });
        ui.end_row();
        dim_label(ui, &t, tr("deliver.srt"));
        ui.label(tr(if preset.sidecar_srt { "deliver.yes" } else { "deliver.no" }));
        ui.end_row();
    });
    ui.add_space(6.0);
    let b = ui.add_enabled(env.ed.deliver.source.is_some(), egui::Button::new(tr("deliver.export")));
    env.note("deliver:props:export", b.rect);
    if b.on_hover_text(tr("deliver.export.tip")).clicked() {
        enqueue(env);
        let ctx = ui.ctx().clone();
        poll(env, &ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::Id;

    #[test]
    fn params_follow_the_preset_and_the_source() {
        let cut = Source::Cut(Id("c1".into()));
        let v = export_params(&cut, &PRESETS[0], "C:\\x\\a.mp4", None);
        assert_eq!(v["cut"], "c1");
        assert_eq!(v["codec"], "h264");
        assert_eq!(v["crf"], 18);
        assert_eq!(v["preset"], "arib");
        assert_eq!(v["sidecar_srt"], true);
        let v = export_params(&Source::Edit(Id("e".into())), &PRESETS[4], "C:\\x\\a_%05d.png", Some(22));
        assert_eq!(v["edit"], "e");
        assert_eq!(v["audio"], false);
        assert_eq!(v["crf"], 22, "o ajuste manual vence o preset");
        assert!(v.get("preset").is_none());
        // os parâmetros são aceitos pelo pedido da bridge (sem campo desconhecido)
        for p in PRESETS {
            let v = export_params(&cut, &p, "C:\\x\\a.mp4", None);
            assert!(v.as_object().unwrap().keys().all(|k| ["cut", "edit", "episode", "out", "codec", "crf", "preset", "audio", "sidecar_srt"].contains(&k.as_str())));
        }
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(safe_name("最後の一輪 — edição"), "最後の一輪 — edição");
        assert_eq!(safe_name("a/b:c*?"), "a_b_c__");
        assert_eq!(safe_name("  .  "), "entrega");
    }

    #[test]
    fn preset_keys_exist_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        for p in PRESETS {
            for k in [p.key, p.tip] {
                assert_ne!(tr_in(Lang::En, k), k);
                assert_ne!(tr_in(Lang::Pt, k), k);
            }
        }
    }

    #[test]
    fn export_runs_on_a_session_copy() {
        // a cópia vê o mesmo documento e os mesmos blobs que a sessão original
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, 6, None).unwrap();
        let before = core.change_counter;
        let mut copy = CoreSession::new(core.project.clone());
        copy.blobs = core.blobs.clone();
        let dir = std::env::temp_dir().join("aiai_deliver_test");
        let out = dir.join("c.mp4");
        let r = actions::run(&mut copy, "EXPORT_VIDEO", export_params(&Source::Cut(cut), &PRESETS[0], &out.display().to_string(), None));
        // sem FFmpeg no ambiente de teste o erro vem como texto; com FFmpeg o arquivo existe
        match r {
            Ok(v) => assert_eq!(v["frames"], 6),
            Err(e) => assert!(!e.is_empty()),
        }
        assert_eq!(core.change_counter, before, "a sessão original não muda");
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! Workspace Áudio: painel Mídia (cortes do projeto; clique = corte ativo), Monitor (o quadro do corte no quadro atual,
//! renderizado em segundo plano) e Mixer (os clipes de áudio do corte por trilha — diálogo, SE, BGM, ambiente — sobre a
//! duração do corte com o cabeçote, ganho, pan, início e fades do clipe selecionado, medição de loudness da mixagem do
//! corte e saída em WAV do corte e da edição).
//!
//! Mudanças no clipe regravam a lista `audio` do corte por `APPLY_COMMAND` `SetProps` (o mesmo caminho do `aiai_apply`;
//! um passo de desfazer por ajuste). Medir e gravar WAV são `LOUDNESS`, `MIX_CUT` e `EDIT_MIX` da bridge; a mixagem da
//! edição inteira roda numa thread com cópia da sessão (só lê o documento).

use super::commit_on_release;
use super::production::{CutThumbs, THUMB_BUDGET};
use crate::app::actions::{self, DocKind};
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_core::{AudioPlacement, AudioTrack, Command, CoreSession, Id, NodeKind, Project};
use egui::{Color32, FontId, Id as EId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

pub(crate) const TRACKS: [(AudioTrack, &str); 4] = [
    (AudioTrack::Dialogue, "audio.track.dialogue"),
    (AudioTrack::Sfx, "audio.track.sfx"),
    (AudioTrack::Bgm, "audio.track.bgm"),
    (AudioTrack::Ambience, "audio.track.ambience"),
];

pub(crate) fn track_key(t: AudioTrack) -> &'static str {
    TRACKS.iter().find(|(x, _)| *x == t).map(|(_, k)| *k).unwrap_or("audio.track.dialogue")
}

type Job = crossbeam_channel::Receiver<Result<Value, String>>;

/// Estado do workspace Áudio que não mora no documento.
#[derive(Default)]
pub(crate) struct AudioState {
    /// Clipe selecionado no corte ativo (id da colocação).
    pub sel: Option<Id>,
    pub thumbs: CutThumbs,
    pub monitor: super::edit::EditPreview,
    /// Duração (s) de cada asset de áudio, lida uma vez do WAV.
    durations: HashMap<String, Option<f32>>,
    /// Formulário "Adicionar ao corte": texto da fala, personagem, preset de efeito, importação em andamento.
    tts_text: String,
    tts_character: Option<Id>,
    sfx_preset: usize,
    /// Clima do BGM gerado (índice em `Mood::ALL`).
    bgm_mood: usize,
    /// Verificação das ferramentas externas (FFmpeg, VOICEVOX, Kokoro): em andamento e o último resultado.
    health_job: Option<crossbeam_channel::Receiver<Vec<aiai_tools::health::Health>>>,
    health: Option<Vec<aiai_tools::health::Health>>,
    import_job: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// Forma de onda de cada asset: pico (0..1) a cada 10 ms, lida uma vez.
    waves: HashMap<String, Option<Vec<f32>>>,
    /// Última medição: (corte, change_counter, relatório).
    pub loudness: Option<(NodeId, u64, Value)>,
    /// Mixagem da edição em andamento.
    edit_mix: Option<Job>,
}

/// Lista nova de clipes do corte, gravada num passo (`SetProps` com a lista inteira).
pub(crate) fn set_audio(core: &mut CoreSession, cut: &NodeId, list: &[AudioPlacement]) -> Result<Value, String> {
    let cmd = Command::SetProps { id: cut.clone(), name: None, data: json!({ "audio": list }) };
    let command = serde_json::to_value(&cmd).map_err(|e| e.to_string())?;
    actions::run(core, "APPLY_COMMAND", json!({ "command": command }))
}

fn cut_audio(p: &Project, cut: &NodeId) -> (u32, Vec<AudioPlacement>) {
    match p.node(cut).map(|n| &n.data) {
        Some(NodeKind::Cut(cd)) => (cd.duration, cd.audio.clone()),
        _ => (0, Vec::new()),
    }
}

fn asset_name(p: &Project, id: &Id) -> String {
    p.node(id).map(|n| n.name.clone()).unwrap_or_else(|| "?".into())
}

/// Duração usada do clipe em segundos (a explícita, ou a do WAV menos o corte do início).
/// Picos a cada 10 ms (máximo absoluto entre os canais) do asset de áudio.
pub(crate) fn wave_peaks<'a>(st: &'a mut AudioState, core: &CoreSession, asset: &Id) -> Option<&'a Vec<f32>> {
    st.waves
        .entry(asset.0.clone())
        .or_insert_with(|| {
            let b = aiai_tools::audio::asset_audio(&core.project, &core.blobs, asset).ok()?;
            let step = (b.rate as usize / 100).max(1);
            let n = b.ch.first().map(|c| c.len()).unwrap_or(0);
            Some((0..n.div_ceil(step)).map(|k| b.ch.iter().flat_map(|c| c[k * step..((k + 1) * step).min(n)].iter()).fold(0.0f32, |m, v| m.max(v.abs())).min(1.0)).collect())
        })
        .as_ref()
}

pub(crate) fn clip_seconds(st: &mut AudioState, core: &CoreSession, a: &AudioPlacement) -> f32 {
    if let Some(d) = a.duration {
        return d;
    }
    let full = *st.durations.entry(a.asset.0.clone()).or_insert_with(|| aiai_tools::audio::asset_audio(&core.project, &core.blobs, &a.asset).ok().map(|b| b.duration_s() as f32));
    (full.unwrap_or(1.0) - a.trim_start).max(0.05)
}

// ------------------------------------------------------------------------------------------------ Mídia (cortes)

pub(crate) fn media(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let cuts = super::edit::project_cuts(&env.core.project);
    let active = env.ed.active_cut(env.core);
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    let fps = env.core.project.settings.fps();
    let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
    egui::ScrollArea::vertical().id_salt("audio_media").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if cuts.is_empty() {
            ui.label(egui::RichText::new(tr("panel.media.empty")).color(t.text_dim));
        }
        for (i, (cut, code, scene, dur)) in cuts.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click());
            env.note(format!("audio:cut:{i}"), rect);
            let painter = ui.painter().clone();
            if active.as_ref() == Some(cut) {
                painter.rect_filled(rect, 4.0, t.accent_dim);
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let th = 36.0;
            let thumb = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 4.0), egui::vec2((th * w / h).min(72.0), th));
            painter.rect_filled(thumb, 2.0, Color32::WHITE);
            if let Some(tex) = env.ed.audio.thumbs.get(&ctx, env.core, cut, &mut budget) {
                painter.image(tex, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let n_clips = cut_audio(&env.core.project, cut).1.len();
            let clip = Rect::from_min_max(egui::pos2(thumb.right() + 8.0, rect.top()), rect.max);
            let tp = painter.with_clip_rect(clip);
            tp.text(egui::pos2(clip.left(), rect.top() + 6.0), egui::Align2::LEFT_TOP, format!("{code}  ·  {:.1} s  ·  {n_clips} {}", *dur as f64 / fps, tr("audio.clips")), FontId::proportional(12.5), t.text);
            tp.text(egui::pos2(clip.left(), rect.top() + 24.0), egui::Align2::LEFT_TOP, scene, FontId::proportional(11.0), t.text_dim);
            if resp.clicked() {
                env.ed.set_cut(cut.clone());
                env.ed.audio.sel = None;
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ Monitor (corte)

pub(crate) fn monitor(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.label(egui::RichText::new(tr("panel.preview.empty")).color(t.text_dim));
        return;
    };
    let frame = env.ed.frame;
    let ctx = ui.ctx().clone();
    let core = &*env.core;
    env.ed.audio.monitor.request(&ctx, (core.change_counter, cut.0.clone(), frame), || {
        let (project, blobs, c2) = (core.project.clone(), core.blobs.clone(), cut.clone());
        move || aiai_render::render_frame(&project, &blobs, &c2, frame, &aiai_render::RenderOptions { background: Some([1.0, 1.0, 1.0]), ..Default::default() }).map(|o| o.image).map_err(|e| e.to_string())
    });
    let area = ui.max_rect();
    let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
    let s = (area.width() / w).min((area.height() - 22.0).max(10.0) / h);
    let img_rect = Rect::from_center_size(egui::pos2(area.center().x, area.top() + h * s / 2.0), egui::vec2(w * s, h * s));
    let painter = ui.painter();
    painter.rect_filled(img_rect, 0.0, Color32::BLACK);
    if let Some(tex) = &env.ed.audio.monitor.texture {
        painter.image(tex.id(), img_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    env.note("audio:monitor", img_rect);
    let fps = env.core.project.settings.fps();
    painter.text(egui::pos2(img_rect.left(), img_rect.bottom() + 4.0), egui::Align2::LEFT_TOP, format!("{} {}  ·  {:.2} s", tr("editing.frame_short"), frame + 1, frame as f64 / fps), FontId::proportional(12.0), t.text_dim);
}

// ------------------------------------------------------------------------------------------------ Mixer

const LANE_H: f32 = 26.0;
const LANE_LABEL_W: f32 = 70.0;

pub(crate) fn mixer(env: &mut Env, ui: &mut Ui, body: Rect) {
    poll_edit_mix(env, ui.ctx());
    let mut child = body_ui(ui, body, 6.0);
    egui::ScrollArea::vertical().id_salt("audio_mixer").auto_shrink([false, false]).show(&mut child, |ui| mixer_body(env, ui));
}

fn mixer_body(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.label(egui::RichText::new(tr("panel.mixer.empty")).color(t.text_dim));
        return;
    };
    let (duration, list) = cut_audio(&env.core.project, &cut);
    if env.ed.audio.sel.as_ref().is_some_and(|s| !list.iter().any(|a| &a.id == s)) {
        env.ed.audio.sel = None;
    }
    let fps = env.core.project.settings.fps() as f32;
    let code = super::edit::project_cuts(&env.core.project).into_iter().find(|c| c.0 == cut).map(|c| c.1).unwrap_or_default();
    ui.label(egui::RichText::new(format!("{code}  ·  {} {}", list.len(), tr("audio.clips"))).strong());

    // ---- faixas: blocos dos clipes sobre a duração do corte, com o cabeçote
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, LANE_H * 4.0 + 6.0), Sense::hover());
    let x0 = rect.left() + LANE_LABEL_W;
    let span = (rect.right() - x0 - 4.0).max(20.0);
    let pps = span / (duration.max(1) as f32 / fps);
    let painter = ui.painter().clone();
    // fundo das faixas primeiro: os clipes, registrados depois, ficam por cima e recebem o clique
    let all = Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x0 + span, rect.top() + LANE_H * 4.0));
    let head = ui.interact(all, EId::new("audio_lanes_bg"), Sense::click());
    let mut clip_clicked = false;
    for (k, (track, key)) in TRACKS.iter().enumerate() {
        let lane = Rect::from_min_size(egui::pos2(x0, rect.top() + k as f32 * LANE_H), egui::vec2(span, LANE_H - 3.0));
        painter.text(egui::pos2(rect.left(), lane.center().y), egui::Align2::LEFT_CENTER, tr(key), FontId::proportional(11.5), t.text_dim);
        painter.rect_filled(lane, 2.0, t.bg_frame);
        for a in list.iter().filter(|a| a.track == *track) {
            let start = a.start_frame as f32 / fps;
            let secs = clip_seconds(&mut env.ed.audio, env.core, a);
            let r = Rect::from_min_max(egui::pos2(x0 + start * pps, lane.top()), egui::pos2((x0 + (start + secs) * pps).min(lane.right()).max(x0 + start * pps + 3.0), lane.bottom()));
            let resp = ui.interact(r, EId::new(("audio_clip", &a.id.0)), Sense::click());
            env.note(format!("audio:clip:{}", a.id.0), r);
            let sel = env.ed.audio.sel.as_ref() == Some(&a.id);
            painter.rect(r, 3.0, if sel { t.accent_dim } else { t.st_progress }, Stroke::new(1.0, if sel { t.accent } else { t.border }), StrokeKind::Inside);
            // forma de onda do trecho tocado (respeita o corte do início do clipe)
            if let Some(peaks) = wave_peaks(&mut env.ed.audio, env.core, &a.asset) {
                let wave = painter.with_clip_rect(r.shrink(1.5));
                let (mid, half) = (r.center().y, (r.height() * 0.5 - 2.0).max(1.0));
                let col = t.text.gamma_multiply(0.45);
                let mut x = r.left() + 1.0;
                while x < r.right() - 1.0 {
                    let k = ((a.trim_start + (x - r.left()) / pps) * 100.0) as usize;
                    let k2 = (((a.trim_start + (x + 1.0 - r.left()) / pps) * 100.0) as usize).max(k + 1);
                    let pk = peaks.get(k..k2.min(peaks.len())).map(|s| s.iter().cloned().fold(0.0f32, f32::max)).unwrap_or(0.0);
                    if pk > 0.004 {
                        let pk = pk.sqrt(); // escala de raiz: trecho baixo (BGM, ambiente) continua legível
                        wave.vline(x, (mid - pk * half)..=(mid + pk * half), Stroke::new(1.0, col));
                    }
                    x += 1.0;
                }
            }
            painter.with_clip_rect(r.shrink(2.0)).text(r.left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, asset_name(&env.core.project, &a.asset), FontId::proportional(10.5), t.text);
            if resp.on_hover_text(format!("{}  ·  {:.2} s  ·  {:+.1} dB", asset_name(&env.core.project, &a.asset), start, a.gain_db)).clicked() {
                env.ed.audio.sel = Some(a.id.clone());
                env.ed.frame = a.start_frame;
                clip_clicked = true;
            }
        }
    }
    if let Some(p) = head.interact_pointer_pos().filter(|_| head.clicked() && !clip_clicked) {
        env.ed.frame = (((p.x - x0) / pps * fps).floor().max(0.0) as u32).min(duration.saturating_sub(1));
    }
    let xh = x0 + env.ed.frame as f32 / fps * pps;
    painter.vline(xh, all.y_range(), Stroke::new(1.5, t.accent));

    // ---- clipe selecionado
    let mut new_list = list.clone();
    let mut changed = false;
    if let Some(k) = env.ed.audio.sel.as_ref().and_then(|s| list.iter().position(|a| &a.id == s)) {
        let a = list[k].clone();
        section(ui, &t, "audio.clip");
        ui.label(egui::RichText::new(format!("{}  ·  {}", asset_name(&env.core.project, &a.asset), tr(track_key(a.track)))).size(12.5));
        egui::Grid::new("audio_clip_grid").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
            dim_label(ui, &t, tr("audio.gain"));
            let mut gr = Rect::NOTHING;
            if let Some(v) = commit_on_release(ui, EId::new(("au_gain", &a.id.0)), a.gain_db, |ui, v| {
                let r = ui.add(egui::DragValue::new(v).range(-60.0..=12.0).speed(0.1).max_decimals(1).suffix(" dB"));
                gr = r.rect;
                r
            }) {
                new_list[k].gain_db = v;
                changed = true;
            }
            env.note("audio:gain", gr);
            dim_label(ui, &t, tr("audio.pan"));
            if let Some(v) = commit_on_release(ui, EId::new(("au_pan", &a.id.0)), a.pan, |ui, v| ui.add(egui::DragValue::new(v).range(-1.0..=1.0).speed(0.01).max_decimals(2))) {
                new_list[k].pan = v;
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("audio.start"));
            if let Some(v) = commit_on_release(ui, EId::new(("au_start", &a.id.0)), a.start_frame, |ui, v| ui.add(egui::DragValue::new(v).range(0..=duration.saturating_sub(1)).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
                new_list[k].start_frame = v;
                changed = true;
            }
            dim_label(ui, &t, tr("audio.track"));
            let mut tk = a.track;
            egui::ComboBox::from_id_salt(("au_track", &a.id.0)).width(90.0).selected_text(tr(track_key(tk))).show_ui(ui, |ui| {
                for (x, key) in TRACKS {
                    ui.selectable_value(&mut tk, x, tr(key));
                }
            });
            if tk != a.track {
                new_list[k].track = tk;
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("audio.fade_in"));
            if let Some(v) = commit_on_release(ui, EId::new(("au_fi", &a.id.0)), a.fade_in, |ui, v| ui.add(egui::DragValue::new(v).range(0.0..=10.0).speed(0.01).max_decimals(2).suffix(" s"))) {
                new_list[k].fade_in = v;
                changed = true;
            }
            dim_label(ui, &t, tr("audio.fade_out"));
            if let Some(v) = commit_on_release(ui, EId::new(("au_fo", &a.id.0)), a.fade_out, |ui, v| ui.add(egui::DragValue::new(v).range(0.0..=10.0).speed(0.01).max_decimals(2).suffix(" s"))) {
                new_list[k].fade_out = v;
                changed = true;
            }
            ui.end_row();
        });
        let del = env.icon_button(ui, crate::icons::IconId::Trash, "audio.remove", false, true, "audio:remove");
        if del.clicked() {
            new_list.remove(k);
            env.ed.audio.sel = None;
            changed = true;
        }
    } else {
        dim_label(ui, &t, tr("audio.pick_clip"));
    }
    if changed {
        let r = set_audio(env.core, &cut, &new_list);
        actions::notify_err(env.ed, r);
    }

    add_to_cut(env, ui, &cut);

    // ---- loudness e saídas
    section(ui, &t, "audio.loudness");
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(!list.is_empty(), egui::Button::new(tr("audio.measure")));
        env.note("audio:measure", b.rect);
        if b.on_hover_text(tr("audio.measure.tip")).clicked() {
            let r = actions::run(env.core, "LOUDNESS", json!({ "cut": cut }));
            if let Some(v) = actions::notify_err(env.ed, r) {
                env.ed.audio.loudness = Some((cut.clone(), env.core.change_counter, v));
            }
        }
        let b = ui.add_enabled(!list.is_empty(), egui::Button::new(tr("audio.cut_wav")));
        env.note("audio:cut_wav", b.rect);
        if b.on_hover_text(tr("audio.cut_wav.tip")).clicked() {
            cut_wav(env, &cut, &code);
        }
        let edits = actions::docs_of(&env.core.project, DocKind::Edit);
        let busy = env.ed.audio.edit_mix.is_some();
        let b = ui.add_enabled(!edits.is_empty() && !busy, egui::Button::new(if busy { tr("audio.edit_wav_busy") } else { tr("audio.edit_wav") }));
        env.note("audio:edit_wav", b.rect);
        if b.on_hover_text(tr("audio.edit_wav.tip")).clicked() {
            let id = env.ed.edit.sel_edit.clone().filter(|e| edits.iter().any(|(i, _)| i == e)).unwrap_or_else(|| edits[0].0.clone());
            let name = edits.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_default();
            edit_wav(env, &id, &name);
        }
    });
    if let Some((lc, at, v)) = &env.ed.audio.loudness {
        if *lc == cut {
            if *at != env.core.change_counter {
                dim_label(ui, &t, tr("audio.stale"));
            }
            let f = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".into());
            egui::Grid::new("audio_lufs").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                for (label, key, unit) in [("audio.integrated", "integrated_lufs", "LUFS"), ("audio.true_peak", "true_peak_dbtp", "dBTP"), ("audio.lra", "lra_lu", "LU"), ("audio.short_max", "short_term_max_lufs", "LUFS")] {
                    dim_label(ui, &t, tr(label));
                    ui.label(egui::RichText::new(format!("{} {unit}", f(key))).size(12.5));
                    ui.end_row();
                }
            });
            dim_label(ui, &t, tr("audio.targets"));
        }
    }
    let mut on = env.ed.live.enabled();
    let c = ui.checkbox(&mut on, tr("audio.live")).on_hover_text(tr("audio.live.tip"));
    env.note("audio:live", c.rect);
    if c.changed() {
        env.ed.live.set_enabled(on);
    }
    if let Some(e) = &env.ed.live.last_error {
        dim_label(ui, &t, e);
    }
}

const SFX_PRESETS: [&str; 10] = ["whoosh", "impact", "kira", "footstep", "door", "wind", "cloth", "chime", "pop", "magic_swell"];

/// Adicionar áudio no cabeçote do corte: importar WAV, fala por TTS (VOICEVOX local), efeito sonoro procedural e
/// lip-sync do clipe de fala selecionado — as mesmas ações da bridge/MCP.
fn add_to_cut(env: &mut Env, ui: &mut Ui, cut: &NodeId) {
    let t = env.t;
    section(ui, &t, "audio.add");
    let frame = env.ed.frame;
    // importar WAV (diálogo nativo numa thread; o resultado chega por canal)
    if let Some(rx) = &env.ed.audio.import_job {
        if let Ok(res) = rx.try_recv() {
            env.ed.audio.import_job = None;
            if let Some(path) = res {
                let r = actions::run(env.core, "IMPORT_AUDIO", json!({ "path": path.display().to_string(), "cut": cut, "start_frame": frame, "track": "sfx" }));
                actions::notify_err(env.ed, r);
            }
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(env.ed.audio.import_job.is_none(), egui::Button::new(tr("audio.import")));
        env.note("audio:import", b.rect);
        if b.on_hover_text(tr("audio.import.tip")).clicked() {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let start = env.core.path.clone();
            let _ = std::thread::Builder::new().name("aiai-import-audio".into()).spawn(move || {
                let mut d = rfd::FileDialog::new().add_filter("WAV", &["wav"]);
                if let Some(p) = start {
                    d = d.set_directory(p);
                }
                let _ = tx.send(d.pick_file());
            });
            env.ed.audio.import_job = Some(rx);
        }
        // efeito sonoro
        let mut k = env.ed.audio.sfx_preset.min(SFX_PRESETS.len() - 1);
        let r = egui::ComboBox::from_id_salt("audio_sfx_preset").selected_text(SFX_PRESETS[k]).show_ui(ui, |ui| {
            for (i, p) in SFX_PRESETS.iter().enumerate() {
                ui.selectable_value(&mut k, i, *p);
            }
        });
        env.note("audio:sfx_preset", r.response.rect);
        env.ed.audio.sfx_preset = k;
        let b = ui.button(tr("audio.add_sfx"));
        env.note("audio:add_sfx", b.rect);
        if b.clicked() {
            let r = actions::run(env.core, "SFX", json!({ "preset": SFX_PRESETS[k], "seed": frame, "cut": cut, "start_frame": frame }));
            actions::notify_err(env.ed, r);
        }
    });
    // ferramentas externas (auditoria 4.6g): verificar numa thread e mostrar o que está pronto e o que falta
    if let Some(rx) = &env.ed.audio.health_job {
        if let Ok(h) = rx.try_recv() {
            env.ed.audio.health = Some(h);
            env.ed.audio.health_job = None;
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
    ui.horizontal_wrapped(|ui| {
        let busy = env.ed.audio.health_job.is_some();
        let b = ui.add_enabled(!busy, egui::Button::new(tr(if busy { "audio.health.running" } else { "audio.health" })));
        env.note("audio:health", b.rect);
        if b.on_hover_text(tr("audio.health.tip")).clicked() {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let _ = std::thread::Builder::new().name("aiai-health".into()).spawn(move || {
                let _ = tx.send(aiai_tools::health::check());
            });
            env.ed.audio.health_job = Some(rx);
        }
    });
    if let Some(h) = env.ed.audio.health.clone() {
        let t = env.t;
        for item in h {
            ui.horizontal_wrapped(|ui| {
                let (mark, color) = match &item.status {
                    Ok(_) => ("✔", egui::Color32::from_rgb(110, 200, 120)),
                    Err(_) => ("✖", egui::Color32::from_rgb(230, 110, 100)),
                };
                ui.label(egui::RichText::new(mark).color(color));
                ui.label(egui::RichText::new(item.name).strong());
                let text = match &item.status {
                    Ok(v) | Err(v) => v.clone(),
                };
                ui.label(egui::RichText::new(text).size(11.5).color(t.text_dim));
            });
        }
    }
    // BGM (auditoria 4.7c): trilha original em loop do tamanho do corte, pelo clima, no sintetizador local
    ui.horizontal_wrapped(|ui| {
        use aiai_audio::bgm::Mood;
        let mut k = env.ed.audio.bgm_mood.min(Mood::ALL.len() - 1);
        let key = |m: Mood| match m {
            Mood::Calm => "audio.bgm.calm",
            Mood::Happy => "audio.bgm.happy",
            Mood::Tense => "audio.bgm.tense",
            Mood::Sad => "audio.bgm.sad",
        };
        let r = egui::ComboBox::from_id_salt("audio_bgm_mood").selected_text(tr(key(Mood::ALL[k]))).show_ui(ui, |ui| {
            for (i, m) in Mood::ALL.iter().enumerate() {
                ui.selectable_value(&mut k, i, tr(key(*m)));
            }
        });
        env.note("audio:bgm_mood", r.response.rect);
        env.ed.audio.bgm_mood = k;
        let b = ui.button(tr("audio.add_bgm"));
        env.note("audio:add_bgm", b.rect);
        if b.on_hover_text(tr("audio.add_bgm.tip")).clicked() {
            let p = &env.core.project;
            let secs = crate::app::commands::cut_data(p, cut).map(|c| c.duration as f64 / p.settings.fps()).unwrap_or(4.0) as f32;
            let mood = Mood::ALL[k];
            let song = aiai_audio::bgm::mood_song(mood, secs, env.core.change_counter);
            let r = actions::run(env.core, "BGM", json!({ "song": song, "cut": cut, "start_frame": 0, "name": format!("BGM {}", tr(key(mood))), "fade_out": 0.5 }));
            actions::notify_err(env.ed, r);
        }
    });
    // fala por TTS
    let chars: Vec<(Id, String)> = env.core.project.nodes.values().filter(|n| matches!(n.data, NodeKind::Character(_))).map(|n| (n.id.clone(), n.name.clone())).collect();
    ui.horizontal_wrapped(|ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut env.ed.audio.tts_text).hint_text(tr("audio.tts_hint")).desired_width(180.0));
        env.note("audio:tts_text", te.rect);
        let cur = env.ed.audio.tts_character.clone().filter(|c| chars.iter().any(|(i, _)| i == c)).or_else(|| chars.first().map(|c| c.0.clone()));
        let label = cur.as_ref().and_then(|c| chars.iter().find(|(i, _)| i == c)).map(|c| c.1.clone()).unwrap_or_else(|| tr("audio.no_character").to_owned());
        let mut pick = cur.clone();
        let r = egui::ComboBox::from_id_salt("audio_tts_char").selected_text(label).show_ui(ui, |ui| {
            for (i, n) in &chars {
                ui.selectable_value(&mut pick, Some(i.clone()), n);
            }
        });
        env.note("audio:tts_character", r.response.rect);
        env.ed.audio.tts_character = pick.clone();
        let b = ui.add_enabled(pick.is_some() && !env.ed.audio.tts_text.trim().is_empty(), egui::Button::new(tr("audio.add_tts")));
        env.note("audio:add_tts", b.rect);
        if b.on_hover_text(tr("audio.add_tts.tip")).clicked() {
            let r = actions::run(env.core, "TTS_LINE", json!({ "text": env.ed.audio.tts_text.trim(), "character": pick, "cut": cut, "start_frame": frame, "track": "dialogue" }));
            if r.is_ok() {
                env.ed.audio.tts_text.clear();
            }
            actions::notify_err(env.ed, r);
        }
    });
    // lip-sync do clipe selecionado na camada ativa (cels 1..6 = fechada, a, i, u, e, o, como o settei de bocas)
    let sel = env.ed.audio.sel.clone();
    let layer = env.ed.active_layer(env.core);
    let b = ui.add_enabled(sel.is_some() && layer.is_some(), egui::Button::new(tr("audio.lipsync")));
    env.note("audio:lipsync", b.rect);
    if b.on_hover_text(tr("audio.lipsync.tip")).clicked() {
        let mapping = json!({ "closed": 1, "a": 2, "i": 3, "u": 4, "e": 5, "o": 6 });
        let r = actions::run(env.core, "LIPSYNC", json!({ "cut": cut, "placement": sel, "mouth_layer": layer, "mapping": mapping }));
        actions::notify_err(env.ed, r);
    }
}

fn out_dir(core: &CoreSession) -> Option<std::path::PathBuf> {
    let path = core.path.clone()?;
    Some(path.parent().map(|d| d.to_path_buf()).unwrap_or(path).join("saida"))
}

/// WAV da mixagem do corte, normalizado para ARIB (−24 LKFS), em `saida/<corte>.wav`.
fn cut_wav(env: &mut Env, cut: &NodeId, code: &str) {
    let Some(dir) = out_dir(env.core) else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let out = dir.join(format!("{}.wav", super::deliver::safe_name(code)));
    let r = actions::run(env.core, "MIX_CUT", json!({ "cut": cut, "out": out.display().to_string(), "preset": "arib" }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        let lufs = v.pointer("/loudness/integrated_lufs").and_then(Value::as_f64).map(|x| format!(" · {x:.1} LUFS")).unwrap_or_default();
        env.ed.notify(format!("{}: {}{lufs}", tr("audio.wav_done"), out.display()));
    }
}

/// WAV da edição inteira (todas as falas, SE e BGM na ordem da edição), ARIB, em segundo plano numa cópia da sessão.
fn edit_wav(env: &mut Env, edit: &NodeId, name: &str) {
    let Some(dir) = out_dir(env.core) else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let out = dir.join(format!("{}.wav", super::deliver::safe_name(name)));
    let mut copy = CoreSession::new(env.core.project.clone());
    copy.blobs = env.core.blobs.clone();
    copy.path = env.core.path.clone();
    let params = json!({ "edit": edit, "out": out.display().to_string(), "preset": "arib" });
    let (tx, rx) = crossbeam_channel::bounded(1);
    if std::thread::Builder::new().name("aiai-edit-mix".into()).spawn(move || {
        let _ = tx.send(actions::run(&mut copy, "EDIT_MIX", params));
    }).is_ok()
    {
        env.ed.audio.edit_mix = Some(rx);
        env.ed.notify(tr("audio.edit_wav_busy"));
    }
}

fn poll_edit_mix(env: &mut Env, ctx: &egui::Context) {
    let Some(rx) = &env.ed.audio.edit_mix else { return };
    match rx.try_recv() {
        Ok(r) => {
            env.ed.audio.edit_mix = None;
            if let Some(v) = actions::notify_err(env.ed, r) {
                let lufs = v.pointer("/loudness/integrated_lufs").and_then(Value::as_f64).map(|x| format!(" · {x:.1} LUFS")).unwrap_or_default();
                let path = v.get("path").and_then(Value::as_str).unwrap_or("");
                env.ed.notify(format!("{}: {path}{lufs}", tr("audio.wav_done")));
            }
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(250)),
        Err(crossbeam_channel::TryRecvError::Disconnected) => env.ed.audio.edit_mix = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;

    #[test]
    fn clip_edits_go_through_apply_command_with_undo() {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, 48, None).unwrap();
        // um asset qualquer basta para a lista (o mixer só lê o WAV ao medir)
        let asset = Id("asset-x".into());
        let a = AudioPlacement { id: Id("pl-1".into()), asset: asset.clone(), track: AudioTrack::Sfx, start_frame: 4, trim_start: 0.0, duration: Some(0.5), gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: None };
        let r = set_audio(&mut core, &cut, &[a.clone()]);
        // o núcleo pode recusar asset inexistente; se aceitar, o desfazer devolve a lista vazia
        if r.is_ok() {
            assert_eq!(cut_audio(&core.project, &cut).1, vec![a.clone()]);
            let mut b = a.clone();
            b.gain_db = -6.0;
            set_audio(&mut core, &cut, &[b.clone()]).unwrap();
            assert_eq!(cut_audio(&core.project, &cut).1[0].gain_db, -6.0);
            assert_eq!(core.log().last().map(|e| e.author.as_str()), Some(AUTHOR));
            core.undo().unwrap();
            assert_eq!(cut_audio(&core.project, &cut).1[0].gain_db, 0.0);
        } else {
            assert!(!r.unwrap_err().is_empty());
        }
    }

    #[test]
    fn track_keys_exist() {
        use crate::i18n::{Lang, tr_in};
        for (t, k) in TRACKS {
            assert_eq!(track_key(t), k);
            assert_ne!(tr_in(Lang::En, k), k);
        }
    }
}

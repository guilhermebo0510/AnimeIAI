//! Painel Medidores (S24 3.11): loudness BS.1770/EBU R128 ao vivo — momentâneo (400 ms), curto prazo (3 s), integrado
//! (com portas), faixa de loudness (LRA), true peak — e o pico de amostra por canal, do que está tocando:
//! - a reprodução do workspace (editor, prévia do rack, takes, multipista): o `LoudnessMeter` roda no fio do player e
//!   recebe cada bloco quando o dispositivo termina de tocá-lo;
//! - a reprodução da linha do tempo (som ao vivo do corte): o medidor acompanha o cabeçote sobre a mixagem do corte.
//!
//! Alvos: EBU R128 (−23 LUFS, TP −1), ATSC A/85 (−24, TP −2), streaming (−14, TP −1) e ARIB TR-B32 (−24, TP −1), com a
//! marca de aprovado (integrado a ±1 LU do alvo e true peak abaixo do teto). "Medir" faz a medição offline do clipe do
//! editor ou da mixagem do corte inteiro (`meters::analyze`).

use super::player::{MeterFeed, MeterSnap};
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_audio::AudioBuf;
use aiai_audio::meters::MeterReport;
use egui::{Color32, FontId, Rect, Sense, Stroke, Ui};
use std::sync::Arc;

/// (chave i18n, alvo integrado LUFS, teto de true peak dBTP).
pub(crate) const TARGETS: [(&str, f64, f64); 4] = [("audio.meter.t.ebu", -23.0, -1.0), ("audio.meter.t.atsc", -24.0, -2.0), ("audio.meter.t.stream", -14.0, -1.0), ("audio.meter.t.arib", -24.0, -1.0)];
/// Tolerância do integrado (LU).
pub(crate) const TOLERANCE_LU: f64 = 1.0;

/// Medidor que acompanha a linha do tempo: mixagem do corte em cache e o ponto até onde já entrou no medidor.
struct Follow {
    cut: NodeId,
    counter: u64,
    mix: Arc<AudioBuf>,
    pos: usize,
    feed: MeterFeed,
}

#[derive(Default)]
pub(crate) struct MeterState {
    pub target: usize,
    follow: Option<Follow>,
    /// Última leitura da linha do tempo (fica depois de parar).
    follow_last: Option<MeterSnap>,
    /// Medição offline: (rótulo, relatório).
    pub offline: Option<(String, MeterReport)>,
    offline_rx: Option<crossbeam_channel::Receiver<Result<(String, MeterReport), String>>>,
    /// Pico segurado por canal (dBFS) e quando.
    hold: Vec<(f32, std::time::Instant)>,
}

/// Aprovado/reprovado contra o alvo: `None` enquanto não há integrado.
pub(crate) fn verdict(integrated: Option<f64>, true_peak: Option<f64>, target: usize) -> Option<(bool, Vec<String>)> {
    let (_, lufs, tp_max) = TARGETS[target.min(TARGETS.len() - 1)];
    let i = integrated?;
    let mut why = Vec::new();
    let d = i - lufs;
    if d.abs() > TOLERANCE_LU {
        why.push(format!("{} {:+.1} LU", tr(if d > 0.0 { "audio.meter.too_loud" } else { "audio.meter.too_soft" }), d));
    }
    if let Some(tp) = true_peak.filter(|tp| *tp > tp_max) {
        why.push(format!("{} {tp:.1} > {tp_max:.1} dBTP", tr("audio.meter.tp_over")));
    }
    Some((why.is_empty(), why))
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    poll_offline(env, ui.ctx());
    let (snap, source) = current(env);
    let mut child = body_ui(ui, body, 6.0);
    egui::ScrollArea::vertical().id_salt("audio_meters").auto_shrink([false, false]).show(&mut child, |ui| meters_body(env, ui, &snap, &source));
}

/// Leitura a mostrar e de onde vem.
fn current(env: &mut Env) -> (MeterSnap, String) {
    let pl = &env.ed.audio_ws.player;
    if let Some(p) = pl.playing() {
        return (pl.last_meter.clone(), format!("{}: {}", tr("audio.meter.src_play"), p.label));
    }
    // linha do tempo tocando: acompanha o cabeçote sobre a mixagem do corte
    let cut = env.ed.cut.clone();
    if env.ed.playback.playing {
        if let Some(cut) = cut {
            let fps = env.core.project.settings.fps().max(1.0);
            let st = &mut env.ed.audio_ws.meters;
            if st.follow.as_ref().is_none_or(|f| f.cut != cut || f.counter != env.core.change_counter) {
                match aiai_tools::audio::mix_cut(&env.core.project, &env.core.blobs, &cut, None) {
                    Ok(m) => {
                        let rate = m.rate;
                        st.follow = Some(Follow { cut: cut.clone(), counter: env.core.change_counter, mix: Arc::new(m), pos: usize::MAX, feed: MeterFeed::new(rate, 2) });
                    }
                    Err(_) => st.follow = None,
                }
            }
            if let Some(f) = &mut st.follow {
                let n = f.mix.frames();
                let now = ((env.ed.frame as f64 / fps) * f.mix.rate as f64) as usize;
                let now = now.min(n);
                if f.pos == usize::MAX || now < f.pos || now > f.pos + f.mix.rate as usize {
                    // começou, deu a volta no laço ou pulou: o medidor recomeça daqui
                    f.feed = MeterFeed::new(f.mix.rate, 2);
                    f.pos = now;
                }
                if now > f.pos {
                    let mut data = Vec::with_capacity((now - f.pos) * 2);
                    for i in f.pos..now {
                        data.push(f.mix.ch[0][i]);
                        data.push(f.mix.ch.get(1).unwrap_or(&f.mix.ch[0])[i]);
                    }
                    f.feed.push(&data, 2);
                    f.pos = now;
                }
                st.follow_last = Some(f.feed.snap.clone());
                return (f.feed.snap.clone(), tr("audio.meter.src_timeline").to_owned());
            }
        }
    } else if let Some(f) = &mut env.ed.audio_ws.meters.follow {
        // parou: a próxima reprodução recomeça a medição
        f.pos = usize::MAX;
    }
    let st = &env.ed.audio_ws;
    match &st.meters.follow_last {
        Some(s) if st.player.last_label.is_empty() || s.seconds > 0.0 && st.player.last_meter.seconds == 0.0 => (s.clone(), tr("audio.meter.src_stopped").to_owned()),
        _ => (st.player.last_meter.clone(), if st.player.last_label.is_empty() { tr("audio.meter.src_none").to_owned() } else { format!("{}: {}", tr("audio.meter.src_stopped"), st.player.last_label) }),
    }
}

fn fmt(v: Option<f64>) -> String {
    v.filter(|x| x.is_finite()).map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".into())
}

fn meters_body(env: &mut Env, ui: &mut Ui, s: &MeterSnap, source: &str) {
    let t = env.t;
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, source);
        dim_label(ui, &t, &format!("· {:.1} s", s.seconds));
    });
    // ---- alvo
    ui.horizontal_wrapped(|ui| {
        let mut k = env.ed.audio_ws.meters.target.min(TARGETS.len() - 1);
        let r = egui::ComboBox::from_id_salt("audio_meter_target").width(190.0).selected_text(tr(TARGETS[k].0)).show_ui(ui, |ui| {
            for (i, (key, _, _)) in TARGETS.iter().enumerate() {
                ui.selectable_value(&mut k, i, tr(key));
            }
        });
        env.note("audio:meter:target", r.response.rect);
        env.ed.audio_ws.meters.target = k;
        match verdict(s.integrated, s.true_peak, k) {
            None => {
                dim_label(ui, &t, tr("audio.meter.measuring"));
            }
            Some((ok, why)) => {
                let (txt, col) = if ok { (format!("✔ {}", tr("audio.meter.pass")), t.st_ok) } else { (format!("✖ {}", tr("audio.meter.fail")), t.st_retake) };
                let r = ui.label(egui::RichText::new(txt).strong().background_color(col).color(t.text));
                env.note("audio:meter:verdict", r.rect);
                if !why.is_empty() {
                    dim_label(ui, &t, &why.join(" · "));
                }
            }
        }
    });
    // ---- números grandes
    let (_, lufs_t, tp_t) = TARGETS[env.ed.audio_ws.meters.target.min(TARGETS.len() - 1)];
    ui.horizontal_wrapped(|ui| {
        for (key, v, unit, name) in [
            ("audio.meter.momentary", s.momentary, "LUFS", "audio:meter:m"),
            ("audio.meter.short", s.short_term, "LUFS", "audio:meter:s"),
            ("audio.meter.integrated", s.integrated, "LUFS", "audio:meter:i"),
            ("audio.meter.lra", s.lra, "LU", "audio:meter:lra"),
            ("audio.meter.true_peak", s.true_peak, "dBTP", "audio:meter:tp"),
        ] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(92.0, 46.0), Sense::hover());
            env.note(name, r);
            let p = ui.painter();
            p.rect_filled(r, 4.0, t.bg_widget);
            p.text(r.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, tr(key), FontId::proportional(10.5), t.text_dim);
            let over = match name {
                "audio:meter:tp" => v.is_some_and(|x| x > tp_t),
                "audio:meter:i" => v.is_some_and(|x| (x - lufs_t).abs() > TOLERANCE_LU),
                _ => false,
            };
            p.text(r.left_bottom() + egui::vec2(6.0, -5.0), egui::Align2::LEFT_BOTTOM, fmt(v), FontId::monospace(18.0), if over { t.danger } else { t.text });
            p.text(r.right_bottom() + egui::vec2(-5.0, -6.0), egui::Align2::RIGHT_BOTTOM, unit, FontId::proportional(9.5), t.text_dim);
        }
    });
    // ---- barras: momentâneo e curto prazo (−60..0 LUFS) com o alvo, picos por canal (−60..0 dBFS)
    section(ui, &t, "audio.meter.bars");
    let w = ui.available_width().min(520.0);
    bar(ui, &t, "M", s.momentary, -60.0, 0.0, Some(lufs_t), w);
    bar(ui, &t, "S", s.short_term, -60.0, 0.0, Some(lufs_t), w);
    let hold = &mut env.ed.audio_ws.meters.hold;
    hold.resize(s.peak.len(), (-90.0, std::time::Instant::now()));
    for (c, pk) in s.peak.iter().enumerate() {
        let db = if *pk > 0.0 { 20.0 * pk.log10() } else { -90.0 };
        let (h, at) = &mut hold[c];
        if db >= *h || at.elapsed().as_secs_f32() > 1.5 {
            *h = db;
            *at = std::time::Instant::now();
        }
        let label = if s.peak.len() == 2 { if c == 0 { "L" } else { "R" } } else { "Pk" };
        bar(ui, &t, label, Some(db as f64), -60.0, 0.0, None, w);
        let _ = h;
    }
    // ---- histórico do curto prazo com a linha do alvo
    section(ui, &t, "audio.meter.history");
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 70.0), Sense::hover());
    env.note("audio:meter:history", r);
    let p = ui.painter();
    p.rect_filled(r, 3.0, t.bg_frame);
    let y = |v: f32| r.bottom() - ((v + 60.0) / 60.0).clamp(0.0, 1.0) * r.height();
    p.hline(r.x_range(), y(lufs_t as f32), Stroke::new(1.0, t.st_ok));
    p.text(egui::pos2(r.right() - 4.0, y(lufs_t as f32) - 2.0), egui::Align2::RIGHT_BOTTOM, format!("{lufs_t:.0}"), FontId::monospace(9.5), t.st_ok);
    let n = s.history.len();
    if n > 1 {
        let shown = n.min(r.width() as usize);
        let pts: Vec<egui::Pos2> = s.history[n - shown..].iter().enumerate().map(|(i, v)| egui::pos2(r.right() - (shown - 1 - i) as f32, y(*v))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.3, Color32::from_rgb(96, 168, 255))));
    }
    // ---- medição offline
    section(ui, &t, "audio.meter.offline");
    ui.horizontal_wrapped(|ui| {
        let busy = env.ed.audio_ws.meters.offline_rx.is_some();
        let b = ui.add_enabled(!busy, egui::Button::new(tr("audio.meter.measure_clip")));
        env.note("audio:meter:measure_clip", b.rect);
        if b.on_hover_text(tr("audio.meter.measure_clip.tip")).clicked() {
            measure_clip(env);
        }
        let b = ui.add_enabled(!busy && env.ed.cut.is_some(), egui::Button::new(tr("audio.meter.measure_cut")));
        env.note("audio:meter:measure_cut", b.rect);
        if b.on_hover_text(tr("audio.meter.measure_cut.tip")).clicked() {
            measure_cut(env);
        }
        if busy {
            ui.spinner();
        }
    });
    if let Some((label, rep)) = &env.ed.audio_ws.meters.offline {
        dim_label(ui, &t, label);
        egui::Grid::new("audio_meter_off").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
            for (k, v, u) in [
                ("audio.meter.integrated", rep.integrated_lufs, "LUFS"),
                ("audio.meter.short_max", rep.short_term_max_lufs, "LUFS"),
                ("audio.meter.lra", rep.lra_lu, "LU"),
                ("audio.meter.true_peak", rep.true_peak_dbtp, "dBTP"),
                ("audio.meter.rms", rep.rms_dbfs, "dBFS"),
            ] {
                dim_label(ui, &t, tr(k));
                ui.label(egui::RichText::new(format!("{} {u}", fmt(v))).monospace());
                ui.end_row();
            }
            dim_label(ui, &t, tr("audio.meter.clipped"));
            ui.label(egui::RichText::new(rep.clipped_samples.to_string()).monospace());
            ui.end_row();
        });
        if let Some((ok, why)) = verdict(rep.integrated_lufs, rep.true_peak_dbtp, env.ed.audio_ws.meters.target) {
            let txt = if ok { format!("✔ {}", tr("audio.meter.pass")) } else { format!("✖ {} — {}", tr("audio.meter.fail"), why.join(" · ")) };
            ui.label(egui::RichText::new(txt).color(if ok { t.st_ok } else { t.danger }));
        }
    }
}

/// Barra horizontal de `lo` a `hi` com marca do alvo (verde ±1 LU), zona quente acima de −6 e vermelha acima de 0.
fn bar(ui: &mut Ui, t: &crate::theme::Tokens, label: &str, v: Option<f64>, lo: f64, hi: f64, target: Option<f64>, w: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 14.0), Sense::hover());
    let p = ui.painter();
    let lab = Rect::from_min_size(r.min, egui::vec2(18.0, r.height()));
    p.text(lab.left_center(), egui::Align2::LEFT_CENTER, label, FontId::monospace(10.0), t.text_dim);
    let track = Rect::from_min_max(egui::pos2(lab.right(), r.top() + 2.0), egui::pos2(r.right() - 44.0, r.bottom() - 2.0));
    p.rect_filled(track, 2.0, t.bg_frame);
    let x = |d: f64| track.left() + (((d - lo) / (hi - lo)).clamp(0.0, 1.0) as f32) * track.width();
    if let Some(tg) = target {
        p.rect_filled(Rect::from_min_max(egui::pos2(x(tg - TOLERANCE_LU), track.top()), egui::pos2(x(tg + TOLERANCE_LU), track.bottom())), 0.0, t.st_ok.gamma_multiply(0.5));
    }
    if let Some(d) = v.filter(|d| d.is_finite()) {
        let col = if d > -0.1 { t.danger } else if d > -6.0 { Color32::from_rgb(235, 190, 60) } else { Color32::from_rgb(80, 190, 120) };
        p.rect_filled(Rect::from_min_max(track.min, egui::pos2(x(d), track.bottom())), 2.0, col);
    }
    if let Some(tg) = target {
        p.vline(x(tg), track.y_range(), Stroke::new(1.5, t.text));
    }
    p.text(egui::pos2(r.right(), r.center().y), egui::Align2::RIGHT_CENTER, fmt(v), FontId::monospace(10.0), t.text);
}

fn measure_clip(env: &mut Env) {
    let Some((asset, _)) = super::wave::resolve(env) else { return };
    let Some(l) = env.ed.audio_ws.load(env.core, &asset) else { return };
    let label = super::node_name(&env.core.project, &asset);
    let (tx, rx) = crossbeam_channel::bounded(1);
    let buf = l.buf.clone();
    if std::thread::Builder::new().name("aiai-meter".into()).spawn(move || {
        let _ = tx.send(Ok((label, aiai_audio::meters::analyze(&buf, 0))));
    }).is_ok()
    {
        env.ed.audio_ws.meters.offline_rx = Some(rx);
    }
}

fn measure_cut(env: &mut Env) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let (project, blobs) = (env.core.project.clone(), env.core.blobs.clone());
    let code = super::super::edit::project_cuts(&project).into_iter().find(|c| c.0 == cut).map(|c| c.1).unwrap_or_default();
    let (tx, rx) = crossbeam_channel::bounded(1);
    if std::thread::Builder::new().name("aiai-meter".into()).spawn(move || {
        let r = aiai_tools::audio::mix_cut(&project, &blobs, &cut, None).map(|m| (format!("{} {code}", tr("audio.meter.cut_mix")), aiai_audio::meters::analyze(&m, 0))).map_err(|e| e.to_string());
        let _ = tx.send(r);
    }).is_ok()
    {
        env.ed.audio_ws.meters.offline_rx = Some(rx);
    }
}

fn poll_offline(env: &mut Env, ctx: &egui::Context) {
    let Some(rx) = &env.ed.audio_ws.meters.offline_rx else { return };
    match rx.try_recv() {
        Ok(r) => {
            env.ed.audio_ws.meters.offline_rx = None;
            match r {
                Ok(v) => env.ed.audio_ws.meters.offline = Some(v),
                Err(e) => env.ed.notify(e),
            }
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(120)),
        Err(crossbeam_channel::TryRecvError::Disconnected) => env.ed.audio_ws.meters.offline_rx = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_needs_integrated_and_checks_tolerance_and_ceiling() {
        assert!(verdict(None, Some(-3.0), 0).is_none());
        assert_eq!(verdict(Some(-23.4), Some(-1.5), 0).map(|v| v.0), Some(true));
        let (ok, why) = verdict(Some(-20.0), Some(-1.5), 0).unwrap();
        assert!(!ok && why.len() == 1);
        let (ok, why) = verdict(Some(-23.0), Some(-0.5), 0).unwrap();
        assert!(!ok && why[0].contains("dBTP"));
        // ATSC: teto -2 dBTP
        assert_eq!(verdict(Some(-24.0), Some(-1.5), 1).map(|v| v.0), Some(false));
        assert_eq!(verdict(Some(-14.3), Some(-1.2), 2).map(|v| v.0), Some(true));
    }
}

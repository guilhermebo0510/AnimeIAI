//! Verificação final (preflight) antes de exportar: um semáforo (passa / atenção / falha) com um link para onde corrigir.
//! Confere o codificador, a fonte e a duração, a imagem e o som da edição, mídia ausente ou danificada, o loudness contra o
//! alvo do formato, as legendas fora do limite, o tamanho do quadro, as fontes dos textos e o espaço em disco.
//!
//! Cada resultado tem um `code` fixo e `args` (valores); o texto em português sai de [`message_pt`] e a interface troca
//! pelo texto do idioma ativo com os mesmos `args`. Falha não trava o botão sozinha: quem exporta decide ("exportar mesmo
//! assim"), mas o motor recusa o que não consegue fazer (quadro ímpar, tamanhos diferentes).

use super::formats::{FormatPreset, Kind};
use super::{SourceKind, SourceRef, mix};
use crate::edit::captions::{CaptionIssue, CaptionLimits, check_captions};
use crate::edit::{ItemSource, TrackKind, load_edit};
use aiai_core::{BlobStore, LayerType, NodeKind, Project};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Pass,
    Warn,
    Fail,
}

/// Para onde ir corrigir: workspace (`edicao`, `audio`...) e, se houver, o painel (`captions`, `meters`...).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fix {
    pub workspace: &'static str,
    pub panel: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Check {
    pub code: &'static str,
    pub level: Level,
    pub args: Vec<String>,
    /// Texto em português (a interface usa o do idioma ativo a partir de `code` e `args`).
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// Loudness medido na mixagem (antes de normalizar).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Loudness {
    pub lufs: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub target_lufs: Option<f64>,
    /// Ganho que a normalização do formato aplicaria (dB).
    pub gain_db: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub worst: Level,
    pub loudness: Option<Loudness>,
}

impl Report {
    pub fn count(&self, l: Level) -> usize {
        self.checks.iter().filter(|c| c.level == l).count()
    }
}

pub struct Input<'a> {
    pub project: &'a Project,
    pub blobs: &'a BlobStore,
    pub source: &'a SourceRef,
    pub preset: &'a FormatPreset,
    /// Pasta onde a entrega será gravada.
    pub out_dir: &'a Path,
    /// Mixa e mede o loudness (demora: mixa a fonte inteira).
    pub measure_loudness: bool,
    /// A pasta não pode ser usada por uma regra de fora (confinamento do MCP): vira a falha da pasta, sem tocar no disco.
    pub folder_error: Option<String>,
}

/// Todos os códigos que [`run`] pode devolver (a interface tem um texto i18n para cada um).
pub const CODES: &[&str] = &[
    "encoder_ok", "encoder_missing", "source_ok", "source_missing", "source_empty", "span_ok", "span_gap", "span_overhang", "media_ok", "media_damaged", "media_asset", "loudness_ok", "loudness_far", "loudness_info", "loudness_clip",
    "loudness_silent", "loudness_mute", "captions_ok", "captions_bad", "size_ok", "size_odd", "size_mixed", "fonts_ok", "fonts_missing", "disk_ok", "disk_low", "folder_ok", "folder_bad",
];

/// Texto em português de um resultado: `{0}`, `{1}`... são trocados por `args`.
pub fn message_pt(code: &str, args: &[String]) -> String {
    let t = match code {
        "encoder_ok" => "Codificador disponível: {0}",
        "encoder_missing" => "Formato indisponível neste PC: {0}",
        "source_ok" => "{0} quadros · {1} s · {2}",
        "source_missing" => "Fonte indisponível: {0}",
        "source_empty" => "A fonte não tem nenhum quadro para exportar",
        "span_ok" => "Imagem e som da edição cobrem a mesma duração",
        "span_gap" => "A edição tem {0} trecho(s) sem imagem (sai preto), o primeiro em {1} s",
        "span_overhang" => "O áudio passa {0} s do fim da imagem",
        "media_ok" => "Mídia completa: {0} corte(s) sem arquivo danificado",
        "media_damaged" => "{0} bloco(s) de imagem danificado(s) nos cortes exportados; por exemplo {1}",
        "media_asset" => "{0} mídia(s) da edição sem arquivo; por exemplo {1}",
        "loudness_ok" => "Loudness {0} LUFS: o alvo {1} LUFS é alcançado com ganho de {2} dB",
        "loudness_far" => "Loudness {0} LUFS está longe do alvo {1} LUFS: a normalização muda {2} dB",
        "loudness_info" => "Loudness {0} LUFS, pico real {1} dBTP (este formato não tem alvo)",
        "loudness_clip" => "Pico real {0} dBTP passa de −1 dBTP e este formato não normaliza",
        "loudness_silent" => "A mixagem é silêncio",
        "loudness_mute" => "A fonte não tem nenhum som: o arquivo sai sem áudio",
        "captions_ok" => "{0} legenda(s) dentro dos limites de leitura",
        "captions_bad" => "{0} de {1} legendas passam dos limites ({2} rápidas, {3} com linha longa, {4} outras)",
        "size_ok" => "{0}×{1} a {2} fps",
        "size_odd" => "{0}×{1}: largura e altura precisam ser pares neste formato",
        "size_mixed" => "O corte {0} tem quadro {1}×{2}, diferente do projeto ({3}×{4}): a exportação falharia",
        "fonts_ok" => "{0} camada(s) de texto com a fonte instalada",
        "fonts_missing" => "{0} camada(s) de texto com fonte ausente: {1}",
        "disk_ok" => "{0} livres no disco {1} — a entrega deve ocupar uns {2}",
        "disk_low" => "Só {0} livres no disco {1} — a entrega deve ocupar uns {2}",
        "folder_ok" => "Pasta de entrega gravável: {0}",
        "folder_bad" => "Não consigo gravar em {0}: {1}",
        other => return format!("{other} {}", args.join(" ")),
    };
    fill(t, args)
}

/// Troca `{0}`, `{1}`... pelos argumentos (usado também pela interface com o texto do idioma ativo).
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = template.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

fn mk(code: &'static str, level: Level, args: Vec<String>, fix: Option<Fix>) -> Check {
    Check { code, level, message: message_pt(code, &args), args, fix }
}

fn fix(workspace: &'static str, panel: Option<&'static str>) -> Option<Fix> {
    Some(Fix { workspace, panel })
}

fn secs(frames: u32, fps: f64) -> String {
    format!("{:.1}", frames as f64 / fps.max(1.0))
}

/// Espaço em disco legível.
fn gb(bytes: u64) -> String {
    super::store::fmt_size(bytes)
}

pub fn run(i: &Input) -> Report {
    let p = i.project;
    let fps = p.settings.fps();
    let mut checks: Vec<Check> = Vec::new();
    let mut loud = None;

    // codificador
    if i.preset.video_codec().is_some() {
        match i.preset.availability() {
            Ok(()) => checks.push(mk("encoder_ok", Level::Pass, vec![i.preset.codec.to_owned()], None)),
            Err(e) => checks.push(mk("encoder_missing", Level::Fail, vec![e], None)),
        }
    }

    // fonte e duração
    let frames = match i.source.info(p) {
        Ok(info) if info.frames == 0 => {
            checks.push(mk("source_empty", Level::Fail, vec![], None));
            0
        }
        Ok(info) => {
            checks.push(mk("source_ok", Level::Pass, vec![info.frames.to_string(), secs(info.frames, fps), format!("{} · {}", info.name, aiai_label(fps))], None));
            info.frames
        }
        Err(e) => {
            checks.push(mk("source_missing", Level::Fail, vec![e.to_string()], None));
            0
        }
    };

    if frames > 0 {
        span_checks(i, fps, &mut checks);
        media_checks(i, &mut checks);
        size_checks(i, fps, &mut checks);
        font_checks(i, &mut checks);
        caption_checks(i, fps, &mut checks);
        if i.measure_loudness {
            loud = loudness_checks(i, &mut checks);
        }
        disk_checks(i, frames, fps, &mut checks);
    }
    folder_check(i, &mut checks);

    let worst = checks.iter().map(|c| c.level).max().unwrap_or(Level::Pass);
    Report { checks, worst, loudness: loud }
}

fn aiai_label(fps: f64) -> String {
    format!("{} fps", if (fps - fps.round()).abs() < 1e-6 { format!("{fps:.0}") } else { format!("{fps:.3}") })
}

/// Edição: trechos sem imagem (preto) e som que passa do fim da imagem.
fn span_checks(i: &Input, fps: f64, out: &mut Vec<Check>) {
    if i.source.kind != SourceKind::Edit {
        return;
    }
    let Ok(doc) = load_edit(i.project, &i.source.id) else { return };
    let video: Vec<u32> = doc.tracks_of(TrackKind::Video).iter().filter(|t| !t.hidden).map(|t| t.id).collect();
    let audio: Vec<u32> = doc.tracks_of(TrackKind::Audio).iter().filter(|t| !t.muted).map(|t| t.id).collect();
    let mut spans: Vec<(u32, u32)> = doc.items.iter().filter(|it| it.enabled && video.contains(&it.track)).map(|it| (it.start, it.end())).collect();
    spans.sort();
    let content_end = doc.content_end();
    let (mut covered, mut gaps, mut first_gap) = (0u32, 0u32, None);
    for (s, e) in &spans {
        if *s > covered && covered < content_end {
            gaps += 1;
            first_gap.get_or_insert(covered);
        }
        covered = covered.max(*e);
    }
    let video_end = covered;
    // o começo da edição sem imagem também é preto
    if spans.first().is_some_and(|(s, _)| *s > 0) && first_gap.is_none() {
        gaps += 1;
        first_gap = Some(0);
    }
    let audio_end = doc.items.iter().filter(|it| it.enabled && audio.contains(&it.track)).map(|it| it.end()).max().unwrap_or(0);
    let mut flagged = false;
    if gaps > 0 {
        flagged = true;
        out.push(mk("span_gap", Level::Warn, vec![gaps.to_string(), secs(first_gap.unwrap_or(0), fps)], fix("edicao", None)));
    }
    if audio_end > video_end + 1 && video_end > 0 {
        flagged = true;
        out.push(mk("span_overhang", Level::Warn, vec![secs(audio_end - video_end, fps)], fix("edicao", None)));
    }
    if !flagged {
        out.push(mk("span_ok", Level::Pass, vec![], None));
    }
}

/// Mídia ausente ou danificada nos cortes que a fonte usa (blobs que não puderam ser lidos ao abrir) e mídias da edição sem arquivo.
fn media_checks(i: &Input, out: &mut Vec<Check>) {
    let p = i.project;
    let Ok(cuts) = i.source.used_cuts(p) else { return };
    let missing = i.blobs.missing();
    let mut damaged = 0usize;
    let mut first: Option<String> = None;
    if !missing.is_empty() {
        for cut in &cuts {
            for id in p.subtree(cut) {
                let Some(n) = p.node(&id) else { continue };
                let NodeKind::Cel(c) = &n.data else { continue };
                let layer = n.parent.as_ref().and_then(|l| p.node(l)).map(|l| l.name.as_str()).unwrap_or("?");
                let mut hit = |h: &aiai_core::BlobHash, what: &str| {
                    if missing.contains(h) {
                        damaged += 1;
                        first.get_or_insert_with(|| format!("camada \"{layer}\", cel \"{}\" ({what})", n.name));
                    }
                };
                for t in &c.tiles {
                    hit(&t.blob, "tile");
                }
                for f in &c.fills {
                    if let Some(m) = &f.mask {
                        hit(m, "máscara");
                    }
                }
                if let Some(img) = &c.image {
                    hit(img, "imagem");
                }
            }
        }
    }
    // camadas 3D que entram no render (visíveis, fora do modo referência): o modelo precisa estar no projeto
    // (auditoria F02: o blob do modelo faltando passava como "mídia completa")
    for cut in &cuts {
        for id in p.subtree(cut) {
            let Some(n) = p.node(&id) else { continue };
            let NodeKind::Layer(l) = &n.data else { continue };
            if l.kind != LayerType::Model3d || !l.visible {
                continue;
            }
            if let Some(Ok(mp)) = aiai_core::model3d::Model3dParams::from_layer_params(&l.params) {
                if !mp.render.reference && (missing.contains(&mp.model_hash) || !i.blobs.contains(&mp.model_hash)) {
                    damaged += 1;
                    first.get_or_insert_with(|| format!("camada 3D \"{}\" (modelo)", n.name));
                }
            }
        }
    }
    // mídias da edição (imagens e áudios importados) e áudio dos cortes
    let mut assets: Vec<aiai_core::Id> = Vec::new();
    if i.source.kind == SourceKind::Edit {
        if let Ok(doc) = load_edit(p, &i.source.id) {
            for it in doc.items.iter().filter(|it| it.enabled) {
                if let ItemSource::Image { asset } | ItemSource::Audio { asset } = &it.source {
                    assets.push(asset.clone());
                }
            }
        }
    }
    for cut in &cuts {
        if let Some(NodeKind::Cut(cd)) = p.node(cut).map(|n| &n.data) {
            assets.extend(cd.audio.iter().map(|a| a.asset.clone()));
        }
    }
    let mut lost = 0usize;
    let mut first_asset: Option<String> = None;
    for a in assets {
        let state = match p.node(&a).map(|n| (&n.data, &n.name)) {
            Some((NodeKind::Asset(ad), name)) => match &ad.blob {
                Some(h) if i.blobs.contains(h) => None,
                _ => Some(name.clone()),
            },
            _ => Some(a.to_string()),
        };
        if let Some(name) = state {
            lost += 1;
            first_asset.get_or_insert(name);
        }
    }
    if damaged > 0 {
        out.push(mk("media_damaged", Level::Fail, vec![damaged.to_string(), first.unwrap_or_default()], fix("pintura", None)));
    }
    if lost > 0 {
        out.push(mk("media_asset", Level::Fail, vec![lost.to_string(), first_asset.unwrap_or_default()], fix("edicao", Some("media"))));
    }
    if damaged == 0 && lost == 0 {
        out.push(mk("media_ok", Level::Pass, vec![cuts.len().to_string()], None));
    }
}

/// Tamanho do quadro: par (exigência dos codecs) e igual em todos os cortes.
fn size_checks(i: &Input, fps: f64, out: &mut Vec<Check>) {
    let p = i.project;
    let (w, h) = (p.settings.width, p.settings.height);
    let mut bad = false;
    if i.preset.kind == Kind::Video && (w % 2 != 0 || h % 2 != 0) {
        bad = true;
        out.push(mk("size_odd", Level::Fail, vec![w.to_string(), h.to_string()], None));
    }
    if let Ok(cuts) = i.source.used_cuts(p) {
        for cut in cuts {
            if let Some(NodeKind::Cut(cd)) = p.node(&cut).map(|n| &n.data) {
                if let Some([cw, ch]) = cd.canvas.filter(|c| *c != [w, h]) {
                    // só quebra a exportação de cortes (o quadro do corte entra direto no codificador); a edição reajusta ao quadro
                    if i.source.kind != SourceKind::Edit {
                        bad = true;
                        let code = p.node(&cut).map(|n| format!("C{:03}", if let NodeKind::Cut(c) = &n.data { c.number } else { 0 })).unwrap_or_default();
                        out.push(mk("size_mixed", Level::Fail, vec![code, cw.to_string(), ch.to_string(), w.to_string(), h.to_string()], fix("layout", None)));
                        break;
                    }
                }
            }
        }
    }
    if !bad {
        out.push(mk("size_ok", Level::Pass, vec![w.to_string(), h.to_string(), format!("{}", aiai_label(fps).trim_end_matches(" fps"))], None));
    }
}

/// Camadas de texto dos cortes usados cuja família não está instalada nem na pasta de fontes do usuário.
fn font_checks(i: &Input, out: &mut Vec<Check>) {
    let p = i.project;
    let Ok(cuts) = i.source.used_cuts(p) else { return };
    let (mut layers, mut missing, mut names): (usize, usize, Vec<String>) = (0, 0, Vec::new());
    for cut in &cuts {
        for id in p.subtree(cut) {
            let Some(NodeKind::Layer(l)) = p.node(&id).map(|n| &n.data) else { continue };
            if l.kind != aiai_core::LayerType::Text {
                continue;
            }
            let Some(Ok(tp)) = aiai_paint::text_layer::TextParams::from_layer_params(&l.params) else { continue };
            layers += 1;
            let fam = tp.font.trim();
            if fam.is_empty() || fam.eq_ignore_ascii_case(aiai_paint::fonts::DEFAULT_FAMILY) {
                continue;
            }
            if aiai_paint::fonts::catalog().pick(fam, tp.bold, tp.italic).is_none() {
                missing += 1;
                if !names.iter().any(|n| n == fam) {
                    names.push(fam.to_owned());
                }
            }
        }
    }
    if missing > 0 {
        out.push(mk("fonts_missing", Level::Warn, vec![missing.to_string(), names.join(", ")], fix("layout", Some("text"))));
    } else if layers > 0 {
        out.push(mk("fonts_ok", Level::Pass, vec![layers.to_string()], None));
    }
}

/// Legendas da edição fora dos limites de leitura (cps, caracteres por linha, linhas, duração).
fn caption_checks(i: &Input, fps: f64, out: &mut Vec<Check>) {
    if i.source.kind != SourceKind::Edit {
        return;
    }
    let Ok(doc) = load_edit(i.project, &i.source.id) else { return };
    let all = check_captions(&doc, None, fps, &CaptionLimits::default());
    if all.is_empty() {
        return;
    }
    let bad: Vec<_> = all.iter().filter(|c| !c.issues.is_empty()).collect();
    if bad.is_empty() {
        out.push(mk("captions_ok", Level::Pass, vec![all.len().to_string()], None));
        return;
    }
    let fast = bad.iter().filter(|c| c.issues.contains(&CaptionIssue::TooFast)).count();
    let long = bad.iter().filter(|c| c.issues.contains(&CaptionIssue::LineTooLong)).count();
    let other = bad.iter().filter(|c| c.issues.iter().any(|x| !matches!(x, CaptionIssue::TooFast | CaptionIssue::LineTooLong))).count();
    out.push(mk("captions_bad", Level::Warn, vec![bad.len().to_string(), all.len().to_string(), fast.to_string(), long.to_string(), other.to_string()], fix("edicao", Some("captions"))));
}

/// Mixa a fonte e mede (integrado e pico real) contra o alvo do formato.
fn loudness_checks(i: &Input, out: &mut Vec<Check>) -> Option<Loudness> {
    if !i.preset.audio || matches!(i.preset.kind, Kind::PngSequence | Kind::ContactSheet) {
        return None;
    }
    let mixed = match mix::mix_source(i.project, i.blobs, i.source) {
        Ok(m) => m,
        Err(e) => {
            out.push(mk("source_missing", Level::Fail, vec![e.to_string()], fix("audio", None)));
            return None;
        }
    };
    let Some(buf) = mixed else {
        out.push(mk("loudness_mute", Level::Warn, vec![], fix("audio", None)));
        return Some(Loudness { lufs: None, true_peak_dbtp: None, target_lufs: i.preset.loudness_target(), gain_db: None });
    };
    let rep = crate::audio::loudness_report(&buf);
    let target = i.preset.loudness_target();
    let tp = rep.true_peak_dbtp;
    let f1 = |v: f64| format!("{v:.1}");
    let Some(l) = rep.integrated_lufs else {
        out.push(mk("loudness_silent", Level::Warn, vec![], fix("audio", Some("meters"))));
        return Some(Loudness { lufs: None, true_peak_dbtp: tp, target_lufs: target, gain_db: None });
    };
    let gain = target.map(|t| t - l);
    match (target, gain) {
        (Some(t), Some(g)) if g.abs() > 6.0 => out.push(mk("loudness_far", Level::Warn, vec![f1(l), f1(t), format!("{g:+.1}")], fix("audio", Some("meters")))),
        (Some(t), Some(g)) => out.push(mk("loudness_ok", Level::Pass, vec![f1(l), f1(t), format!("{g:+.1}")], None)),
        _ if tp.is_some_and(|v| v > -1.0) => out.push(mk("loudness_clip", Level::Warn, vec![f1(tp.unwrap_or(0.0))], fix("audio", Some("meters")))),
        _ => out.push(mk("loudness_info", Level::Pass, vec![f1(l), tp.map(f1).unwrap_or_else(|| "—".into())], None)),
    }
    Some(Loudness { lufs: Some(l), true_peak_dbtp: tp, target_lufs: target, gain_db: gain })
}

/// Espaço livre no disco do destino contra o tamanho estimado.
fn disk_checks(i: &Input, frames: u32, fps: f64, out: &mut Vec<Check>) {
    let p = i.project;
    let est = i.preset.estimate_bytes(frames as u64, fps, p.settings.width, p.settings.height, p.settings.audio_sample_rate);
    let Some(free) = aiai_core::persist::free_space(i.out_dir) else { return };
    let drive = i.out_dir.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    // sobra de segurança: o dobro do estimado e 1 GB (a estimativa é grosseira; o disco C: deste PC vive quase cheio)
    let level = if free < est { Level::Fail } else if free < est.saturating_mul(2) + 1_000_000_000 { Level::Warn } else { Level::Pass };
    let code = if level == Level::Pass { "disk_ok" } else { "disk_low" };
    out.push(mk(code, level, vec![gb(free), drive, gb(est)], None));
}

/// A pasta de entrega existe ou pode ser criada e dá para gravar nela.
fn folder_check(i: &Input, out: &mut Vec<Check>) {
    let dir = i.out_dir;
    if let Some(e) = &i.folder_error {
        out.push(mk("folder_bad", Level::Fail, vec![dir.display().to_string(), e.clone()], None));
        return;
    }
    let probe = || -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        // nome próprio de cada conferência (duas verificações ao mesmo tempo não pisam uma na outra)
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let f = dir.join(format!(".aiai-teste-escrita-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(&f, b"x").map_err(|e| e.to_string())?;
        std::fs::remove_file(&f).map_err(|e| e.to_string())
    };
    match probe() {
        Ok(()) => out.push(mk("folder_ok", Level::Pass, vec![dir.display().to_string()], None)),
        Err(e) => out.push(mk("folder_bad", Level::Fail, vec![dir.display().to_string(), e], None)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::formats::preset;
    use super::super::testkit::session_with_cut;
    use super::*;
    use aiai_core::*;

    fn run_for(s: &CoreSession, src: &SourceRef, id: &str, loud: bool) -> Report {
        let dir = std::env::temp_dir().join(format!("aiai_pf_{}", std::process::id()));
        run(&Input { project: &s.project, blobs: &s.blobs, source: src, preset: preset(id).unwrap(), out_dir: &dir, measure_loudness: loud, folder_error: None })
    }

    fn level(r: &Report, code: &str) -> Option<Level> {
        r.checks.iter().find(|c| c.code == code).map(|c| c.level)
    }

    #[test]
    fn every_code_has_a_message_and_arguments_fill_in() {
        for c in CODES {
            assert!(!message_pt(c, &["a".into(), "b".into(), "c".into(), "d".into(), "e".into()]).starts_with(c), "{c} sem texto");
        }
        assert_eq!(fill("{0} de {1}", &["3".into(), "9".into()]), "3 de 9");
    }

    #[test]
    fn a_healthy_cut_passes_the_basics() {
        let (s, _ep, cut) = session_with_cut(24);
        let r = run_for(&s, &SourceRef::cut(cut), "preview", false);
        assert_eq!(level(&r, "source_ok"), Some(Level::Pass));
        assert_eq!(level(&r, "media_ok"), Some(Level::Pass));
        assert_eq!(level(&r, "size_ok"), Some(Level::Pass));
        assert_eq!(level(&r, "folder_ok"), Some(Level::Pass));
        assert_eq!(level(&r, "disk_ok").or(level(&r, "disk_low")).is_some(), true);
        assert!(r.checks.iter().find(|c| c.code == "source_ok").unwrap().message.starts_with("24 quadros · 1.0 s"));
        assert!(level(&r, "loudness_ok").is_none() && r.loudness.is_none(), "sem medir, sem loudness");
    }

    #[test]
    fn a_missing_source_and_an_empty_one_fail() {
        let (s, _ep, _cut) = session_with_cut(24);
        let r = run_for(&s, &SourceRef::cut(Id("nada".into())), "tv", false);
        assert_eq!(level(&r, "source_missing"), Some(Level::Fail));
        assert_eq!(r.worst, Level::Fail);
        assert!(level(&r, "media_ok").is_none(), "sem fonte, sem as outras conferências");
    }

    #[test]
    fn odd_sizes_and_mixed_canvases_fail_before_the_export_would() {
        let (mut s, ep, cut) = session_with_cut(12);
        s.apply(Command::SetProjectProps { patch: serde_json::json!({"settings": {"width": 65, "height": 36}}) }).unwrap();
        let r = run_for(&s, &SourceRef::cut(cut.clone()), "web", false);
        assert_eq!(level(&r, "size_odd"), Some(Level::Fail));
        assert!(r.checks.iter().any(|c| c.code == "size_odd" && c.message.contains("65×36")));
        // o WAV não liga para o tamanho do quadro
        let r = run_for(&s, &SourceRef::cut(cut.clone()), "wav", false);
        assert!(level(&r, "size_odd").is_none());
        // corte com outro quadro dentro de um episódio
        s.apply(Command::SetProjectProps { patch: serde_json::json!({"settings": {"width": 64, "height": 36}}) }).unwrap();
        let mut cd = match &s.project.node(&cut).unwrap().data {
            NodeKind::Cut(c) => c.clone(),
            _ => unreachable!(),
        };
        cd.canvas = Some([128, 72]);
        s.apply(Command::ReplaceData { id: cut.clone(), data: NodeKind::Cut(cd) }).unwrap();
        let r = run_for(&s, &SourceRef::episode(ep), "tv", false);
        assert_eq!(level(&r, "size_mixed"), Some(Level::Fail));
        assert_eq!(r.checks.iter().find(|c| c.code == "size_mixed").unwrap().fix, Some(Fix { workspace: "layout", panel: None }));
    }

    #[test]
    fn damaged_blobs_in_an_exported_cut_are_reported_with_where() {
        let (mut s, _ep, cut) = session_with_cut(12);
        // marca como danificado um tile do corte (como o abrir tolerante faz)
        let hash = s.project.subtree(&cut).iter().find_map(|id| match &s.project.node(id).unwrap().data {
            NodeKind::Cel(c) => c.tiles.first().map(|t| t.blob.clone()),
            _ => None,
        });
        let hash = hash.expect("o céu pintou tiles");
        let mut blobs = BlobStore::default();
        blobs.mark_missing(hash.clone());
        s.blobs = blobs;
        let r = run_for(&s, &SourceRef::cut(cut), "tv", false);
        assert_eq!(level(&r, "media_damaged"), Some(Level::Fail));
        let c = r.checks.iter().find(|c| c.code == "media_damaged").unwrap();
        assert!(c.message.contains("camada \"BG\"") && c.fix.is_some(), "{}", c.message);
    }

    #[test]
    fn loudness_is_measured_against_the_format_target() {
        use aiai_audio::AudioBuf;
        let (mut s, _ep, cut) = session_with_cut(48);
        // 2 s de seno a −20 dBFS de pico: bem abaixo do −14 LUFS do streaming
        let n = 96_000usize;
        let tone: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.1).collect();
        let wav = crate::audio::wav_bytes(&AudioBuf::mono(48_000, tone), crate::audio::WavBits::Pcm16).unwrap();
        let blob = s.blobs.put(wav);
        let asset = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "tom".into(), data: NodeKind::Asset(AssetData { asset: AssetType::AudioClip, blob: Some(blob), params: serde_json::Value::Null, source: "procedural".into() }) }).unwrap().created[0].clone();
        let mut cd = match &s.project.node(&cut).unwrap().data {
            NodeKind::Cut(c) => c.clone(),
            _ => unreachable!(),
        };
        cd.audio.push(AudioPlacement { id: Id("p0".into()), asset, track: AudioTrack::Dialogue, start_frame: 0, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: None });
        s.apply(Command::ReplaceData { id: cut.clone(), data: NodeKind::Cut(cd) }).unwrap();
        let src = SourceRef::cut(cut.clone());
        let web = run_for(&s, &src, "web", true);
        let l = web.loudness.clone().expect("mediu");
        assert!(l.lufs.is_some_and(|v| v < -20.0 && v > -40.0), "{l:?}");
        assert_eq!(l.target_lufs, Some(-14.0));
        let c = web.checks.iter().find(|c| c.code.starts_with("loudness_")).unwrap();
        assert!(matches!(c.code, "loudness_ok" | "loudness_far"), "{}", c.message);
        // sem alvo (WAV cru): só informa; a fonte sem som sai como aviso de mudo
        let wav_r = run_for(&s, &src, "wav", true);
        assert_eq!(level(&wav_r, "loudness_info"), Some(Level::Pass));
        let (silent, _e, silent_cut) = session_with_cut(24);
        let mute = run_for(&silent, &SourceRef::cut(silent_cut), "tv", true);
        assert_eq!(level(&mute, "loudness_mute"), Some(Level::Warn));
        // formato sem áudio não mede
        assert!(run_for(&s, &src, "png", true).loudness.is_none());
    }

    #[test]
    fn edit_checks_find_gaps_overhang_and_fast_captions() {
        use crate::edit::{EditDoc, EditItem, ItemSource, TrackKind, edit_command};
        let (mut s, _ep, cut) = session_with_cut(48);
        let mut doc = EditDoc::with_default_tracks();
        let v1 = doc.tracks_of(TrackKind::Video)[0].id;
        let a1 = doc.tracks_of(TrackKind::Audio)[0].id;
        let s1 = doc.tracks_of(TrackKind::Subtitle)[0].id;
        // imagem de 0 a 24, buraco, imagem de 36 a 48; som até 60 (passa 12 quadros)
        doc.items.push(EditItem::new(1, v1, 0, 24, ItemSource::Cut { cut: cut.clone() }));
        doc.items.push(EditItem::new(2, v1, 36, 12, ItemSource::Cut { cut: cut.clone() }));
        doc.items.push(EditItem::new(3, a1, 0, 60, ItemSource::CutAudio { cut: cut.clone(), track: None }));
        // legenda de 60 caracteres em 1 s: 60 cps
        doc.items.push(EditItem::new(4, s1, 0, 24, ItemSource::Caption { text: "x".repeat(60), speaker: None, style: None }));
        let c = edit_command(&s.project, &mut s.ids, "Ed", &doc, None).unwrap();
        let id = s.apply(c).unwrap().created[0].clone();
        let r = run_for(&s, &SourceRef::edit(id), "web", false);
        assert_eq!(level(&r, "span_gap"), Some(Level::Warn));
        assert_eq!(r.checks.iter().find(|c| c.code == "span_gap").unwrap().args, vec!["1".to_owned(), "1.0".to_owned()]);
        assert_eq!(level(&r, "span_overhang"), Some(Level::Warn));
        assert_eq!(level(&r, "captions_bad"), Some(Level::Warn));
        let c = r.checks.iter().find(|c| c.code == "captions_bad").unwrap();
        assert_eq!(c.fix, Some(Fix { workspace: "edicao", panel: Some("captions") }));
        assert!(c.message.starts_with("1 de 1 legendas"), "{}", c.message);
        assert_eq!(r.worst, Level::Warn.max(r.worst));
    }

    #[test]
    fn text_layers_with_a_missing_font_are_flagged() {
        let (mut s, _ep, cut) = session_with_cut(12);
        let mut tp = aiai_paint::text_layer::TextParams::default();
        tp.content = "Olá".into();
        tp.font = "Fonte Que Não Existe 123".into();
        let mut data = LayerData::new(LayerType::Text);
        data.params = serde_json::json!({ aiai_paint::text_layer::TextParams::KEY: tp.to_value() });
        s.apply(Command::CreateNode { id: None, parent: Some(cut.clone()), index: None, name: "T".into(), data: NodeKind::Layer(data) }).unwrap();
        let r = run_for(&s, &SourceRef::cut(cut), "tv", false);
        assert_eq!(level(&r, "fonts_missing"), Some(Level::Warn));
        let c = r.checks.iter().find(|c| c.code == "fonts_missing").unwrap();
        assert!(c.message.contains("Fonte Que Não Existe 123") && c.fix == Some(Fix { workspace: "layout", panel: Some("text") }));
    }
}

//! Legendas da edição (S24 c.4): trilhas S com itens `Caption`, importar/exportar SRT e WebVTT, verificação de
//! caracteres por segundo e por linha (limites configuráveis; padrões 17 cps e 42 caracteres, a conferir com o guia
//! do destino) e geração a partir das falas: cada clipe de fala dos cortes da edição vira uma legenda no tempo real do
//! áudio, com o texto do roteiro (tradução quando a trilha não é do idioma original) e presa ao clipe de imagem.

use super::{EditDoc, EditItem, ItemSource, TrackKind};
use crate::context::*;
use aiai_core::*;
use aiai_media::Subtitle;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Legendas de uma trilha em segundos (ordem do tempo; itens desligados ficam de fora).
pub fn track_subtitles(doc: &EditDoc, track: u32, fps: f64) -> Vec<Subtitle> {
    doc.items_on(track)
        .into_iter()
        .filter(|i| i.enabled)
        .filter_map(|i| i.caption_text().map(|t| Subtitle { start: i.start as f64 / fps, end: i.end() as f64 / fps, text: t.to_owned() }))
        .collect()
}

fn vtt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

/// SRT de entrega (o mesmo do `aiai-media`).
pub fn write_srt(subs: &[Subtitle]) -> String {
    aiai_media::write_srt(subs)
}

pub fn write_vtt(subs: &[Subtitle]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for s in subs {
        out.push_str(&format!("{} --> {}\n{}\n\n", vtt_time(s.start), vtt_time(s.end), s.text.trim()));
    }
    out
}

/// "00:01:02,345", "01:02.345" (VTT curto) → segundos.
fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Lê SRT ou WebVTT (detecta pelo cabeçalho). Tags simples (`<i>`, `{\an8}`) são removidas; blocos sem tempo, ignorados.
pub fn parse_captions(text: &str) -> Result<Vec<Subtitle>, String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let (a, b) = lines[ti].split_once("-->").ok_or("linha de tempo inválida")?;
        // VTT: "00:01.000 --> 00:02.000 align:center"
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else { return Err(format!("tempo inválido: {}", lines[ti])) };
        let body: Vec<String> = lines[ti + 1..].iter().map(|l| strip_tags(l)).filter(|l| !l.is_empty()).collect();
        if body.is_empty() || end <= start {
            continue;
        }
        out.push(Subtitle { start, end, text: body.join("\n") });
    }
    if out.is_empty() && !text.trim().is_empty() && !text.trim_start().starts_with("WEBVTT") {
        return Err("nenhuma legenda reconhecida (esperado SRT ou WebVTT)".into());
    }
    Ok(out)
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut depth_angle = false;
    let mut depth_brace = false;
    for c in s.chars() {
        match c {
            '<' => depth_angle = true,
            '>' if depth_angle => depth_angle = false,
            '{' => depth_brace = true,
            '}' if depth_brace => depth_brace = false,
            c if !depth_angle && !depth_brace => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

/// Põe legendas (segundos) na trilha `track`; `replace` apaga as que já estavam. Devolve os ids criados.
pub fn import_into(doc: &mut EditDoc, track: u32, subs: &[Subtitle], fps: f64, replace: bool) -> Result<Vec<u32>, String> {
    if doc.track(track).map(|t| t.kind) != Some(TrackKind::Subtitle) {
        return Err(format!("trilha {track} não é de legenda"));
    }
    if replace {
        doc.items.retain(|i| i.track != track);
    }
    let mut ids = Vec::new();
    for s in subs {
        let start = (s.start * fps).round().max(0.0) as u32;
        let end = ((s.end * fps).round() as u32).max(start + 1);
        let id = doc.next_item_id();
        doc.clear_range(track, start, end, &[]);
        doc.items.push(EditItem::new(id, track, start, end - start, ItemSource::Caption { text: s.text.clone(), speaker: None, style: None }));
        ids.push(id);
    }
    Ok(ids)
}

/// Limites da verificação.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CaptionLimits {
    /// Caracteres por segundo (padrão 17).
    pub max_cps: f32,
    /// Caracteres por linha (padrão 42).
    pub max_chars_per_line: u32,
    /// Duração mínima (s, padrão 0,8) e máxima (s, padrão 7).
    pub min_seconds: f32,
    pub max_seconds: f32,
    /// Linhas por legenda (padrão 2).
    pub max_lines: u32,
}

impl Default for CaptionLimits {
    fn default() -> Self {
        CaptionLimits { max_cps: 17.0, max_chars_per_line: 42, min_seconds: 0.8, max_seconds: 7.0, max_lines: 2 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionIssue {
    /// Rápida demais para ler (cps acima do limite).
    TooFast,
    LineTooLong,
    TooManyLines,
    TooShort,
    TooLong,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionCheck {
    pub item: u32,
    pub cps: f32,
    pub max_line: u32,
    pub lines: u32,
    pub seconds: f32,
    pub issues: Vec<CaptionIssue>,
}

/// Caracteres por segundo de um texto numa duração (quebras de linha não contam).
pub fn cps(text: &str, seconds: f32) -> f32 {
    let n = text.chars().filter(|c| *c != '\n' && *c != '\r').count() as f32;
    if seconds <= 0.0 { f32::INFINITY } else { n / seconds }
}

pub fn check_item(it: &EditItem, fps: f64, lim: &CaptionLimits) -> Option<CaptionCheck> {
    let text = it.caption_text()?;
    let seconds = (it.dur as f64 / fps) as f32;
    let c = cps(text, seconds);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let max_line = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u32;
    let mut issues = Vec::new();
    if lines.is_empty() {
        issues.push(CaptionIssue::Empty);
    }
    if c > lim.max_cps {
        issues.push(CaptionIssue::TooFast);
    }
    if max_line > lim.max_chars_per_line {
        issues.push(CaptionIssue::LineTooLong);
    }
    if lines.len() as u32 > lim.max_lines {
        issues.push(CaptionIssue::TooManyLines);
    }
    if seconds < lim.min_seconds {
        issues.push(CaptionIssue::TooShort);
    }
    if seconds > lim.max_seconds {
        issues.push(CaptionIssue::TooLong);
    }
    Some(CaptionCheck { item: it.id, cps: c, max_line, lines: lines.len() as u32, seconds, issues })
}

/// Verificação de todas as legendas de uma trilha (ou de todas as trilhas S com `None`).
pub fn check_captions(doc: &EditDoc, track: Option<u32>, fps: f64, lim: &CaptionLimits) -> Vec<CaptionCheck> {
    let mut v: Vec<&EditItem> = doc.items.iter().filter(|i| track.is_none_or(|t| i.track == t) && i.caption_text().is_some()).collect();
    v.sort_by_key(|i| (i.track, i.start));
    v.into_iter().filter_map(|i| check_item(i, fps, lim)).collect()
}

/// Uma fala do roteiro (texto original, tradução, falante).
struct Line {
    text: String,
    translation: String,
    speaker: Option<String>,
}

fn script_lines(p: &Project) -> (Vec<(Id, Line)>, String) {
    let mut out = Vec::new();
    let mut original_lang = String::from("ja");
    for n in p.nodes.values() {
        let NodeKind::Document(d) = &n.data else { continue };
        if d.doc != DocumentType::Script {
            continue;
        }
        let Ok(s) = crate::preprod::script_read(p, &n.id) else { continue };
        original_lang = s.language.clone();
        for sc in &s.scenes {
            for e in &sc.elements {
                if e.kind != crate::preprod::ElementKind::Dialogue {
                    continue;
                }
                let speaker = e.character.as_ref().and_then(|c| p.node(c)).map(|n| n.name.clone()).or_else(|| e.speaker.clone());
                out.push((e.id.clone(), Line { text: e.text.clone(), translation: e.translation.clone(), speaker }));
            }
        }
    }
    (out, original_lang)
}

/// Legendas geradas das falas (não grava): para cada clipe de imagem de corte nas trilhas de vídeo, cada clipe de fala
/// do corte vira uma legenda no tempo do áudio (presa ao clipe); corte sem áudio de fala mas com falas vinculadas do
/// roteiro reparte a duração do clipe entre elas, pelo tamanho do texto. `lang` vazio ou diferente do idioma original
/// do roteiro usa a tradução (quando houver).
pub fn captions_from_script(p: &Project, doc: &EditDoc, track: u32, lang: &str) -> ToolResult<Vec<EditItem>> {
    let fps = p.settings.fps();
    let (lines, original) = script_lines(p);
    let want_original = !lang.is_empty() && lang.eq_ignore_ascii_case(&original);
    let pick = |l: &Line| if want_original || l.translation.trim().is_empty() { l.text.clone() } else { l.translation.clone() };
    let mut next = doc.next_item_id();
    let mut out: Vec<EditItem> = Vec::new();
    let mut vids: Vec<&EditItem> = doc.items.iter().filter(|i| i.enabled && matches!(i.source, ItemSource::Cut { .. }) && doc.track(i.track).is_some_and(|t| t.kind == TrackKind::Video)).collect();
    vids.sort_by_key(|i| i.start);
    for it in vids {
        let ItemSource::Cut { cut } = &it.source else { continue };
        let cd = cut_data(p, cut)?;
        let (src0, src1) = (it.src_in as f64, it.src_out() as f64);
        let to_tl = |sf: f64| it.start as f64 + (sf - src0) / it.speed as f64;
        let mut made = 0;
        for pl in cd.audio.iter().filter(|a| a.track == AudioTrack::Dialogue) {
            let Some(NodeKind::Asset(a)) = p.node(&pl.asset).map(|n| &n.data) else { continue };
            let atext = a.params.get("text").and_then(Value::as_str).unwrap_or("").trim().to_owned();
            let secs = pl.duration.map(|d| d as f64).or_else(|| a.params.get("duration_s").and_then(Value::as_f64).map(|d| d - pl.trim_start as f64)).unwrap_or(1.5);
            let (a0, a1) = (pl.start_frame as f64, pl.start_frame as f64 + secs * fps);
            let (s0, s1) = (a0.max(src0), a1.min(src1));
            if s1 <= s0 {
                continue;
            }
            // texto do roteiro: a fala vinculada ao corte com o mesmo texto do áudio (ou a primeira com texto igual)
            let line = lines.iter().find(|(id, l)| cd.dialogue_links.contains(id) && l.text.trim() == atext).or_else(|| lines.iter().find(|(_, l)| !atext.is_empty() && l.text.trim() == atext));
            let text = line.map(|(_, l)| pick(l)).unwrap_or_else(|| atext.clone());
            if text.trim().is_empty() {
                continue;
            }
            let speaker = pl.character.as_ref().and_then(|c| p.node(c)).map(|n| n.name.clone()).or_else(|| line.and_then(|(_, l)| l.speaker.clone()));
            let (t0, t1) = (to_tl(s0).round() as u32, (to_tl(s1).round() as u32).max(to_tl(s0).round() as u32 + 1));
            let mut c = EditItem::new(next, track, t0, t1 - t0, ItemSource::Caption { text, speaker, style: None });
            c.anchor = Some(it.id);
            next += 1;
            out.push(c);
            made += 1;
        }
        if made == 0 && !cd.dialogue_links.is_empty() {
            let linked: Vec<&Line> = cd.dialogue_links.iter().filter_map(|id| lines.iter().find(|(i, _)| i == id).map(|(_, l)| l)).collect();
            let weights: Vec<f64> = linked.iter().map(|l| pick(l).chars().count().max(1) as f64).collect();
            let sum: f64 = weights.iter().sum();
            let mut t = it.start as f64;
            for (l, wgt) in linked.iter().zip(&weights) {
                let d = it.dur as f64 * wgt / sum.max(1.0);
                let (t0, t1) = (t.round() as u32, ((t + d).round() as u32).max(t.round() as u32 + 1));
                let mut c = EditItem::new(next, track, t0, t1 - t0, ItemSource::Caption { text: pick(l), speaker: l.speaker.clone(), style: None });
                c.anchor = Some(it.id);
                next += 1;
                out.push(c);
                t += d;
            }
        }
    }
    // sem sobreposição na trilha: cada legenda termina antes da seguinte começar
    out.sort_by_key(|c| c.start);
    for k in 1..out.len() {
        let s = out[k].start;
        let prev = &mut out[k - 1];
        if prev.end() > s {
            prev.dur = s.saturating_sub(prev.start).max(1);
        }
    }
    out.retain(|c| c.dur > 0);
    let mut fixed: Vec<EditItem> = Vec::new();
    for c in out {
        if fixed.last().is_some_and(|p| p.end() > c.start) {
            continue;
        }
        fixed.push(c);
    }
    Ok(fixed)
}

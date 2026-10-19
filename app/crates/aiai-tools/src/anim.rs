//! Animação: intercalar entre dois desenhos-chave (cria os cels e preenche o timesheet) e propagar o balde das
//! chaves para as intercalações.

use crate::context::*;
use crate::fill::{FillRequest, FillSeed, fill_command};
use aiai_anim::{InbetweenOptions, halves, inbetween, spacing};
use aiai_core::command::CellEdit;
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InbetweenRequest {
    pub cut: Id,
    /// Camada de linha (vetorial) com os cels-chave.
    pub layer: Id,
    /// Números dos cels-chave (ex.: 1 e 2 para A1 e A2).
    pub key_a: u32,
    pub key_b: u32,
    /// Quantidade de intercalações.
    pub count: usize,
    /// Tempos explícitos (0..1) de cada intercalação; senão `ease` ou "metades".
    #[serde(default)]
    pub times: Vec<f32>,
    #[serde(default)]
    pub ease: Option<Ease>,
    /// Usar a tabela de metades favorecendo a chave B (true) ou A (false), quando `times` e `ease` não vêm.
    #[serde(default)]
    pub halves_toward_b: Option<bool>,
    /// Número do primeiro cel novo; padrão = maior número existente + 1.
    #[serde(default)]
    pub first_number: Option<u32>,
    /// Se definido, escreve no timesheet: A no `start_frame`, intercalações e B, cada um por `hold` quadros.
    #[serde(default)]
    pub start_frame: Option<u32>,
    #[serde(default = "two")]
    pub hold: u32,
    #[serde(default)]
    pub options: InbetweenOptions,
    /// Camada de preenchimento ligada: ganha cels com os mesmos números e a mesma exposição.
    #[serde(default)]
    pub fill_layer: Option<Id>,
}

fn two() -> u32 {
    2
}

fn cels_of(p: &Project, layer: &Id) -> Vec<(u32, Id)> {
    p.node(layer)
        .map(|l| {
            l.children
                .iter()
                .filter_map(|c| p.node(c))
                .filter_map(|n| match &n.data {
                    NodeKind::Cel(c) => Some((c.number, n.id.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strokes_of<'a>(p: &'a Project, layer: &Id, number: u32) -> ToolResult<&'a [Stroke]> {
    match p.cel_by_number(layer, number).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => Ok(&c.strokes),
        _ => Err(ToolError::NotFound(format!("cel {number} da camada {layer}"))),
    }
}

/// Lote: cria os cels intercalados (e os de preenchimento) e, se pedido, as células do timesheet.
pub fn inbetween_command(p: &Project, idgen: &mut IdGen, req: &InbetweenRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let a = strokes_of(p, &req.layer, req.key_a)?;
    let b = strokes_of(p, &req.layer, req.key_b)?;
    if req.count == 0 || req.count > 64 {
        return Err(ToolError::Invalid("count entre 1 e 64".into()));
    }
    let times: Vec<f32> = if !req.times.is_empty() {
        if req.times.len() != req.count || req.times.iter().any(|t| !(0.0..=1.0).contains(t)) {
            return Err(ToolError::Invalid("times precisa ter `count` valores em 0..1".into()));
        }
        req.times.clone()
    } else if let Some(e) = req.ease {
        spacing(req.count, e)
    } else if let Some(tb) = req.halves_toward_b {
        halves(req.count, tb)
    } else {
        spacing(req.count, Ease::Linear)
    };
    let existing = cels_of(p, &req.layer);
    let mut next = req.first_number.unwrap_or_else(|| existing.iter().map(|c| c.0).max().unwrap_or(0) + 1);
    let fill_existing = req.fill_layer.as_ref().map(|f| cels_of(p, f)).unwrap_or_default();
    let mut cmds = Vec::new();
    let mut numbers = Vec::new();
    for (k, &t) in times.iter().enumerate() {
        while existing.iter().any(|c| c.0 == next) || fill_existing.iter().any(|c| c.0 == next) {
            next += 1;
        }
        let strokes = inbetween(a, b, t, &req.options);
        let mut cel = CelData::new(next);
        cel.role = CelRole::Inbetween;
        cel.strokes = strokes;
        let layer_name = p.node(&req.layer).map(|n| n.name.clone()).unwrap_or_default();
        cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(req.layer.clone()), index: None, name: format!("{layer_name}{next}"), data: NodeKind::Cel(cel) });
        if let Some(fl) = &req.fill_layer {
            let fname = p.node(fl).map(|n| n.name.clone()).unwrap_or_default();
            let mut fc = CelData::new(next);
            fc.role = CelRole::Inbetween;
            cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(fl.clone()), index: None, name: format!("{fname}{next}"), data: NodeKind::Cel(fc) });
        }
        numbers.push(next);
        let _ = k;
        next += 1;
    }
    if let Some(start) = req.start_frame {
        let seq: Vec<u32> = std::iter::once(req.key_a).chain(numbers.iter().copied()).chain(std::iter::once(req.key_b)).collect();
        let entries = exposure_entries(&seq, req.hold, start);
        if let Some((f, _)) = entries.last() {
            if *f >= cut.duration {
                return Err(ToolError::Invalid(format!("a sequência passa da duração do corte ({} ≥ {})", f, cut.duration)));
            }
        }
        for layer in std::iter::once(&req.layer).chain(req.fill_layer.iter()) {
            let track = match cut.xsheet.track_for_layer(layer) {
                Some(t) => t.id.clone(),
                None => {
                    let tid = idgen.next();
                    let name = p.node(layer).map(|n| n.name.clone()).unwrap_or_else(|| "A".into());
                    cmds.push(Command::AddTrack { cut: req.cut.clone(), index: None, track: Track::new(tid.clone(), &name, TrackKind::Drawing, Some(layer.clone())) });
                    tid
                }
            };
            cmds.push(Command::SetCells { cut: req.cut.clone(), track, cells: entries.iter().map(|(f, c)| CellEdit { frame: *f, cell: Some(*c) }).collect() });
        }
    }
    Ok(Command::Batch { commands: cmds, label: Some(format!("Intercalar {} desenho(s) entre {} e {}", req.count, req.key_a, req.key_b)) })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PropagateFillsRequest {
    pub cut: Id,
    pub fill_layer: Id,
    /// Quadros onde as chaves já estão pintadas.
    pub key_frame_a: u32,
    pub key_frame_b: u32,
    /// Quadros a pintar (intercalações); o tempo de cada um é a posição entre as chaves.
    pub frames: Vec<u32>,
    #[serde(default = "gap")]
    pub gap: f32,
}

fn gap() -> f32 {
    3.0
}

fn fills_at(p: &Project, cut: &CutData, layer: &Id, frame: u32) -> Vec<FillRegion> {
    match exposed_cel(p, cut, layer, frame).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => c.fills.clone(),
        _ => Vec::new(),
    }
}

/// Gera os comandos de balde para cada quadro, com sementes interpoladas entre as regiões das chaves (casadas por cor
/// e proximidade). Aplicar em ordem; cada um é independente.
pub fn propagate_fills(p: &Project, blobs: &mut BlobStore, req: &PropagateFillsRequest) -> ToolResult<Vec<Command>> {
    let cut = cut_data(p, &req.cut)?;
    let fa = fills_at(p, cut, &req.fill_layer, req.key_frame_a);
    let fb = fills_at(p, cut, &req.fill_layer, req.key_frame_b);
    if fa.is_empty() {
        return Err(ToolError::Invalid("a chave A não tem preenchimento para propagar".into()));
    }
    // casa cada região de A com a de B de mesma cor mais próxima
    let mut used = vec![false; fb.len()];
    let mut pairs: Vec<(FillRegion, Option<FillRegion>)> = Vec::new();
    for ra in &fa {
        let best = fb
            .iter()
            .enumerate()
            .filter(|(j, rb)| !used[*j] && rb.color == ra.color)
            .min_by(|(_, x), (_, y)| {
                let dx = (x.seed[0] - ra.seed[0]).powi(2) + (x.seed[1] - ra.seed[1]).powi(2);
                let dy = (y.seed[0] - ra.seed[0]).powi(2) + (y.seed[1] - ra.seed[1]).powi(2);
                dx.partial_cmp(&dy).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(j, _)| j);
        if let Some(j) = best {
            used[j] = true;
        }
        pairs.push((ra.clone(), best.map(|j| fb[j].clone())));
    }
    let span = (req.key_frame_b as f32 - req.key_frame_a as f32).max(1.0);
    let mut out = Vec::new();
    for &f in &req.frames {
        let t = ((f as f32 - req.key_frame_a as f32) / span).clamp(0.0, 1.0);
        let seeds: Vec<FillSeed> = pairs
            .iter()
            .map(|(ra, rb)| {
                let sb = rb.as_ref().map(|r| r.seed).unwrap_or(ra.seed);
                FillSeed { x: ra.seed[0] + (sb[0] - ra.seed[0]) * t, y: ra.seed[1] + (sb[1] - ra.seed[1]) * t, color: ra.color.clone() }
            })
            .collect();
        let req_f = FillRequest { cut: req.cut.clone(), frame: f, layer: req.fill_layer.clone(), seeds, line_layers: vec![], gap: req.gap, expand: 2, line_threshold: 0.35, close_leftovers: None };
        out.push(fill_command(p, blobs, &req_f)?);
    }
    Ok(out)
}

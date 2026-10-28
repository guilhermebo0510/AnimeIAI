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

/// Máscara de uma região pintada já decodificada, para perguntar muitas vezes "este ponto está nela?".
struct KeyRegion {
    bounds: [i32; 4],
    mask: Option<aiai_paint::Mask>,
    color: ColorRef,
}

impl KeyRegion {
    fn covers(&self, x: f32, y: f32) -> bool {
        let [bx, by, bw, bh] = self.bounds;
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        if xi < bx || yi < by || xi >= bx + bw || yi >= by + bh {
            return false;
        }
        self.mask.as_ref().is_some_and(|m| m.get((xi - bx) as u32, (yi - by) as u32) > 0.5)
    }
}

fn key_regions(blobs: &BlobStore, fills: &[FillRegion]) -> Vec<KeyRegion> {
    fills
        .iter()
        .map(|f| KeyRegion { bounds: f.bounds, mask: f.mask.as_ref().and_then(|h| blobs.get(h)).and_then(|b| aiai_paint::Mask::decode(&b).ok()), color: f.color.clone() })
        .collect()
}

/// Sementes para as regiões do quadro `frame` (no tempo `t` entre as chaves) que nenhuma das `taken` alcança: até 9
/// pontos de cada região voltam para a chave A e vão para a B pelo movimento rígido; cada ponto que cai numa região
/// pintada vota na cor dela (peso 1-t na A, t na B). Ganha a cor com mais de 40% dos votos possíveis.
#[allow(clippy::too_many_arguments)]
fn inclusion_seeds(
    p: &Project,
    blobs: &BlobStore,
    cut: &Id,
    frame: u32,
    refs: &[Id],
    gap: f32,
    t: f32,
    motion: &aiai_anim::GlobalMotion,
    fa: &[FillRegion],
    fb: &[FillRegion],
    taken: &[FillSeed],
) -> ToolResult<Vec<FillSeed>> {
    let params = aiai_paint::fill::FillParams { gap, expand: 2, line_threshold: 0.35 };
    let lr = crate::fill::compute_regions(p, blobs, cut, frame, refs, &params)?;
    let r = &lr.regions;
    let n = r.count as usize + 1;
    let (w, h) = (r.width, r.height);
    let mut area = vec![0usize; n];
    for &l in &r.labels {
        area[l as usize] += 1;
    }
    let mut seeded = vec![false; n];
    for s in taken {
        if s.x >= 0.0 && s.y >= 0.0 && (s.x as usize) < w && (s.y as usize) < h {
            seeded[r.label_at(s.x as u32, s.y as u32) as usize] = true;
        }
    }
    // até 9 amostras por região, espalhadas pela ordem de varredura
    let mut samples: Vec<Vec<[f32; 2]>> = vec![Vec::new(); n];
    let mut seen = vec![0usize; n];
    for (i, &l) in r.labels.iter().enumerate() {
        let l = l as usize;
        if l == 0 || seeded[l] || area[l] < 12 {
            continue;
        }
        let step = (area[l] / 9).max(1);
        if seen[l] % step == step / 2 && samples[l].len() < 9 {
            samples[l].push([(i % w) as f32 + 0.5, (i / w) as f32 + 0.5]);
        }
        seen[l] += 1;
    }
    let (ka, kb) = (key_regions(blobs, fa), key_regions(blobs, fb));
    let mut out = Vec::new();
    for pts in samples.iter().filter(|s| !s.is_empty()) {
        let mut votes: Vec<(ColorRef, f32)> = Vec::new();
        let mut vote = |c: &ColorRef, wgt: f32| match votes.iter_mut().find(|(x, _)| x == c) {
            Some(v) => v.1 += wgt,
            None => votes.push((c.clone(), wgt)),
        };
        for q in pts {
            let (a, b) = motion.back_and_forth(*q, t);
            if let Some(k) = ka.iter().find(|k| k.covers(a[0], a[1])) {
                vote(&k.color, 1.0 - t);
            }
            if let Some(k) = kb.iter().find(|k| k.covers(b[0], b[1])) {
                vote(&k.color, t);
            }
        }
        let possible = pts.len() as f32;
        if let Some((c, wgt)) = votes.into_iter().max_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal)) {
            if wgt >= 0.4 * possible {
                let mid = pts[pts.len() / 2];
                out.push(FillSeed { x: mid[0], y: mid[1], color: c });
            }
        }
    }
    Ok(out)
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
    // as sementes andam com o desenho (auditoria 4.6c): o mesmo movimento global das intercalações, medido nas linhas
    // de referência da camada de cor nas duas chaves; numa rotação (cabeça virando) a semente segue o arco e não sai
    // da região como na reta
    let refs = layer_data(p, &req.fill_layer).map(|l| l.line_refs.clone()).unwrap_or_default();
    let strokes_at = |frame: u32| -> Vec<Stroke> {
        refs.iter()
            .filter_map(|l| exposed_cel(p, cut, l, frame))
            .filter_map(|n| match &n.data {
                NodeKind::Cel(c) => Some(c.strokes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    };
    let motion = aiai_anim::GlobalMotion::of(&strokes_at(req.key_frame_a), &strokes_at(req.key_frame_b), &aiai_anim::InbetweenOptions::default());
    let mut out = Vec::new();
    for &f in &req.frames {
        let t = ((f as f32 - req.key_frame_a as f32) / span).clamp(0.0, 1.0);
        let seeds: Vec<FillSeed> = pairs
            .iter()
            .map(|(ra, rb)| {
                let sb = rb.as_ref().map(|r| r.seed).unwrap_or(ra.seed);
                let q = motion.at(ra.seed, sb, t);
                FillSeed { x: q[0], y: q[1], color: ra.color.clone() }
            })
            .collect();
        let mut seeds = seeds;
        // casamento por inclusão (auditoria 4.8f, a ideia do BasicPBC): cada região do quadro que nenhuma semente
        // interpolada alcançou ganha a cor da região das chaves que a contém, desfeito o movimento do desenho
        if !refs.is_empty() {
            seeds.extend(inclusion_seeds(p, blobs, &req.cut, f, &refs, req.gap, t, &motion, &fa, &fb, &seeds)?);
        }
        let req_f = FillRequest { cut: req.cut.clone(), frame: f, layer: req.fill_layer.clone(), seeds, line_layers: vec![], gap: req.gap, expand: 2, line_threshold: 0.35, close_leftovers: None, skip_seeds_on_line: false, only_unpainted: false };
        out.push(fill_command(p, blobs, &req_f)?);
    }
    Ok(out)
}

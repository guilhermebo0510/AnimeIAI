//! Colorir por dicas (auditoria 4.7h), sem IA: o artista rabisca as cores numa camada raster de dicas, por cima ou por
//! baixo da linha, sem capricho; cada região fechada pela linha ganha a cor mais presente entre os rabiscos que caem
//! nela. Com `spread`, as regiões sem rabisco que encostam numa colorida herdam a cor da vizinha com quem dividem mais
//! borda (útil para dobras e recortes pequenos), sem pintar o "fora" do desenho (regiões que tocam a borda do quadro).
//! Vira um balde comum na camada de cor: regiões, máscaras e desfazer de sempre.

use crate::context::*;
use crate::fill::{FillRequest, FillSeed, compute_regions, fill_command};
use aiai_core::*;
use aiai_paint::fill::FillParams;
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColorizeHintsRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada de cor (preenchimento) que recebe as regiões; as linhas vêm do `line_refs` dela.
    pub fill_layer: Id,
    /// Camada raster com os rabiscos de cor.
    pub hint_layer: Id,
    #[serde(default = "gap")]
    pub gap: f32,
    /// Regiões sem rabisco herdam a cor da vizinha colorida com mais borda em comum (padrão sim).
    #[serde(default = "yes")]
    pub spread: bool,
    /// Pixels de rabisco mínimos para uma região contar como dica (padrão 3).
    #[serde(default = "three")]
    pub min_pixels: usize,
}

fn gap() -> f32 {
    3.0
}
fn yes() -> bool {
    true
}
fn three() -> usize {
    3
}

/// As sementes que o balde vai usar e quantas regiões vieram de rabisco e quantas da vizinha.
pub struct HintPlan {
    pub seeds: Vec<FillSeed>,
    pub hinted: usize,
    pub spread: usize,
}

pub fn colorize_hints_plan(p: &Project, blobs: &BlobStore, req: &ColorizeHintsRequest) -> ToolResult<HintPlan> {
    let fill = layer_data(p, &req.fill_layer)?;
    if fill.line_refs.is_empty() {
        return Err(ToolError::Invalid("a camada de cor não tem linhas de referência (line_refs)".into()));
    }
    let hint = layer_data(p, &req.hint_layer)?;
    if hint.kind != LayerType::Raster {
        return Err(ToolError::Invalid("as dicas ficam numa camada raster".into()));
    }
    let params = FillParams { gap: req.gap, expand: 2, line_threshold: 0.35 };
    let lr = compute_regions(p, blobs, &req.cut, req.frame, &fill.line_refs, &params)?;
    let r = &lr.regions;
    let (w, h) = (r.width, r.height);
    let opts = RenderOptions { only_layers: Some(vec![req.hint_layer.clone()]), include_guides: true, include_reference: true, background: None, camera: false, layer_motion: false };
    let img = render_frame(p, blobs, &req.cut, req.frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?.image;
    if img.width as usize != w || img.height as usize != h {
        return Err(ToolError::Invalid("as dicas e as linhas têm tamanhos diferentes".into()));
    }
    let n = r.count as usize + 1;
    // por região: cor quantizada (4 bits por canal) -> (pixels, soma da cor, um pixel)
    let mut tally: Vec<HashMap<u16, (usize, [f32; 3], usize)>> = vec![HashMap::new(); n];
    let mut first = vec![usize::MAX; n];
    let mut border = vec![false; n];
    for (i, &l) in r.labels.iter().enumerate() {
        let l = l as usize;
        if l == 0 {
            continue;
        }
        if first[l] == usize::MAX {
            first[l] = i;
        }
        let (x, y) = (i % w, i / w);
        if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
            border[l] = true;
        }
        let px = img.data[i];
        if px[3] < 0.5 {
            continue;
        }
        let c = [px[0] / px[3], px[1] / px[3], px[2] / px[3]];
        let q = |v: f32| ((v.clamp(0.0, 1.0) * 15.0).round() as u16) & 15;
        let key = (q(c[0]) << 8) | (q(c[1]) << 4) | q(c[2]);
        let e = tally[l].entry(key).or_insert((0, [0.0; 3], i));
        e.0 += 1;
        for k in 0..3 {
            e.1[k] += c[k];
        }
    }
    let mut color: Vec<Option<[f32; 3]>> = vec![None; n];
    let mut seed_px = vec![usize::MAX; n];
    let mut hinted = 0;
    for l in 1..n {
        if let Some((_, (cnt, sum, at))) = tally[l].iter().max_by_key(|(_, v)| v.0) {
            if *cnt >= req.min_pixels {
                color[l] = Some([sum[0] / *cnt as f32, sum[1] / *cnt as f32, sum[2] / *cnt as f32]);
                seed_px[l] = *at;
                hinted += 1;
            }
        }
    }
    let mut spread = 0;
    if req.spread {
        // vizinhança: rótulos que se encostam, ou que ficam a até 3 px de um mesmo pixel de linha, dividem essa borda
        let mut shared: HashMap<(u32, u32), usize> = HashMap::new();
        for (i, &l) in r.labels.iter().enumerate() {
            if l != 0 {
                // regiões que avançaram por baixo da linha se encostam direto (vizinho da direita e de baixo)
                let (x, y) = (i % w, i / w);
                for j in [(x + 1 < w).then(|| i + 1), (y + 1 < h).then(|| i + w)].into_iter().flatten() {
                    let m = r.labels[j];
                    if m != 0 && m != l {
                        *shared.entry((l.min(m), l.max(m))).or_insert(0) += 1;
                    }
                }
                continue;
            }
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let mut near: Vec<u32> = Vec::new();
            for dy in -3..=3i64 {
                for dx in -3..=3i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let m = r.labels[ny as usize * w + nx as usize];
                    if m != 0 && !near.contains(&m) {
                        near.push(m);
                    }
                }
            }
            for a in 0..near.len() {
                for b in a + 1..near.len() {
                    let k = (near[a].min(near[b]), near[a].max(near[b]));
                    *shared.entry(k).or_insert(0) += 1;
                }
            }
        }
        for _ in 0..8 {
            let mut changed = false;
            let mut best: Vec<Option<(usize, [f32; 3])>> = vec![None; n];
            for (&(a, b), &len) in &shared {
                for (from, to) in [(a as usize, b as usize), (b as usize, a as usize)] {
                    if let (Some(c), None) = (color[from], color[to]) {
                        if border[to] {
                            continue;
                        }
                        if best[to].is_none_or(|(l0, _)| len > l0) {
                            best[to] = Some((len, c));
                        }
                    }
                }
            }
            for (l, b) in best.into_iter().enumerate() {
                if let Some((_, c)) = b {
                    color[l] = Some(c);
                    seed_px[l] = first[l];
                    spread += 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    let seeds = (1..n)
        .filter_map(|l| {
            let c = color[l]?;
            let i = seed_px[l];
            (i != usize::MAX).then(|| FillSeed { x: (i % w) as f32 + 0.5, y: (i / w) as f32 + 0.5, color: ColorRef::Direct { rgba: Rgba([c[0], c[1], c[2], 1.0]) } })
        })
        .collect();
    Ok(HintPlan { seeds, hinted, spread })
}

/// O balde com as cores das dicas, num passo de desfazer, e o resumo (regiões por rabisco e por vizinhança).
pub fn colorize_hints_command(p: &Project, blobs: &mut BlobStore, req: &ColorizeHintsRequest) -> ToolResult<(Command, Value)> {
    let plan = colorize_hints_plan(p, blobs, req)?;
    if plan.seeds.is_empty() {
        return Err(ToolError::Invalid("nenhuma região com rabisco de dica (pinte as cores por cima das regiões na camada de dicas)".into()));
    }
    let fr = FillRequest { cut: req.cut.clone(), frame: req.frame, layer: req.fill_layer.clone(), seeds: plan.seeds, line_layers: vec![], gap: req.gap, expand: 2, line_threshold: 0.35, close_leftovers: None, skip_seeds_on_line: true, only_unpainted: false };
    let cmd = fill_command(p, blobs, &fr)?;
    Ok((cmd, json!({ "regions_from_hints": plan.hinted, "regions_from_neighbors": plan.spread })))
}

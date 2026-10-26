//! Balde de tinta-e-pinta e lint de regiões não pintadas.

use crate::context::*;
use aiai_core::*;
use aiai_paint::fill::{FillParams, Regions, boundary_from_alpha, label_regions};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillSeed {
    pub x: f32,
    pub y: f32,
    pub color: ColorRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillRequest {
    pub cut: Id,
    pub frame: u32,
    /// Camada de preenchimento (tipo `fill`) cujo cel exposto recebe as regiões.
    pub layer: Id,
    pub seeds: Vec<FillSeed>,
    /// Camadas de linha que delimitam as regiões; vazio = `line_refs` da camada de preenchimento.
    #[serde(default)]
    pub line_layers: Vec<Id>,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_expand")]
    pub expand: u32,
    #[serde(default = "default_threshold")]
    pub line_threshold: f32,
    /// Fecha as frestas: depois das sementes, cada região fechada SEM tinta de até este número de pixels (longe da
    /// borda do quadro) recebe a cor que mais aparece em volta dela, atravessando a linha (até 8 px) — o "preencher
    /// área não pintada" dos programas de pintura de cel. Só para frestas (cantos de olho, entre dedos); regiões
    /// maiores continuam no lint e pedem semente com a cor certa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_leftovers: Option<u32>,
    /// Semente que cai em cima da linha é ignorada (em vez de pintar a região mais perto, que pode ser a vizinha
    /// errada — num giro de cabeça a semente da pele entre mechas cai na linha e pintava o cabelo de pele). A região
    /// sem tinta aparece no lint.
    #[serde(default)]
    pub skip_seeds_on_line: bool,
    /// Só pinta regiões ainda sem tinta: semente numa região já pintada é ignorada (sementes "de reserva", que só
    /// valem quando a região existe separada — ex.: a pele entre duas mechas que a sobrancelha fecha).
    #[serde(default)]
    pub only_unpainted: bool,
}

fn default_gap() -> f32 {
    3.0
}
fn default_expand() -> u32 {
    2
}
fn default_threshold() -> f32 {
    0.35
}

/// Contexto calculado uma vez: as regiões delimitadas pelas linhas no quadro.
pub struct LineRegions {
    pub regions: Regions,
    pub width: u32,
    pub height: u32,
    /// Pixels de linha (antes da expansão por baixo da linha).
    pub boundary: Vec<bool>,
}

/// Assinatura do que as regiões dependem: projeto, corte (timesheet), quadro, parâmetros e o conteúdo das camadas de
/// linha (maior revisão na subárvore + número de traços/tiles e ids dos traços do cel exposto).
fn regions_key(p: &Project, cut_id: &Id, frame: u32, line_layers: &[Id], params: &FillParams) -> String {
    use std::fmt::Write;
    let cut_rev = p.node(cut_id).map(|n| n.provenance.revision).unwrap_or(0);
    let mut k = format!("{}|{}|{}|{}|{:?}|{}|{}|{}", p.name, cut_id, cut_rev, frame, (p.settings.width, p.settings.height), params.gap, params.expand, params.line_threshold);
    for l in line_layers {
        let mut rev = p.node(l).map(|n| n.provenance.revision).unwrap_or(0);
        let (mut strokes, mut tiles, mut ids) = (0usize, 0usize, 0u64);
        for d in p.subtree(l) {
            if let Some(n) = p.node(&d) {
                rev = rev.max(n.provenance.revision);
                if let NodeKind::Cel(c) = &n.data {
                    strokes += c.strokes.len();
                    tiles += c.tiles.len();
                    for s in &c.strokes {
                        ids = ids.rotate_left(5) ^ s.id.0.bytes().fold(0u64, |a, b| a.wrapping_mul(131).wrapping_add(b as u64));
                    }
                }
            }
        }
        let _ = write!(k, "|{l}:{rev}:{strokes}:{tiles}:{ids:x}");
    }
    k
}

type RegionsCache = std::sync::Mutex<Vec<(String, std::sync::Arc<LineRegions>)>>;
static REGIONS_CACHE: std::sync::OnceLock<RegionsCache> = std::sync::OnceLock::new();

/// Regiões delimitadas pelas linhas no quadro. As duas últimas ficam guardadas (balde e lint usam parâmetros
/// diferentes): clicar várias vezes com o balde sem mexer na linha não refaz o render das linhas nem a distância.
pub fn compute_regions(
    p: &Project,
    blobs: &BlobStore,
    cut_id: &Id,
    frame: u32,
    line_layers: &[Id],
    params: &FillParams,
) -> ToolResult<std::sync::Arc<LineRegions>> {
    if line_layers.is_empty() {
        return Err(ToolError::Invalid("nenhuma camada de linha para delimitar o balde (defina line_refs ou line_layers)".into()));
    }
    let key = regions_key(p, cut_id, frame, line_layers, params);
    let cache = REGIONS_CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return Ok(hit);
    }
    let lr = std::sync::Arc::new(compute_regions_uncached(p, blobs, cut_id, frame, line_layers, params)?);
    if let Ok(mut c) = cache.lock() {
        c.retain(|(k, _)| *k != key);
        c.insert(0, (key, lr.clone()));
        c.truncate(2);
    }
    Ok(lr)
}

fn compute_regions_uncached(
    p: &Project,
    blobs: &BlobStore,
    cut_id: &Id,
    frame: u32,
    line_layers: &[Id],
    params: &FillParams,
) -> ToolResult<LineRegions> {
    let opts = RenderOptions { only_layers: Some(line_layers.to_vec()), include_guides: false, include_reference: false, background: None, camera: false, layer_motion: false };
    // Iro-Tresse (auditoria 4.7d): as linhas de cor delimitam o balde mesmo quando a camada as esconde no render, e as
    // regiões avançam por baixo delas até o meio: escondida a linha, as duas cores se encontram sem fresta
    let cut = cut_data(p, cut_id)?;
    let mut trace_half = 0.0f32;
    let mut hidden = false;
    for l in line_layers {
        let Some(cel) = exposed_cel(p, cut, l, frame) else { continue };
        if let NodeKind::Cel(c) = &cel.data {
            for st in c.strokes.iter().filter(|st| st.color_trace) {
                trace_half = trace_half.max(st.width * 0.5);
                hidden |= layer_data(p, l).is_ok_and(|ld| ld.params.get("hide_color_trace").and_then(|v| v.as_bool()).unwrap_or(false));
            }
        }
    }
    let shown;
    let src = if hidden {
        let mut q = p.clone();
        for l in line_layers {
            if let Some(NodeKind::Layer(ld)) = q.node_mut(l).map(|n| &mut n.data) {
                if let Some(m) = ld.params.as_object_mut() {
                    m.remove("hide_color_trace");
                }
            }
        }
        shown = q;
        &shown
    } else {
        p
    };
    let out = render_frame(src, blobs, cut_id, frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?;
    let (w, h) = (out.image.width, out.image.height);
    let boundary = boundary_from_alpha(&out.image, params.line_threshold);
    let mut params = *params;
    if trace_half > 0.0 {
        params.expand = params.expand.max((trace_half + 1.0).ceil() as u32);
    }
    let regions = label_regions(&boundary, w as usize, h as usize, &params);
    Ok(LineRegions { regions, width: w, height: h, boundary })
}

/// Gera o comando que pinta as regiões das sementes. Regiões já pintadas que contêm a semente são substituídas.
pub fn fill_command(p: &Project, blobs: &mut BlobStore, req: &FillRequest) -> ToolResult<Command> {
    fill_command_paint(p, blobs, req, None)
}

/// `fill_command` com um degradê nas regiões novas das sementes (mapeado na caixa de cada região).
pub fn fill_command_paint(p: &Project, blobs: &mut BlobStore, req: &FillRequest, gradient: Option<&Gradient>) -> ToolResult<Command> {
    if let Some(g) = gradient {
        g.validate().map_err(ToolError::Invalid)?;
    }
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.kind != LayerType::Fill && layer.kind != LayerType::Raster && layer.kind != LayerType::Vector {
        return Err(ToolError::Invalid("o balde pinta em camada de preenchimento (fill), raster ou vetorial".into()));
    }
    let cel_node = exposed_cel(p, cut, &req.layer, req.frame)
        .ok_or_else(|| ToolError::Invalid(format!("a camada não expõe nenhum cel no quadro {} (crie o cel e/ou a célula do timesheet)", req.frame)))?;
    let NodeKind::Cel(cel) = &cel_node.data else { unreachable!() };
    let lines = if req.line_layers.is_empty() { layer.line_refs.clone() } else { req.line_layers.clone() };
    let params = FillParams { gap: req.gap, expand: req.expand, line_threshold: req.line_threshold };
    let lr = compute_regions(p, blobs, &req.cut, req.frame, &lines, &params)?;
    let mut fills = cel.fills.clone();
    for s in &req.seeds {
        if !(s.x >= 0.0 && s.y >= 0.0 && (s.x as u32) < lr.width && (s.y as u32) < lr.height) {
            return Err(ToolError::Invalid(format!("semente ({}, {}) fora do canvas {}x{}", s.x, s.y, lr.width, lr.height)));
        }
        let (sx, sy) = (s.x as u32, s.y as u32);
        if req.skip_seeds_on_line && lr.regions.label_at(sx, sy) == 0 {
            continue;
        }
        let label = lr.regions.label_near(sx, sy);
        if label == 0 {
            return Err(ToolError::Invalid(format!("semente ({}, {}) caiu sobre a linha, sem região por perto", s.x, s.y)));
        }
        if req.only_unpainted && fills.iter().any(|f| fill_covers(blobs, f, sx, sy)) {
            continue;
        }
        let (bounds, mask) = lr.regions.mask_of(label).expect("rótulo existe");
        // remove preenchimentos anteriores que cobrem a semente
        fills.retain(|f| !fill_covers(blobs, f, sx, sy));
        let hash = blobs.put(mask.encode());
        fills.push(FillRegion { id: Id(String::new()), seed: [s.x, s.y], color: s.color.clone(), mask: Some(hash), bounds, gradient: gradient.cloned() });
    }
    if let Some(max_area) = req.close_leftovers {
        let extra = leftover_fills(blobs, &lr, &fills, max_area as usize);
        fills.extend(extra);
    }
    Ok(Command::ReplaceFills { cel: cel_node.id.clone(), fills })
}

/// Preenchimentos para as frestas (regiões fechadas, sem tinta, até `max_area` px, sem tocar a borda): a cor é a
/// que mais cobre os pixels em volta da fresta (dilatação de 8 px que atravessa a linha).
fn leftover_fills(blobs: &mut BlobStore, lr: &LineRegions, fills: &[FillRegion], max_area: usize) -> Vec<FillRegion> {
    let (w, h) = (lr.width as usize, lr.height as usize);
    let labels = &lr.regions.labels;
    // dono de cada pixel (índice do preenchimento que o cobre; o último vence, como no render)
    let mut owner = vec![-1i32; w * h];
    for (k, f) in fills.iter().enumerate() {
        let [bx, by, bw, bh] = f.bounds;
        let Some(m) = f.mask.as_ref().and_then(|hh| blobs.get(hh)).and_then(|b| aiai_paint::Mask::decode(&b).ok()) else { continue };
        for y in by.max(0)..(by + bh).min(h as i32) {
            for x in bx.max(0)..(bx + bw).min(w as i32) {
                if m.get((x - bx) as u32, (y - by) as u32) > 0.5 {
                    owner[y as usize * w + x as usize] = k as i32;
                }
            }
        }
    }
    let border = lr.regions.touches_border();
    let n = lr.regions.count as usize + 1;
    let mut covered = vec![0usize; n];
    let mut pixels: Vec<Vec<usize>> = vec![Vec::new(); n];
    // tamanho e cobertura contam só o miolo (sem a faixa que a expansão pôs por baixo da linha), como o lint
    let mut inner = vec![0usize; n];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 && !lr.boundary[i] {
            inner[l as usize] += 1;
        }
    }
    for (i, &l) in labels.iter().enumerate() {
        let l = l as usize;
        if l == 0 || lr.boundary[i] {
            continue;
        }
        if owner[i] >= 0 {
            covered[l] += 1;
        }
        if inner[l] <= max_area && !border[l] {
            pixels[l].push(i);
        }
    }
    let mut out = Vec::new();
    for l in 1..n {
        if pixels[l].is_empty() || covered[l] * 2 >= inner[l] {
            continue;
        }
        // dilatação em anéis a partir da fresta; conta os donos achados fora dela
        let mut seen: std::collections::HashSet<usize> = pixels[l].iter().copied().collect();
        let mut ring = pixels[l].clone();
        let mut votes: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
        for _ in 0..8 {
            let mut next = Vec::new();
            for &i in &ring {
                let (x, y) = (i % w, i / w);
                let neigh = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)];
                for j in neigh.into_iter().flatten() {
                    if seen.insert(j) {
                        if owner[j] >= 0 && labels[j] as usize != l {
                            *votes.entry(owner[j]).or_default() += 1;
                        }
                        next.push(j);
                    }
                }
            }
            ring = next;
        }
        // cor mais votada (agrupando preenchimentos da mesma cor); empate: a do preenchimento mais antigo
        let mut by_color: Vec<(String, usize, i32)> = Vec::new();
        for (&k, &v) in &votes {
            let key = serde_json::to_string(&fills[k as usize].color).unwrap_or_default();
            match by_color.iter_mut().find(|c| c.0 == key) {
                Some(c) => {
                    c.1 += v;
                    c.2 = c.2.min(k);
                }
                None => by_color.push((key, v, k)),
            }
        }
        let Some(best) = by_color.iter().max_by_key(|c| (c.1, std::cmp::Reverse(c.2))) else { continue };
        let color = fills[best.2 as usize].color.clone();
        let Some((bounds, mask)) = lr.regions.mask_of(l as u32) else { continue };
        let i = pixels[l][pixels[l].len() / 2];
        let hash = blobs.put(mask.encode());
        out.push(FillRegion { id: Id(String::new()), seed: [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5], color, mask: Some(hash), bounds, gradient: None });
    }
    out
}

pub(crate) fn fill_covers(blobs: &BlobStore, f: &FillRegion, x: u32, y: u32) -> bool {
    let [bx, by, bw, bh] = f.bounds;
    let (xi, yi) = (x as i32, y as i32);
    if xi < bx || yi < by || xi >= bx + bw || yi >= by + bh {
        return false;
    }
    let Some(h) = &f.mask else { return false };
    let Some(b) = blobs.get(h) else { return false };
    match aiai_paint::Mask::decode(&b) {
        Ok(m) => m.get((xi - bx) as u32, (yi - by) as u32) > 0.5,
        Err(_) => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LintRegion {
    pub label: u32,
    pub area: usize,
    /// Um ponto dentro da região (para usar como semente).
    pub point: [u32; 2],
    pub bounds: [i32; 4],
    /// Fração dos pixels da região coberta por preenchimentos (0 = vazia; parcial quando < 0,95).
    #[serde(default)]
    pub coverage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LintReport {
    pub regions_total: u32,
    pub unpainted: Vec<LintRegion>,
    pub painted: usize,
    /// Sem `include_border`: regiões que tocam a borda do quadro e não estão pintadas, tirando a maior delas (o
    /// "fora" do desenho). Num close a manga ou o cabelo cortados pelo quadro aparecem aqui em vez de sumirem.
    #[serde(default)]
    pub border_unpainted: Vec<LintRegion>,
}

/// Regiões fechadas (que não tocam a borda e têm área ≥ `min_area`) sem preenchimento no cel exposto.
pub fn lint(p: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, fill_layer: &Id, gap: f32, min_area: usize) -> ToolResult<LintReport> {
    lint_with(p, blobs, cut_id, frame, fill_layer, gap, min_area, false)
}

/// Como [`lint`]; com `include_border` também lista as regiões que tocam a borda do quadro (cabelo cortado pelo
/// enquadramento etc.). O ponto de cada região é o seu pixel mais interno (maior distância às bordas da região).
#[allow(clippy::too_many_arguments)]
pub fn lint_with(
    p: &Project,
    blobs: &BlobStore,
    cut_id: &Id,
    frame: u32,
    fill_layer: &Id,
    gap: f32,
    min_area: usize,
    include_border: bool,
) -> ToolResult<LintReport> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, fill_layer)?;
    let params = FillParams { gap, expand: 0, line_threshold: 0.35 };
    let lr = compute_regions(p, blobs, cut_id, frame, &layer.line_refs, &params)?;
    let fills: Vec<FillRegion> = match exposed_cel(p, cut, fill_layer, frame).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => c.fills.clone(),
        _ => Vec::new(),
    };
    let areas = lr.regions.areas();
    let border = lr.regions.touches_border();
    let mut unpainted = Vec::new();
    let mut painted = 0;
    // um ponto representativo por rótulo: o pixel mais interno (distância de chanfro 3-4 até outro rótulo/linha)
    let (w, h) = (lr.width as usize, lr.height as usize);
    let labels = &lr.regions.labels;
    let mut dist = vec![u32::MAX / 2; w * h];
    let same = |i: usize, j: usize| labels[i] == labels[j];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if labels[i] == 0 {
                dist[i] = 0;
                continue;
            }
            let mut d = dist[i];
            for (dx, dy, c) in [(-1i32, 0i32, 3u32), (0, -1, 3), (-1, -1, 4), (1, -1, 4)] {
                let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                let v = if xx < 0 || yy < 0 || xx >= w as i32 { 0 } else {
                    let j = yy as usize * w + xx as usize;
                    if same(i, j) { dist[j] } else { 0 }
                };
                d = d.min(v + c);
            }
            dist[i] = d;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if labels[i] == 0 {
                continue;
            }
            let mut d = dist[i];
            for (dx, dy, c) in [(1i32, 0i32, 3u32), (0, 1, 3), (1, 1, 4), (-1, 1, 4)] {
                let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                let v = if xx < 0 || xx >= w as i32 || yy >= h as i32 { 0 } else {
                    let j = yy as usize * w + xx as usize;
                    if same(i, j) { dist[j] } else { 0 }
                };
                d = d.min(v + c);
            }
            dist[i] = d;
        }
    }
    let mut first: Vec<Option<(u32, u32)>> = vec![None; lr.regions.count as usize + 1];
    let mut best = vec![0u32; lr.regions.count as usize + 1];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 && (first[l as usize].is_none() || dist[i] > best[l as usize]) {
            first[l as usize] = Some(((i % w) as u32, (i / w) as u32));
            best[l as usize] = dist[i];
        }
    }
    // cobertura por pixel (cada máscara decodificada uma vez): a região conta como pintada com ≥ 95% coberta,
    // não por um ponto só (uma máscara velha que pegasse o ponto representativo escondia a região inteira)
    let mut cov = vec![false; w * h];
    for f in &fills {
        let [bx, by, bw, bh] = f.bounds;
        let Some(m) = f.mask.as_ref().and_then(|hh| blobs.get(hh)).and_then(|b| aiai_paint::Mask::decode(&b).ok()) else { continue };
        for yy in by.max(0)..(by + bh).min(h as i32) {
            for xx in bx.max(0)..(bx + bw).min(w as i32) {
                if m.get((xx - bx) as u32, (yy - by) as u32) > 0.5 {
                    cov[yy as usize * w + xx as usize] = true;
                }
            }
        }
    }
    let mut covered = vec![0usize; lr.regions.count as usize + 1];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 && cov[i] {
            covered[l as usize] += 1;
        }
    }
    let outside = (1..=lr.regions.count).filter(|&l| border[l as usize]).max_by_key(|&l| (areas[l as usize], std::cmp::Reverse(l)));
    let mut border_unpainted = Vec::new();
    for label in 1..=lr.regions.count {
        if areas[label as usize] < min_area {
            continue;
        }
        let Some((x, y)) = first[label as usize] else { continue };
        let frac = covered[label as usize] as f32 / areas[label as usize].max(1) as f32;
        if frac >= 0.95 {
            painted += 1;
            continue;
        }
        let (bounds, _) = lr.regions.mask_of(label).expect("existe");
        let r = LintRegion { label, area: areas[label as usize], point: [x, y], bounds, coverage: (frac * 1000.0).round() / 1000.0 };
        if border[label as usize] && !include_border {
            if Some(label) != outside {
                border_unpainted.push(r);
            }
        } else {
            unpainted.push(r);
        }
    }
    Ok(LintReport { regions_total: lr.regions.count, unpainted, painted, border_unpainted })
}

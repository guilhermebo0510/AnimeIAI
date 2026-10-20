//! Mesclar camadas (Ctrl+E, como no Photoshop): junta camadas de desenho numa só, num passo único de undo.
//!
//! O resultado fica na camada de BAIXO (nome, posição na pilha, opacidade, modo, travas e máscara dela); as de cima saem.
//!
//! Operações (`op`): `down` (a camada `layer` vai para a irmã logo abaixo), `selected` (as `layers` viram uma só, na mais
//! baixa) e `visible` (as camadas raster/vetor visíveis do corte, ou da pasta `parent`, na mais baixa).
//!
//! Regras de tipo:
//! - vetor + vetor: os traços das de cima entram no cel de baixo (geometria intacta, ids novos), depois dos de baixo; a
//!   opacidade da camada de cima vira opacidade de traço (`opacidade_cima / opacidade_baixo`) e o modo de mistura tem de
//!   ser o mesmo das duas;
//! - raster + raster: composição por tile, com a opacidade e o modo de mistura da camada de cima, dentro do cel de baixo
//!   (também o que passa da borda do quadro); a opacidade da camada de baixo continua valendo depois da mesclagem;
//! - vetor + raster (em qualquer ordem): os traços são rasterizados e a camada resultante vira raster;
//! - outros tipos (pasta, preenchimento, texto, guia, referência, ajuste, efeito, partículas, 3D): recusa com mensagem.
//!
//! Tempo (o que é composto com o quê): a mesclagem vale para o corte inteiro. Cada trecho de quadros em que a combinação
//! de cels expostos (um por camada) é a mesma vira UM cel na camada de baixo: se o cel de baixo só aparece naquela
//! combinação, ele mesmo recebe o resultado (continua com o número e o papel); senão nasce um cel novo (maior número + 1)
//! e a trilha da camada de baixo passa a expô-lo nesses quadros (a trilha é criada se a camada não tinha). Camadas com
//! a mesma exposição ficam com os mesmos cels; camadas com exposições diferentes geram um cel por combinação.
//!
//! Recusas (nada é alterado): camadas ocultas ou travadas, de pais diferentes, com cel transformado, com posição/escala/
//! rotação/âncora/profundidade/chaves diferentes das da de baixo, com máscara de recorte ou usadas como trilha de boca ou
//! olhos, e camadas visíveis de outro tipo entre as escolhidas (a ordem de empilhamento mudaria). Máscaras e fronteiras de
//! balde (`line_refs`) de outras camadas que apontavam para uma das que saem passam a apontar para a de baixo.

use crate::context::*;
use aiai_core::command::CellEdit;
use aiai_core::xsheet::{CellRef, Track, TrackKind};
use aiai_core::*;
use aiai_paint::blend::blend_pixel;
use aiai_paint::{Image, tile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MergeOp {
    /// `layer` (ou a única de `layers`) mescla na camada irmã logo abaixo.
    Down,
    /// As `layers` (2 ou mais, no mesmo pai) viram uma só, na mais baixa delas.
    Selected,
    /// Todas as camadas raster/vetor visíveis do `cut` (ou da pasta `parent`) viram uma só, na mais baixa.
    Visible,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LayerMergeRequest {
    /// `down` | `selected` | `visible`. Omitido: `visible` se só há `cut`; `selected` com 2 ou mais `layers`; senão `down`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<MergeOp>,
    /// Camada de cima da mesclagem para baixo (`down`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Id>,
    /// Camadas a mesclar (`selected`; com uma só vale como `down`), em qualquer ordem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Id>,
    /// Corte (`visible`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Id>,
    /// Pasta cujas camadas visíveis mesclam (`visible`); omitido = as camadas do próprio corte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
}

/// O plano pronto para aplicar (um `Batch`) e o que ele faz.
#[derive(Debug, Clone)]
pub struct MergePlan {
    pub command: Command,
    /// A camada que fica (a de baixo).
    pub layer: Id,
    /// As camadas que saem (de baixo para cima).
    pub merged: Vec<Id>,
    /// Tipo da camada resultante.
    pub kind: LayerType,
    /// Cels que receberam o resultado (já existiam) e cels novos.
    pub cels_changed: usize,
    pub cels_created: usize,
    /// Texto para o aviso da interface.
    pub label: String,
}

fn invalid<T>(msg: impl Into<String>) -> ToolResult<T> {
    Err(ToolError::Invalid(msg.into()))
}

fn name_of(p: &Project, id: &Id) -> String {
    p.node(id).map(|n| n.name.clone()).unwrap_or_else(|| id.to_string())
}

fn kind_label(k: LayerType) -> &'static str {
    match k {
        LayerType::Group => "pasta",
        LayerType::Vector => "vetor",
        LayerType::Raster => "raster",
        LayerType::Fill => "preenchimento",
        LayerType::Text => "texto",
        LayerType::Guide => "guia",
        LayerType::Reference => "referência",
        LayerType::Adjustment => "ajuste",
        LayerType::Effect => "efeito",
        LayerType::Particles => "partículas",
        LayerType::Model3d => "3D",
    }
}

fn mergeable(k: LayerType) -> bool {
    matches!(k, LayerType::Raster | LayerType::Vector)
}

/// O corte (ancestral) de uma camada.
fn cut_of(p: &Project, layer: &Id) -> Option<Id> {
    let mut cur = p.node(layer)?.parent.clone();
    while let Some(id) = cur {
        let n = p.node(&id)?;
        if matches!(n.data, NodeKind::Cut(_)) {
            return Some(id);
        }
        cur = n.parent.clone();
    }
    None
}

/// Camadas (ids) filhas diretas de `parent`, de baixo para cima (a ordem do documento).
fn layer_children(p: &Project, parent: &Id) -> Vec<Id> {
    p.node(parent).map(|n| n.children.iter().filter(|c| matches!(p.node(c).map(|x| &x.data), Some(NodeKind::Layer(_)))).cloned().collect()).unwrap_or_default()
}

/// Escolhe as camadas a mesclar, de baixo para cima (a primeira é a que fica).
fn pick_layers(p: &Project, req: &LayerMergeRequest) -> ToolResult<Vec<Id>> {
    let op = req.op.unwrap_or(if req.layer.is_none() && req.layers.is_empty() && req.cut.is_some() {
        MergeOp::Visible
    } else if req.layers.len() >= 2 {
        MergeOp::Selected
    } else {
        MergeOp::Down
    });
    match op {
        MergeOp::Down => {
            let top = match (&req.layer, req.layers.as_slice()) {
                (Some(l), []) => l.clone(),
                (None, [l]) => l.clone(),
                (Some(l), [m]) if l == m => l.clone(),
                (None, []) => return invalid("mesclar para baixo exige `layer` (a camada de cima)"),
                _ => return invalid("mesclar para baixo é uma camada só; para várias use op `selected` com `layers`"),
            };
            layer_data(p, &top)?;
            let parent = p.node(&top).and_then(|n| n.parent.clone()).ok_or_else(|| ToolError::Invalid("camada sem pai".into()))?;
            let sibs = layer_children(p, &parent);
            let i = sibs.iter().position(|c| c == &top).ok_or_else(|| ToolError::Invalid("camada fora da lista do pai".into()))?;
            if i == 0 {
                return invalid(format!("não há camada abaixo de \"{}\" para mesclar", name_of(p, &top)));
            }
            Ok(vec![sibs[i - 1].clone(), top])
        }
        MergeOp::Selected => {
            let mut ids: Vec<Id> = req.layers.clone();
            if let Some(l) = &req.layer {
                if !ids.contains(l) {
                    ids.push(l.clone());
                }
            }
            ids.sort();
            ids.dedup();
            if ids.len() < 2 {
                return invalid("mesclar selecionadas exige duas ou mais camadas em `layers`");
            }
            let mut parent: Option<Id> = None;
            for id in &ids {
                layer_data(p, id)?;
                let par = p.node(id).and_then(|n| n.parent.clone());
                match &parent {
                    None => parent = par,
                    Some(q) if Some(q) != par.as_ref() => return invalid("as camadas escolhidas estão em pastas diferentes; mova-as para o mesmo lugar antes de mesclar"),
                    _ => {}
                }
            }
            let parent = parent.ok_or_else(|| ToolError::Invalid("camada sem pai".into()))?;
            Ok(layer_children(p, &parent).into_iter().filter(|c| ids.contains(c)).collect())
        }
        MergeOp::Visible => {
            let parent = match (&req.parent, &req.cut) {
                (Some(g), _) => g.clone(),
                (None, Some(c)) => {
                    cut_data(p, c)?;
                    c.clone()
                }
                (None, None) => return invalid("mesclar visíveis exige `cut` (ou `parent`, uma pasta)"),
            };
            if let Some(g) = &req.parent {
                // o próprio corte ou uma pasta
                if cut_data(p, g).is_err() && !layer_data(p, g).is_ok_and(|l| l.kind == LayerType::Group) {
                    return invalid("`parent` tem de ser o corte ou uma pasta");
                }
            }
            let ids: Vec<Id> = layer_children(p, &parent)
                .into_iter()
                .filter(|c| layer_data(p, c).is_ok_and(|l| l.visible && mergeable(l.kind)))
                .collect();
            if ids.len() < 2 {
                return invalid("há menos de duas camadas raster/vetor visíveis para mesclar");
            }
            Ok(ids)
        }
    }
}

/// Verifica o que a mesclagem não consegue fazer sem mudar o desenho; devolve o corte.
fn check_layers(p: &Project, chosen: &[Id]) -> ToolResult<Id> {
    let bottom = &chosen[0];
    let b = layer_data(p, bottom)?;
    let cut_id = cut_of(p, bottom).ok_or_else(|| ToolError::Invalid("a camada não está num corte".into()))?;
    for id in chosen {
        let l = layer_data(p, id)?;
        let name = name_of(p, id);
        if !mergeable(l.kind) {
            return invalid(format!("a camada \"{name}\" é de {} e só camadas raster e vetor mesclam", kind_label(l.kind)));
        }
        if !l.visible {
            return invalid(format!("a camada \"{name}\" está oculta; mostre-a antes de mesclar"));
        }
        if l.locked {
            return invalid(format!("a camada \"{name}\" está travada"));
        }
        if cut_of(p, id).as_ref() != Some(&cut_id) {
            return invalid("as camadas são de cortes diferentes");
        }
        if id != bottom {
            if l.clip_to_below {
                return invalid(format!("a camada \"{name}\" é um recorte da camada abaixo; solte o recorte antes de mesclar"));
            }
            if l.mask.is_some() {
                return invalid(format!("a camada \"{name}\" tem máscara; remova a máscara antes de mesclar"));
            }
            if (l.offset, l.scale, l.rotation, l.anchor, l.depth) != (b.offset, b.scale, b.rotation, b.anchor, b.depth) || (!l.keys.is_empty() && l.keys != b.keys) {
                return invalid(format!("a camada \"{name}\" tem posição, escala, rotação, profundidade ou chaves diferentes das de \"{}\"; iguale-as antes de mesclar", name_of(p, bottom)));
            }
        }
        // cels com transformação própria (imagem posicionada) não entram na soma de pixels do canvas
        for c in &p.node(id).map(|n| n.children.clone()).unwrap_or_default() {
            if let Some(NodeKind::Cel(cd)) = p.node(c).map(|n| &n.data) {
                if cd.transform != [0.0, 0.0, 1.0, 0.0] {
                    return invalid(format!("o cel {} de \"{name}\" tem transformação própria; aplique-a antes de mesclar", cd.number));
                }
            }
        }
        // trilhas de boca e olhos (sincronia labial) somem junto com a camada
        if id != bottom {
            if let Some(t) = cut_data(p, &cut_id)?.xsheet.track_for_layer(id) {
                if matches!(t.kind, TrackKind::Mouth | TrackKind::Eyes) {
                    return invalid(format!("a camada \"{name}\" é a trilha de {} da sincronia labial; não dá para mesclá-la", if t.kind == TrackKind::Mouth { "boca" } else { "olhos" }));
                }
            }
        }
    }
    // outra camada visível entre as escolhidas mudaria de lugar em relação ao resultado
    let parent = p.node(bottom).and_then(|n| n.parent.clone()).ok_or_else(|| ToolError::Invalid("camada sem pai".into()))?;
    let sibs = layer_children(p, &parent);
    let pos = |id: &Id| sibs.iter().position(|c| c == id).unwrap_or(usize::MAX);
    let (lo, hi) = (pos(bottom), chosen.iter().map(pos).max().unwrap_or(0));
    for id in &sibs[lo.min(sibs.len())..=hi.min(sibs.len().saturating_sub(1))] {
        if chosen.contains(id) {
            continue;
        }
        if let Ok(l) = layer_data(p, id) {
            if l.visible && !l.kind.is_non_rendering() {
                return invalid(format!(
                    "a camada visível \"{}\" ({}) está entre as que seriam mescladas e mudaria de ordem; oculte-a, mova-a ou escolha só camadas vizinhas",
                    name_of(p, id),
                    kind_label(l.kind)
                ));
            }
        }
    }
    Ok(cut_id)
}

type Tiles = BTreeMap<(i32, i32), Vec<[f32; 4]>>;

fn zero_tile() -> Vec<[f32; 4]> {
    vec![[0.0; 4]; tile::TILE_PIXELS]
}

/// O conteúdo de um cel como tiles (y, x) → pixels pré-multiplicados: os tiles do cel e, por cima, imagem, preenchimentos e
/// traços rasterizados no quadro (a ordem do render: imagem, tiles, preenchimentos, traços).
fn cel_tiles(p: &Project, blobs: &BlobStore, layer: &LayerData, layer_name: &str, cel: &CelData, canvas: (u32, u32)) -> ToolResult<Tiles> {
    let mut out = Tiles::new();
    for t in &cel.tiles {
        let b = blobs.get(&t.blob).ok_or_else(|| ToolError::NotFound(format!("blob do tile ({}, {}) de \"{layer_name}\"", t.x, t.y)))?;
        out.insert((t.y, t.x), tile::decode_tile(&b).map_err(ToolError::Invalid)?);
    }
    if cel.image.is_some() || !cel.fills.is_empty() || !cel.strokes.is_empty() {
        let mut extras = cel.clone();
        extras.tiles.clear();
        let hide = layer.params.get("hide_color_trace").and_then(|v| v.as_bool()).unwrap_or(false);
        let (img, warnings) = aiai_render::compose::render_cel_image(p, blobs, &extras, hide, canvas, layer_name);
        if let Some(w) = warnings.first() {
            return invalid(format!("não deu para rasterizar \"{layer_name}\": {w}"));
        }
        for (tx, ty, px) in tile::image_to_tiles(&img, [0, 0]) {
            let Some(px) = px else { continue };
            let dst = out.entry((ty, tx)).or_insert_with(zero_tile);
            for (d, s) in dst.iter_mut().zip(px) {
                Image::over(d, s, 1.0);
            }
        }
    }
    Ok(out)
}

/// `top` sobre `acc` com o modo e a opacidade da camada de cima.
fn composite(acc: &mut Tiles, top: &Tiles, blend: BlendMode, opacity: f32) {
    for (k, tpx) in top {
        let dst = acc.entry(*k).or_insert_with(zero_tile);
        for (d, s) in dst.iter_mut().zip(tpx) {
            if s[3] > 0.0 {
                blend_pixel(d, *s, blend, opacity);
            }
        }
    }
}

type Key = Vec<Option<u32>>;

/// Trecho de quadros [start, end) com a mesma combinação de cels expostos (um número por camada, na ordem de `chosen`).
struct Seg {
    start: u32,
    key: Key,
}

fn segments(p: &Project, cut: &CutData, chosen: &[Id]) -> Vec<Seg> {
    let mut frames: BTreeSet<u32> = BTreeSet::new();
    frames.insert(0);
    for id in chosen {
        if let Some(t) = cut.xsheet.track_for_layer(id) {
            frames.extend(t.cells.keys().copied().filter(|f| *f < cut.duration));
        }
    }
    let mut segs: Vec<Seg> = Vec::new();
    for f in frames {
        let key: Key = chosen
            .iter()
            .map(|l| {
                exposed_cel(p, cut, l, f).and_then(|n| match &n.data {
                    NodeKind::Cel(c) => Some(c.number),
                    _ => None,
                })
            })
            .collect();
        if segs.last().is_none_or(|s| s.key != key) {
            segs.push(Seg { start: f, key });
        }
    }
    segs
}

fn has_tops(key: &Key) -> bool {
    key[1..].iter().any(Option::is_some)
}

/// Monta o comando que mescla as camadas. Os tiles novos entram em `blobs` (idempotente); nada muda no documento até o
/// comando ser aplicado.
pub fn layer_merge_command(p: &Project, blobs: &mut BlobStore, ids: &mut IdGen, req: &LayerMergeRequest) -> ToolResult<MergePlan> {
    let chosen = pick_layers(p, req)?;
    let cut_id = check_layers(p, &chosen)?;
    let cut = cut_data(p, &cut_id)?;
    let canvas = canvas_size(p, cut);
    let bottom_id = chosen[0].clone();
    let layers: Vec<LayerData> = chosen.iter().map(|id| layer_data(p, id).cloned()).collect::<ToolResult<_>>()?;
    let names: Vec<String> = chosen.iter().map(|id| name_of(p, id)).collect();
    let bottom = &layers[0];
    let any_raster = layers.iter().any(|l| l.kind == LayerType::Raster);
    let result_kind = if any_raster { LayerType::Raster } else { LayerType::Vector };

    // vetor + vetor guarda a geometria: a opacidade da camada de cima vira opacidade de traço e o modo tem de ser o mesmo
    let mut stroke_factor = vec![1.0f32; layers.len()];
    if !any_raster {
        for (i, l) in layers.iter().enumerate().skip(1) {
            if l.blend != bottom.blend {
                return invalid(format!("as camadas vetoriais \"{}\" e \"{}\" têm modos de mistura diferentes; iguale-os para manter os traços (ou rasterize uma delas)", names[i], names[0]));
            }
            if (l.opacity - bottom.opacity).abs() > 1e-6 {
                if bottom.opacity <= 1e-4 {
                    return invalid(format!("a camada \"{}\" está com opacidade 0%; não dá para mesclar traços nela", names[0]));
                }
                stroke_factor[i] = l.opacity / bottom.opacity;
            }
        }
    }

    let segs = segments(p, cut, &chosen);
    // combinações distintas, na ordem em que aparecem
    let mut keys: Vec<Key> = Vec::new();
    for s in &segs {
        if !keys.contains(&s.key) {
            keys.push(s.key.clone());
        }
    }
    let nontrivial: Vec<Key> = keys.iter().filter(|k| has_tops(k)).cloned().collect();
    // o cel de baixo recebe o resultado no lugar só se aparece apenas em combinações com algo por cima (senão os quadros
    // sem nada por cima perderiam o desenho original); com várias combinações, a primeira fica nele e as outras ganham cel novo
    let c0_in_place_ok = |c0: Option<u32>| keys.iter().filter(|k| k[0] == c0).all(has_tops);
    let mut c0_taken: BTreeSet<u32> = BTreeSet::new();

    // cels da camada de baixo
    let bottom_cels: BTreeMap<u32, Id> = p
        .node(&bottom_id)
        .map(|n| n.children.iter().filter_map(|c| match p.node(c).map(|x| &x.data) { Some(NodeKind::Cel(cd)) => Some((cd.number, c.clone())), _ => None }).collect())
        .unwrap_or_default();
    let mut next_number = bottom_cels.keys().next_back().copied().unwrap_or(0) + 1;

    // conteúdo raster em cache (camada, cel)
    let mut cache: HashMap<(usize, u32), Arc<Tiles>> = HashMap::new();
    let mut tiles_of = |blobs: &BlobStore, li: usize, number: u32| -> ToolResult<Arc<Tiles>> {
        if let Some(t) = cache.get(&(li, number)) {
            return Ok(t.clone());
        }
        let t = match p.cel_by_number(&chosen[li], number) {
            Some(Node { data: NodeKind::Cel(cd), .. }) => cel_tiles(p, blobs, &layers[li], &names[li], cd, canvas)?,
            _ => Tiles::new(),
        };
        let t = Arc::new(t);
        cache.insert((li, number), t.clone());
        Ok(t)
    };

    let mut cmds: Vec<Command> = Vec::new();
    // combinação → (número do cel que a mostra, id do cel se já existe)
    let mut target: HashMap<Key, (u32, Option<Id>)> = HashMap::new();
    let (mut changed, mut created) = (0usize, 0usize);
    for key in &nontrivial {
        let in_place = key[0].is_some_and(|n| c0_in_place_ok(key[0]) && bottom_cels.contains_key(&n) && c0_taken.insert(n));
        let (number, existing) = if in_place {
            let n = key[0].unwrap_or(0);
            (n, bottom_cels.get(&n).cloned())
        } else {
            let n = next_number;
            next_number += 1;
            (n, None)
        };
        // dados de partida: o cel de baixo (ou um novo, vazio)
        let base = key[0].and_then(|n| match p.cel_by_number(&bottom_id, n) { Some(Node { data: NodeKind::Cel(cd), .. }) => Some(cd.clone()), _ => None });
        let mut cel = base.clone().unwrap_or_else(|| CelData::new(number));
        cel.number = number;
        if result_kind == LayerType::Raster {
            let mut acc = match key[0] {
                Some(n) => (*tiles_of(blobs, 0, n)?).clone(),
                None => Tiles::new(),
            };
            for (li, c) in key.iter().enumerate().skip(1) {
                if let Some(n) = c {
                    let top = tiles_of(blobs, li, *n)?;
                    composite(&mut acc, &top, layers[li].blend, layers[li].opacity);
                }
            }
            cel.strokes.clear();
            cel.fills.clear();
            cel.image = None;
            cel.tiles = acc
                .into_iter()
                .filter(|(_, px)| px.iter().any(|q| q[3] > 0.0))
                .map(|((y, x), px)| TileEntry { x, y, blob: blobs.put(tile::encode_tile(&px)) })
                .collect();
        } else {
            for (li, c) in key.iter().enumerate().skip(1) {
                let Some(n) = c else { continue };
                if let Some(Node { data: NodeKind::Cel(top), .. }) = p.cel_by_number(&chosen[li], *n) {
                    for s in &top.strokes {
                        let mut s = s.clone();
                        s.id = ids.next();
                        s.opacity *= stroke_factor[li];
                        if s.opacity > 1.0 + 1e-4 {
                            return invalid(format!("a camada \"{}\" é mais opaca que \"{}\"; iguale as opacidades para mesclar os traços", names[li], names[0]));
                        }
                        s.opacity = s.opacity.min(1.0);
                        cel.strokes.push(s);
                    }
                }
            }
        }
        match &existing {
            Some(id) => {
                cmds.push(Command::ReplaceData { id: id.clone(), data: NodeKind::Cel(cel) });
                changed += 1;
            }
            None => {
                let id = ids.next();
                cmds.push(Command::CreateNode { id: Some(id), parent: Some(bottom_id.clone()), index: None, name: number.to_string(), data: NodeKind::Cel(cel) });
                created += 1;
            }
        }
        target.insert(key.clone(), (number, existing));
    }

    // a camada de baixo muda de tipo quando o resultado é raster
    if result_kind != bottom.kind {
        let mut nl = bottom.clone();
        nl.kind = result_kind;
        cmds.push(Command::ReplaceData { id: bottom_id.clone(), data: NodeKind::Layer(nl) });
    }

    // timesheet da camada de baixo: só mexe quando nasceu cel novo
    if created > 0 {
        let track = cut.xsheet.track_for_layer(&bottom_id);
        let orig = |f: u32| -> Option<CellRef> {
            match track {
                Some(t) => t.exposed_at(f),
                None => bottom_cels.keys().next().map(|n| CellRef::Cel { number: *n }),
            }
        };
        let mut cells: BTreeMap<u32, CellRef> = BTreeMap::new();
        let mut last: Option<CellRef> = None;
        for s in &segs {
            let v = match target.get(&s.key) {
                Some((n, _)) if has_tops(&s.key) => Some(CellRef::Cel { number: *n }),
                _ => orig(s.start),
            };
            if let Some(v) = v {
                if last != Some(v) {
                    cells.insert(s.start, v);
                }
            }
            last = v;
        }
        if let Some(t) = track {
            cells.extend(t.cells.iter().filter(|(f, _)| **f >= cut.duration).map(|(f, c)| (*f, *c)));
            let frames: BTreeSet<u32> = t.cells.keys().chain(cells.keys()).copied().collect();
            let edits: Vec<CellEdit> = frames.into_iter().filter(|f| t.cells.get(f) != cells.get(f)).map(|f| CellEdit { frame: f, cell: cells.get(&f).copied() }).collect();
            if !edits.is_empty() {
                cmds.push(Command::SetCells { cut: cut_id.clone(), track: t.id.clone(), cells: edits });
            }
        } else {
            let mut t = Track::new(ids.next(), &names[0], TrackKind::Drawing, Some(bottom_id.clone()));
            t.cells = cells;
            cmds.push(Command::AddTrack { cut: cut_id.clone(), index: None, track: t });
        }
    }

    // máscaras e fronteiras de balde de outras camadas que apontavam para as que saem passam a apontar para a de baixo
    let gone: BTreeSet<&Id> = chosen[1..].iter().collect();
    for (id, n) in &p.nodes {
        let NodeKind::Layer(l) = &n.data else { continue };
        if chosen.contains(id) {
            continue;
        }
        let mut nl = l.clone();
        let mut touched = false;
        if nl.mask.as_ref().is_some_and(|m| gone.contains(m)) {
            nl.mask = Some(bottom_id.clone());
            touched = true;
        }
        if nl.line_refs.iter().any(|r| gone.contains(r)) {
            let mut refs: Vec<Id> = Vec::new();
            for r in &nl.line_refs {
                let r = if gone.contains(r) { bottom_id.clone() } else { r.clone() };
                if !refs.contains(&r) {
                    refs.push(r);
                }
            }
            nl.line_refs = refs;
            touched = true;
        }
        if touched {
            cmds.push(Command::ReplaceData { id: id.clone(), data: NodeKind::Layer(nl) });
        }
    }

    // as de cima saem (o DeleteNode leva os cels e as trilhas delas)
    for id in &chosen[1..] {
        cmds.push(Command::DeleteNode { id: id.clone() });
    }

    let label = if chosen.len() == 2 { format!("Mesclar \"{}\" em \"{}\"", names[1], names[0]) } else { format!("Mesclar {} camadas em \"{}\"", chosen.len(), names[0]) };
    Ok(MergePlan { command: Command::Batch { commands: cmds, label: Some(label.clone()) }, layer: bottom_id, merged: chosen[1..].to_vec(), kind: result_kind, cels_changed: changed, cels_created: created, label })
}

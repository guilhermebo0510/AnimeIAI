//! Modelagem por comando (S24 3D, fase 12): o documento da Modelagem (`aiai_3d_edit::ModelDoc`) mora num ativo 3D do
//! projeto (`params.modeling`), ao lado do GLB normalizado que a camada 3D e a biblioteca usam (`blob`). Cada operação lê
//! o documento, muda e grava o ativo inteiro de volta (um passo de undo), então a MCP (agente fazendo adereços simples), o
//! headless e o workspace Modelagem falam do mesmo objeto. "Enviar ao Layout" cria a camada 3D do ativo no corte.

use crate::context::{ToolError, ToolResult};
pub use aiai_3d_edit::doc::ModelDoc;
use aiai_3d_edit::doc::{self, ModifierSpec, Primitive, Shading};
use aiai_3d_edit::{EditMesh, SelectMode, Selection};
use aiai_core::model3d::{hex_rgb, parse_hex_rgb};
use aiai_core::*;
use glam::Vec3;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelingOp {
    /// Cria um modelo novo (ativo 3D editável), opcionalmente já com uma primitiva.
    New,
    /// Lê o modelo: objetos, faces (normal e centro, para escolher onde extrudar), modificadores (não altera nada).
    Info,
    /// Lista os modelos editáveis do projeto (não altera nada).
    List,
    /// Acrescenta uma primitiva (cube, sphere, cylinder, cone, plane, torus).
    AddPrimitive,
    /// Posição/rotação/escala do objeto inteiro.
    Transform,
    /// Extrude das faces ao longo da normal.
    Extrude,
    /// Inset das faces.
    Inset,
    /// Move as faces (os vértices delas) por `offset`.
    MoveFaces,
    /// Escala as faces em volta do centro delas por `factor` (0 num eixo achata: tampa de telhado → cumeeira).
    ScaleFaces,
    /// Bevel de arestas (`edges: "all"` ou pares de vértices).
    Bevel,
    /// Funde vértices coincidentes (depois de achatar faces, por exemplo).
    MergeByDistance,
    /// Acrescenta (`add`) ou tira (`remove`) um modificador.
    Modifier,
    /// Um operador do modo Edição do catálogo do ANIGO (`operator`: bevel, loop_cut, bisect, bridge, fill_hole,
    /// triangulate, smooth, subdivide...) sobre `faces`/`edges`, com `params` (os do "Ajustar última operação").
    Operator,
    /// Cor toon e sombreamento do objeto.
    Material,
    /// Apaga um objeto.
    DeleteObject,
    /// Cria a camada 3D do modelo no corte `cut` (Enviar ao Layout).
    Send,
    /// Exporta o GLB para `path` (absoluto, .glb).
    Export,
}

/// Faces: lista de índices ou uma direção (`+x`, `-x`, `+y`/`top`, `-y`/`bottom`, `+z`/`front`, `-z`/`back`) ou `all`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FaceSel {
    Indices(Vec<u32>),
    Named(String),
}

/// Arestas: `all` ou pares de vértices.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EdgeSel {
    Pairs(Vec<[u32; 2]>),
    Named(String),
}

/// Pedido `MODELING`. Chave desconhecida é erro.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelingRequest {
    pub op: Option<ModelingOp>,
    /// O ativo do modelo (todas as ops menos new e list).
    #[serde(default)]
    pub asset: Option<Id>,
    /// new: nome do modelo; add_primitive: nome do objeto.
    #[serde(default)]
    pub name: Option<String>,
    /// new/add_primitive: cube | sphere | cylinder | cone | plane | torus.
    #[serde(default)]
    pub primitive: Option<String>,
    /// new/add_primitive: tamanho em metros (aresta do cubo, diâmetro, altura). Padrão 1.
    #[serde(default)]
    pub size: Option<f32>,
    /// Objeto (índice ou nome). Padrão: o último.
    #[serde(default)]
    pub object: Option<String>,
    /// add_primitive/transform: posição [x, y, z] (m, Y para cima).
    #[serde(default)]
    pub pos: Option<[f32; 3]>,
    /// add_primitive/transform: rotação em graus [x, y, z].
    #[serde(default)]
    pub rot_deg: Option<[f32; 3]>,
    /// add_primitive/transform: escala [x, y, z].
    #[serde(default)]
    pub scale: Option<[f32; 3]>,
    /// extrude/inset/move_faces/scale_faces: faces (índices, direção ou "all"; padrão "+y").
    #[serde(default)]
    pub faces: Option<FaceSel>,
    /// extrude: distância (m; negativa afunda).
    #[serde(default)]
    pub distance: Option<f32>,
    /// inset: espessura da borda (m).
    #[serde(default)]
    pub thickness: Option<f32>,
    /// inset: profundidade (m).
    #[serde(default)]
    pub depth: Option<f32>,
    /// bevel: arestas ("all" ou [[a, b], ...]).
    #[serde(default)]
    pub edges: Option<EdgeSel>,
    /// bevel: largura (m).
    #[serde(default)]
    pub width: Option<f32>,
    /// bevel: segmentos (1 = chanfro).
    #[serde(default)]
    pub segments: Option<u32>,
    /// move_faces: deslocamento [x, y, z] (m, eixos do objeto).
    #[serde(default)]
    pub offset: Option<[f32; 3]>,
    /// scale_faces: fatores [x, y, z].
    #[serde(default)]
    pub factor: Option<[f32; 3]>,
    /// merge_by_distance: distância (m). Padrão 0,0001.
    #[serde(default)]
    pub merge_distance: Option<f32>,
    /// modifier: acrescenta mirror | subsurf | array.
    #[serde(default)]
    pub add: Option<String>,
    /// modifier: tira o modificador desta posição da pilha.
    #[serde(default)]
    pub remove: Option<usize>,
    /// modifier mirror: eixos [x, y, z].
    #[serde(default)]
    pub axes: Option<[bool; 3]>,
    /// modifier subsurf: níveis 1..3.
    #[serde(default)]
    pub levels: Option<u32>,
    /// modifier array: cópias 1..100.
    #[serde(default)]
    pub count: Option<u32>,
    /// modifier array: deslocamento por cópia (fração do tamanho em cada eixo).
    #[serde(default)]
    pub array_offset: Option<[f32; 3]>,
    /// material: cor "#rrggbb".
    #[serde(default)]
    pub color: Option<String>,
    /// material: auto | flat | smooth.
    #[serde(default)]
    pub shading: Option<String>,
    /// send: corte onde criar a camada 3D.
    #[serde(default)]
    pub cut: Option<Id>,
    /// export: caminho absoluto do .glb.
    #[serde(default)]
    pub path: Option<String>,
    /// operator: chave do operador (`extrude`, `inset`, `bevel`, `loop_cut`, `bisect`, `bridge`, `fill_hole`, `grid_fill`,
    /// `triangulate`, `tris_to_quads`, `smooth`, `relax`, `subdivide`, `poke`, `merge_center`, `dissolve_edges`,
    /// `limited_dissolve`, `flip_normals`, `recalc_outside`, `mark_sharp`, `mark_seam`, `set_crease`...).
    #[serde(default)]
    pub operator: Option<String>,
    /// operator: parâmetros (`thickness`, `width`, `segments`, `profile`, `cuts`, `slide`, `plane_axis`, `plane_offset`,
    /// `fill`, `factor`, `iterations`, `merge_distance`, `dissolve_angle_deg`, `subdiv_cuts`, `crease`...).
    #[serde(default)]
    pub params: Option<serde_json::Map<String, Value>>,
}

/// Documento de um ativo 3D editável (com `params.modeling`).
pub fn asset_doc(p: &Project, asset: &Id) -> ToolResult<ModelDoc> {
    let n = p.node(asset).ok_or_else(|| ToolError::NotFound(format!("ativo {asset}")))?;
    let NodeKind::Asset(a) = &n.data else { return Err(ToolError::Invalid(format!("{asset} não é um ativo"))) };
    if a.asset != AssetType::Model3d {
        return Err(ToolError::Invalid(format!("{asset} não é um ativo 3D")));
    }
    let v = a.params.get("modeling").ok_or_else(|| ToolError::Invalid(format!("o ativo 3D '{}' não é editável (foi importado; a Modelagem edita os modelos feitos nela)", n.name)))?;
    ModelDoc::from_json(v).map_err(ToolError::Invalid)
}

/// Modelos editáveis do projeto: (ativo, nome, objetos).
pub fn editable_assets(p: &Project) -> Vec<(Id, String, usize)> {
    p.nodes
        .iter()
        .filter_map(|(id, n)| match &n.data {
            NodeKind::Asset(a) if a.asset == AssetType::Model3d => {
                let objs = a.params.get("modeling").and_then(|m| m.get("objects")).and_then(Value::as_array).map(|o| o.len())?;
                Some((id.clone(), n.name.clone(), objs))
            }
            _ => None,
        })
        .collect()
}

/// Comando que grava o documento no ativo (cria com `existing = None`): GLB novo no blob, informações e o documento em
/// `params`. Devolve o comando, o id do ativo e as informações do modelo.
pub fn save_command(p: &Project, ids: &mut IdGen, blobs: &mut BlobStore, existing: Option<&Id>, d: &ModelDoc) -> ToolResult<(Command, Id, aiai_3d_model::ModelInfo)> {
    d.validate().map_err(ToolError::Invalid)?;
    let model = d.to_model().map_err(ToolError::Invalid)?;
    let hash = blobs.put(aiai_3d_model::glb::write_glb(&model));
    let data = NodeKind::Asset(AssetData {
        asset: AssetType::Model3d,
        blob: Some(hash),
        params: json!({ "model3d": model.info, "modeling": d }),
        source: "Modelagem do AnimeIAI".into(),
    });
    match existing {
        Some(id) => {
            let n = p.node(id).ok_or_else(|| ToolError::NotFound(format!("ativo {id}")))?;
            if !matches!(&n.data, NodeKind::Asset(a) if a.asset == AssetType::Model3d) {
                return Err(ToolError::Invalid(format!("{id} não é um ativo 3D")));
            }
            let cmd = if n.name != d.name {
                Command::Batch { commands: vec![Command::ReplaceData { id: id.clone(), data }, Command::SetProps { id: id.clone(), name: Some(d.name.clone()), data: json!({}) }], label: Some("Modelagem".into()) }
            } else {
                Command::ReplaceData { id: id.clone(), data }
            };
            Ok((cmd, id.clone(), model.info))
        }
        None => {
            let id = ids.next();
            Ok((Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: d.name.clone(), data }, id, model.info))
        }
    }
}

fn object_index(d: &ModelDoc, key: Option<&str>) -> ToolResult<usize> {
    match key {
        Some(k) => d.find(k).ok_or_else(|| ToolError::Invalid(format!("objeto '{k}' não existe (veja op info)"))),
        None => d.objects.len().checked_sub(1).ok_or_else(|| ToolError::Invalid("o modelo não tem objetos (op add_primitive)".into())),
    }
}

fn faces_of(m: &EditMesh, sel: Option<&FaceSel>) -> ToolResult<Vec<u32>> {
    let faces = match sel {
        None => doc::faces_facing(m, Vec3::Y, 0.9),
        Some(FaceSel::Indices(v)) => {
            if let Some(bad) = v.iter().find(|f| **f as usize >= m.face_count()) {
                return Err(ToolError::Invalid(format!("face {bad} não existe (o objeto tem {} faces)", m.face_count())));
            }
            v.clone()
        }
        Some(FaceSel::Named(n)) if n == "all" => (0..m.face_count() as u32).collect(),
        Some(FaceSel::Named(n)) => {
            let d = doc::direction(n).ok_or_else(|| ToolError::Invalid(format!("faces '{n}': use índices, \"all\" ou uma direção (+x, -x, +y, -y, +z, -z)")))?;
            doc::faces_facing(m, d, 0.9)
        }
    };
    if faces.is_empty() {
        return Err(ToolError::Invalid("nenhuma face escolhida (nenhuma aponta para essa direção?)".into()));
    }
    Ok(faces)
}

fn finite3(v: &[f32; 3], what: &str) -> ToolResult<()> {
    if v.iter().all(|x| x.is_finite() && x.abs() <= 1e4) { Ok(()) } else { Err(ToolError::Invalid(format!("`{what}` com valor não finito ou fora de ±10000"))) }
}

/// Aplica uma op de edição ao documento (sem tocar no projeto). Devolve um resumo para a resposta.
pub fn apply_op(d: &mut ModelDoc, r: &ModelingRequest) -> ToolResult<Value> {
    let op = r.op.ok_or_else(|| ToolError::Invalid("falta `op`".into()))?;
    let inv = ToolError::Invalid;
    match op {
        ModelingOp::AddPrimitive | ModelingOp::New => {
            let Some(pk) = &r.primitive else {
                return if op == ModelingOp::New { Ok(json!({})) } else { Err(inv("falta `primitive` (cube, sphere, cylinder, cone, plane, torus)".into())) };
            };
            let kind = Primitive::from_key(pk).ok_or_else(|| inv(format!("primitiva '{pk}' desconhecida (cube, sphere, cylinder, cone, plane, torus)")))?;
            let pos = r.pos.unwrap_or([0.0; 3]);
            finite3(&pos, "pos")?;
            let size = r.size.unwrap_or(1.0);
            if !(size.is_finite() && (1e-3..=1e3).contains(&size)) {
                return Err(inv("`size` fora de 0,001..1000".into()));
            }
            let name = if op == ModelingOp::New { None } else { r.name.as_deref() };
            let i = d.add_primitive(kind, size, pos, name).map_err(inv)?;
            if let Some(v) = r.rot_deg {
                finite3(&v, "rot_deg")?;
                d.objects[i].transform.rot_deg = v;
            }
            if let Some(v) = r.scale {
                finite3(&v, "scale")?;
                d.objects[i].transform.scale = v;
            }
            if let Some(c) = &r.color {
                d.objects[i].color = parse_hex_rgb(c).map_err(inv)?;
            }
            Ok(json!({ "object": i, "name": d.objects[i].name }))
        }
        ModelingOp::Transform => {
            let i = object_index(d, r.object.as_deref())?;
            let t = &mut d.objects[i].transform;
            for (v, what, dst) in [(r.pos, "pos", &mut t.pos), (r.rot_deg, "rot_deg", &mut t.rot_deg)] {
                if let Some(v) = v {
                    finite3(&v, what)?;
                    *dst = v;
                }
            }
            if let Some(v) = r.scale {
                finite3(&v, "scale")?;
                if v.iter().any(|s| s.abs() < 1e-4) {
                    return Err(inv("`scale` com eixo zero".into()));
                }
                t.scale = v;
            }
            Ok(json!({ "object": i }))
        }
        ModelingOp::Extrude | ModelingOp::Inset | ModelingOp::MoveFaces | ModelingOp::ScaleFaces => {
            let i = object_index(d, r.object.as_deref())?;
            let m = &mut d.objects[i].mesh;
            let faces = faces_of(m, r.faces.as_ref())?;
            let out = match op {
                ModelingOp::Extrude => {
                    let dist = r.distance.ok_or_else(|| inv("falta `distance`".into()))?;
                    if !dist.is_finite() || dist.abs() > 1e3 {
                        return Err(inv("`distance` inválida".into()));
                    }
                    doc::extrude(m, &faces, dist).map_err(inv)?
                }
                ModelingOp::Inset => {
                    let th = r.thickness.ok_or_else(|| inv("falta `thickness`".into()))?;
                    doc::inset(m, &faces, th, r.depth.unwrap_or(0.0)).map_err(inv)?
                }
                ModelingOp::MoveFaces => {
                    let o = r.offset.ok_or_else(|| inv("falta `offset`".into()))?;
                    finite3(&o, "offset")?;
                    let verts = doc::selection_verts(m, &Selection::from_faces(m, faces.iter().copied()), SelectMode::Face);
                    doc::translate(m, &verts, Vec3::from(o)).map_err(inv)?;
                    aiai_3d_edit::ops::OpOutput { selection: Selection::from_faces(m, faces.iter().copied()), ..Default::default() }
                }
                _ => {
                    let f = r.factor.ok_or_else(|| inv("falta `factor`".into()))?;
                    finite3(&f, "factor")?;
                    let verts = doc::selection_verts(m, &Selection::from_faces(m, faces.iter().copied()), SelectMode::Face);
                    doc::scale(m, &verts, Vec3::from(f)).map_err(inv)?;
                    aiai_3d_edit::ops::OpOutput { selection: Selection::from_faces(m, faces.iter().copied()), ..Default::default() }
                }
            };
            Ok(json!({ "object": i, "faces": out.selection.faces, "face_count": m.face_count(), "vert_count": m.vert_count() }))
        }
        ModelingOp::Bevel => {
            let i = object_index(d, r.object.as_deref())?;
            let m = &mut d.objects[i].mesh;
            let edges = match &r.edges {
                None => doc::all_edges(m),
                Some(EdgeSel::Named(n)) if n == "all" => doc::all_edges(m),
                Some(EdgeSel::Named(n)) => return Err(inv(format!("edges '{n}': use \"all\" ou [[a, b], ...]"))),
                Some(EdgeSel::Pairs(v)) => v.iter().map(|[a, b]| aiai_3d_edit::edge_key(*a, *b)).collect(),
            };
            let w = r.width.ok_or_else(|| inv("falta `width`".into()))?;
            doc::bevel(m, &edges, w, r.segments.unwrap_or(1)).map_err(inv)?;
            Ok(json!({ "object": i, "face_count": m.face_count() }))
        }
        ModelingOp::MergeByDistance => {
            let i = object_index(d, r.object.as_deref())?;
            let m = &mut d.objects[i].mesh;
            let before = m.vert_count();
            doc::merge_by_distance(m, None, r.merge_distance.unwrap_or(1e-4)).map_err(inv)?;
            Ok(json!({ "object": i, "merged": before - m.vert_count(), "face_count": m.face_count() }))
        }
        ModelingOp::Modifier => {
            let i = object_index(d, r.object.as_deref())?;
            let o = &mut d.objects[i];
            if let Some(k) = r.remove {
                if k >= o.modifiers.len() {
                    return Err(inv(format!("modificador {k} não existe (a pilha tem {})", o.modifiers.len())));
                }
                o.modifiers.remove(k);
            }
            if let Some(a) = &r.add {
                let mut m = ModifierSpec::new(a).ok_or_else(|| inv(format!("modificador '{a}' desconhecido ({})", ModifierSpec::KEYS.join(", "))))?;
                match &mut m {
                    ModifierSpec::Mirror { axes, .. } => {
                        if let Some(x) = r.axes {
                            *axes = x;
                        }
                    }
                    ModifierSpec::Subsurf { levels, .. } => {
                        if let Some(l) = r.levels {
                            *levels = l;
                        }
                    }
                    ModifierSpec::Array { count, offset, .. } => {
                        if let Some(c) = r.count {
                            *count = c;
                        }
                        if let Some(v) = r.array_offset {
                            *offset = v;
                        }
                    }
                    // os outros entram com os valores padrão (a interface ajusta)
                    _ => {}
                }
                m.validate().map_err(inv)?;
                o.modifiers.push(m);
            }
            o.evaluated().map_err(|e| inv(e.to_string()))?;
            Ok(json!({ "object": i, "modifiers": o.modifiers }))
        }
        ModelingOp::Operator => {
            use aiai_3d_edit::ops::catalog::{OpKind, OpParams, apply_kind};
            let i = object_index(d, r.object.as_deref())?;
            let key = r.operator.as_deref().ok_or_else(|| inv("falta `operator`".into()))?;
            let kind = OpKind::from_key(key).ok_or_else(|| inv(format!("operador '{key}' desconhecido ({})", OpKind::ALL.iter().map(|k| k.key()).collect::<Vec<_>>().join(", "))))?;
            let mut params = OpParams::default();
            if let Some(m) = &r.params {
                params.apply_json(m).map_err(inv)?;
            }
            let m = &mut d.objects[i].mesh;
            let mut sel = Selection::default();
            if r.faces.is_some() {
                sel = Selection::from_faces(m, faces_of(m, r.faces.as_ref())?);
            }
            match &r.edges {
                None => {}
                Some(EdgeSel::Named(n)) if n == "all" => sel = Selection::from_edges(doc::all_edges(m)),
                Some(EdgeSel::Named(n)) => return Err(inv(format!("edges '{n}': use \"all\" ou [[a, b], ...]"))),
                Some(EdgeSel::Pairs(v)) => sel = Selection::from_edges(v.iter().map(|[a, b]| aiai_3d_edit::edge_key(*a, *b))),
            }
            let out = apply_kind(kind, &params, m, &sel).map_err(|e| inv(format!("{key}: {e}")))?;
            m.validate().map_err(|e| inv(e.to_string()))?;
            Ok(json!({ "object": i, "operator": key, "faces": out.selection.faces, "face_count": m.face_count(), "vert_count": m.vert_count() }))
        }
        ModelingOp::Material => {
            let i = object_index(d, r.object.as_deref())?;
            if let Some(c) = &r.color {
                d.objects[i].color = parse_hex_rgb(c).map_err(inv)?;
            }
            if let Some(s) = &r.shading {
                d.objects[i].shading = Shading::ALL.into_iter().find(|x| x.key() == s).ok_or_else(|| inv(format!("sombreamento '{s}' desconhecido (auto, flat, smooth)")))?;
            }
            Ok(json!({ "object": i, "color": hex_rgb(d.objects[i].color), "shading": d.objects[i].shading }))
        }
        ModelingOp::DeleteObject => {
            let i = object_index(d, r.object.as_deref())?;
            let o = d.objects.remove(i);
            Ok(json!({ "deleted": o.name }))
        }
        ModelingOp::Info | ModelingOp::List | ModelingOp::Send | ModelingOp::Export => Err(inv("op sem edição do documento".into())),
    }
}

/// Resumo legível do documento (op info).
pub fn info_json(d: &ModelDoc) -> Value {
    let objs: Vec<Value> = d
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let m = &o.mesh;
            let faces: Vec<Value> = (0..m.face_count().min(200) as u32)
                .map(|f| {
                    let n = m.face_normal(f);
                    let c = m.face_center(f);
                    json!({ "face": f, "normal": [round3(n.x), round3(n.y), round3(n.z)], "center": [round3(c.x), round3(c.y), round3(c.z)] })
                })
                .collect();
            let (lo, hi) = m.bounds().unwrap_or((Vec3::ZERO, Vec3::ZERO));
            json!({
                "object": i, "name": o.name, "transform": o.transform, "color": hex_rgb(o.color), "shading": o.shading,
                "verts": m.vert_count(), "faces": m.face_count(), "bbox_local": [lo.to_array(), hi.to_array()],
                "modifiers": o.modifiers, "visible": o.visible, "face_list": faces,
            })
        })
        .collect();
    json!({ "name": d.name, "objects": objs })
}

fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (CoreSession, Id) {
        let mut s = CoreSession::for_tests("mdl");
        s.project.settings.width = 96;
        s.project.settings.height = 54;
        let mk = |s: &mut CoreSession, parent: Option<&Id>, data: NodeKind| s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: "n".into(), data }).unwrap().created[0].clone();
        let ep = mk(&mut s, None, NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut s, Some(&ep), NodeKind::Scene(SceneData::default()));
        let cut = mk(&mut s, Some(&sc), NodeKind::Cut(CutData::new(4)));
        (s, cut)
    }

    fn run(s: &mut CoreSession, asset: Option<&Id>, r: ModelingRequest) -> Id {
        let mut d = match asset {
            Some(a) => asset_doc(&s.project, a).unwrap(),
            None => ModelDoc::new(r.name.as_deref().unwrap_or("Modelo")),
        };
        apply_op(&mut d, &r).unwrap();
        let (cmd, id, _) = save_command(&s.project, &mut s.ids, &mut s.blobs, asset, &d).unwrap();
        s.apply(cmd).unwrap();
        id
    }

    #[test]
    fn a_house_by_commands_saves_glb_and_goes_to_the_layout() {
        let (mut s, cut) = session();
        let a = run(&mut s, None, ModelingRequest { op: Some(ModelingOp::New), name: Some("Casa".into()), primitive: Some("cube".into()), pos: Some([0.0, 0.5, 0.0]), ..Default::default() });
        run(&mut s, Some(&a), ModelingRequest { op: Some(ModelingOp::Extrude), faces: Some(FaceSel::Named("top".into())), distance: Some(0.6), ..Default::default() });
        // a tampa nova é a face de cima de novo: achata em X (cumeeira) e funde os vértices repetidos
        run(&mut s, Some(&a), ModelingRequest { op: Some(ModelingOp::ScaleFaces), faces: Some(FaceSel::Named("+y".into())), factor: Some([0.0, 1.0, 1.0]), ..Default::default() });
        run(&mut s, Some(&a), ModelingRequest { op: Some(ModelingOp::MergeByDistance), ..Default::default() });
        run(&mut s, Some(&a), ModelingRequest { op: Some(ModelingOp::Material), color: Some("#c86448".into()), ..Default::default() });
        let d = asset_doc(&s.project, &a).unwrap();
        assert_eq!(d.objects.len(), 1);
        let m = &d.objects[0].mesh;
        let (_, hi) = m.bounds().unwrap();
        assert!((hi.y - 1.1).abs() < 1e-4);
        assert!(m.vert_count() == 10, "8 da caixa + 2 da cumeeira: {}", m.vert_count());
        // o blob é o GLB do documento (a camada 3D e a biblioteca leem dele)
        let NodeKind::Asset(ad) = &s.project.node(&a).unwrap().data else { panic!() };
        let model = aiai_3d_model::load_model(&s.blobs, ad.blob.as_ref().unwrap()).unwrap();
        assert_eq!(model.info.triangles, d.to_model().unwrap().info.triangles);
        assert_eq!(editable_assets(&s.project).len(), 1);
        // enviar ao Layout: camada 3D do ativo no corte, que renderiza
        let req = crate::model3d::Model3dRequest { op: Some(crate::model3d::Model3dOp::Import), asset: Some(a.clone()), cut: Some(cut.clone()), ..Default::default() };
        let out = crate::model3d::import(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
        s.apply(out.batch("Enviar ao Layout")).unwrap();
        let img = aiai_render::render_frame(&s.project, &s.blobs, &cut, 0, &aiai_render::RenderOptions { include_reference: true, camera: false, ..Default::default() }).unwrap();
        assert!(img.image.data.iter().filter(|p| p[3] > 0.5).count() > 100);
        // cada op é um passo de undo
        s.undo().unwrap();
        assert_eq!(asset_doc(&s.project, &a).unwrap().objects[0].color, d.objects[0].color, "desfez só a camada");
        s.undo().unwrap();
        assert_ne!(asset_doc(&s.project, &a).unwrap().objects[0].color, d.objects[0].color, "desfez a cor");
    }

    #[test]
    fn bad_requests_give_clear_errors() {
        let mut d = ModelDoc::new("x");
        let e = apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Extrude), distance: Some(1.0), ..Default::default() }).unwrap_err();
        assert!(e.to_string().contains("add_primitive"), "{e}");
        apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::AddPrimitive), primitive: Some("cube".into()), ..Default::default() }).unwrap();
        assert!(apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Extrude), faces: Some(FaceSel::Indices(vec![99])), distance: Some(1.0), ..Default::default() }).is_err());
        assert!(apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Extrude), faces: Some(FaceSel::Named("diagonal".into())), distance: Some(1.0), ..Default::default() }).is_err());
        assert!(apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::AddPrimitive), primitive: Some("dodecaedro".into()), ..Default::default() }).is_err());
        assert!(apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Modifier), add: Some("subsurf".into()), levels: Some(7), ..Default::default() }).is_err());
        let v = serde_json::json!({ "op": "extrude", "asset": "a", "lado": 1 });
        assert!(serde_json::from_value::<ModelingRequest>(v).is_err(), "chave desconhecida é erro");
    }

    #[test]
    fn catalog_operators_and_new_modifiers_by_command() {
        let mut d = ModelDoc::new("op");
        apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::AddPrimitive), primitive: Some("cube".into()), ..Default::default() }).unwrap();
        let params = serde_json::json!({ "width": 0.1, "segments": 3 }).as_object().cloned();
        let v = apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Operator), operator: Some("bevel".into()), edges: Some(EdgeSel::Named("all".into())), params, ..Default::default() }).unwrap();
        assert!(v["face_count"].as_u64().unwrap() > 6 + 12 * 2, "{v}");
        let e = apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Operator), operator: Some("nao_existe".into()), ..Default::default() }).unwrap_err();
        assert!(e.to_string().contains("bridge"), "a mensagem lista os operadores: {e}");
        let bad = serde_json::json!({ "segmentos": 3 }).as_object().cloned();
        assert!(apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Operator), operator: Some("bevel".into()), params: bad, ..Default::default() }).is_err());
        for k in ["solidify", "weld", "edge_split", "decimate", "displace", "triangulate"] {
            apply_op(&mut d, &ModelingRequest { op: Some(ModelingOp::Modifier), add: Some(k.into()), ..Default::default() }).unwrap();
        }
        assert_eq!(d.objects[0].modifiers.len(), 6);
        d.objects[0].evaluated().unwrap();
        // o documento com os modificadores novos volta igual do JSON
        let back = ModelDoc::from_json(&serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}

//! Camada 3D de referência (S24 3D): importar modelo (glTF/GLB/OBJ → GLB normalizado num ativo `model3d`), criar a
//! camada `model3d` com câmera que enquadra o modelo, listar, e mudar transformação, câmera e aparência. Tudo vira
//! comando do núcleo (um passo de undo), igual para a UI, a bridge/MCP e o headless.
//!
//! Regra das chaves: uma propriedade que já tem chaves (`m3d.*` no `KeyTracks` da camada) recebe chave no quadro do
//! pedido; as outras mudam o valor-base. `key: true` força chave em todas as propriedades mudadas.

use crate::context::{ToolError, ToolResult};
use aiai_3d_model::camera::framing_camera;
use aiai_3d_model::{ModelInfo, glb, import_path};
use aiai_core::model3d::{KEY_PROPS, Model3dParams, Model3dState, OrbitCam3d, Render3d, RenderMode3d, Transform3d, hex_rgb, parse_hex_rgb, state_prop};
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Model3dOp {
    /// Importa um arquivo (.glb, .gltf, .obj) para um ativo e, com `cut`, cria a camada 3D nele.
    Import,
    /// Lista os ativos 3D e as camadas 3D (não altera nada).
    List,
    /// Muda posição/rotação/escala do modelo.
    Transform,
    /// Muda a câmera orbital da camada (`frame_model` = reenquadrar o modelo).
    Camera,
    /// Muda a aparência: modo, contorno, cor, referência, opacidade.
    Mode,
}

/// Pedido `MODEL3D`. Chave desconhecida é erro.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct Model3dRequest {
    pub op: Option<Model3dOp>,
    /// import: caminho absoluto do arquivo (.glb, .gltf, .obj; .vrm ainda não).
    #[serde(default)]
    pub path: Option<String>,
    /// import: corte onde criar a camada (omitido = só o ativo). Com `asset` no lugar de `path`, cria a camada de um
    /// ativo 3D que já está no projeto.
    #[serde(default)]
    pub cut: Option<Id>,
    /// import: ativo 3D já importado (em vez de `path`).
    #[serde(default)]
    pub asset: Option<Id>,
    /// import: nome da camada/ativo (padrão: nome do arquivo).
    #[serde(default)]
    pub name: Option<String>,
    /// transform/camera/mode: a camada 3D.
    #[serde(default)]
    pub layer: Option<Id>,
    /// Quadro das chaves (0-based, padrão 0).
    #[serde(default)]
    pub frame: Option<u32>,
    /// true = grava chave no `frame` para toda propriedade mudada (padrão: só nas que já são animadas).
    #[serde(default)]
    pub key: Option<bool>,
    /// transform: posição em metros [x, y, z] (Y para cima).
    #[serde(default)]
    pub pos: Option<[f32; 3]>,
    /// transform: rotação em graus [x, y, z] (Euler X→Y→Z nos eixos do mundo).
    #[serde(default)]
    pub rot_deg: Option<[f32; 3]>,
    /// transform: escala [x, y, z].
    #[serde(default)]
    pub scale: Option<[f32; 3]>,
    /// camera: ponto que a câmera olha [x, y, z].
    #[serde(default)]
    pub target: Option<[f32; 3]>,
    /// camera: giro em volta do alvo, graus (0 = de frente, olhando para -Z).
    #[serde(default)]
    pub yaw_deg: Option<f32>,
    /// camera: elevação, graus (positivo = de cima).
    #[serde(default)]
    pub pitch_deg: Option<f32>,
    /// camera: distância ao alvo, metros.
    #[serde(default)]
    pub distance: Option<f32>,
    /// camera: abertura vertical, graus (1..150).
    #[serde(default)]
    pub fov_deg: Option<f32>,
    /// camera: true = reenquadra o modelo (alvo no centro da caixa, distância para caber), mantendo yaw/pitch/fov.
    #[serde(default)]
    pub frame_model: Option<bool>,
    /// mode: toon | flat | wire | silhouette.
    #[serde(default)]
    pub mode: Option<RenderMode3d>,
    /// mode: largura do contorno em px do canvas (0 = sem contorno; no wire é a largura das arestas).
    #[serde(default)]
    pub outline_px: Option<f32>,
    /// mode: cor do contorno "#rrggbb".
    #[serde(default)]
    pub outline_color: Option<String>,
    /// mode: cor única "#rrggbb" no lugar das cores do modelo; "" volta às cores do modelo.
    #[serde(default)]
    pub color: Option<String>,
    /// mode: referência (não sai no render final nem na exportação). Padrão da camada nova: true.
    #[serde(default)]
    pub reference: Option<bool>,
    /// mode: opacidade da camada 0..1.
    #[serde(default)]
    pub opacity: Option<f32>,
}

/// Resultado da importação: comandos (um passo de undo), ids e informações do modelo.
pub struct ImportOutput {
    pub commands: Vec<Command>,
    pub asset: Id,
    pub layer: Option<Id>,
    pub info: ModelInfo,
}

impl ImportOutput {
    pub fn batch(&self, label: &str) -> Command {
        Command::Batch { commands: self.commands.clone(), label: Some(label.to_string()) }
    }
}

fn asset_info(p: &Project, asset: &Id) -> ToolResult<(BlobHash, ModelInfo, String)> {
    let n = p.node(asset).ok_or_else(|| ToolError::NotFound(format!("ativo {asset}")))?;
    let NodeKind::Asset(a) = &n.data else { return Err(ToolError::Invalid(format!("{asset} não é um ativo"))) };
    if a.asset != AssetType::Model3d {
        return Err(ToolError::Invalid(format!("{asset} não é um ativo 3D")));
    }
    let hash = a.blob.clone().ok_or_else(|| ToolError::Invalid(format!("ativo 3D {asset} sem blob")))?;
    let info = a.params.get("model3d").and_then(|v| serde_json::from_value::<ModelInfo>(v.clone()).ok()).unwrap_or_default();
    Ok((hash, info, n.name.clone()))
}

/// Camada 3D nova para um ativo: câmera que enquadra o modelo na proporção do canvas do corte.
pub fn layer_data_for(p: &Project, cut: &Id, asset: &Id, hash: &BlobHash, info: &ModelInfo) -> ToolResult<LayerData> {
    let cd = crate::context::cut_data(p, cut)?;
    let (w, h) = aiai_render::cut_canvas(p, cd);
    let camera = framing_camera(info.bbox, w as f32 / h.max(1) as f32);
    let params = Model3dParams { version: 1, asset: asset.clone(), model_hash: hash.clone(), transform: Transform3d::default(), camera, render: Render3d::default() };
    let mut ld = LayerData::new(LayerType::Model3d);
    ld.params = json!({ "model3d": params });
    Ok(ld)
}

/// Importa (ou reusa `asset`) e, com `cut`, cria a camada 3D no topo do corte.
pub fn import(p: &Project, ids: &mut IdGen, blobs: &mut BlobStore, req: &Model3dRequest) -> ToolResult<ImportOutput> {
    let mut commands = Vec::new();
    let (asset, hash, info, base_name) = match (&req.path, &req.asset) {
        (Some(_), Some(_)) => return Err(ToolError::Invalid("use `path` OU `asset`, não os dois".into())),
        (Some(path), None) => {
            let model = import_path(std::path::Path::new(path)).map_err(|e| ToolError::Invalid(format!("{path}: {e}")))?;
            let bytes = glb::write_glb(&model);
            let hash = blobs.put(bytes);
            let info = model.info.clone();
            let stem = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "modelo".into());
            let name = req.name.clone().unwrap_or(stem);
            let asset = ids.next();
            let source = format!("importado: {}; {}", info.file_name, info.license.summary());
            commands.push(Command::CreateNode {
                id: Some(asset.clone()),
                parent: None,
                index: None,
                name: name.clone(),
                data: NodeKind::Asset(AssetData { asset: AssetType::Model3d, blob: Some(hash.clone()), params: json!({ "model3d": info }), source }),
            });
            (asset, hash, info, name)
        }
        (None, Some(a)) => {
            let (hash, info, name) = asset_info(p, a)?;
            (a.clone(), hash, info, req.name.clone().unwrap_or(name))
        }
        (None, None) => return Err(ToolError::Invalid("import precisa de `path` (arquivo) ou `asset` (ativo 3D do projeto)".into())),
    };
    let mut layer = None;
    if let Some(cut) = &req.cut {
        let ld = layer_data_for(p, cut, &asset, &hash, &info)?;
        let id = ids.next();
        commands.push(Command::CreateNode { id: Some(id.clone()), parent: Some(cut.clone()), index: None, name: format!("3D: {base_name}"), data: NodeKind::Layer(ld) });
        layer = Some(id);
    }
    if commands.is_empty() {
        return Err(ToolError::Invalid("nada a fazer: o ativo já existe e não foi dado `cut`".into()));
    }
    Ok(ImportOutput { commands, asset, layer, info })
}

/// Camada 3D e os parâmetros dela.
pub fn layer_params(p: &Project, layer: &Id) -> ToolResult<(LayerData, Model3dParams)> {
    let n = p.node(layer).ok_or_else(|| ToolError::NotFound(format!("camada {layer}")))?;
    let NodeKind::Layer(l) = &n.data else { return Err(ToolError::Invalid(format!("{layer} não é uma camada"))) };
    if l.kind != LayerType::Model3d {
        return Err(ToolError::Invalid(format!("a camada '{}' não é 3D (tipo model3d)", n.name)));
    }
    let m = Model3dParams::from_layer_params(&l.params).ok_or_else(|| ToolError::Invalid("camada 3D sem params.model3d".into()))?.map_err(ToolError::Invalid)?;
    Ok((l.clone(), m))
}

/// Comando que leva a camada ao estado `new` (transformação e câmera) a partir do estado avaliado no `frame`: cada
/// propriedade mudada vira chave se já é animada (ou se `force_key`), senão valor-base. `None` se nada mudou.
pub fn state_command(p: &Project, layer: &Id, frame: u32, new: &Model3dState, force_key: bool) -> ToolResult<Option<Command>> {
    let (l, mut params) = layer_params(p, layer)?;
    let old = params.eval(&l.keys, frame as f32);
    let mut keys = l.keys.clone();
    let mut base = params.eval(&KeyTracks::default(), 0.0);
    let mut keys_changed = Vec::new();
    let mut base_changed = false;
    for prop in KEY_PROPS {
        let (Some(a), Some(b)) = (state_prop(&old, prop), state_prop(new, prop)) else { continue };
        if (a - b).abs() <= 1e-6 * a.abs().max(1.0) {
            continue;
        }
        if force_key || keys.has(prop) {
            keys.set_key(prop, frame, b);
            keys_changed.push(prop);
        } else {
            set_state_prop(&mut base, prop, b);
            base_changed = true;
        }
    }
    if keys_changed.is_empty() && !base_changed {
        return Ok(None);
    }
    params.set_base_from(&base);
    let mut data = serde_json::Map::new();
    if base_changed {
        data.insert("params".into(), json!({ "model3d": params }));
    }
    if !keys_changed.is_empty() {
        let k: serde_json::Map<String, Value> = keys_changed.iter().map(|prop| ((*prop).to_string(), json!(keys.0.get(*prop)))).collect();
        data.insert("keys".into(), Value::Object(k));
    }
    Ok(Some(Command::SetProps { id: layer.clone(), name: None, data: Value::Object(data) }))
}

fn set_state_prop(s: &mut Model3dState, prop: &str, v: f32) {
    let t = &mut s.transform;
    let c = &mut s.camera;
    match prop {
        "m3d.pos.x" => t.pos[0] = v,
        "m3d.pos.y" => t.pos[1] = v,
        "m3d.pos.z" => t.pos[2] = v,
        "m3d.rot.x" => t.rot_deg[0] = v,
        "m3d.rot.y" => t.rot_deg[1] = v,
        "m3d.rot.z" => t.rot_deg[2] = v,
        "m3d.scale.x" => t.scale[0] = v,
        "m3d.scale.y" => t.scale[1] = v,
        "m3d.scale.z" => t.scale[2] = v,
        "m3d.cam.yaw" => c.yaw_deg = v,
        "m3d.cam.pitch" => c.pitch_deg = v,
        "m3d.cam.dist" => c.distance = v,
        "m3d.cam.fov" => c.fov_deg = v,
        "m3d.cam.target.x" => c.target[0] = v,
        "m3d.cam.target.y" => c.target[1] = v,
        "m3d.cam.target.z" => c.target[2] = v,
        _ => {}
    }
}

/// Câmera que reenquadra o modelo mantendo yaw/pitch/fov da câmera atual.
pub fn reframed_camera(p: &Project, blobs: &BlobStore, layer: &Id, cur: &OrbitCam3d) -> ToolResult<OrbitCam3d> {
    let (l, params) = layer_params(p, layer)?;
    let model = aiai_3d_model::load_model(blobs, &params.model_hash).map_err(ToolError::Invalid)?;
    let st = params.eval(&l.keys, 0.0);
    let (c, r) = aiai_3d_model::raster::world_bounds(&model, &st);
    let mut out = *cur;
    out.target = c.to_array();
    out.distance = (r / (cur.fov_deg.to_radians() * 0.5).sin().max(0.1)).clamp(0.05, 1e5);
    Ok(out)
}

/// Comando de transform/camera/mode.
pub fn edit_command(p: &Project, blobs: &BlobStore, req: &Model3dRequest) -> ToolResult<Option<Command>> {
    let op = req.op.ok_or_else(|| ToolError::Invalid("falta `op`".into()))?;
    let layer = req.layer.as_ref().ok_or_else(|| ToolError::Invalid("falta `layer` (camada 3D)".into()))?;
    let frame = req.frame.unwrap_or(0);
    let force = req.key.unwrap_or(false);
    let (l, params) = layer_params(p, layer)?;
    let mut st = params.eval(&l.keys, frame as f32);
    let finite3 = |v: &[f32; 3], what: &str| -> ToolResult<()> { if v.iter().all(|x| x.is_finite()) { Ok(()) } else { Err(ToolError::Invalid(format!("`{what}` com valor não finito"))) } };
    let no_mode_fields = req.mode.is_none() && req.outline_px.is_none() && req.outline_color.is_none() && req.color.is_none() && req.reference.is_none() && req.opacity.is_none();
    let no_cam_fields = req.target.is_none() && req.yaw_deg.is_none() && req.pitch_deg.is_none() && req.distance.is_none() && req.fov_deg.is_none() && req.frame_model.is_none();
    let no_xf_fields = req.pos.is_none() && req.rot_deg.is_none() && req.scale.is_none();
    match op {
        Model3dOp::Transform => {
            if !(no_mode_fields && no_cam_fields) {
                return Err(ToolError::Invalid("op transform aceita só pos, rot_deg, scale (câmera: op camera; aparência: op mode)".into()));
            }
            if let Some(v) = req.pos {
                finite3(&v, "pos")?;
                st.transform.pos = v;
            }
            if let Some(v) = req.rot_deg {
                finite3(&v, "rot_deg")?;
                st.transform.rot_deg = v;
            }
            if let Some(v) = req.scale {
                finite3(&v, "scale")?;
                if v.iter().any(|s| s.abs() < aiai_core::model3d::MIN_SCALE) {
                    return Err(ToolError::Invalid("`scale` com eixo zero".into()));
                }
                st.transform.scale = v;
            }
            state_command(p, layer, frame, &st, force)
        }
        Model3dOp::Camera => {
            if !(no_mode_fields && no_xf_fields) {
                return Err(ToolError::Invalid("op camera aceita só target, yaw_deg, pitch_deg, distance, fov_deg, frame_model".into()));
            }
            if req.frame_model == Some(true) {
                st.camera = reframed_camera(p, blobs, layer, &st.camera)?;
            }
            if let Some(v) = req.target {
                finite3(&v, "target")?;
                st.camera.target = v;
            }
            if let Some(v) = req.yaw_deg {
                st.camera.yaw_deg = v;
            }
            if let Some(v) = req.pitch_deg {
                st.camera.pitch_deg = v.clamp(-89.9, 89.9);
            }
            if let Some(v) = req.distance {
                st.camera.distance = v;
            }
            if let Some(v) = req.fov_deg {
                st.camera.fov_deg = v;
            }
            if !(st.camera.yaw_deg.is_finite() && st.camera.distance.is_finite() && st.camera.distance > 0.0 && (aiai_core::model3d::MIN_FOV_DEG..=aiai_core::model3d::MAX_FOV_DEG).contains(&st.camera.fov_deg)) {
                return Err(ToolError::Invalid("câmera inválida (distance > 0, fov_deg 1..150)".into()));
            }
            state_command(p, layer, frame, &st, force)
        }
        Model3dOp::Mode => {
            if !(no_xf_fields && no_cam_fields) {
                return Err(ToolError::Invalid("op mode aceita só mode, outline_px, outline_color, color, reference, opacity".into()));
            }
            let mut r = params.render.clone();
            if let Some(m) = req.mode {
                r.mode = m;
            }
            if let Some(v) = req.outline_px {
                r.outline_px = v;
            }
            if let Some(c) = &req.outline_color {
                r.outline_color = hex_rgb(parse_hex_rgb(c).map_err(ToolError::Invalid)?);
            }
            if let Some(c) = &req.color {
                r.color = if c.is_empty() { None } else { Some(hex_rgb(parse_hex_rgb(c).map_err(ToolError::Invalid)?)) };
            }
            if let Some(b) = req.reference {
                r.reference = b;
            }
            let mut data = serde_json::Map::new();
            if r != params.render {
                let mut np = params.clone();
                np.render = r;
                np.validate().map_err(ToolError::Invalid)?;
                // merge patch: `color: null` remove a cor única
                let mut v = json!({ "model3d": np });
                if np.render.color.is_none() {
                    v["model3d"]["render"]["color"] = Value::Null;
                }
                data.insert("params".into(), v);
            }
            if let Some(o) = req.opacity {
                if !(0.0..=1.0).contains(&o) {
                    return Err(ToolError::Invalid("`opacity` fora de 0..1".into()));
                }
                if o != l.opacity {
                    data.insert("opacity".into(), json!(o));
                }
            }
            Ok((!data.is_empty()).then(|| Command::SetProps { id: layer.clone(), name: None, data: Value::Object(data) }))
        }
        Model3dOp::Import | Model3dOp::List => Err(ToolError::Invalid("op sem comando de edição".into())),
    }
}

/// Ativos 3D e camadas 3D do projeto.
pub fn list(p: &Project) -> Value {
    let mut models = Vec::new();
    let mut layers = Vec::new();
    for (id, n) in &p.nodes {
        match &n.data {
            NodeKind::Asset(a) if a.asset == AssetType::Model3d => {
                models.push(json!({ "asset": id, "name": n.name, "blob": a.blob, "info": a.params.get("model3d"), "source": a.source }));
            }
            NodeKind::Layer(l) if l.kind == LayerType::Model3d => {
                layers.push(json!({ "layer": id, "name": n.name, "cut": p.cut_of(id), "opacity": l.opacity, "visible": l.visible, "params": l.params.get("model3d"), "keys": l.keys.0.keys().filter(|k| k.starts_with("m3d.")).collect::<Vec<_>>() }));
            }
            _ => {}
        }
    }
    json!({ "models": models, "layers": layers })
}

/// Estado atual da camada num quadro (para a resposta da bridge e para a UI).
pub fn state_json(p: &Project, layer: &Id, frame: u32) -> Value {
    match layer_params(p, layer) {
        Ok((l, m)) => json!({ "model3d": m, "state": m.eval(&l.keys, frame as f32) }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj_file() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aiai_m3d_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("caixa.obj");
        std::fs::write(&p, "# license: CC0\nv -0.5 0 -0.5\nv 0.5 0 -0.5\nv 0.5 1 -0.5\nv -0.5 1 -0.5\nv -0.5 0 0.5\nv 0.5 0 0.5\nv 0.5 1 0.5\nv -0.5 1 0.5\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 5 8 4 1\n").unwrap();
        p
    }

    fn session() -> (CoreSession, Id) {
        let mut s = CoreSession::for_tests("m3d");
        s.project.settings.width = 96;
        s.project.settings.height = 54;
        let mk = |s: &mut CoreSession, parent: Option<&Id>, data: NodeKind| s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: "n".into(), data }).unwrap().created[0].clone();
        let ep = mk(&mut s, None, NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut s, Some(&ep), NodeKind::Scene(SceneData::default()));
        let cut = mk(&mut s, Some(&sc), NodeKind::Cut(CutData::new(12)));
        (s, cut)
    }

    #[test]
    fn import_creates_asset_and_layer_in_one_step_and_undo_removes_both() {
        let (mut s, cut) = session();
        let req = Model3dRequest { op: Some(Model3dOp::Import), path: Some(obj_file().display().to_string()), cut: Some(cut.clone()), ..Default::default() };
        let out = import(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
        assert_eq!(out.info.triangles, 12);
        assert_eq!(out.info.license.license, "CC0");
        let before = s.project.nodes.len();
        s.apply(out.batch("Importar modelo 3D")).unwrap();
        assert_eq!(s.project.nodes.len(), before + 2);
        let layer = out.layer.clone().unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert!(m.render.reference, "referência ligada por padrão");
        assert_eq!(l.kind, LayerType::Model3d);
        // a camada renderiza (CPU) e aparece no canvas
        let img = aiai_render::render_frame(&s.project, &s.blobs, &cut, 0, &aiai_render::RenderOptions { include_reference: true, camera: false, ..Default::default() }).unwrap();
        assert!(img.warnings.is_empty(), "{:?}", img.warnings);
        assert!(img.image.data.iter().filter(|p| p[3] > 0.5).count() > 200);
        s.undo().unwrap();
        assert_eq!(s.project.nodes.len(), before);
    }

    #[test]
    fn edits_write_base_values_or_keys_and_are_one_undo_step() {
        let (mut s, cut) = session();
        let req = Model3dRequest { op: Some(Model3dOp::Import), path: Some(obj_file().display().to_string()), cut: Some(cut), ..Default::default() };
        let out = import(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
        s.apply(out.batch("Importar")).unwrap();
        let layer = out.layer.unwrap();
        // valor-base
        let t = Model3dRequest { op: Some(Model3dOp::Transform), layer: Some(layer.clone()), pos: Some([1.0, 0.0, 0.0]), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &t).unwrap().unwrap()).unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert_eq!(m.transform.pos, [1.0, 0.0, 0.0]);
        assert!(l.keys.is_empty());
        // chave forçada no quadro 6; depois, sem `key`, a mesma propriedade (já animada) recebe chave no quadro 10
        let k = Model3dRequest { op: Some(Model3dOp::Transform), layer: Some(layer.clone()), frame: Some(6), key: Some(true), rot_deg: Some([0.0, 90.0, 0.0]), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &k).unwrap().unwrap()).unwrap();
        let k2 = Model3dRequest { op: Some(Model3dOp::Transform), layer: Some(layer.clone()), frame: Some(10), rot_deg: Some([0.0, 180.0, 0.0]), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &k2).unwrap().unwrap()).unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert_eq!(l.keys.frames("m3d.rot.y"), vec![6, 10]);
        assert!((m.eval(&l.keys, 8.0).transform.rot_deg[1] - 135.0).abs() < 1e-3);
        // câmera reenquadrada e modo
        let c = Model3dRequest { op: Some(Model3dOp::Camera), layer: Some(layer.clone()), yaw_deg: Some(-40.0), frame_model: Some(true), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &c).unwrap().unwrap()).unwrap();
        let md = Model3dRequest { op: Some(Model3dOp::Mode), layer: Some(layer.clone()), mode: Some(RenderMode3d::Wire), color: Some("#ff8800".into()), opacity: Some(0.25), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &md).unwrap().unwrap()).unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert_eq!((m.render.mode, m.render.color.as_deref(), l.opacity), (RenderMode3d::Wire, Some("#ff8800"), 0.25));
        assert_eq!(m.camera.yaw_deg, -40.0);
        let clear = Model3dRequest { op: Some(Model3dOp::Mode), layer: Some(layer.clone()), color: Some(String::new()), ..Default::default() };
        s.apply(edit_command(&s.project, &s.blobs, &clear).unwrap().unwrap()).unwrap();
        assert_eq!(layer_params(&s.project, &layer).unwrap().1.render.color, None);
        // campos da op errada são erro claro
        let bad = Model3dRequest { op: Some(Model3dOp::Transform), layer: Some(layer.clone()), yaw_deg: Some(1.0), ..Default::default() };
        assert!(edit_command(&s.project, &s.blobs, &bad).is_err());
        // nada mudou = sem comando
        let same = Model3dRequest { op: Some(Model3dOp::Transform), layer: Some(layer.clone()), pos: Some([1.0, 0.0, 0.0]), ..Default::default() };
        assert!(edit_command(&s.project, &s.blobs, &same).unwrap().is_none());
        let v = list(&s.project);
        assert_eq!(v["models"].as_array().unwrap().len(), 1);
        assert_eq!(v["layers"].as_array().unwrap().len(), 1);
    }
}

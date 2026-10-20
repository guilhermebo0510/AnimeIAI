//! Camada 3D de referência (S24 3D): importar modelo (glTF/GLB/OBJ → GLB normalizado num ativo `model3d`), criar a
//! camada `model3d` com câmera que enquadra o modelo, listar, e mudar transformação, câmera e aparência. Tudo vira
//! comando do núcleo (um passo de undo), igual para a UI, a bridge/MCP e o headless.
//!
//! Regra das chaves: uma propriedade que já tem chaves (`m3d.*` no `KeyTracks` da camada) recebe chave no quadro do
//! pedido; as outras mudam o valor-base. `key: true` força chave em todas as propriedades mudadas.
//!
//! Pose (S24 3D, fases 8 e 9): `mannequin` cria o Manequim (ativo embutido, reusado) com uma camada em pé; `pose` muda a
//! pose (pronta, inteira, por osso, espelhar, IK de mão/pé), com a mesma regra das chaves (`m3d.pose`); `poses` lista as
//! poses prontas, os ossos e a pose atual.

use crate::context::{ToolError, ToolResult};
use aiai_3d_model::camera::framing_camera;
use aiai_3d_model::{ModelInfo, glb, import_path, mannequin, pose as mpose};
use aiai_core::model3d::{KEY_PROPS, Model3dParams, Model3dState, OrbitCam3d, POSE_KEY, Pose3d, PoseParams, Render3d, RenderMode3d, Transform3d, hex_rgb, parse_hex_rgb, state_prop};
use std::collections::BTreeMap;
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
    /// Cria o Manequim posável (ativo embutido, reusado no projeto) e, com `cut`, a camada em pé no topo do corte.
    Mannequin,
    /// Muda a pose de uma camada com esqueleto (Manequim ou modelo com pele).
    Pose,
    /// Lista as poses prontas, os ossos posáveis e a pose atual da camada (não altera nada).
    Poses,
}

/// IK de mão/pé: leva a junta `bone` até `target` (metros, mundo da camada).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IkRequest {
    /// leftHand | rightHand | leftFoot | rightFoot (ou outro osso com pai e avô).
    pub bone: String,
    pub target: [f32; 3],
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
    /// pose: pose pronta (stand | walk | run | sit | point | tpose), aplicada antes das outras mudanças.
    #[serde(default)]
    pub preset: Option<String>,
    /// pose: pose inteira {rotations: {osso: [x,y,z,w]}, hips: [x,y,z]} (substitui a atual).
    #[serde(default)]
    pub pose: Option<Pose3d>,
    /// pose: rotação de ossos em graus [x, y, z] (Euler X→Y→Z nos eixos do mundo, delta sobre o repouso); substitui a
    /// rotação desses ossos. Ex.: {"leftUpperArm": [0, 0, -70]} baixa o braço esquerdo.
    #[serde(default)]
    pub bones: Option<BTreeMap<String, [f32; 3]>>,
    /// pose: deslocamento do quadril em metros [x, y, z] (y negativo abaixa o corpo).
    #[serde(default)]
    pub hips: Option<[f32; 3]>,
    /// pose: true = espelha a pose (esquerda ↔ direita) depois das outras mudanças.
    #[serde(default)]
    pub mirror: Option<bool>,
    /// pose: true = volta ao repouso (T) antes das outras mudanças.
    #[serde(default)]
    pub reset: Option<bool>,
    /// pose: IK de mão/pé (aplicado por último).
    #[serde(default)]
    pub ik: Option<IkRequest>,
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
    let params = Model3dParams { version: 1, asset: asset.clone(), model_hash: hash.clone(), transform: Transform3d::default(), camera, render: Render3d::default(), pose: None };
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
        Model3dOp::Import | Model3dOp::List | Model3dOp::Mannequin | Model3dOp::Pose | Model3dOp::Poses => Err(ToolError::Invalid("op sem comando de edição".into())),
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

// ------------------------------------------------------------------------------------------------------------ pose

/// Ativo do Manequim (o embutido da versão atual) que já está no projeto.
pub fn find_mannequin(p: &Project) -> Option<(Id, BlobHash)> {
    p.nodes.iter().find_map(|(id, n)| match &n.data {
        NodeKind::Asset(a)
            if a.asset == AssetType::Model3d
                && a.params.get("builtin").and_then(Value::as_str) == Some(mannequin::BUILTIN)
                && a.params.get("builtin_version").and_then(Value::as_u64) == Some(mannequin::VERSION as u64) =>
        {
            a.blob.clone().map(|b| (id.clone(), b))
        }
        _ => None,
    })
}

/// Cria (ou reusa) o ativo do Manequim e, com `cut`, a camada dele em pé no topo do corte (um passo de undo).
pub fn create_mannequin(p: &Project, ids: &mut IdGen, blobs: &mut BlobStore, cut: Option<&Id>, name: Option<&str>) -> ToolResult<ImportOutput> {
    let model = mannequin::model();
    let mut commands = Vec::new();
    let (asset, hash) = match find_mannequin(p) {
        Some((a, h)) if blobs.contains(&h) => (a, h),
        _ => {
            let hash = blobs.put(glb::write_glb(&model));
            let asset = ids.next();
            commands.push(Command::CreateNode {
                id: Some(asset.clone()),
                parent: None,
                index: None,
                name: mannequin::NAME.into(),
                data: NodeKind::Asset(AssetData {
                    asset: AssetType::Model3d,
                    blob: Some(hash.clone()),
                    params: json!({ "model3d": model.info, "builtin": mannequin::BUILTIN, "builtin_version": mannequin::VERSION }),
                    source: "procedural: Manequim do AnimeIAI (base de personagem do ANIGO)".into(),
                }),
            });
            (asset, hash)
        }
    };
    let mut layer = None;
    if let Some(cut) = cut {
        let skin = model.skin.as_ref().ok_or_else(|| ToolError::Invalid("Manequim sem esqueleto".into()))?;
        let stand = mpose::preset("stand", skin).unwrap_or_default();
        // enquadra o boneco em pé (não a caixa larga da pose em T)
        let standing = mpose::apply(&model, &stand);
        let mut ld = layer_data_for(p, cut, &asset, &hash, &standing.info)?;
        let mut params = Model3dParams::from_layer_params(&ld.params).ok_or_else(|| ToolError::Invalid("camada sem params.model3d".into()))?.map_err(ToolError::Invalid)?;
        params.pose = Some(PoseParams { base: stand, keyed: Vec::new() });
        ld.params = json!({ "model3d": params });
        let id = ids.next();
        commands.push(Command::CreateNode { id: Some(id.clone()), parent: Some(cut.clone()), index: None, name: format!("3D: {}", name.unwrap_or(mannequin::NAME)), data: NodeKind::Layer(ld) });
        layer = Some(id);
    }
    if commands.is_empty() {
        return Err(ToolError::Invalid("o Manequim já está no projeto; dê `cut` para criar a camada dele".into()));
    }
    Ok(ImportOutput { commands, asset, layer, info: model.info.clone() })
}

/// Camada com esqueleto: dados, parâmetros, modelo e a pose no quadro.
pub fn pose_ctx(p: &Project, blobs: &BlobStore, layer: &Id, frame: u32) -> ToolResult<(LayerData, Model3dParams, std::sync::Arc<aiai_3d_model::Model>, Pose3d)> {
    let (l, params) = layer_params(p, layer)?;
    let model = aiai_3d_model::load_model(blobs, &params.model_hash).map_err(ToolError::Invalid)?;
    if model.skin.is_none() {
        return Err(ToolError::Invalid("o modelo desta camada não tem esqueleto (use o Manequim ou um modelo .glb com pele)".into()));
    }
    let cur = params.pose.as_ref().map(|pp| pp.eval(&l.keys, frame as f32)).unwrap_or_default();
    Ok((l, params, model, cur))
}

/// Tira de `pose.keyed` as poses que nenhuma chave usa e renumera as chaves (ordem preservada).
fn compact_keyed(pp: &mut PoseParams, keys: &mut KeyTracks) {
    let Some(track) = keys.0.get_mut(POSE_KEY) else {
        pp.keyed.clear();
        return;
    };
    let mut map: BTreeMap<usize, usize> = BTreeMap::new();
    let mut kept = Vec::new();
    for k in track.iter() {
        let i = k.value.round();
        if i >= 0.0 && (i as usize) < pp.keyed.len() && !map.contains_key(&(i as usize)) {
            map.insert(i as usize, kept.len());
            kept.push(pp.keyed[i as usize].clone());
        }
    }
    for k in track.iter_mut() {
        let i = k.value.round();
        if i >= 0.0 {
            if let Some(n) = map.get(&(i as usize)) {
                k.value = *n as f32;
            }
        }
    }
    pp.keyed = kept;
}

/// Comando que grava `new` como a pose da camada no quadro: chave `m3d.pose` se a pose já é animada (ou `force_key`),
/// senão a pose-base. A camada inteira é trocada (um passo de undo exato, sem o "merge" que não remove ossos). `None`
/// se nada mudou.
pub fn pose_command(p: &Project, layer: &Id, frame: u32, new: &Pose3d, force_key: bool) -> ToolResult<Option<Command>> {
    new.validate().map_err(ToolError::Invalid)?;
    let (l, mut params) = layer_params(p, layer)?;
    let mut pp = params.pose.clone().unwrap_or_default();
    let mut keys = l.keys.clone();
    let animated = keys.has(POSE_KEY);
    let unchanged = pp.eval(&keys, frame as f32) == *new;
    if unchanged && (!(force_key || animated) || keys.frames(POSE_KEY).contains(&frame)) {
        return Ok(None);
    }
    if force_key || animated {
        let track = keys.0.get(POSE_KEY).cloned().unwrap_or_default();
        let at = track.iter().find(|k| k.frame == frame).map(|k| k.value.round());
        let shared = |i: f32| track.iter().filter(|k| k.value.round() == i).count() > 1;
        match at {
            Some(i) if i >= 0.0 && (i as usize) < pp.keyed.len() && !shared(i) => pp.keyed[i as usize] = new.clone(),
            _ => {
                pp.keyed.push(new.clone());
                keys.set_key(POSE_KEY, frame, (pp.keyed.len() - 1) as f32);
            }
        }
        compact_keyed(&mut pp, &mut keys);
    } else {
        pp.base = new.clone();
    }
    params.pose = Some(pp);
    params.validate().map_err(ToolError::Invalid)?;
    let mut nl = l.clone();
    if !nl.params.is_object() {
        nl.params = json!({});
    }
    nl.params["model3d"] = serde_json::to_value(&params).map_err(|e| ToolError::Invalid(e.to_string()))?;
    nl.keys = keys;
    Ok(Some(Command::ReplaceData { id: layer.clone(), data: NodeKind::Layer(nl) }))
}

/// Pose pedida por `op: pose` a partir da atual: repouso, pronta, inteira, por osso, quadril, espelho e IK, nessa ordem.
pub fn requested_pose(model: &aiai_3d_model::Model, layer_xf: &Transform3d, cur: &Pose3d, req: &Model3dRequest) -> ToolResult<Pose3d> {
    let skin = model.skin.as_ref().ok_or_else(|| ToolError::Invalid("modelo sem esqueleto".into()))?;
    let mut pose = if req.reset == Some(true) { Pose3d::default() } else { cur.clone() };
    if let Some(name) = &req.preset {
        pose = mpose::preset(name, skin).ok_or_else(|| ToolError::Invalid(format!("pose pronta '{name}' desconhecida (use: {})", mpose::PRESETS.join(", "))))?;
    }
    if let Some(pz) = &req.pose {
        pz.validate().map_err(ToolError::Invalid)?;
        pose = pz.clone();
    }
    if let Some(b) = &req.bones {
        for (n, e) in b {
            if skin.index(n).is_none() {
                return Err(ToolError::Invalid(format!("osso '{n}' não existe neste esqueleto (veja op poses)")));
            }
            if !e.iter().all(|x| x.is_finite()) {
                return Err(ToolError::Invalid(format!("rotação de '{n}' com valor não finito")));
            }
            pose.set_rotation(n, aiai_3d_model::camera::rotation_quat(*e).to_array());
        }
    }
    if let Some(h) = req.hips {
        pose.hips = h;
    }
    if req.mirror == Some(true) {
        pose = pose.mirrored();
    }
    if let Some(ik) = &req.ik {
        let end = skin.index(&ik.bone).ok_or_else(|| ToolError::Invalid(format!("osso '{}' não existe neste esqueleto", ik.bone)))?;
        if !ik.target.iter().all(|x| x.is_finite()) {
            return Err(ToolError::Invalid("ik.target com valor não finito".into()));
        }
        // alvo no mundo da camada → espaço do modelo
        let inv = aiai_3d_model::camera::model_matrix(layer_xf).inverse();
        let t = inv.transform_point3(glam::Vec3::from(ik.target));
        pose = mpose::solve_ik(skin, &pose, end, t).ok_or_else(|| ToolError::Invalid(format!("'{}' não tem dois ossos acima para o IK", ik.bone)))?;
    }
    pose.validate().map_err(ToolError::Invalid)?;
    Ok(pose)
}

/// `op: pose`: o comando (ou `None` se nada mudou).
pub fn pose_edit(p: &Project, blobs: &BlobStore, req: &Model3dRequest) -> ToolResult<Option<Command>> {
    let layer = req.layer.as_ref().ok_or_else(|| ToolError::Invalid("falta `layer` (camada 3D com esqueleto)".into()))?;
    let frame = req.frame.unwrap_or(0);
    let (l, params, model, cur) = pose_ctx(p, blobs, layer, frame)?;
    let st = params.eval(&l.keys, frame as f32);
    let new = requested_pose(&model, &st.transform, &cur, req)?;
    pose_command(p, layer, frame, &new, req.key.unwrap_or(false))
}

/// `op: poses`: poses prontas e, com `layer`, os ossos posáveis, a pose no quadro e os quadros com chave de pose.
pub fn poses_json(p: &Project, blobs: &BlobStore, layer: Option<&Id>, frame: u32) -> ToolResult<Value> {
    let mut out = json!({ "presets": mpose::PRESETS, "ik_bones": mpose::IK_ENDS });
    if let Some(layer) = layer {
        let (l, params, model, cur) = pose_ctx(p, blobs, layer, frame)?;
        let skin = model.skin.as_ref().ok_or_else(|| ToolError::Invalid("modelo sem esqueleto".into()))?;
        let st = params.eval(&l.keys, frame as f32);
        // juntas posáveis no mundo da camada (para pedir IK com alvo perto delas)
        let g = mpose::globals(skin, &cur);
        let mm = aiai_3d_model::camera::model_matrix(&st.transform);
        let joints: Vec<Value> = mpose::pose_bones(skin).into_iter().map(|i| json!({ "bone": skin.bones[i].name, "world": mm.transform_point3(g[i].pos).to_array() })).collect();
        out["layer"] = json!(layer);
        out["frame"] = json!(frame);
        out["pose"] = json!(cur);
        out["joints"] = json!(joints);
        out["bones"] = json!(skin.bones.iter().map(|b| b.name.as_str()).collect::<Vec<_>>());
        out["pose_key_frames"] = json!(l.keys.frames(POSE_KEY));
    }
    Ok(out)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn obj_file() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aiai_m3d_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // um arquivo por chamada: os testes rodam em paralelo e não podem ler o arquivo de outro pela metade
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let p = dir.join(format!("caixa{}.obj", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
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

    #[test]
    fn mannequin_layer_poses_keys_and_undo() {
        let (mut s, cut) = session();
        let out = create_mannequin(&s.project, &mut s.ids, &mut s.blobs, Some(&cut), None).unwrap();
        s.apply(out.batch("Manequim")).unwrap();
        let layer = out.layer.clone().unwrap();
        let (_, m) = layer_params(&s.project, &layer).unwrap();
        assert!(m.pose.as_ref().is_some_and(|p| !p.base.is_rest()), "o Manequim nasce em pé");
        // uma segunda camada reusa o mesmo ativo
        let again = create_mannequin(&s.project, &mut s.ids, &mut s.blobs, Some(&cut), None).unwrap();
        assert_eq!(again.asset, out.asset);
        assert_eq!(again.commands.len(), 1, "só a camada nova");
        // a camada renderiza (CPU) e a pose muda o desenho
        let opts = aiai_render::RenderOptions { include_reference: true, camera: false, ..Default::default() };
        let px = |s: &CoreSession| aiai_render::render_frame(&s.project, &s.blobs, &cut, 0, &opts).unwrap().image.data.iter().filter(|p| p[3] > 0.5).count();
        let standing = px(&s);
        assert!(standing > 50, "{standing}");
        // pose pronta como base
        let req = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), preset: Some("sit".into()), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &req).unwrap().unwrap()).unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert!(!l.keys.has(POSE_KEY));
        assert!(m.pose.as_ref().unwrap().base.hips[1] < -0.2);
        assert_ne!(px(&s), standing);
        // chaves: quadro 0 em pé, quadro 10 correndo; no meio, mistura
        let k0 = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), frame: Some(0), key: Some(true), preset: Some("stand".into()), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &k0).unwrap().unwrap()).unwrap();
        let k10 = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), frame: Some(10), preset: Some("run".into()), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &k10).unwrap().unwrap()).unwrap();
        let (l, m) = layer_params(&s.project, &layer).unwrap();
        assert_eq!(l.keys.frames(POSE_KEY), vec![0, 10]);
        assert_eq!(m.pose.as_ref().unwrap().keyed.len(), 2);
        let mid = m.eval(&l.keys, 5.0).pose.unwrap();
        let run = m.eval(&l.keys, 10.0).pose.unwrap();
        assert!(mid.hips[1] > run.hips[1] && mid.hips[1] < 0.0, "quadril no meio do caminho: {:?} {:?}", mid.hips, run.hips);
        // regravar a chave do quadro 10 troca a pose no lugar (sem pose órfã)
        let k10b = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), frame: Some(10), preset: Some("point".into()), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &k10b).unwrap().unwrap()).unwrap();
        let (_, m) = layer_params(&s.project, &layer).unwrap();
        assert_eq!(m.pose.as_ref().unwrap().keyed.len(), 2);
        // por osso + espelho + IK
        let b = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), frame: Some(10), bones: Some([("leftUpperArm".to_string(), [0.0, 0.0, -10.0])].into()), mirror: Some(true), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &b).unwrap().unwrap()).unwrap();
        let j = poses_json(&s.project, &s.blobs, Some(&layer), 10).unwrap();
        let hand = j["joints"].as_array().unwrap().iter().find(|x| x["bone"] == "leftHand").unwrap()["world"].clone();
        let hand: [f32; 3] = serde_json::from_value(hand).unwrap();
        let sh: [f32; 3] = serde_json::from_value(j["joints"].as_array().unwrap().iter().find(|x| x["bone"] == "leftUpperArm").unwrap()["world"].clone()).unwrap();
        // ao alcance: entre o ombro e a mão, um pouco acima
        let target = [(hand[0] + sh[0]) * 0.5, (hand[1] + sh[1]) * 0.5 + 0.08, (hand[2] + sh[2]) * 0.5 + 0.05];
        let ik = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), frame: Some(10), ik: Some(IkRequest { bone: "leftHand".into(), target }), ..Default::default() };
        s.apply(pose_edit(&s.project, &s.blobs, &ik).unwrap().unwrap()).unwrap();
        let j = poses_json(&s.project, &s.blobs, Some(&layer), 10).unwrap();
        let got: [f32; 3] = serde_json::from_value(j["joints"].as_array().unwrap().iter().find(|x| x["bone"] == "leftHand").unwrap()["world"].clone()).unwrap();
        assert!((0..3).all(|k| (got[k] - target[k]).abs() < 0.01), "IK: {got:?} → {target:?}");
        // erros claros
        let bad = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), preset: Some("voar".into()), ..Default::default() };
        assert!(pose_edit(&s.project, &s.blobs, &bad).unwrap_err().to_string().contains("stand"));
        let bad = Model3dRequest { op: Some(Model3dOp::Pose), layer: Some(layer.clone()), bones: Some([("asa".to_string(), [0.0; 3])].into()), ..Default::default() };
        assert!(pose_edit(&s.project, &s.blobs, &bad).is_err());
        // cada pose é um passo de undo
        s.undo().unwrap();
        let j = poses_json(&s.project, &s.blobs, Some(&layer), 10).unwrap();
        let back: [f32; 3] = serde_json::from_value(j["joints"].as_array().unwrap().iter().find(|x| x["bone"] == "leftHand").unwrap()["world"].clone()).unwrap();
        assert!((0..3).any(|k| (back[k] - target[k]).abs() > 0.02));
        // camada de modelo rígido: pose é erro claro
        let req = Model3dRequest { op: Some(Model3dOp::Import), path: Some(obj_file().display().to_string()), cut: Some(cut.clone()), ..Default::default() };
        let o = import(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
        s.apply(o.batch("Importar")).unwrap();
        let rigid = Model3dRequest { op: Some(Model3dOp::Pose), layer: o.layer.clone(), preset: Some("stand".into()), ..Default::default() };
        assert!(pose_edit(&s.project, &s.blobs, &rigid).unwrap_err().to_string().contains("esqueleto"));
    }

}

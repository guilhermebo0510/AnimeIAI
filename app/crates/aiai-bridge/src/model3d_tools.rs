//! Camada 3D de referência (S24 3D): `MODEL3D` importa um modelo (.glb/.gltf/.obj) para um ativo e cria a camada 3D,
//! lista os modelos e camadas, e muda transformação, câmera e aparência. Cada mudança é UM passo de undo; a camada é
//! renderizada pelo serviço da GPU no app ou pelo rasterizador CPU no headless. A lógica fica em `aiai_tools::model3d`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::model3d::{self, Model3dOp, Model3dRequest};
use serde_json::{Value, json};

pub const MODEL3D_ACTIONS: &[&str] = &["MODEL3D"];

pub fn is_mutating(action: &str) -> bool {
    action == "MODEL3D"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "MODEL3D" => model3d_action(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn merge(mut v: Value, extra: Value) -> Value {
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

fn model3d_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: Model3dRequest = request(params, "MODEL3D")?;
    let op = r.op.ok_or_else(|| BridgeError::invalid_params("MODEL3D: falta `op` (import | list | transform | camera | mode | mannequin | pose | poses)"))?;
    match op {
        Model3dOp::List => Ok(model3d::list(&session.project)),
        Model3dOp::Poses => model3d::poses_json(&session.project, &session.blobs, r.layer.as_ref(), r.frame.unwrap_or(0)).map_err(tool_error),
        Model3dOp::Mannequin => {
            let out = model3d::create_mannequin(&session.project, &mut session.ids, &mut session.blobs, r.cut.as_ref(), r.name.as_deref()).map_err(tool_error)?;
            let cmd = out.batch("Manequim 3D");
            let v = apply(session, params, cmd)?;
            let state = out.layer.as_ref().map(|l| model3d::state_json(&session.project, l, 0));
            Ok(merge(v, json!({ "asset": out.asset, "layer": out.layer, "info": out.info, "layer_state": state })))
        }
        Model3dOp::Pose => {
            let layer = r.layer.clone().ok_or_else(|| BridgeError::invalid_params("MODEL3D: falta `layer` (camada 3D com esqueleto)"))?;
            let frame = r.frame.unwrap_or(0);
            let cmd = model3d::pose_edit(&session.project, &session.blobs, &r).map_err(tool_error)?;
            let after = |s: &CoreSession| model3d::poses_json(&s.project, &s.blobs, Some(&layer), frame).unwrap_or_else(|e| json!({ "error": e.to_string() }));
            match cmd {
                Some(cmd) => {
                    let v = apply(session, params, cmd)?;
                    let extra = after(session);
                    Ok(merge(v, extra))
                }
                None => Ok(merge(json!({ "changed": false }), after(session))),
            }
        }
        Model3dOp::Import => {
            if let Some(p) = &r.path {
                let path = std::path::Path::new(p);
                if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                    return Err(BridgeError::invalid_params("'path' deve ser um caminho absoluto sem '..'"));
                }
            }
            let out = model3d::import(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
            let cmd = out.batch("Importar modelo 3D");
            let v = apply(session, params, cmd)?;
            let state = out.layer.as_ref().map(|l| model3d::state_json(&session.project, l, 0));
            Ok(merge(v, json!({ "asset": out.asset, "layer": out.layer, "info": out.info, "layer_state": state })))
        }
        Model3dOp::Transform | Model3dOp::Camera | Model3dOp::Mode => {
            let layer = r.layer.clone().ok_or_else(|| BridgeError::invalid_params("MODEL3D: falta `layer` (camada 3D)"))?;
            let frame = r.frame.unwrap_or(0);
            let cmd = model3d::edit_command(&session.project, &session.blobs, &r).map_err(tool_error)?;
            let after = |s: &CoreSession| model3d::state_json(&s.project, &layer, frame);
            match cmd {
                Some(cmd) => {
                    let v = apply(session, params, cmd)?;
                    let extra = after(session);
                    Ok(merge(v, extra))
                }
                None => Ok(merge(json!({ "changed": false }), after(session))),
            }
        }
    }
}

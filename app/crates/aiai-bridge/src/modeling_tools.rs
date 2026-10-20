//! Modelagem por comando (S24 3D, fase 12): `MODELING` cria e edita modelos simples (primitivas, extrude, inset, mover e
//! escalar faces, bevel, modificadores, material) guardados num ativo 3D editável do projeto; cada mudança é UM passo de
//! undo e regrava o GLB que a camada 3D usa. `send` cria a camada no corte; `export` grava o .glb num caminho. A lógica
//! fica em `aiai_tools::modeling`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::modeling::{self, ModelingOp, ModelingRequest};
use serde_json::{Value, json};

pub const MODELING_ACTIONS: &[&str] = &["MODELING"];

pub fn is_mutating(action: &str) -> bool {
    action == "MODELING"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "MODELING" => modeling_action(session, params),
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

fn modeling_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: ModelingRequest = request(params, "MODELING")?;
    let op = r.op.ok_or_else(|| BridgeError::invalid_params("MODELING: falta `op` (new | info | list | add_primitive | transform | extrude | inset | move_faces | scale_faces | bevel | merge_by_distance | modifier | material | delete_object | send | export)"))?;
    let need_asset = || r.asset.clone().ok_or_else(|| BridgeError::invalid_params("MODELING: falta `asset` (o modelo; crie com op new)"));
    match op {
        ModelingOp::List => {
            let list: Vec<Value> = modeling::editable_assets(&session.project).into_iter().map(|(id, name, objs)| json!({ "asset": id, "name": name, "objects": objs })).collect();
            Ok(json!({ "models": list }))
        }
        ModelingOp::Info => {
            let a = need_asset()?;
            let d = modeling::asset_doc(&session.project, &a).map_err(tool_error)?;
            Ok(merge(json!({ "asset": a }), modeling::info_json(&d)))
        }
        ModelingOp::Send => {
            let a = need_asset()?;
            let cut = r.cut.clone().ok_or_else(|| BridgeError::invalid_params("MODELING send: falta `cut` (corte onde criar a camada 3D)"))?;
            modeling::asset_doc(&session.project, &a).map_err(tool_error)?;
            let req = aiai_tools::model3d::Model3dRequest { op: Some(aiai_tools::model3d::Model3dOp::Import), asset: Some(a.clone()), cut: Some(cut), ..Default::default() };
            let out = aiai_tools::model3d::import(&session.project, &mut session.ids, &mut session.blobs, &req).map_err(tool_error)?;
            let v = apply(session, params, out.batch("Enviar ao Layout"))?;
            Ok(merge(v, json!({ "asset": a, "layer": out.layer })))
        }
        ModelingOp::Export => {
            let a = need_asset()?;
            let path = r.path.clone().ok_or_else(|| BridgeError::invalid_params("MODELING export: falta `path` (absoluto, .glb)"))?;
            let p = std::path::Path::new(&path);
            if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) || p.extension().map(|e| e.to_string_lossy().to_lowercase()) != Some("glb".into()) {
                return Err(BridgeError::invalid_params("'path' deve ser um caminho absoluto .glb sem '..'"));
            }
            let d = modeling::asset_doc(&session.project, &a).map_err(tool_error)?;
            let bytes = d.to_glb().map_err(BridgeError::invalid_params)?;
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| BridgeError::failed(format!("{}: {e}", dir.display())))?;
            }
            std::fs::write(p, &bytes).map_err(|e| BridgeError::failed(format!("{path}: {e}")))?;
            Ok(json!({ "asset": a, "path": path, "bytes": bytes.len() }))
        }
        _ => {
            let existing = if op == ModelingOp::New { None } else { Some(need_asset()?) };
            let mut d = match &existing {
                Some(a) => modeling::asset_doc(&session.project, a).map_err(tool_error)?,
                None => modeling::ModelDoc::new(r.name.as_deref().filter(|n| !n.trim().is_empty()).unwrap_or("Modelo")),
            };
            let summary = modeling::apply_op(&mut d, &r).map_err(tool_error)?;
            if op == ModelingOp::New && d.objects.is_empty() {
                return Err(BridgeError::invalid_params("MODELING new: dê `primitive` (o modelo nasce com um objeto)"));
            }
            let (cmd, id, info) = modeling::save_command(&session.project, &mut session.ids, &mut session.blobs, existing.as_ref(), &d).map_err(tool_error)?;
            let v = apply(session, params, cmd)?;
            Ok(merge(v, json!({ "asset": id, "result": summary, "info": info, "objects": d.objects.iter().map(|o| json!({ "name": o.name, "faces": o.mesh.face_count(), "verts": o.mesh.vert_count() })).collect::<Vec<_>>() })))
        }
    }
}

//! Balde do S25 (à moda do Clip Studio): `FILL_AREA {cut, frame, layer, mode, color, ...}` faz no documento o que os
//! jeitos novos do balde fazem no canvas: `editing` (as cores da camada — ou de todas, `refer_all` — delimitam o clique
//! `point`, com tolerância, fechar intervalo e dimensionamento de área; pixels na raster), `enclose` (as regiões fechadas
//! que o laço `path` cerca) e `unfilled` (as regiões sem tinta que o pincel `path` toca). Um pedido = um passo de undo.
//! A lógica fica em `aiai_tools::fill_area`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::fill_area::{FillAreaRequest, fill_area};
use serde_json::Value;

pub const FILL_AREA_ACTIONS: &[&str] = &["FILL_AREA"];

pub fn is_mutating(action: &str) -> bool {
    action == "FILL_AREA"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "FILL_AREA" => {
            let r: FillAreaRequest = request(params, "FILL_AREA")?;
            let plan = fill_area(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            let mut out = apply(session, params, plan.command)?;
            if let Some(o) = out.as_object_mut() {
                o.insert("regions".into(), Value::from(plan.regions));
            }
            Ok(out)
        }
        _ => Err(BridgeError::unknown_action(action)),
    }
}

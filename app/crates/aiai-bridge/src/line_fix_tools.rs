//! Corrigir linha (S25, à moda do Clip Studio): `LINE_FIX {cut, frame, layer, op, ...}` faz no documento o que as
//! ferramentas do grupo Corrigir linha fazem no canvas: puxar linha (`pinch`), simplificar debaixo do pincel
//! (`simplify`), conectar pontas (`connect`), ajustar largura (`width`), redesenhar linha (`redraw`) e largura
//! (`redraw_width`) e remover poeira (`dust`, raster ou vetorial). Coordenadas em px do canvas (o movimento da camada é
//! desfeito); cada pedido é UM passo de undo. `dry_run` devolve o relatório sem mudar nada. A lógica fica em
//! `aiai_tools::line_fix`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::line_fix::{LineFixRequest, line_fix_plan};
use serde_json::{Value, json};

pub const LINE_FIX_ACTIONS: &[&str] = &["LINE_FIX"];

pub fn is_mutating(action: &str) -> bool {
    action == "LINE_FIX"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "LINE_FIX" => line_fix_action(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

/// Devolve o Outcome (sem comando quando nada mudou, ou no ensaio) + o relatório: `changed`/`removed` (ids de traço),
/// `removed_points`, `joins`, `dust` e `dust_boxes`.
fn line_fix_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: LineFixRequest = request(params, "LINE_FIX")?;
    let plan = line_fix_plan(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
    let report = serde_json::to_value(&plan.report).map_err(|e| BridgeError::failed(e.to_string()))?;
    let mut out = match plan.command {
        Some(cmd) => apply(session, params, cmd)?,
        None => json!({ "changed_document": false, "dry_run": r.dry_run, "revision": session.project.revision }),
    };
    if let (Some(o), Some(e)) = (out.as_object_mut(), report.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    Ok(out)
}

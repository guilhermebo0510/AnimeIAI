//! Réguas editáveis (S24 2.1–2.2): `RULER` cria, edita, lista, ativa, apaga e assa as réguas (perspectiva de 1/2/3
//! pontos, paralelas, radial, simetria, grade em perspectiva) guardadas em `params.rulers` de uma camada de guia. São as
//! mesmas que o app desenha por cima do canvas e em que a caneta e o pincel encaixam; mudar é UM passo de undo. A lógica
//! fica em `aiai_tools::ruler`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::ruler::{RulerRequest, ruler_plan};
use serde_json::{Value, json};

pub const RULER_ACTIONS: &[&str] = &["RULER"];

pub fn is_mutating(action: &str) -> bool {
    action == "RULER"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "RULER" => ruler_action(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

/// `list` não muda nada e devolve as réguas; as outras aplicam o comando e devolvem o Outcome + `rulers` (como ficaram),
/// `active` e `id` (a régua criada/editada).
fn ruler_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: RulerRequest = request(params, "RULER")?;
    let plan = ruler_plan(&session.project, &r).map_err(tool_error)?;
    let extra = json!({ "rulers": plan.set.items, "active": plan.set.active_ruler().map(|x| x.id.clone()), "id": plan.id });
    let Some(cmd) = plan.command else { return Ok(extra) };
    let mut v = apply(session, params, cmd)?;
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    Ok(v)
}

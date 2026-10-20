//! Biblioteca de recursos (S24 2.7–2.8): `LIBRARY` lista/busca, importa, usa no projeto, salva do projeto, edita metadados,
//! duplica e apaga (lixeira) os recursos de `Documentos\AnimeIAI\{Modelos 3D, Materiais, Texturas, Paletas, Poses, Pincéis}`.
//! Usar um recurso muda o documento (UM passo de undo); o resto só mexe na pasta. A lógica fica em `aiai_tools::library`
//! e o índice/importação em `aiai-assets`. Sem sessão (`--no-session`) e sem `AIAI_LIBRARY_DIR` a biblioteca está desligada:
//! `list` responde `enabled: false` e as outras operações recusam, sem tocar em disco.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::CoreSession;
use aiai_tools::library::{LibraryOp, LibraryRequest, run};
use serde_json::{Value, json};

pub const LIBRARY_ACTIONS: &[&str] = &["LIBRARY"];

pub fn is_mutating(action: &str) -> bool {
    action == "LIBRARY"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "LIBRARY" => library_action(session, params),
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

fn library_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: LibraryRequest = request(params, "LIBRARY")?;
    let op = r.op.ok_or_else(|| BridgeError::invalid_params("LIBRARY: falta `op` (list | search | import | use | save | update | duplicate | delete)"))?;
    let Some(root) = aiai_assets::root() else {
        return match op {
            LibraryOp::List | LibraryOp::Search => Ok(json!({ "enabled": false, "total": 0, "count": 0, "items": [], "note": "biblioteca desligada (--no-session sem AIAI_LIBRARY_DIR): nada é lido nem gravado" })),
            _ => Err(BridgeError::failed("a biblioteca está desligada (--no-session sem AIAI_LIBRARY_DIR): nada é lido nem gravado")),
        };
    };
    if !matches!(op, LibraryOp::List | LibraryOp::Search) {
        // a primeira gravação cria as pastas dos tipos (a biblioteca aparece no Explorador mesmo vazia)
        aiai_assets::store::ensure_folders(&root).map_err(BridgeError::failed)?;
    }
    let out = run(&root, &session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
    match out.command {
        Some(cmd) => {
            let v = apply(session, params, cmd)?;
            Ok(merge(v, out.value))
        }
        None => Ok(out.value),
    }
}

//! LIBRARY (S24 2.7–2.8) pela bridge: desligada sem sessão e sem `AIAI_LIBRARY_DIR` (nada é lido nem gravado), e com a
//! pasta de teste: importar, listar, usar no projeto (um passo de undo), salvar do projeto, editar metadados, duplicar,
//! apagar (lixeira) e chave desconhecida = -32602. Um só teste: a variável de ambiente é do processo.

use aiai_bridge::code;
use aiai_bridge::core_ops::handle_core_action;
use aiai_core::CoreSession;
use serde_json::{Value, json};

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn err(s: &mut CoreSession, params: Value) -> aiai_bridge::BridgeError {
    handle_core_action(s, "LIBRARY", &params).expect("é do núcleo").unwrap_err()
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    act(s, "APPLY_COMMAND", json!({ "command": c }))["created"][0].as_str().unwrap().to_string()
}

#[test]
fn library_off_without_session_then_import_use_save_edit_delete() {
    let mut s = CoreSession::for_tests("lib");
    act(&mut s, "NEW_PROJECT", json!({"name": "lib", "settings": {"width": 160, "height": 90}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C1", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));

    // --- desligada: list responde enabled:false; o resto recusa; nada é criado
    assert!(aiai_assets::root().is_none(), "sem AIAI_LIBRARY_DIR e sem pastas do usuário ligadas");
    let off = act(&mut s, "LIBRARY", json!({"op": "list"}));
    assert_eq!((off["enabled"].as_bool(), off["count"].as_u64()), (Some(false), Some(0)), "{off}");
    let e = err(&mut s, json!({"op": "import", "path": "C:\\x.glb"}));
    assert_eq!(e.code, code::ACTION_FAILED, "{}", e.message);
    assert!(e.message.contains("desligada"), "{}", e.message);

    // --- com a pasta de teste
    let root = std::env::temp_dir().join(format!("aiai_bridge_lib_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let src = root.join("origem");
    std::fs::create_dir_all(&src).unwrap();
    // SAFETY: este é o único teste do binário e nenhuma outra thread lê o ambiente durante ele.
    unsafe { std::env::set_var("AIAI_LIBRARY_DIR", root.join("lib")) };
    std::fs::write(src.join("cubo.obj"), "# license: CC0\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n").unwrap();
    std::fs::write(src.join("cores.gpl"), "GIMP Palette\nName: Cores\n255 0 0 Vermelho\n0 0 255 Azul\n").unwrap();

    let imp = act(&mut s, "LIBRARY", json!({"op": "import", "path": src.join("cubo.obj").display().to_string(), "license": "CC0-1.0", "tags": ["cenário"]}));
    assert_eq!(imp["count"], 1, "{imp}");
    act(&mut s, "LIBRARY", json!({"op": "import", "path": src.join("cores.gpl").display().to_string()}));
    for kind in ["Modelos 3D", "Paletas"] {
        assert!(root.join("lib").join(kind).is_dir(), "a pasta {kind} foi criada");
    }

    let l = act(&mut s, "LIBRARY", json!({"op": "search", "query": "cubo"}));
    assert_eq!((l["total"].as_u64(), l["items"][0]["kind"].as_str(), l["items"][0]["stats"]["triangles"].as_u64()), (Some(1), Some("model3d"), Some(12)), "{l}");
    let model = l["items"][0]["id"].as_str().unwrap().to_string();
    let pal = act(&mut s, "LIBRARY", json!({"op": "list", "kind": "palette"}))["items"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(act(&mut s, "LIBRARY", json!({"op": "list", "kind": "palette"}))["items"][0]["license"], "desconhecida — verifique com o autor");

    // usar: camada 3D de referência no corte, ativo com a procedência; um passo de undo
    let u = act(&mut s, "LIBRARY", json!({"op": "use", "id": model, "cut": cut}));
    assert!(u["layer"].is_string() && u["asset"].is_string(), "{u}");
    assert!(act(&mut s, "MODEL3D", json!({"op": "list"}))["layers"].as_array().unwrap().len() == 1);
    let up = act(&mut s, "LIBRARY", json!({"op": "use", "id": pal}));
    assert!(up["palette"].is_string());
    // salvar a paleta do projeto como recurso novo
    let saved = act(&mut s, "LIBRARY", json!({"op": "save", "from": "palette", "palette": up["palette"], "name": "Cores do projeto", "license": "CC-BY-4.0", "asset_author": "Ana"}));
    assert_eq!((saved["item"]["license"].as_str(), saved["item"]["author"].as_str()), (Some("CC-BY-4.0"), Some("Ana")), "{saved}");
    // editar metadados, duplicar e apagar
    let ed = act(&mut s, "LIBRARY", json!({"op": "update", "id": model, "name": "Cubo 1m", "tags": ["a", "b"], "source": "https://exemplo.org/cubo"}));
    assert_eq!((ed["item"]["name"].as_str(), ed["item"]["source"].as_str()), (Some("Cubo 1m"), Some("https://exemplo.org/cubo")));
    let dup = act(&mut s, "LIBRARY", json!({"op": "duplicate", "id": model}));
    let del = act(&mut s, "LIBRARY", json!({"op": "delete", "id": dup["item"]["id"]}));
    assert!(std::path::Path::new(del["trash"].as_str().unwrap()).exists());

    // chave desconhecida, `op` ausente e tipo inexistente = -32602
    for bad in [json!({"op": "list", "querry": "x"}), json!({"op": "use", "id": model, "cutt": cut}), json!({"query": "x"}), json!({"op": "list", "kind": "fonte"}), json!({"op": "use", "id": "nao-existe"}), json!({"op": "import", "path": "relativo.glb"})] {
        let e = err(&mut s, bad.clone());
        assert_eq!(e.code, code::INVALID_PARAMS, "{bad}: {}", e.message);
    }
    // `author` continua sendo o autor da mudança no documento (não o do recurso)
    act(&mut s, "LIBRARY", json!({"op": "use", "id": model, "mode": "asset", "author": "agent:teste"}));

    // undo: o último passo é o uso do modelo como ativo; antes, a paleta salva não mexe no documento; o uso com camada some em um passo
    act(&mut s, "UNDO", json!({}));
    assert_eq!(act(&mut s, "MODEL3D", json!({"op": "list"}))["layers"].as_array().unwrap().len(), 1);
    unsafe { std::env::remove_var("AIAI_LIBRARY_DIR") };
    let _ = std::fs::remove_dir_all(&root);
}

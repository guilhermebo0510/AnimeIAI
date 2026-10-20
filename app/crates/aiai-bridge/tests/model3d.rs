//! MODEL3D (S24 3D) pela bridge: importar um GLB gerado no teste (dois nós), camada criada como referência, RENDER
//! com e sem `include_reference`, transform/camera/mode com chave desconhecida = -32602, e undo de um passo por mudança.

use aiai_bridge::code;
use aiai_bridge::core_ops::handle_core_action;
use aiai_core::CoreSession;
use serde_json::{Value, json};

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    let o = act(s, "APPLY_COMMAND", json!({ "command": c }));
    o["created"][0].as_str().unwrap().to_string()
}

/// GLB pequeno escrito à mão: dois nós (o segundo deslocado e girado), duas malhas (um quadrado e um triângulo).
fn tiny_glb() -> Vec<u8> {
    let quad: [f32; 12] = [-0.5, 0.0, 0.0, 0.5, 0.0, 0.0, 0.5, 1.0, 0.0, -0.5, 1.0, 0.0];
    let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let mut bin = Vec::new();
    for v in quad {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let doc = json!({
        "asset": { "version": "2.0", "extras": { "license": "CC0", "author": "teste" } },
        "scene": 0,
        "scenes": [{ "nodes": [0, 1] }],
        "nodes": [
            { "name": "placa", "mesh": 0 },
            { "name": "placa2", "mesh": 1, "translation": [1.2, 0.0, 0.0], "rotation": [0.0, 0.3826834, 0.0, 0.9238795] }
        ],
        "meshes": [
            { "name": "Q", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] },
            { "name": "T", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }
        ],
        "materials": [{ "name": "laranja", "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.4, 0.1, 1.0] } }],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-0.5, 0, 0], "max": [0.5, 1, 0] },
            { "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }, { "buffer": 0, "byteOffset": 48, "byteLength": 12 }],
        "buffers": [{ "byteLength": 60 }]
    });
    aiai_3d_model::gltf::build_glb(&doc, &bin)
}

#[test]
fn model3d_import_render_edit_and_undo() {
    let mut s = CoreSession::for_tests("m3d");
    act(&mut s, "NEW_PROJECT", json!({"name": "m3d", "settings": {"width": 160, "height": 90}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C1", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));
    let dir = std::env::temp_dir().join(format!("aiai_bridge_m3d_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("placas.glb");
    std::fs::write(&path, tiny_glb()).unwrap();

    let o = act(&mut s, "MODEL3D", json!({"op": "import", "path": path.display().to_string(), "cut": cut}));
    assert_eq!(o["can_undo"], true);
    assert_eq!(o["info"]["triangles"], 4);
    assert_eq!(o["info"]["parts"], 2);
    assert_eq!(o["info"]["license"]["license"], "CC0");
    let layer = o["layer"].as_str().unwrap().to_string();
    assert_eq!(o["layer_state"]["model3d"]["render"]["reference"], true);

    // canvas (com referência) mostra o modelo; render final (sem referência) não
    let with = act(&mut s, "RENDER", json!({"cut": cut, "include_reference": true, "camera": false, "inline": true}));
    let without = act(&mut s, "RENDER", json!({"cut": cut, "camera": false, "inline": true}));
    assert_ne!(with["png_base64"], without["png_base64"], "a referência 3D aparece só com include_reference");

    // transform com chave forçada, câmera e modo
    let t = act(&mut s, "MODEL3D", json!({"op": "transform", "layer": layer, "frame": 4, "key": true, "rot_deg": [0.0, 45.0, 0.0]}));
    assert_eq!(t["state"]["transform"]["rot_deg"][1], 45.0);
    let c = act(&mut s, "MODEL3D", json!({"op": "camera", "layer": layer, "yaw_deg": 90.0, "frame_model": true}));
    assert_eq!(c["model3d"]["camera"]["yaw_deg"], 90.0);
    let m = act(&mut s, "MODEL3D", json!({"op": "mode", "layer": layer, "mode": "silhouette", "outline_px": 0.0, "opacity": 0.4}));
    assert_eq!(m["model3d"]["render"]["mode"], "silhouette");
    let l = act(&mut s, "MODEL3D", json!({"op": "list"}));
    assert_eq!(l["models"].as_array().unwrap().len(), 1);
    assert!((l["layers"][0]["opacity"].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert_eq!(l["layers"][0]["keys"], json!(["m3d.rot.y"]));

    // chave desconhecida, op errada, arquivo inexistente e VRM são -32602
    for bad in [
        json!({"op": "transform", "layer": layer, "glow": 1}),
        json!({"op": "transform", "layer": layer, "yaw_deg": 3.0}),
        json!({"op": "import", "path": dir.join("nada.glb").display().to_string()}),
        json!({"op": "import", "path": dir.join("x.vrm").display().to_string()}),
        json!({"op": "mode", "layer": layer, "outline_color": "azul"}),
    ] {
        let e = handle_core_action(&mut s, "MODEL3D", &bad).unwrap().unwrap_err();
        assert_eq!(e.code, code::INVALID_PARAMS, "{bad}: {}", e.message);
    }

    // undo: um passo por mudança (modo, câmera, transform, importação)
    for _ in 0..4 {
        act(&mut s, "UNDO", json!({}));
    }
    let l = act(&mut s, "MODEL3D", json!({"op": "list"}));
    assert!(l["models"].as_array().unwrap().is_empty() && l["layers"].as_array().unwrap().is_empty(), "{l}");
}

/// Manequim e pose (S24 3D, fases 8–9) e modelagem por comando (fase 12) pela bridge: MODEL3D mannequin/pose/poses,
/// MODELING new/extrude/scale_faces/merge/material/send/info, export num arquivo, erros -32602 e undo.
#[test]
fn mannequin_pose_and_modeling_through_the_bridge() {
    let mut s = CoreSession::for_tests("mdl");
    act(&mut s, "NEW_PROJECT", json!({"name": "mdl", "settings": {"width": 160, "height": 90}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C1", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));

    // manequim em pé, pose pronta com chave, poses lista as juntas
    let o = act(&mut s, "MODEL3D", json!({"op": "mannequin", "cut": cut}));
    let layer = o["layer"].as_str().unwrap().to_string();
    assert!(o["info"]["bones"].as_u64().unwrap() >= 55);
    let stand = act(&mut s, "RENDER", json!({"cut": cut, "include_reference": true, "camera": false, "inline": true}));
    let p = act(&mut s, "MODEL3D", json!({"op": "pose", "layer": layer, "frame": 0, "key": true, "preset": "run"}));
    assert_eq!(p["pose_key_frames"], json!([0]));
    let run = act(&mut s, "RENDER", json!({"cut": cut, "include_reference": true, "camera": false, "inline": true}));
    assert_ne!(stand["png_base64"], run["png_base64"], "a pose muda o desenho");
    let l = act(&mut s, "MODEL3D", json!({"op": "poses", "layer": layer}));
    assert!(l["presets"].as_array().unwrap().iter().any(|x| x == "sit"));
    assert!(l["joints"].as_array().unwrap().iter().any(|j| j["bone"] == "rightHand"));
    for bad in [json!({"op": "pose", "layer": layer, "preset": "voar"}), json!({"op": "pose", "layer": layer, "bones": {"asa": [0, 0, 0]}}), json!({"op": "pose", "layer": layer, "rumo": 1})] {
        let e = handle_core_action(&mut s, "MODEL3D", &bad).unwrap().unwrap_err();
        assert_eq!(e.code, code::INVALID_PARAMS, "{bad}: {}", e.message);
    }

    // casa por comandos
    let n = act(&mut s, "MODELING", json!({"op": "new", "name": "Casa", "primitive": "cube", "pos": [0.0, 0.5, 0.0]}));
    let asset = n["asset"].as_str().unwrap().to_string();
    act(&mut s, "MODELING", json!({"op": "extrude", "asset": asset, "faces": "top", "distance": 0.6}));
    act(&mut s, "MODELING", json!({"op": "scale_faces", "asset": asset, "faces": "top", "factor": [0.0, 1.0, 1.0]}));
    let m = act(&mut s, "MODELING", json!({"op": "merge_by_distance", "asset": asset}));
    assert_eq!(m["result"]["merged"], 2);
    act(&mut s, "MODELING", json!({"op": "material", "asset": asset, "color": "#c86448"}));
    let info = act(&mut s, "MODELING", json!({"op": "info", "asset": asset}));
    assert_eq!(info["objects"][0]["verts"], 10);
    let sent = act(&mut s, "MODELING", json!({"op": "send", "asset": asset, "cut": cut}));
    assert!(sent["layer"].is_string());
    let list = act(&mut s, "MODELING", json!({"op": "list"}));
    assert_eq!(list["models"].as_array().unwrap().len(), 1);
    let dir = std::env::temp_dir().join(format!("aiai_bridge_mdl_{}", std::process::id()));
    let out = dir.join("casa.glb");
    let e = act(&mut s, "MODELING", json!({"op": "export", "asset": asset, "path": out.display().to_string()}));
    let back = aiai_3d_model::glb::read_glb(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(e["bytes"].as_u64().unwrap() as usize, std::fs::read(&out).unwrap().len());
    assert_eq!(back.parts.len(), 1);
    for bad in [
        json!({"op": "extrude", "asset": asset, "faces": "diagonal", "distance": 1.0}),
        json!({"op": "extrude", "asset": asset}),
        json!({"op": "new", "name": "x", "primitive": "dodecaedro"}),
        json!({"op": "export", "asset": asset, "path": "relativo.glb"}),
        json!({"op": "info", "asset": layer}),
    ] {
        let e = handle_core_action(&mut s, "MODELING", &bad).unwrap().unwrap_err();
        assert_eq!(e.code, code::INVALID_PARAMS, "{bad}: {}", e.message);
    }
    // undo: send, material, merge, scale, extrude, new
    for _ in 0..6 {
        act(&mut s, "UNDO", json!({}));
    }
    assert!(act(&mut s, "MODELING", json!({"op": "list"}))["models"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

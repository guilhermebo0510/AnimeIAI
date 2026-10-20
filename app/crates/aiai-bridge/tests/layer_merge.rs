//! LAYER_MERGE pela bridge: mescla para baixo, escolhidas e visíveis, um passo de undo, recusas com -32602 e a opção
//! de preset "Borracha" que saiu da biblioteca de fábrica mas continua valendo no PAINT.

use aiai_bridge::code;
use aiai_bridge::core_ops::handle_core_action;
use aiai_core::CoreSession;
use serde_json::{Value, json};

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn act_err(s: &mut CoreSession, action: &str, params: Value) -> aiai_bridge::BridgeError {
    handle_core_action(s, action, &params).expect("é do núcleo").unwrap_err()
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    act(s, "APPLY_COMMAND", json!({ "command": c }))["created"][0].as_str().unwrap().to_string()
}

fn doc() -> (CoreSession, String) {
    let mut s = CoreSession::for_tests("merge");
    act(&mut s, "NEW_PROJECT", json!({"name": "merge", "settings": {"width": 320, "height": 180}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));
    (s, cut)
}

fn layer(s: &mut CoreSession, cut: &str, name: &str, kind: &str, extra: Value) -> String {
    let mut d = json!({"type": "layer", "kind": kind});
    if let Some(o) = extra.as_object() {
        for (k, v) in o {
            d[k] = v.clone();
        }
    }
    let l = made(s, name, Some(cut), d);
    made(s, "c1", Some(&l), json!({"type": "cel", "number": 1}));
    l
}

fn dab(s: &mut CoreSession, cut: &str, layer: &str, x: f32, rgb: [f32; 3]) {
    act(s, "PAINT", json!({"cut": cut, "frame": 0, "layer": layer, "preset": "redondo duro", "preset_overrides": {"size": 30}, "color": {"type": "direct", "rgba": [rgb[0], rgb[1], rgb[2], 1.0]},
        "strokes": [{"points": [[x, 40], [x + 80.0, 40]]}]}));
}

fn decode(b64: &str) -> image::RgbaImage {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap().to_rgba8()
}

/// Mesma imagem a menos de 3/255 por canal (a mesclagem regrava tiles em 16 bits).
fn close(a: &image::RgbaImage, b: &image::RgbaImage) -> bool {
    a.dimensions() == b.dimensions() && a.pixels().zip(b.pixels()).all(|(p, q)| p.0.iter().zip(q.0.iter()).all(|(x, y)| x.abs_diff(*y) <= 3))
}

#[test]
fn layer_merge_down_selected_visible_and_errors() {
    let (mut s, cut) = doc();
    let a = layer(&mut s, &cut, "A", "raster", json!({}));
    let b = layer(&mut s, &cut, "B", "raster", json!({"opacity": 0.5}));
    let c = layer(&mut s, &cut, "C", "vector", json!({}));
    dab(&mut s, &cut, &a, 20.0, [1.0, 0.0, 0.0]);
    dab(&mut s, &cut, &b, 60.0, [0.0, 0.0, 1.0]);
    act(&mut s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": c, "curves": [{"points": [[20, 100], [200, 100]], "width": 6, "color": {"type": "direct", "rgba": [0.0, 0.0, 0.0, 1.0]}}]}));
    let png = |s: &mut CoreSession| decode(act(s, "RENDER", json!({"cut": cut, "frame": 0, "background": "#ffffff"}))["png_base64"].as_str().unwrap());
    let before = png(&mut s);

    // B em A: um passo de undo, o resultado é a camada de baixo
    let o = act(&mut s, "LAYER_MERGE", json!({"layer": b}));
    assert_eq!((o["layer"].as_str(), o["kind"].as_str(), o["merged"][0].as_str()), (Some(a.as_str()), Some("raster"), Some(b.as_str())), "{o}");
    assert_eq!(o["can_undo"], true);
    assert!(close(&png(&mut s), &before), "o quadro não muda com a mesclagem");

    // o vetor C com a raster A: rasteriza
    let o = act(&mut s, "LAYER_MERGE", json!({"op": "selected", "layers": [c, a]}));
    assert_eq!((o["layer"].as_str(), o["kind"].as_str()), (Some(a.as_str()), Some("raster")), "{o}");
    assert!(close(&png(&mut s), &before), "vetor + raster");

    // desfaz as duas mesclagens, uma de cada vez
    act(&mut s, "UNDO", json!({}));
    act(&mut s, "UNDO", json!({}));
    assert!(close(&png(&mut s), &before), "depois dos dois undos");
    let o = act(&mut s, "LAYER_MERGE", json!({"op": "visible", "cut": cut}));
    assert_eq!(o["merged"].as_array().unwrap().len(), 2, "{o}");
    assert!(close(&png(&mut s), &before), "visíveis");

    // recusas: -32602, nada muda
    let t = layer(&mut s, &cut, "T", "text", json!({}));
    let e = act_err(&mut s, "LAYER_MERGE", json!({"layer": t}));
    assert_eq!(e.code, code::INVALID_PARAMS, "{}", e.message);
    assert!(e.message.contains("texto"), "{}", e.message);
    assert_eq!(act_err(&mut s, "LAYER_MERGE", json!({})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "LAYER_MERGE", json!({"layer": "nao-existe"})).code, code::INVALID_PARAMS);
}

#[test]
fn borracha_preset_left_the_factory_library_but_still_paints_by_name() {
    let (mut s, cut) = doc();
    let a = layer(&mut s, &cut, "A", "raster", json!({}));
    dab(&mut s, &cut, &a, 20.0, [1.0, 0.0, 0.0]);
    let presets = act(&mut s, "BRUSH_PRESETS", json!({}));
    let names: Vec<&str> = presets["names"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(!names.contains(&"Borracha"), "fora da biblioteca de fábrica: {names:?}");
    // o nome continua valendo no PAINT (apaga)
    let r0 = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0}))["png_base64"].as_str().unwrap().to_string();
    act(&mut s, "PAINT", json!({"cut": cut, "frame": 0, "layer": a, "preset": "Borracha", "preset_overrides": {"size": 40}, "color": {"type": "direct", "rgba": [0.0, 0.0, 0.0, 1.0]}, "strokes": [{"points": [[20, 40], [100, 40]]}]}));
    let r1 = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0}))["png_base64"].as_str().unwrap().to_string();
    assert_ne!(r0, r1, "a borracha apagou o traço");
}

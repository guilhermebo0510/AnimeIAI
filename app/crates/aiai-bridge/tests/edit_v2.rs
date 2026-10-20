//! Edição v2 pela bridge (S24 3.13): `EDIT_SET` aceita o v1 e devolve trilhas/itens; `EDIT_OPS` (navalha, transição,
//! apagar com ripple, inserir, set_track) num passo de desfazer; `EDIT_CAPTIONS` exporta/importa SRT e VTT, verifica cps;
//! `EXPORT_VIDEO {edit}` com legenda de entrega por trilha.

use aiai_bridge::core_ops::handle_core_action;
use aiai_bridge::{BridgeError, code};
use aiai_core::CoreSession;
use serde_json::{Value, json};

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn act_err(s: &mut CoreSession, action: &str, params: Value) -> BridgeError {
    handle_core_action(s, action, &params).unwrap().unwrap_err()
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    let o = act(s, "APPLY_COMMAND", json!({ "command": c }));
    o["created"][0].as_str().unwrap().to_string()
}

#[test]
fn edit_v2_ops_captions_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = CoreSession::for_tests("edit-v2");
    act(&mut s, "NEW_PROJECT", json!({"name": "ed", "settings": {"width": 160, "height": 90}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let a = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 24}));
    let b = made(&mut s, "C002", Some(&sc), json!({"type": "cut", "number": 2, "duration": 24}));
    act(&mut s, "SFX", json!({"preset": "impact", "seed": 3, "cut": a, "start_frame": 2}));

    // v1 entra e sai como v2: V1 com dois clipes, som vinculado, legenda em S1
    let e = act(&mut s, "EDIT_SET", json!({"name": "Ep", "clips": [{"cut": a}, {"cut": b, "transition_in": {"kind": "dissolve", "frames": 6}}], "subtitles": [{"start_frame": 2, "end_frame": 20, "text": "Olá"}]}));
    let edit = e["edit"].as_str().unwrap().to_string();
    assert_eq!(e["timeline"]["total_frames"], 42, "24 + 24 − 6 de sobreposição: {e}");
    let items = e["timeline"]["items"].as_array().unwrap().clone();
    assert_eq!(items.iter().filter(|i| i["source"]["type"] == "cut").count(), 2);
    assert_eq!(items.iter().filter(|i| i["source"]["type"] == "cut_audio").count(), 2);
    let second = items.iter().find(|i| i["source"]["type"] == "cut" && i["source"]["cut"] == json!(b)).unwrap();
    assert_eq!((second["start"].as_u64(), second["src_in"].as_u64(), second["transition_in"]["align"].as_str()), (Some(24), Some(6), Some("end")));
    let first_id = items.iter().find(|i| i["source"]["type"] == "cut" && i["source"]["cut"] == json!(a)).unwrap()["id"].as_u64().unwrap();

    // ops: navalha em 10 (imagem e som do 1º), mergulho no preto na entrada da parte direita, apagar com ripple
    let o = act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [
        {"op": "razor", "frame": 10, "items": [first_id]},
        {"op": "set_track", "track": 6, "patch": {"lang": "pt"}}
    ]}));
    let made_ids = o["created"][0].as_array().unwrap().clone();
    assert_eq!(made_ids.len(), 2, "imagem e som vinculados divididos: {o}");
    let right = made_ids[0].as_u64().unwrap();
    act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [{"op": "transition", "item": right, "edge": "in", "transition": {"kind": "dip_black", "frames": 4}}]}));
    let o = act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [{"op": "delete", "items": [right], "ripple": true}]}));
    assert_eq!(o["timeline"]["total_frames"], 42 - 14, "14 quadros apagados com ripple: {}", o["timeline"]);
    // tudo ou nada: um op inválido não muda nada
    let before = act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": []}))["timeline"].clone();
    assert_eq!(act_err(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [{"op": "razor", "frame": 3}, {"op": "delete", "items": [999]}]})).code, code::INVALID_PARAMS);
    assert_eq!(act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": []}))["timeline"], before);
    // inserir o 2º corte no começo empurra tudo
    let o = act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [{"op": "place", "source": {"type": "cut", "cut": b}, "at": 0, "src_in": 0, "dur": 8, "mode": "insert"}]}));
    assert_eq!(o["timeline"]["total_frames"], 28 + 8);
    // desfazer volta um passo (a inserção)
    act(&mut s, "UNDO", json!({}));
    assert_eq!(act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": []}))["timeline"]["total_frames"], 28);

    // legendas: exporta SRT, importa numa trilha nova (en) e verifica
    let srt = dir.path().join("leg.srt");
    let x = act(&mut s, "EDIT_CAPTIONS", json!({"edit": edit, "action": "export", "out": srt.display().to_string()}));
    assert_eq!(x["captions"], 1);
    let text = std::fs::read_to_string(&srt).unwrap();
    assert!(text.contains("Olá"), "{text}");
    let o = act(&mut s, "EDIT_OPS", json!({"edit": edit, "ops": [{"op": "add_track", "kind": "subtitle", "lang": "en"}]}));
    let s2 = o["created"][0][0].as_u64().unwrap();
    std::fs::write(dir.path().join("en.vtt"), "WEBVTT\n\n00:00:00.100 --> 00:00:01.100\nHello\n\n00:00:01.200 --> 00:00:01.400\nThis line is far too fast to be read comfortably by anyone\n").unwrap();
    let x = act(&mut s, "EDIT_CAPTIONS", json!({"edit": edit, "action": "import", "track": s2, "path": dir.path().join("en.vtt").display().to_string()}));
    assert_eq!(x["captions"].as_array().unwrap().len(), 2);
    let c = act(&mut s, "EDIT_CAPTIONS", json!({"edit": edit, "action": "check", "track": s2}));
    assert_eq!(c["with_issues"], 1, "{c}");
    assert_eq!(act_err(&mut s, "EDIT_CAPTIONS", json!({"edit": edit, "action": "export", "out": dir.path().join("x.txt").display().to_string()})).code, code::INVALID_PARAMS);

    // exportação com legenda de entrega por trilha (precisa do ffmpeg)
    if std::process::Command::new(aiai_media::ffmpeg()).arg("-version").output().is_ok() {
        let mp4 = dir.path().join("ep.mp4");
        let r = act(&mut s, "EXPORT_VIDEO", json!({"edit": edit, "out": mp4.display().to_string(), "sidecar_srt": true, "audio": false}));
        assert_eq!(r["probe"]["frames"].as_u64(), Some(28), "{r}");
        let side = r["sidecar_srts"].as_array().unwrap();
        assert_eq!(side.len(), 2, "uma por trilha: {r}");
        assert!(dir.path().join("ep.srt").is_file() && dir.path().join("ep.en.srt").is_file());
    }
}

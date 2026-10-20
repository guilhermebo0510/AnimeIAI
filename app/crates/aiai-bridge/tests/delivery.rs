//! Entrega pela bridge (S24 4.8–4.12): `DELIVERY` com formatos, destinos, verificação final, fila persistida, exportação
//! de verdade (precisa do ffmpeg; pula sem ele), entregáveis com miniatura, exportar de novo com versão nova, pacote de
//! entrega, pôster e lixeira.

use aiai_bridge::core_ops::handle_core_action;
use aiai_bridge::{BridgeError, code};
use aiai_core::CoreSession;
use serde_json::{Value, json};
use std::path::Path;

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action} {params}: {e}"))
}

fn act_err(s: &mut CoreSession, action: &str, params: Value) -> BridgeError {
    handle_core_action(s, action, &params).unwrap().unwrap_err()
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    act(s, "APPLY_COMMAND", json!({ "command": c }))["created"][0].as_str().unwrap().to_string()
}

fn ffmpeg_ok() -> bool {
    std::process::Command::new(aiai_media::ffmpeg()).arg("-version").output().is_ok()
}

fn project(dir: &Path) -> (CoreSession, String, String, String) {
    let mut s = CoreSession::for_tests("rosa");
    act(&mut s, "NEW_PROJECT", json!({"name": "Última Rosa", "settings": {"width": 160, "height": 90}}));
    let ep = made(&mut s, "Ep 1", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let a = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));
    let b = made(&mut s, "C002", Some(&sc), json!({"type": "cut", "number": 2, "duration": 12}));
    act(&mut s, "SFX", json!({"preset": "impact", "seed": 3, "cut": a, "start_frame": 2}));
    act(&mut s, "SAVE_PROJECT", json!({"path": dir.join("rosa.aiai").display().to_string()}));
    (s, ep, a, b)
}

#[test]
fn formats_settings_and_preflight() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _ep, a, _b) = project(dir.path());
    let f = act(&mut s, "DELIVERY", json!({"op": "formats"}));
    let ids: Vec<&str> = f["formats"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    for want in ["tv", "web", "youtube", "master", "nvenc", "prores", "h265", "webm", "preview", "png", "wav", "stems", "contact"] {
        assert!(ids.contains(&want), "falta {want}: {ids:?}");
    }
    let webm = f["formats"].as_array().unwrap().iter().find(|x| x["id"] == "webm").unwrap();
    assert!(webm["available"].is_boolean() && (webm["available"] == true || webm["reason"].as_str().is_some_and(|r| !r.is_empty())), "{webm}");
    assert!(f["name_tokens"].as_array().unwrap().iter().any(|t| t == "{versao}"));

    // destinos: modelo de nome validado, pasta absoluta
    let st = act(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"name_template": "{projeto}_{corte}_{formato}", "extra_dirs": []}}));
    assert!(st["out_dir"].as_str().unwrap().ends_with("saida"), "{st}");
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"name_template": "{nada}"}})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"dest": "custom", "custom_dir": "relativa"}})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"campo_que_nao_existe": 1}})).code, code::INVALID_PARAMS);
    // o que foi gravado volta numa chamada nova (o estado mora em entrega.json)
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "settings"}))["name_template"], "{projeto}_{corte}_{formato}");
    assert!(dir.path().join("rosa.aiai").join("entrega.json").is_file());

    // verificação final: fonte boa passa nas conferências básicas; fonte que não existe falha
    let p = act(&mut s, "DELIVERY", json!({"op": "preflight", "cut": a, "preset": "preview", "loudness": true}));
    assert_eq!(p["blocking"], false, "{p}");
    let codes: Vec<&str> = p["checks"].as_array().unwrap().iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"source_ok") && codes.contains(&"media_ok") && codes.contains(&"folder_ok"), "{codes:?}");
    assert!(p["loudness"]["lufs"].is_number(), "o SFX tem som: {p}");
    let bad = act(&mut s, "DELIVERY", json!({"op": "preflight", "cut": "nada", "preset": "tv"}));
    assert_eq!(bad["blocking"], true);
    assert!(bad["checks"].as_array().unwrap().iter().any(|c| c["code"] == "source_missing" && c["level"] == "fail"));
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "preflight", "cut": a, "preset": "nada"})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "preflight", "preset": "tv"})).code, code::INVALID_PARAMS, "sem fonte");
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "nada"})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "formats", "chave_errada": 1})).code, code::INVALID_PARAMS);
}

#[test]
fn queue_persists_reorders_and_reports_failures() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ep, a, b) = project(dir.path());
    // matriz: 2 fontes × 2 formatos = 4 itens, nada roda sozinho
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "sources": [{"kind": "cut", "id": a}, {"kind": "cut", "id": b}], "presets": ["preview", "wav"]}));
    assert_eq!(r["added"].as_array().unwrap().len(), 4, "{r}");
    assert_eq!(r["queued"], 4);
    let outs: Vec<String> = r["added"].as_array().unwrap().iter().map(|j| j["out"].as_str().unwrap().to_owned()).collect();
    let mut uniq = outs.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), 4, "nomes distintos: {outs:?}");
    assert!(outs[0].contains("saida") && outs[0].contains("Última Rosa_C001_preview_v001.mp4"), "{}", outs[0]);
    // a fila fica no disco: outra leitura enxerga os mesmos itens
    let l = act(&mut s, "DELIVERY", json!({"op": "queue_list"}));
    assert_eq!(l["jobs"].as_array().unwrap().len(), 4);
    let ids: Vec<u64> = l["jobs"].as_array().unwrap().iter().map(|j| j["id"].as_u64().unwrap()).collect();
    act(&mut s, "DELIVERY", json!({"op": "queue_move", "id": ids[3], "to": 0}));
    let l = act(&mut s, "DELIVERY", json!({"op": "queue_list"}));
    assert_eq!(l["jobs"][0]["id"], ids[3]);
    act(&mut s, "DELIVERY", json!({"op": "queue_cancel", "id": ids[1]}));
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "queue_list"}))["jobs"][2]["status"], "canceled");
    act(&mut s, "DELIVERY", json!({"op": "queue_retry", "id": ids[1]}));
    act(&mut s, "DELIVERY", json!({"op": "queue_remove", "id": ids[2]}));
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "queue_list"}))["jobs"].as_array().unwrap().len(), 3);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "queue_remove", "id": 999})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut s, "DELIVERY", json!({"op": "queue_move", "id": ids[0]})).code, code::INVALID_PARAMS, "falta `to`");
    act(&mut s, "DELIVERY", json!({"op": "queue_clear"}));

    // falha com motivo claro: a pasta de saída está atrás de um arquivo (esvazia a fila antes)
    for j in act(&mut s, "DELIVERY", json!({"op": "queue_list"}))["jobs"].as_array().unwrap() {
        act(&mut s, "DELIVERY", json!({"op": "queue_cancel", "id": j["id"]}));
    }
    act(&mut s, "DELIVERY", json!({"op": "queue_clear"}));
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "queue_list"}))["jobs"].as_array().unwrap().len(), 0);
    std::fs::write(dir.path().join("arquivo.txt"), "x").unwrap();
    act(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"dest": "custom", "custom_dir": dir.path().join("arquivo.txt").join("filho").display().to_string()}}));
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "episode": ep, "preset": "wav", "run": true}));
    let ran = r["ran"].as_array().unwrap();
    assert_eq!(ran.len(), 1, "{r}");
    assert_eq!(ran[0]["status"], "failed", "{r}");
    let err = ran[0]["error"].as_str().unwrap();
    assert!(err.contains("criar a pasta") && err.contains("arquivo.txt"), "o motivo vem inteiro: {err}");
    // um formato que o ffmpeg do PC não tem é recusado ao enfileirar, com o nome do que falta
    if !aiai_media::VideoCodec::Vp9.availability().is_ok() {
        let e = act_err(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "webm"}));
        assert!(e.message.contains("indisponível"), "{}", e.message);
    }
}

#[test]
fn export_thumbnail_reexport_package_poster_and_trash() {
    if !ffmpeg_ok() {
        eprintln!("ffmpeg ausente: teste pulado");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ep, a, _b) = project(dir.path());
    let extra = dir.path().join("copia");
    act(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"name_template": "{projeto}_{episodio}_{formato}", "extra_dirs": [extra.display().to_string()]}}));
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "episode": ep, "preset": "preview", "run": true}));
    let job = &r["ran"][0];
    assert_eq!(job["status"], "done", "{r}");
    let out = job["out"].as_str().unwrap().to_owned();
    assert!(out.ends_with("Última Rosa_Ep 1_preview_v001.mp4"), "{out}");
    assert!(Path::new(&out).is_file());

    // entregável: metadados, miniatura, cópia na pasta extra
    let l = act(&mut s, "DELIVERY", json!({"op": "list"}));
    let d = &l["deliverables"][0];
    assert_eq!((d["frames"].as_u64(), d["preset"].as_str(), d["missing"].as_bool()), (Some(24), Some("preview"), Some(false)), "{d}");
    assert!(d["width"].as_u64().unwrap() <= 160 && d["duration_s"].as_f64().unwrap() > 0.5, "{d}");
    assert!(Path::new(d["thumbnail"].as_str().unwrap()).is_file(), "miniatura gerada ao concluir: {d}");
    assert!(extra.join("Última Rosa_Ep 1_preview_v001.mp4").is_file(), "cópia na pasta extra: {d}");
    let id = d["id"].as_u64().unwrap();

    // exportar de novo: mesmo pedido, nome com a versão seguinte
    let r = act(&mut s, "DELIVERY", json!({"op": "reexport", "id": id, "run": true}));
    assert!(r["ran"][0]["out"].as_str().unwrap().ends_with("_v002.mp4"), "{r}");
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "list"}))["total"], 2);

    // pacote: vídeo, créditos, pôster e checksums
    let p = act(&mut s, "DELIVERY", json!({"op": "package", "id": id}));
    let zip = p["package"].as_str().unwrap();
    let names: Vec<String> = p["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.iter().any(|n| n.ends_with(".mp4")) && names.iter().any(|n| n.ends_with("creditos.txt")) && names.iter().any(|n| n.ends_with("poster.png")) && names.iter().any(|n| n.ends_with("checksums.sha256")), "{names:?}");
    assert!(Path::new(zip).is_file());
    let back = aiai_tools::delivery::package::read_zip(Path::new(zip)).unwrap();
    assert!(back.iter().any(|(n, body)| n.ends_with("creditos.txt") && String::from_utf8_lossy(body).contains("Última Rosa")));

    // pôster com título: arquivo novo, o quadro e o título ficam guardados
    let po = act(&mut s, "DELIVERY", json!({"op": "poster", "id": id, "frame": 5, "title": "Última Rosa", "subtitle": "Episódio 1"}));
    assert_eq!((po["poster"]["frame"].as_u64(), po["poster"]["title"].as_str()), (Some(5), Some("Última Rosa")));
    assert!(Path::new(po["poster"]["file"].as_str().unwrap()).is_file());
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "list"}))["deliverables"][1]["poster"]["frame"], 5, "o pôster ficou no entregável");

    // lixeira: o arquivo vai para _Lixeira e sai da biblioteca
    let del = act(&mut s, "DELIVERY", json!({"op": "delete", "id": id}));
    assert!(!Path::new(&out).exists());
    assert!(del["moved_to"].as_array().unwrap().iter().all(|m| m.as_str().unwrap().contains("_Lixeira")));
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "list"}))["total"], 1);
    let _ = a;
}

#[test]
fn audio_formats_contact_sheet_and_png_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _ep, a, _b) = project(dir.path());
    // WAV da mixagem e stems por trilha (não precisam do ffmpeg)
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "wav", "run": true}));
    assert_eq!(r["ran"][0]["status"], "done", "{r}");
    let wav = r["ran"][0]["out"].as_str().unwrap();
    assert!(wav.ends_with("_v001.wav") && Path::new(wav).metadata().unwrap().len() > 1000);
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "stems", "run": true}));
    assert_eq!(r["ran"][0]["status"], "done", "{r}");
    let folder = Path::new(r["ran"][0]["out"].as_str().unwrap()).parent().unwrap().to_path_buf();
    let l = act(&mut s, "DELIVERY", json!({"op": "list"}));
    let stems = &l["deliverables"][0];
    assert_eq!(stems["is_folder"], true);
    let files: Vec<&str> = stems["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(files.iter().any(|f| f.ends_with("_sfx.wav")) && files.iter().all(|f| Path::new(f).starts_with(&folder)), "{files:?}");
    // folha de contato e sequência PNG
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "contact", "run": true}));
    assert_eq!(r["ran"][0]["status"], "done", "{r}");
    let sheet = r["ran"][0]["out"].as_str().unwrap();
    assert!(sheet.ends_with(".png") && Path::new(sheet).is_file());
    if ffmpeg_ok() {
        let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "png", "run": true}));
        assert_eq!(r["ran"][0]["status"], "done", "{r}");
        let pattern = r["ran"][0]["out"].as_str().unwrap();
        assert!(pattern.contains("%05d"));
        let folder = Path::new(pattern).parent().unwrap();
        assert_eq!(std::fs::read_dir(folder).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "png")).count(), 12);
    }
}

#[test]
fn export_progress_pause_and_cancel_reach_the_running_export() {
    if !ffmpeg_ok() {
        eprintln!("ffmpeg ausente: teste pulado");
        return;
    }
    use aiai_bridge::delivery_tools::{JobError, run_job};
    use aiai_bridge::export_ctl::ExportControl;
    use aiai_tools::delivery::store::JobSpec;
    use aiai_tools::delivery::SourceRef;
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _ep, a, _b) = project(dir.path());
    let proj = dir.path().join("rosa.aiai");
    let out = dir.path().join("cancelada.mp4");
    let spec = JobSpec { id: 1, source: SourceRef::cut(aiai_core::Id(a.clone())), source_name: "C001".into(), preset: "preview".into(), crf: None, out: out.display().to_string(), extra_dirs: vec![], package: false, poster_frame: None, revision: 0, created_ms: 0 };
    // cancelado antes de começar: erro de cancelamento e nenhum arquivo
    let ctl = ExportControl::new();
    ctl.cancel();
    let r = run_job(&mut s, &spec, &proj, Some(ctl.clone()));
    assert_eq!(r.unwrap_err(), JobError::Canceled);
    assert!(!out.exists(), "nada pela metade");
    // concluído: o progresso chegou ao total e o ETA some no fim
    let ctl = ExportControl::new();
    let res = run_job(&mut s, &spec, &proj, Some(ctl.clone())).unwrap();
    assert_eq!(ctl.frames(), (12, 12));
    assert_eq!(res.frames, 12);
    assert!(out.is_file());
    // pausado: o relógio de quadros para; retomar libera
    let ctl = ExportControl::new();
    ctl.set_paused(true);
    let ctl2 = ctl.clone();
    let mut s2 = s;
    let proj2 = proj.clone();
    let out2 = dir.path().join("pausada.mp4");
    let spec2 = JobSpec { out: out2.display().to_string(), ..spec.clone() };
    let h = std::thread::spawn(move || run_job(&mut s2, &spec2, &proj2, Some(ctl2)));
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(ctl.frames().0 == 0 && !h.is_finished(), "pausada: nenhum quadro andou");
    ctl.set_paused(false);
    let res = h.join().unwrap().unwrap();
    assert_eq!(res.frames, 12);
}

#[test]
fn h265_webm_and_the_small_preview_really_encode() {
    if !ffmpeg_ok() {
        eprintln!("ffmpeg ausente: teste pulado");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = CoreSession::for_tests("rosa");
    act(&mut s, "NEW_PROJECT", json!({"name": "Grande", "settings": {"width": 640, "height": 480}}));
    let ep = made(&mut s, "Ep 1", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let a = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 6}));
    act(&mut s, "SAVE_PROJECT", json!({"path": dir.path().join("g.aiai").display().to_string()}));
    // prévia pequena: 360 de altura, largura proporcional (480), mais leve que o original
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "preview", "run": true}));
    assert_eq!(r["ran"][0]["status"], "done", "{r}");
    let l = act(&mut s, "DELIVERY", json!({"op": "list"}));
    let d = &l["deliverables"][0];
    assert_eq!((d["width"].as_u64(), d["height"].as_u64(), d["frames"].as_u64()), (Some(480), Some(360), Some(6)), "{d}");
    // H.265 e WebM, quando o ffmpeg tem os codificadores
    for (preset, codec) in [("h265", "hevc"), ("webm", "vp9")] {
        let f = act(&mut s, "DELIVERY", json!({"op": "formats"}));
        let av = f["formats"].as_array().unwrap().iter().find(|x| x["id"] == preset).unwrap()["available"].as_bool().unwrap();
        if !av {
            eprintln!("{preset} indisponível neste ffmpeg: pulado");
            continue;
        }
        let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": preset, "run": true}));
        assert_eq!(r["ran"][0]["status"], "done", "{r}");
        let out = r["ran"][0]["out"].as_str().unwrap();
        let info = aiai_media::probe(Path::new(out)).unwrap();
        assert_eq!(info.video_codec.as_deref(), Some(codec), "{preset}: {info:?}");
        assert_eq!(info.frames, Some(6));
    }
}

#[test]
fn the_mcp_sandbox_confines_every_folder_the_delivery_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _ep, a, _b) = project(dir.path());
    let other = tempfile::tempdir().unwrap();
    let only_other = json!([other.path().display().to_string()]);
    // a pasta padrão (`saida` ao lado do projeto) está fora: nada entra na fila
    let e = act_err(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "wav", "allowed_dirs": only_other}));
    assert!(e.message.contains("fora das pastas permitidas"), "{}", e.message);
    assert_eq!(act(&mut s, "DELIVERY", json!({"op": "queue_list"}))["jobs"].as_array().unwrap().len(), 0);
    // pasta de saída e extras fora: recusadas ao mudar os destinos
    assert!(act_err(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"dest": "custom", "custom_dir": dir.path().join("x").display().to_string()}, "allowed_dirs": only_other})).message.contains("fora das pastas"));
    assert!(act_err(&mut s, "DELIVERY", json!({"op": "settings", "settings": {"extra_dirs": [dir.path().join("y").display().to_string()]}, "allowed_dirs": only_other})).message.contains("fora das pastas"));
    // dentro: passa
    let ok = json!([dir.path().display().to_string()]);
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "wav", "run": true, "allowed_dirs": ok}));
    assert_eq!(r["ran"][0]["status"], "done", "{r}");
    // a verificação não toca no disco fora das pastas permitidas: a pasta vira falha
    let p = act(&mut s, "DELIVERY", json!({"op": "preflight", "cut": a, "preset": "wav", "loudness": false, "allowed_dirs": only_other}));
    assert!(p["checks"].as_array().unwrap().iter().any(|c| c["code"] == "folder_bad" && c["level"] == "fail"), "{p}");
    // item guardado antes (sem confinamento) não roda fora das pastas de quem manda rodar
    act(&mut s, "DELIVERY", json!({"op": "queue_add", "cut": a, "preset": "wav"}));
    let r = act(&mut s, "DELIVERY", json!({"op": "queue_run", "allowed_dirs": only_other}));
    assert_eq!(r["ran"][0]["status"], "failed", "{r}");
    assert!(r["ran"][0]["error"].as_str().unwrap().contains("fora das pastas"));
}

//! Model sheet com grade de cabeças e manequim em 5 vistas; checagem de proporção.

use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::character::{ConsistencyRequest, ModelSheetRequest, consistency, model_sheet};

#[test]
fn model_sheet_with_turnaround_guides() {
    let mut s = CoreSession::for_tests("chara");
    let ch = s
        .apply(Command::CreateNode { id: None, parent: None, index: None, name: "Rin".into(), data: NodeKind::Character(CharacterData::default()) })
        .unwrap()
        .created[0]
        .clone();
    let req = ModelSheetRequest { character: ch.clone(), name: "Rin — model sheet".into(), heads_tall: 6.5, figure_px: 650.0, views: vec![0.0, 45.0, 90.0, 135.0, 180.0], pose: None };
    let plan = model_sheet(&s.project, &mut s.ids, &req).unwrap();
    s.apply(Command::Batch { commands: plan.commands, label: Some("model sheet".into()) }).unwrap();
    let out = render_frame(&s.project, &s.blobs, &plan.cut, 0, &RenderOptions { include_guides: true, background: Some([1.0; 3]), camera: false, ..Default::default() }).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        out.image.save_png(&std::path::Path::new(&dir).join("model_sheet_guias.png")).unwrap();
    }
    let ink = out.image.data.iter().filter(|p| p[2] > p[0] + 0.2).count();
    assert!(ink > 3000, "linhas do manequim nas 5 vistas: {ink}");
    // checagem: desenho com cabeça de 100 px, olhos a 55 px, pés a 650 → ok; pés a 560 → erro grande
    let ok = consistency(&s.project, &ConsistencyRequest { character: ch.clone(), head_top: 0.0, chin: 100.0, eyes: Some(55.0), shoulders: None, feet: Some(650.0) }).unwrap();
    assert!(ok.ok, "{ok:?}");
    let bad = consistency(&s.project, &ConsistencyRequest { character: ch.clone(), head_top: 0.0, chin: 100.0, eyes: Some(55.0), shoulders: None, feet: Some(560.0) }).unwrap();
    assert!(!bad.ok && bad.max_error_pct > 10.0, "{bad:?}");
    // settei com olhos mais baixos (0,60 cabeça): olhos a 60 px passam a valer e 55 px vira erro
    let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { unreachable!() };
    let mut m = serde_json::to_value(&c.measurements).unwrap();
    m["landmarks"] = serde_json::json!({"olhos": 0.60});
    s.apply(Command::SetProps { id: ch.clone(), name: None, data: serde_json::json!({"measurements": m}) }).unwrap();
    let own = consistency(&s.project, &ConsistencyRequest { character: ch.clone(), head_top: 0.0, chin: 100.0, eyes: Some(60.0), shoulders: None, feet: Some(650.0) }).unwrap();
    assert!(own.ok && own.errors_pct["olhos"] < 0.01, "{own:?}");
    let old = consistency(&s.project, &ConsistencyRequest { character: ch, head_top: 0.0, chin: 100.0, eyes: Some(55.0), shoulders: None, feet: Some(650.0) }).unwrap();
    assert!(old.errors_pct["olhos"] > 8.0, "{old:?}");
}

//! Suíte de integração do núcleo (um binário só — ver lições sobre disco).

use aiai_core::command::{CellEdit, IndexedStroke, TileEdit};
use aiai_core::persist;
use aiai_core::*;
use proptest::prelude::*;
use serde_json::json;

fn snapshot(s: &CoreSession) -> String {
    // Revisão muda a cada comando; o conteúdo é o que importa para conferir o inverso.
    let mut p = s.project.clone();
    p.revision = 0;
    for n in p.nodes.values_mut() {
        n.provenance = Provenance::default();
    }
    persist::to_canonical_json(&p).unwrap()
}

/// Projeto típico: episódio → cena → corte com camadas (pasta, vetor, raster, preenchimento) e cels, paleta, personagem.
fn build_basic(s: &mut CoreSession) -> (Id, Id, Id, Id) {
    let ep = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "EP01".into(), data: NodeKind::Episode(EpisodeData { number: 1 }) }).unwrap().created[0].clone();
    let sc = s.apply(Command::CreateNode { id: None, parent: Some(ep.clone()), index: None, name: "Cena 1".into(), data: NodeKind::Scene(SceneData::default()) }).unwrap().created[0].clone();
    let cut = s.apply(Command::CreateNode { id: None, parent: Some(sc), index: None, name: "C001".into(), data: NodeKind::Cut(CutData::new(48)) }).unwrap().created[0].clone();
    let layer = s
        .apply(Command::CreateNode { id: None, parent: Some(cut.clone()), index: None, name: "A".into(), data: NodeKind::Layer(LayerData::new(LayerType::Vector)) })
        .unwrap()
        .created[0]
        .clone();
    let cel = s
        .apply(Command::CreateNode { id: None, parent: Some(layer.clone()), index: None, name: "A1".into(), data: NodeKind::Cel(CelData::new(1)) })
        .unwrap()
        .created[0]
        .clone();
    (ep, cut, layer, cel)
}

fn stroke(id: &str, x: f32) -> Stroke {
    Stroke {
        id: Id::from(id),
        points: vec![
            StrokePoint { x, y: 10.0, pressure: 0.5, tilt: None, t: None },
            StrokePoint { x: x + 50.0, y: 60.0, pressure: 1.0, tilt: None, t: None },
        ],
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::GPen,
        width: 4.0,
        profile: WidthProfile::default(),
        color: ColorRef::default(),
        opacity: 1.0,
        closed: false,
        aliased: true,
        color_trace: false,
    }
}

#[test]
fn every_basic_command_undoes_and_redoes_exactly() {
    let mut s = CoreSession::for_tests("t");
    let (_ep, cut, layer, cel) = build_basic(&mut s);
    let blob = s.blobs.put(vec![1, 2, 3, 4]);
    let track = Track::new(Id::from(""), "A", TrackKind::Drawing, Some(layer.clone()));
    let commands = vec![
        Command::SetProps { id: layer.clone(), name: Some("A-linha".into()), data: json!({"opacity": 0.5, "blend": "multiply"}) },
        Command::SetProjectProps { patch: json!({"name": "Novo", "settings": {"width": 1280}, "bible": {"logline": "x"}}) },
        Command::AddTrack { cut: cut.clone(), index: None, track },
        Command::InsertStrokes { cel: cel.clone(), items: vec![IndexedStroke { index: 0, stroke: stroke("", 0.0) }, IndexedStroke { index: 1, stroke: stroke("", 5.0) }] },
        Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 0, y: 0, blob: Some(blob.clone()) }, TileEdit { x: -1, y: 2, blob: Some(blob.clone()) }] },
        Command::ReplaceFills { cel: cel.clone(), fills: vec![FillRegion { id: Id::from(""), seed: [3.0, 4.0], color: ColorRef::default(), mask: Some(blob.clone()), bounds: [0, 0, 10, 10] }] },
        Command::CreateNode { id: None, parent: None, index: None, name: "Settei".into(), data: NodeKind::Palette(PaletteData::default()) },
    ];
    for c in commands {
        let before = snapshot(&s);
        s.apply(c.clone()).unwrap_or_else(|e| panic!("{c:?}: {e}"));
        let after = snapshot(&s);
        assert_ne!(before, after, "comando sem efeito: {c:?}");
        s.undo().unwrap();
        assert_eq!(snapshot(&s), before, "undo não restaurou: {c:?}");
        s.redo().unwrap();
        assert_eq!(snapshot(&s), after, "redo não refez: {c:?}");
    }
    // Timesheet com a trilha criada acima.
    let tid = match &s.project.node(&cut).unwrap().data {
        NodeKind::Cut(c) => c.xsheet.tracks[0].id.clone(),
        _ => unreachable!(),
    };
    let before = snapshot(&s);
    s.apply(Command::SetCells {
        cut: cut.clone(),
        track: tid.clone(),
        cells: exposure_entries(&[1], 2, 0).into_iter().map(|(f, c)| CellEdit { frame: f, cell: Some(c) }).collect(),
    })
    .unwrap();
    let r = Query::Exposure { cut: cut.clone(), frame: 5 }.run(&s).unwrap();
    assert_eq!(r["tracks"][0]["cel"], json!(cel), "a exposição resolve o id do cel");
    s.undo().unwrap();
    assert_eq!(snapshot(&s), before);
}

#[test]
fn delete_cleans_external_references_and_undo_restores_them() {
    let mut s = CoreSession::for_tests("t");
    let (_ep, cut, layer, _cel) = build_basic(&mut s);
    let fill = s
        .apply(Command::CreateNode { id: None, parent: Some(cut.clone()), index: None, name: "A-cor".into(), data: NodeKind::Layer(LayerData::new(LayerType::Fill)) })
        .unwrap()
        .created[0]
        .clone();
    s.apply(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [layer.0]}) }).unwrap();
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from(""), "A", TrackKind::Drawing, Some(layer.clone())) }).unwrap();
    let before = snapshot(&s);
    s.apply(Command::DeleteNode { id: layer.clone() }).unwrap();
    assert!(s.project.node(&layer).is_none());
    match &s.project.node(&fill).unwrap().data {
        NodeKind::Layer(l) => assert!(l.line_refs.is_empty()),
        _ => unreachable!(),
    }
    match &s.project.node(&cut).unwrap().data {
        NodeKind::Cut(c) => assert!(c.xsheet.tracks.is_empty(), "trilha da camada apagada some"),
        _ => unreachable!(),
    }
    s.undo().unwrap();
    assert_eq!(snapshot(&s), before);
}

#[test]
fn invalid_commands_leave_project_intact() {
    let mut s = CoreSession::for_tests("t");
    let (ep, cut, layer, cel) = build_basic(&mut s);
    let before = snapshot(&s);
    let bad = vec![
        Command::SetProps { id: layer.clone(), name: None, data: json!({"opacity": 3.0}) },
        Command::SetProps { id: layer.clone(), name: None, data: json!({"type": "cut"}) },
        Command::CreateNode { id: None, parent: Some(cel.clone()), index: None, name: "x".into(), data: NodeKind::Layer(LayerData::new(LayerType::Raster)) },
        Command::CreateNode { id: None, parent: Some(layer.clone()), index: None, name: "dup".into(), data: NodeKind::Cel(CelData::new(1)) },
        Command::CreateNode { id: None, parent: None, index: None, name: "cena solta".into(), data: NodeKind::Scene(SceneData::default()) },
        Command::MoveNode { id: ep.clone(), parent: Some(cut.clone()), index: None },
        Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 0, y: 0, blob: Some(BlobHash("0".repeat(64))) }] },
        Command::Batch {
            commands: vec![
                Command::SetProps { id: layer.clone(), name: Some("ok".into()), data: serde_json::Value::Null },
                Command::RemoveStrokes { cel: cel.clone(), ids: vec![Id::from("nao-existe")] },
            ],
            label: None,
        },
    ];
    for c in bad {
        assert!(s.apply(c.clone()).is_err(), "deveria falhar: {c:?}");
        assert_eq!(snapshot(&s), before, "estado mudou depois de erro: {c:?}");
    }
}

#[test]
fn save_open_roundtrip_is_byte_identical_and_wal_recovers() {
    let dir = std::env::temp_dir().join(format!("aiai-core-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = CoreSession::for_tests("roundtrip");
    let (_ep, _cut, _layer, cel) = build_basic(&mut s);
    let blob = s.blobs.put(vec![9; 1000]);
    s.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 0, y: 0, blob: Some(blob) }] }).unwrap();
    persist::save(&mut s, &dir).unwrap();
    let json1 = std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap();
    let (mut s2, rep) = persist::open(&dir).unwrap();
    assert_eq!(rep.replayed, 0);
    assert_eq!(persist::to_canonical_json(&s2.project).unwrap(), json1);
    persist::save(&mut s2, &dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap(), json1, "salvar de novo não muda bytes");

    // Edições depois de salvar vão para o WAL; "queda" = abrir sem salvar.
    s2.ids = IdGen::sequential("wal-");
    s2.apply(Command::InsertStrokes { cel: cel.clone(), items: vec![IndexedStroke { index: 0, stroke: stroke("", 1.0) }] }).unwrap();
    let b2 = s2.blobs.put(vec![7; 64]);
    s2.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 1, y: 0, blob: Some(b2) }] }).unwrap();
    s2.undo().unwrap();
    let expected = snapshot(&s2);
    drop(s2);
    let (s3, rep3) = persist::open(&dir).unwrap();
    assert_eq!(rep3.replayed, 3, "{rep3:?}");
    assert_eq!(snapshot(&s3), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schema_lists_commands_and_queries() {
    let s = CoreSession::for_tests("t");
    let schema = Query::Schema { which: "commands".into() }.run(&s).unwrap();
    let text = schema.to_string();
    for k in ["create_node", "set_props", "insert_strokes", "set_tiles", "set_cells", "batch"] {
        assert!(text.contains(k), "schema sem {k}");
    }
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("command_schema.json"), serde_json::to_string_pretty(&schema).unwrap()).unwrap();
    }
}

// ---------------------------------------------------------------- propriedade: sequências aleatórias

#[derive(Debug, Clone)]
enum Op {
    AddLayer(u8),
    AddCel(u8, u8),
    Rename(u8),
    Opacity(u8, u8),
    Draw(u8),
    EraseFirst(u8),
    Delete(u8),
    Move(u8, u8),
    Cells(u8, u8),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        any::<u8>().prop_map(Op::AddLayer),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::AddCel(a, b)),
        any::<u8>().prop_map(Op::Rename),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Opacity(a, b)),
        any::<u8>().prop_map(Op::Draw),
        any::<u8>().prop_map(Op::EraseFirst),
        any::<u8>().prop_map(Op::Delete),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Move(a, b)),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Cells(a, b)),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

fn pick<T: Clone>(v: &[T], i: u8) -> Option<T> {
    if v.is_empty() { None } else { Some(v[i as usize % v.len()].clone()) }
}

fn ids_of(s: &CoreSession, t: &str) -> Vec<Id> {
    s.project.nodes.values().filter(|n| n.data.type_name() == t).map(|n| n.id.clone()).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, .. ProptestConfig::default() })]

    /// Qualquer sequência de comandos (válidos ou não), desfeita até o começo, volta ao estado inicial;
    /// e refeita até o fim, volta ao estado final. O documento nunca fica inválido.
    #[test]
    fn random_sequences_undo_to_start(ops in proptest::collection::vec(op(), 1..40)) {
        let mut s = CoreSession::for_tests("p");
        let (_ep, cut, _layer, _cel) = build_basic(&mut s);
        // linha de base: histórico começa aqui
        let base = snapshot(&s);
        let base_depth = s.history().0.len();
        for o in ops {
            let layers = ids_of(&s, "layer");
            let cels = ids_of(&s, "cel");
            let cmd = match o {
                Op::AddLayer(k) => Some(Command::CreateNode { id: None, parent: Some(cut.clone()), index: Some(k as usize % 4), name: format!("L{k}"), data: NodeKind::Layer(LayerData::new(if k % 2 == 0 { LayerType::Vector } else { LayerType::Group })) }),
                Op::AddCel(l, n) => pick(&layers, l).map(|l| Command::CreateNode { id: None, parent: Some(l), index: None, name: format!("c{n}"), data: NodeKind::Cel(CelData::new(1 + n as u32 % 5)) }),
                Op::Rename(i) => pick(&layers, i).map(|l| Command::SetProps { id: l, name: Some(format!("r{i}")), data: serde_json::Value::Null }),
                Op::Opacity(i, v) => pick(&layers, i).map(|l| Command::SetProps { id: l, name: None, data: json!({"opacity": v as f32 / 200.0}) }),
                Op::Draw(i) => pick(&cels, i).map(|c| Command::InsertStrokes { cel: c, items: vec![IndexedStroke { index: 0, stroke: stroke("", i as f32) }] }),
                Op::EraseFirst(i) => pick(&cels, i).and_then(|c| match &s.project.node(&c).unwrap().data {
                    NodeKind::Cel(cd) => cd.strokes.first().map(|st| Command::RemoveStrokes { cel: c.clone(), ids: vec![st.id.clone()] }),
                    _ => None,
                }),
                Op::Delete(i) => pick(&layers, i).map(|l| Command::DeleteNode { id: l }),
                Op::Move(a, b) => match (pick(&layers, a), pick(&layers, b)) {
                    (Some(x), Some(y)) => Some(Command::MoveNode { id: x, parent: Some(y), index: Some(0) }),
                    _ => None,
                },
                Op::Cells(i, f) => {
                    let tracks = match &s.project.node(&cut).unwrap().data { NodeKind::Cut(c) => c.xsheet.tracks.clone(), _ => vec![] };
                    match pick(&tracks, i) {
                        Some(t) => Some(Command::SetCells { cut: cut.clone(), track: t.id, cells: vec![CellEdit { frame: f as u32 % 48, cell: Some(CellRef::Cel { number: 1 + i as u32 % 3 }) }] }),
                        None => pick(&layers, i).map(|l| Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from(""), "T", TrackKind::Drawing, Some(l)) }),
                    }
                }
                Op::Undo => { if s.history().0.len() > base_depth { let _ = s.undo(); } None }
                Op::Redo => { let _ = s.redo(); None }
            };
            if let Some(c) = cmd {
                let _ = s.apply(c); // pode falhar (ex.: mover pasta para dentro de camada de vetor); o estado continua válido
            }
            prop_assert!(s.project.validate().is_ok(), "{:?}", s.project.validate());
        }
        let end = snapshot(&s);
        let mut undone = 0;
        while s.history().0.len() > base_depth {
            s.undo().unwrap();
            undone += 1;
        }
        prop_assert_eq!(snapshot(&s), base);
        for _ in 0..undone {
            s.redo().unwrap();
        }
        prop_assert_eq!(snapshot(&s), end);
    }
}


#[test]
fn project_with_timesheet_reopens_and_accepts_set_props() {
    let dir = std::env::temp_dir().join(format!("aiai-core-xsheet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = CoreSession::for_tests("xs");
    let (_ep, cut, layer, _cel) = build_basic(&mut s);
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from(""), "A", TrackKind::Drawing, Some(layer)) }).unwrap();
    let tid = match &s.project.node(&cut).unwrap().data { NodeKind::Cut(c) => c.xsheet.tracks[0].id.clone(), _ => unreachable!() };
    s.apply(Command::SetCells { cut: cut.clone(), track: tid, cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }, CellEdit { frame: 12, cell: Some(CellRef::Empty) }] }).unwrap();
    // set_props num corte com células (antes: "invalid type: string \"0\", expected u32")
    s.apply(Command::SetProps { id: cut.clone(), name: None, data: json!({"notes": "teste"}) }).unwrap();
    persist::save(&mut s, &dir).unwrap();
    let (s2, _) = persist::open(&dir).unwrap();
    assert_eq!(snapshot(&s2), snapshot(&s));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blobs_dropped_from_history_are_collected_but_planned_ones_survive() {
    let mut s = CoreSession::for_tests("gc");
    s.undo_limit = 4;
    let (_ep, _cut, _layer, cel) = build_basic(&mut s);
    // 80 pinceladas que trocam o mesmo tile: só o último + os 4 do undo continuam vivos
    for i in 0..80u32 {
        let b = s.blobs.put(vec![(i % 251) as u8; 300 + i as usize]);
        s.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 0, y: 0, blob: Some(b) }] }).unwrap();
    }
    assert!(s.blobs.len() < 80, "a coleta automática rodou: {} blobs", s.blobs.len());
    // tiles planejados antes de uma sequência de comandos não somem no meio dela
    let pending = s.blobs.put(vec![1; 999]);
    s.collect_garbage();
    assert!(s.blobs.contains(&pending), "planejado e ainda não aplicado sobrevive à coleta");
    s.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 1, y: 0, blob: Some(pending.clone()) }] }).unwrap();
    // depois de bastante trabalho, só sobra o que o documento e o undo usam
    for i in 0..40u32 {
        s.apply(Command::SetProps { id: cel.clone(), name: Some(format!("c{i}")), data: json!({}) }).unwrap();
    }
    s.collect_garbage();
    assert!(s.blobs.contains(&pending), "referenciado pelo documento fica");
    assert_eq!(s.blobs.len(), 2, "só os dois tiles do documento");
}

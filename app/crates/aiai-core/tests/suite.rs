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
            StrokePoint { x, y: 10.0, pressure: 0.5, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None },
            StrokePoint { x: x + 50.0, y: 60.0, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None },
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
        group: String::new(),
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
        Command::ReplaceFills { cel: cel.clone(), fills: vec![FillRegion { id: Id::from(""), seed: [3.0, 4.0], color: ColorRef::default(), mask: Some(blob.clone()), bounds: [0, 0, 10, 10], gradient: None }] },
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

// ------------------------------------------------------------------------------------------ robustez da pasta (S24 P0)

fn fresh_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aiai-core-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Projeto salvo com dois tiles; devolve (sessão, cel, camada, bytes do tile 1, hash do tile 1).
fn saved_with_tiles(dir: &std::path::Path) -> (CoreSession, Id, Id, Vec<u8>, BlobHash) {
    let mut s = CoreSession::for_tests("robusto");
    let (_ep, _cut, layer, cel) = build_basic(&mut s);
    let b0 = s.blobs.put(vec![1; 3000]);
    let bytes1 = vec![2; 5000];
    let b1 = s.blobs.put(bytes1.clone());
    s.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 0, y: 0, blob: Some(b0) }, TileEdit { x: 1, y: 0, blob: Some(b1.clone()) }] }).unwrap();
    persist::save(&mut s, dir).unwrap();
    (s, cel, layer, bytes1, b1)
}

fn rename(id: &Id, name: &str) -> Command {
    Command::SetProps { id: id.clone(), name: Some(name.into()), data: serde_json::Value::Null }
}

#[test]
fn truncated_blob_opens_with_report_keeps_reference_and_is_repaired() {
    let dir = fresh_dir("blob-trunc");
    let (s, cel, layer, bytes1, b1) = saved_with_tiles(&dir);
    drop(s);
    // queda de energia antiga: o renomear chegou ao disco, metade dos dados não
    let path = persist::blob_path(&dir, &b1);
    std::fs::write(&path, &bytes1[..2000]).unwrap();
    let json_before = std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap();

    let (mut s2, rep) = persist::open(&dir).expect("blob truncado não impede abrir");
    assert_eq!(rep.damaged_blobs.len(), 1, "{rep:?}");
    let d = &rep.damaged_blobs[0];
    assert_eq!(d.hash, b1);
    assert!(d.problem.contains("2000 bytes"), "{}", d.problem);
    assert!(d.used_by.iter().any(|u| u.contains("camada \"A\"") && u.contains("tile (1, 0)")), "{:?}", d.used_by);
    assert_eq!((d.kind, d.missing), ("tile", false), "existe, mas truncado");
    assert!(s2.blobs.is_missing(&b1) && !s2.blobs.contains(&b1));
    // Salvar não exige o conteúdo danificado, mantém a referência e não toca no arquivo ruim
    persist::save(&mut s2, &dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap(), json_before, "a referência continua no documento");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 2000);
    // desfazer de um comando que tira o tile danificado o devolve (referência herdada é aceita)
    s2.apply(Command::SetTiles { cel: cel.clone(), tiles: vec![TileEdit { x: 1, y: 0, blob: None }] }).unwrap();
    s2.undo().unwrap();

    // o mesmo conteúdo volta (repintado/importado de novo): o arquivo ruim de mesmo nome é regravado, não "confiado"
    assert_eq!(s2.blobs.put(bytes1.clone()), b1);
    s2.apply(rename(&layer, "A2")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes1, "o diário regravou o blob ruim");
    drop(s2);
    let (_s3, rep3) = persist::open(&dir).unwrap();
    assert!(rep3.damaged_blobs.is_empty(), "{rep3:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_found_copy_restores_a_missing_blob_only_when_the_content_matches() {
    let dir = fresh_dir("blob-restore");
    let (s, _cel, _layer, bytes1, b1) = saved_with_tiles(&dir);
    drop(s);
    let path = persist::blob_path(&dir, &b1);
    std::fs::remove_file(&path).unwrap();
    let (mut s2, rep) = persist::open(&dir).expect("blob ausente não impede abrir");
    let d = rep.damaged_blobs.iter().find(|d| d.hash == b1).expect("relatado");
    assert!(d.missing && d.kind == "tile", "{d:?}");
    // outro arquivo não serve: nada muda
    assert_eq!(persist::restore_blob(&mut s2, b"outro arquivo".to_vec()).unwrap(), None);
    assert!(s2.blobs.is_missing(&b1));
    // a cópia certa volta para a pasta e para a memória
    assert_eq!(persist::restore_blob(&mut s2, bytes1.clone()).unwrap(), Some(b1.clone()));
    assert!(!s2.blobs.is_missing(&b1) && s2.blobs.contains(&b1));
    assert_eq!(std::fs::read(&path).unwrap(), bytes1);
    drop(s2);
    let (_s3, rep3) = persist::open(&dir).unwrap();
    assert!(rep3.damaged_blobs.is_empty(), "{rep3:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn truncated_last_wal_line_is_reported_skipped_and_new_commands_survive() {
    let dir = fresh_dir("wal-trunc");
    let (mut s, _cel, layer, _, _) = saved_with_tiles(&dir);
    s.apply(rename(&layer, "B")).unwrap();
    s.apply(rename(&layer, "C")).unwrap();
    drop(s);
    // queda no meio da escrita: meia linha sem quebra no fim
    let wal = dir.join(persist::WAL_FILE);
    let mut f = std::fs::OpenOptions::new().append(true).open(&wal).unwrap();
    std::io::Write::write_all(&mut f, b"{\"seq\":9,\"revision\":99,\"op\":\"app").unwrap();
    drop(f);

    let (mut s2, rep) = persist::open(&dir).expect("linha truncada não impede abrir");
    assert_eq!(rep.replayed, 2, "{rep:?}");
    assert!(rep.wal_truncated_at.is_some() && rep.wal_tail_only && rep.wal_lines_skipped == 1, "{rep:?}");
    let copy = rep.wal_backup.clone().expect("cópia do diário original");
    assert!(std::fs::read_to_string(&copy).unwrap().ends_with("\"op\":\"app"), "a cópia guarda a linha ruim");
    assert!(std::fs::read(&wal).unwrap().ends_with(b"\n"), "o diário termina na última linha boa");
    assert_eq!(s2.project.node(&layer).unwrap().name, "C");
    // comando novo depois da recuperação não gruda na linha ruim: a próxima abertura recupera os 3
    s2.apply(rename(&layer, "D")).unwrap();
    drop(s2);
    let (s3, rep3) = persist::open(&dir).unwrap();
    assert_eq!((rep3.replayed, rep3.wal_truncated_at), (3, None), "{rep3:?}");
    assert_eq!(s3.project.node(&layer).unwrap().name, "D");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pid de um processo vivo que não é este (filho que espera enquanto o teste roda).
fn live_child_pid() -> u32 {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
        c
    } else {
        let mut c = std::process::Command::new("sleep");
        c.arg("30");
        c
    };
    let child = c.stdout(std::process::Stdio::null()).spawn().expect("processo filho");
    let pid = child.id();
    std::mem::forget(child);
    pid
}

#[test]
fn project_lock_live_opens_read_only_stale_is_replaced_and_clean_close_removes_it() {
    let dir = fresh_dir("lock");
    let (s, _cel, layer, _, _) = saved_with_tiles(&dir);
    let lock_path = dir.join(aiai_core::lock::LOCK_FILE);
    assert!(lock_path.is_file(), "salvar trava a pasta");
    drop(s);
    assert!(!lock_path.exists(), "fechar limpo solta a trava");

    // outro processo vivo com o projeto aberto
    let other = aiai_core::lock::LockInfo { pid: live_child_pid(), host: aiai_core::lock::host_name(), started_ms: u64::MAX / 4, exe: "aiai-mcp.exe".into(), token: "outro".into() };
    std::fs::write(&lock_path, serde_json::to_string(&other).unwrap()).unwrap();
    let wal_before = std::fs::read(dir.join(persist::WAL_FILE)).unwrap();
    let (mut ro, rep) = persist::open(&dir).unwrap();
    assert_eq!(rep.locked_by.as_ref().map(|i| i.pid), Some(other.pid));
    assert!(rep.read_only() && ro.lock().is_none() && ro.wal_mut().is_none(), "só leitura: sem trava nem diário");
    ro.apply(rename(&layer, "X")).unwrap();
    assert_eq!(std::fs::read(dir.join(persist::WAL_FILE)).unwrap(), wal_before, "o diário do outro não é tocado");
    let err = persist::save(&mut ro, &dir).unwrap_err();
    assert!(matches!(err, CoreError::Locked(_)), "{err}");
    assert!(err.to_string().contains("aiai-mcp.exe"), "{err}");
    drop(ro);
    assert!(lock_path.is_file(), "a trava do outro continua");

    // assumir
    let (mut mine, rep) = persist::open_with(&dir, true).unwrap();
    assert_eq!(rep.replaced_lock.as_ref().map(|i| i.token.as_str()), Some("outro"));
    persist::save(&mut mine, &dir).unwrap();
    drop(mine);

    // trava velha (processo que caiu): substituída sem perguntar
    let dead = aiai_core::lock::LockInfo { pid: 0x7fff_fff1, token: "velho".into(), ..other };
    std::fs::write(&lock_path, serde_json::to_string(&dead).unwrap()).unwrap();
    let (s4, rep) = persist::open(&dir).unwrap();
    assert!(!rep.read_only() && rep.replaced_lock.as_ref().is_some_and(|i| i.token == "velho"), "{rep:?}");
    assert!(s4.lock().is_some());
    drop(s4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_keeps_three_rotating_document_backups_without_blobs() {
    let dir = fresh_dir("backup");
    let (mut s, _cel, layer, _, _) = saved_with_tiles(&dir);
    for name in ["B", "C", "D", "E"] {
        s.apply(rename(&layer, name)).unwrap();
        persist::save(&mut s, &dir).unwrap();
        let (s2, rep) = persist::open(&dir).unwrap();
        assert!(rep.backup.is_some(), "documento novo = cópia nova");
        s = s2;
    }
    let (s2, rep) = persist::open(&dir).unwrap();
    assert!(rep.backup.is_none(), "igual à última: não repete");
    drop(s2);
    let list = persist::backups(&dir, "open");
    assert_eq!(list.len(), persist::BACKUP_KEEP);
    assert_eq!(std::fs::read(&list[0]).unwrap(), std::fs::read(dir.join(persist::PROJECT_FILE)).unwrap(), "1 = mais nova");
    assert!(std::fs::read_to_string(&list[2]).unwrap().contains("\"C\""), "as mais velhas saem da fila");
    // cópia periódica do documento em memória
    s.apply(rename(&layer, "F")).unwrap();
    let auto = persist::backup_document(&dir, &s.project).unwrap().expect("cópia");
    assert!(std::fs::read_to_string(&auto).unwrap().contains("\"F\""));
    assert!(persist::backup_document(&dir, &s.project).unwrap().is_none(), "sem mudança, sem cópia nova");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_settings_file_gets_one_backup_copy_before_overwrite() {
    // o caso do brushes.json: ilegível -> cópia ao lado antes de qualquer gravação; abrir de novo não acumula cópias
    let dir = fresh_dir("corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("brushes.json");
    std::fs::write(&path, b"[{\"name\": \"Nuvem\", \"size\": 3").unwrap();
    let c1 = persist::keep_corrupt_copy(&path).unwrap();
    assert!(c1.file_name().unwrap().to_string_lossy().starts_with("brushes.json.corrupt-"), "{c1:?}");
    assert_eq!(std::fs::read(&c1).unwrap(), std::fs::read(&path).unwrap());
    assert_eq!(persist::keep_corrupt_copy(&path).unwrap(), c1, "mesma cópia");
    persist::write_file_atomic(&path, b"[]").unwrap();
    assert_eq!(std::fs::read(&c1).unwrap(), b"[{\"name\": \"Nuvem\", \"size\": 3", "a cópia sobrevive à regravação");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lowering_the_undo_limit_drops_the_excess_at_once() {
    // Preferências > Desempenho > níveis de desfazer: baixar o limite com histórico cheio não deixa o excesso para trás
    let mut s = CoreSession::for_tests("undo-limit");
    let (_ep, _cut, _layer, cel) = build_basic(&mut s);
    for i in 0..30u32 {
        s.apply(Command::SetProps { id: cel.clone(), name: Some(format!("c{i}")), data: json!({}) }).unwrap();
    }
    assert!(s.history().0.len() >= 30);
    s.undo_limit = 10;
    s.apply(Command::SetProps { id: cel.clone(), name: Some("ultimo".into()), data: json!({}) }).unwrap();
    assert_eq!(s.history().0.len(), 10, "o histórico foi cortado de uma vez no limite novo");
}

#[test]
fn merge_last_makes_one_undo_step() {
    // auditoria V2-04: a camada criada sozinha e o traço nela viram um passo de desfazer/refazer
    let mut s = CoreSession::for_tests("merge");
    let ep = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "EP".into(), data: NodeKind::Episode(EpisodeData::default()) }).unwrap().created[0].clone();
    let r0 = s.project.revision;
    let a = s.apply(Command::CreateNode { id: None, parent: Some(ep.clone()), index: None, name: "S".into(), data: NodeKind::Scene(SceneData::default()) }).unwrap().created[0].clone();
    s.apply(Command::CreateNode { id: None, parent: Some(a.clone()), index: None, name: "C".into(), data: NodeKind::Cut(CutData::new(12)) }).unwrap();
    let before = s.history().0.len();
    assert!(s.merge_last(2, "cena e corte"));
    assert_eq!(s.history().0.len(), before - 1);
    assert_eq!(s.history().0.last().map(String::as_str), Some("cena e corte"));
    s.undo().unwrap();
    assert!(s.project.node(&a).is_none(), "um desfazer tira os dois");
    assert!(s.project.revision > r0);
    s.redo().unwrap();
    assert!(s.project.node(&a).is_some_and(|n| n.children.len() == 1), "um refazer põe os dois");
    assert!(!s.merge_last(5, "x"), "não junta mais do que existe");
}

/// Rodada 7 (CX10 C10-06): falha ao gravar o diário não deixa comando aplicado sem passo de desfazer — o comando volta,
/// a revisão e o histórico ficam como antes e o diário não guarda linha parcial (reabrir dá o mesmo projeto).
#[test]
fn wal_failure_rolls_the_command_back() {
    let dir = std::env::temp_dir().join(format!("aiai-walfail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = CoreSession::for_tests("walfail");
    let (_ep, _cut, _layer, cel) = build_basic(&mut s);
    persist::save(&mut s, &dir).unwrap();
    s.apply(Command::SetProps { id: cel.clone(), name: Some("antes".into()), data: json!({}) }).unwrap();
    let (rev, hist, log) = (s.project.revision, s.history().0.len(), s.log().len());
    // conteúdo sem a proveniência (metadado de auditoria que o inverso regrava)
    let content = |p: &Project| {
        let mut v = serde_json::to_value(p).unwrap();
        for n in v["nodes"].as_object_mut().unwrap().values_mut() {
            n.as_object_mut().unwrap().remove("provenance");
        }
        v.to_string()
    };
    let snap = content(&s.project);
    s.wal_mut().unwrap().fail_next_append_for_tests();
    let e = s.apply(Command::SetProps { id: cel.clone(), name: Some("depois".into()), data: json!({}) });
    assert!(e.is_err());
    assert_eq!(content(&s.project), snap, "o comando voltou");
    assert_eq!((s.project.revision, s.history().0.len(), s.log().len()), (rev, hist, log));
    // undo com o diário falhando também volta
    s.wal_mut().unwrap().fail_next_append_for_tests();
    assert!(s.undo().is_err());
    assert_eq!(content(&s.project), snap);
    assert_eq!(s.history().0.len(), hist, "o passo continua desfazível");
    // o próximo comando grava normalmente e a recuperação pelo diário dá o mesmo estado, sem linha cortada
    s.apply(Command::SetProps { id: cel.clone(), name: Some("ok".into()), data: json!({}) }).unwrap();
    let want = serde_json::to_string(&s.project.nodes.get(&cel).unwrap().name).unwrap();
    drop(s);
    let (re, rep) = persist::open(&dir).unwrap();
    assert_eq!(rep.wal_truncated_at, None, "nenhuma linha parcial no diário");
    assert_eq!(serde_json::to_string(&re.project.nodes.get(&cel).unwrap().name).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// R16 REQ-P01/P03: cada entrada do log leva a fonte da sessão naquele momento; juntar passos de fontes diferentes dá
/// `Mixed`; entrada antiga sem o campo lê como `UnknownLegacy`.
#[test]
fn log_entries_carry_input_source_and_merge_mixes() {
    let mut s = CoreSession::for_tests("fonte");
    s.source = InputSource::DirectToolCall;
    let ep = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "EP".into(), data: NodeKind::Episode(EpisodeData::default()) }).unwrap().created[0].clone();
    assert_eq!(s.log().last().unwrap().source, InputSource::DirectToolCall);
    s.source = InputSource::BridgeInjected;
    let a = s.apply(Command::CreateNode { id: None, parent: Some(ep.clone()), index: None, name: "S".into(), data: NodeKind::Scene(SceneData::default()) }).unwrap().created[0].clone();
    s.apply(Command::CreateNode { id: None, parent: Some(a), index: None, name: "C".into(), data: NodeKind::Cut(CutData::new(12)) }).unwrap();
    assert!(s.merge_last(2, "mesmo"));
    assert_eq!(s.log().last().unwrap().source, InputSource::BridgeInjected, "mesma fonte continua");
    s.source = InputSource::UiPlatformUnattributed;
    s.apply(Command::SetProps { id: ep.clone(), name: Some("E".into()), data: json!({}) }).unwrap();
    assert!(s.merge_last(2, "misto"));
    assert_eq!(s.log().last().unwrap().source, InputSource::Mixed, "fontes diferentes juntadas = Mixed");
    s.undo().unwrap();
    assert_eq!(s.log().last().unwrap().source, InputSource::UiPlatformUnattributed, "desfazer leva a fonte de quem desfez");
    let mut v = serde_json::to_value(s.log().last().unwrap()).unwrap();
    assert_eq!(v["source"], "ui_platform_unattributed");
    v.as_object_mut().unwrap().remove("source");
    let old: LogEntry = serde_json::from_value(v).unwrap();
    assert_eq!(old.source, InputSource::UnknownLegacy, "entrada antiga sem o campo");
}

/// Projeto dividido por corte (auditoria 4.6k): manifesto + um arquivo por corte, regravado só quando muda; abre igual;
/// desligar a divisão volta ao arquivo único; corte travado por outro processo não aceita edição nem é sobrescrito.
#[test]
fn split_cuts_storage_round_trips_and_cut_locks_guard_edits() {
    let dir = std::env::temp_dir().join(format!("aiai-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = CoreSession::for_tests("t");
    let (_ep, cut_a, _layer, cel_a) = build_basic(&mut s);
    let sc = s.project.nodes.get(&cut_a).unwrap().parent.clone().unwrap();
    let cut_b = s.apply(Command::CreateNode { id: None, parent: Some(sc), index: None, name: "C002".into(), data: NodeKind::Cut(CutData::new(24)) }).unwrap().created[0].clone();
    let lb = s.apply(Command::CreateNode { id: None, parent: Some(cut_b.clone()), index: None, name: "B".into(), data: NodeKind::Layer(LayerData::new(LayerType::Vector)) }).unwrap().created[0].clone();
    let cel_b = s.apply(Command::CreateNode { id: None, parent: Some(lb), index: None, name: "B1".into(), data: NodeKind::Cel(CelData::new(1)) }).unwrap().created[0].clone();
    s.apply(Command::InsertStrokes { cel: cel_a.clone(), items: vec![IndexedStroke { index: 0, stroke: stroke("sa", 5.0) }] }).unwrap();
    s.apply(Command::InsertStrokes { cel: cel_b.clone(), items: vec![IndexedStroke { index: 0, stroke: stroke("sb", 9.0) }] }).unwrap();
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"split_cuts": true}}) }).unwrap();
    persist::save(&mut s, &dir).unwrap();
    let manifest = std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap();
    let fa = dir.join("cuts").join(format!("{}.json", aiai_core::lock::cut_file_stem(&cut_a)));
    let fb = dir.join("cuts").join(format!("{}.json", aiai_core::lock::cut_file_stem(&cut_b)));
    assert!(fa.is_file() && fb.is_file(), "um arquivo por corte");
    assert!(manifest.contains("cut_files") && !manifest.contains(&format!("\"{}\"", cel_a.0)), "o manifesto não guarda os nós dos cortes");
    // abre igual (mesmo conteúdo, com as camadas e os traços dos dois cortes)
    let before = snapshot(&s);
    drop(s);
    let (mut s2, rep) = persist::open(&dir).unwrap();
    assert!(!rep.read_only());
    assert_eq!(snapshot(&s2), before);
    // mexer só no corte A não regrava o arquivo do B
    let b_bytes = std::fs::read(&fb).unwrap();
    let b_time = std::fs::metadata(&fb).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    s2.apply(Command::InsertStrokes { cel: cel_a.clone(), items: vec![IndexedStroke { index: 1, stroke: stroke("sa2", 30.0) }] }).unwrap();
    assert!(s2.claimed_cuts().any(|c| *c == cut_a), "o corte editado ficou travado para esta sessão");
    assert!(dir.join("cuts").join(format!("{}.lock", aiai_core::lock::cut_file_stem(&cut_a))).is_file());
    persist::save(&mut s2, &dir).unwrap();
    assert_eq!(std::fs::read(&fb).unwrap(), b_bytes);
    assert_eq!(std::fs::metadata(&fb).unwrap().modified().unwrap(), b_time, "o corte B não foi regravado");
    // outro processo vivo (o pid 4, "System") com a trava do corte B: editar o B volta com erro; o arquivo dele fica
    let other = json!({"pid": 4, "host": aiai_core::lock::host_name(), "started_ms": 0, "exe": "outro.exe", "token": "x"});
    std::fs::write(aiai_core::lock::cut_lock_path(&dir, &cut_b), other.to_string()).unwrap();
    let rev = s2.project.revision;
    let r = s2.apply(Command::InsertStrokes { cel: cel_b.clone(), items: vec![IndexedStroke { index: 1, stroke: stroke("sb2", 40.0) }] });
    assert!(matches!(r, Err(CoreError::Locked(_))), "{r:?}");
    assert_eq!(s2.project.revision, rev, "o comando foi desfeito");
    std::fs::write(&fb, b"{\"format\":2,\"cut\":\"mexido por outro\",\"nodes\":{}}\n").unwrap_or(());
    let theirs = std::fs::read(&fb).unwrap();
    s2.apply(Command::InsertStrokes { cel: cel_a.clone(), items: vec![IndexedStroke { index: 2, stroke: stroke("sa3", 50.0) }] }).unwrap();
    persist::save(&mut s2, &dir).unwrap();
    assert_eq!(std::fs::read(&fb).unwrap(), theirs, "o corte travado por outro não é sobrescrito");
    std::fs::remove_file(aiai_core::lock::cut_lock_path(&dir, &cut_b)).unwrap();
    // desligar a divisão: volta ao arquivo único e os arquivos de corte saem
    s2.apply(Command::SetProjectProps { patch: json!({"settings": {"split_cuts": false}}) }).unwrap();
    persist::save(&mut s2, &dir).unwrap();
    assert!(!fa.exists() && !fb.exists());
    let whole = std::fs::read_to_string(dir.join(persist::PROJECT_FILE)).unwrap();
    assert!(!whole.contains("cut_files") && whole.contains(&cel_a.0));
    drop(s2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Projeto dividido aberto enquanto outro processo tem a pasta: só dá para editar dentro de cortes, e salvar grava só
/// os cortes travados por esta sessão (o manifesto e o diário ficam com o dono).
#[test]
fn read_only_split_session_saves_only_its_cuts() {
    let dir = std::env::temp_dir().join(format!("aiai-split-ro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = CoreSession::for_tests("t");
    let (_ep, cut_a, _layer, cel_a) = build_basic(&mut s);
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"split_cuts": true}}) }).unwrap();
    persist::save(&mut s, &dir).unwrap();
    drop(s);
    // o dono da pasta é outro processo vivo
    let other = json!({"pid": 4, "host": aiai_core::lock::host_name(), "started_ms": 0, "exe": "outro.exe", "token": "x"});
    std::fs::write(dir.join(aiai_core::lock::LOCK_FILE), other.to_string()).unwrap();
    let manifest = std::fs::read(dir.join(persist::PROJECT_FILE)).unwrap();
    let (mut s, rep) = persist::open(&dir).unwrap();
    assert!(rep.read_only());
    // fora dos cortes (um personagem novo): recusado
    let r = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "Pal".into(), data: NodeKind::Palette(PaletteData::default()) });
    assert!(matches!(r, Err(CoreError::Locked(_))), "{r:?}");
    // dentro do corte: vale, e salvar grava o corte
    s.apply(Command::InsertStrokes { cel: cel_a.clone(), items: vec![IndexedStroke { index: 0, stroke: stroke("ro", 7.0) }] }).unwrap();
    persist::save(&mut s, &dir).unwrap();
    assert_eq!(std::fs::read(dir.join(persist::PROJECT_FILE)).unwrap(), manifest, "o manifesto é do dono");
    let fa = dir.join("cuts").join(format!("{}.json", aiai_core::lock::cut_file_stem(&cut_a)));
    assert!(std::fs::read_to_string(&fa).unwrap().contains("\"ro\""), "o corte foi gravado");
    drop(s);
    std::fs::remove_file(dir.join(aiai_core::lock::LOCK_FILE)).unwrap();
    let (s, _) = persist::open(&dir).unwrap();
    let NodeKind::Cel(c) = &s.project.nodes[&cel_a].data else { panic!() };
    assert!(c.strokes.iter().any(|t| t.id.0 == "ro"), "o dono abre com o traço do outro");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

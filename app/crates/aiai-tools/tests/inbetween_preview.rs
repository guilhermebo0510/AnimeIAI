//! Prévia de intercalação de um quadro (R16 MVP-0): testes de aceite do núcleo (AT-01, 03, 04, 06–10, 15–21).

use aiai_anim::MotionMode;
use aiai_core::command::CellEdit;
use aiai_core::*;
use aiai_tools::inbetween_preview::{self as ibp, InbetweenPreviewRequest, NoticeCode};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn pt(x: f32, y: f32) -> StrokePoint {
    StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
}

fn st(id: &str, pts: &[[f32; 2]], closed: bool) -> Stroke {
    Stroke {
        id: Id::from(id),
        points: pts.iter().map(|p| pt(p[0], p[1])).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::GPen,
        width: 3.0,
        profile: Default::default(),
        color: ColorRef::Direct { rgba: Rgba::BLACK },
        opacity: 1.0,
        closed,
        aliased: false,
        color_trace: false,
        group: String::new(),
    }
}

fn shift(s: &Stroke, dx: f32, dy: f32, id: &str) -> Stroke {
    let mut o = s.clone();
    o.id = Id::from(id);
    for q in &mut o.points {
        q.x += dx;
        q.y += dy;
    }
    o
}

fn circle(id: &str, cx: f32, cy: f32, r: f32, n: usize, start: usize) -> Stroke {
    let pts: Vec<[f32; 2]> = (0..n).map(|i| {
        let a = ((i + start) % n) as f32 / n as f32 * std::f32::consts::TAU;
        [cx + r * a.cos(), cy + r * a.sin()]
    }).collect();
    st(id, &pts, true)
}

struct Fix {
    s: CoreSession,
    cut: Id,
    layer: Id,
    track: Id,
}

/// Corte de 24 quadros, camada vetorial "A" com faixa; `cels` = (número, papel, traços); `cells` = (quadro, número ou 0 = vazio).
fn fixture(cels: &[(u32, CelRole, Vec<Stroke>)], cells: &[(u32, u32)]) -> Fix {
    let mut s = CoreSession::for_tests("ibp");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 640, "height": 360}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(24)));
    let layer = create(&mut s, Some(&cut), "A", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    for (n, role, strokes) in cels {
        let mut c = CelData::new(*n);
        c.role = *role;
        c.strokes = strokes.clone();
        create(&mut s, Some(&layer), &format!("A{n}"), NodeKind::Cel(c));
    }
    let track = Id::from("trk-a");
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(track.clone(), "A", TrackKind::Drawing, Some(layer.clone())) }).unwrap();
    let edits: Vec<CellEdit> = cells.iter().map(|&(f, n)| CellEdit { frame: f, cell: Some(if n == 0 { CellRef::Empty } else { CellRef::Cel { number: n } }) }).collect();
    s.apply(Command::SetCells { cut: cut.clone(), track: track.clone(), cells: edits }).unwrap();
    s.source = InputSource::DirectToolCall;
    Fix { s, cut, layer, track }
}

/// Duas chaves: uma linha aberta e um contorno fechado andando 80 px para a direita.
fn two_keys() -> Fix {
    let line = st("la", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0]], false);
    let ring = circle("ra", 300.0, 200.0, 30.0, 12, 0);
    let a = vec![line.clone(), ring.clone()];
    let b = vec![shift(&line, 80.0, 0.0, "lb"), shift(&ring, 80.0, 0.0, "rb")];
    fixture(&[(1, CelRole::Key, a), (2, CelRole::Key, b)], &[(0, 1), (8, 2)])
}

fn track_cells(f: &Fix) -> Vec<(u32, CellRef)> {
    let cut = match &f.s.project.node(&f.cut).unwrap().data {
        NodeKind::Cut(c) => c,
        _ => unreachable!(),
    };
    cut.xsheet.track(&f.track).unwrap().cells.iter().map(|(k, v)| (*k, *v)).collect()
}

fn req(cut: &Id, layer: &Id, frame: u32) -> InbetweenPreviewRequest {
    InbetweenPreviewRequest::new(cut.clone(), layer.clone(), frame)
}

fn codes(n: &[ibp::Notice]) -> Vec<NoticeCode> {
    n.iter().map(|x| x.code).collect()
}

#[test]
fn preview_resolves_keys_and_writes_nothing_then_discard_leaves_project_identical() {
    // AT-01
    let mut f = two_keys();
    let before = serde_json::to_string(&f.s.project).unwrap();
    let rev = f.s.project.revision;
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert_eq!((p.frame_a, p.frame_b, p.key_a, p.key_b), (0, 8, 1, 2));
    assert!((p.t - 0.5).abs() < 1e-6);
    assert_eq!(p.candidate.len(), 2);
    assert!(p.refusals.is_empty(), "{:?}", p.refusals);
    assert!(p.acceptable().is_ok());
    // a linha anda junto: no meio do caminho
    let x0 = p.candidate[0].points[0].x;
    assert!((x0 - 140.0).abs() < 1.0, "x = {x0}");
    assert_eq!(p.pairs.len(), 2);
    assert_eq!(p.pairs[0].a.as_ref().map(|i| i.0.as_str()), Some("la"));
    assert_eq!(p.pairs[0].b.as_ref().map(|i| i.0.as_str()), Some("lb"));
    ibp::discard(&mut f.s, p.id).unwrap();
    assert_eq!(serde_json::to_string(&f.s.project).unwrap(), before, "byte a byte igual");
    assert_eq!(f.s.project.revision, rev);
    assert!(ibp::current(&f.s).is_none());
}

#[test]
fn accept_creates_one_cel_and_restoring_entry_and_second_accept_fails() {
    // AT-06, AT-08: A3 exposto em 10–12; aceitar em 11 → 10 = A3, 11 = novo, 12 = A3
    let line = st("l3", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0]], false);
    let l2 = shift(&line, 60.0, 0.0, "l2");
    let mut f = fixture(&[(1, CelRole::Key, vec![line.clone()]), (2, CelRole::Key, vec![l2]), (3, CelRole::Key, vec![line.clone()])], &[(0, 1), (10, 3), (13, 2)]);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 11)).unwrap();
    assert_eq!((p.frame_a, p.frame_b, p.key_a, p.key_b), (10, 13, 3, 2));
    let scope = ibp::accept_scope(&f.s.project, &p).unwrap();
    assert_eq!(scope.button_text("A"), "Criar desenho A4 no quadro 12 (o quadro 13 continua mostrando A3)");
    let acc = ibp::accept(&mut f.s, p.id, "agent:claude").unwrap();
    assert_eq!(acc.number, 4);
    assert_eq!(acc.frames, vec![11, 12]);
    let cells = track_cells(&f);
    assert_eq!(cells, vec![(0, CellRef::Cel { number: 1 }), (10, CellRef::Cel { number: 3 }), (11, CellRef::Cel { number: 4 }), (12, CellRef::Cel { number: 3 }), (13, CellRef::Cel { number: 2 })]);
    let cel = f.s.project.cel_by_number(&f.layer, 4).unwrap();
    match &cel.data {
        NodeKind::Cel(c) => {
            assert_eq!(c.role, CelRole::Inbetween);
            assert_eq!(c.strokes.len(), 1);
            assert!(!c.strokes[0].id.0.is_empty(), "o id do traço é resolvido no commit");
        }
        _ => unreachable!(),
    }
    assert_eq!(f.s.history().0.last().map(String::as_str), Some("Intercalar no quadro 12 entre 3 e 2"));
    // segundo aceite
    let e = ibp::accept(&mut f.s, p.id, "agent:claude").unwrap_err().to_string();
    assert!(e.contains("já usada"), "{e}");
    let cels = f.s.project.node(&f.layer).unwrap().children.len();
    assert_eq!(cels, 4, "um único cel criado");
}

#[test]
fn accept_next_frame_rules_explicit_entry_and_empty() {
    // 12 já tem entrada: não mexe; 12 vazio: grava Empty
    let line = st("l", &[[100.0, 100.0], [180.0, 100.0]], false);
    let l2 = shift(&line, 60.0, 0.0, "l2");
    let mut f = fixture(&[(1, CelRole::Key, vec![line.clone()]), (2, CelRole::Key, vec![l2])], &[(0, 1), (5, 1), (6, 2)]);
    // bloco 0..6 é A1 (a entrada em 5 repete A1: o mesmo bloco); a prévia em 5 grava só 5 (6 tem entrada)
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 5)).unwrap();
    assert_eq!((p.frame_a, p.frame_b), (0, 6));
    ibp::accept(&mut f.s, p.id, "a").unwrap();
    assert_eq!(track_cells(&f), vec![(0, CellRef::Cel { number: 1 }), (5, CellRef::Cel { number: 3 }), (6, CellRef::Cel { number: 2 })]);

    let mut g = fixture(&[(1, CelRole::Key, vec![line.clone()]), (2, CelRole::Key, vec![shift(&line, 9.0, 0.0, "x")])], &[(0, 1), (3, 0), (8, 2)]);
    let p = ibp::preview(&mut g.s, &req(&g.cut, &g.layer, 3)).unwrap();
    ibp::accept(&mut g.s, p.id, "a").unwrap();
    assert_eq!(track_cells(&g), vec![(0, CellRef::Cel { number: 1 }), (3, CellRef::Cel { number: 3 }), (4, CellRef::Empty), (8, CellRef::Cel { number: 2 })]);
}

#[test]
fn undo_redo_take_cel_and_entries_together_and_no_track_refuses() {
    // AT-07
    let mut f = two_keys();
    let before = track_cells(&f);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 3)).unwrap();
    ibp::accept(&mut f.s, p.id, "a").unwrap();
    let after = track_cells(&f);
    assert_ne!(before, after);
    f.s.undo().unwrap();
    assert_eq!(track_cells(&f), before);
    assert!(f.s.project.cel_by_number(&f.layer, 3).is_none());
    f.s.redo().unwrap();
    assert_eq!(track_cells(&f), after);
    assert!(f.s.project.cel_by_number(&f.layer, 3).is_some());
    // sem faixa: erro, nada muda
    let mut g = two_keys();
    g.s.apply(Command::RemoveTrack { cut: g.cut.clone(), track: g.track.clone() }).unwrap();
    let snap = serde_json::to_string(&g.s.project).unwrap();
    let e = ibp::preview(&mut g.s, &req(&g.cut, &g.layer, 3)).unwrap_err().to_string();
    assert!(e.contains("configure o timesheet"), "{e}");
    assert_eq!(serde_json::to_string(&g.s.project).unwrap(), snap);
}

#[test]
fn unsupported_attributes_are_refused_and_block_accept() {
    // AT-09
    let base = st("a", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0]], false);
    let mut w = base.clone();
    w.id = Id::from("w");
    w.points[1].width = 1.5;
    let mut c = shift(&base, 0.0, 40.0, "c");
    c.points[1].corner = true;
    let mut h = shift(&base, 0.0, 80.0, "h");
    h.points[1].handle = Some([1.0, 1.0]);
    let mut pa = shift(&base, 0.0, 120.0, "pa");
    pa.profile = WidthProfile(vec![[0.0, 1.0], [1.0, 0.5]]);
    let mut pb = shift(&pa, 10.0, 0.0, "pb");
    pb.profile = WidthProfile(vec![[0.0, 0.5], [1.0, 1.0]]);
    let mut bz = st("bz", &[[100.0, 300.0], [120.0, 280.0], [140.0, 280.0], [160.0, 300.0]], false);
    bz.geometry = StrokeGeometry::Bezier;
    let mut bz2 = st("bz2", &[[110.0, 300.0], [120.0, 270.0], [140.0, 270.0], [160.0, 300.0], [180.0, 330.0], [200.0, 330.0], [220.0, 300.0]], false);
    bz2.geometry = StrokeGeometry::Bezier;
    let a = vec![w.clone(), c.clone(), h.clone(), pa, bz];
    let b = vec![shift(&w, 10.0, 0.0, "w2"), shift(&c, 10.0, 0.0, "c2"), shift(&h, 10.0, 0.0, "h2"), pb, bz2];
    let mut f = fixture(&[(1, CelRole::Key, a), (2, CelRole::Key, b)], &[(0, 1), (8, 2)]);
    let mut r = req(&f.cut, &f.layer, 4);
    // pares explícitos para o Bézier (contagens diferentes não casam pela estrutura)
    r.manual_pairs = vec![(Id::from("bz"), Id::from("bz2"))];
    let p = ibp::preview(&mut f.s, &r).unwrap();
    let got = codes(&p.refusals);
    for want in [NoticeCode::PointWidth, NoticeCode::PointCorner, NoticeCode::PointHandle, NoticeCode::ProfileDiffers, NoticeCode::BezierResampled] {
        assert!(got.contains(&want), "falta {want:?} em {got:?}");
    }
    assert!(p.candidate.is_empty());
    assert!(p.acceptable().is_err());
    let e = ibp::accept(&mut f.s, p.id, "a").unwrap_err().to_string();
    assert!(e.contains("recusa"), "{e}");
}

#[test]
fn cel_transform_refuses_whole_preview() {
    // AT-16
    let line = st("l", &[[100.0, 100.0], [180.0, 100.0]], false);
    for (ta, tb) in [([40.0, 0.0, 1.0, 0.0], [40.0, 0.0, 1.0, 0.0]), ([0.0, 0.0, 1.0, 0.0], [40.0, 0.0, 1.0, 0.0])] {
        let mut f = fixture(&[(1, CelRole::Key, vec![line.clone()]), (2, CelRole::Key, vec![shift(&line, 9.0, 0.0, "m")])], &[(0, 1), (8, 2)]);
        for (n, t) in [(1u32, ta), (2, tb)] {
            let id = f.s.project.cel_by_number(&f.layer, n).unwrap().id.clone();
            f.s.apply(Command::SetProps { id, name: None, data: json!({ "transform": t }) }).unwrap();
        }
        let snap = serde_json::to_string(&f.s.project).unwrap();
        let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
        assert!(codes(&p.refusals).contains(&NoticeCode::CelTransform), "{:?}", p.refusals);
        assert!(ibp::accept(&mut f.s, p.id, "a").is_err());
        assert_eq!(serde_json::to_string(&f.s.project).unwrap(), snap, "sem efeito");
    }
}

#[test]
fn reversed_direct_pairs_are_refused() {
    // AT-17: aberto invertido (inclusive Linear) e fechado com começo deslocado
    let line = st("l", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0], [220.0, 120.0]], false);
    let mut rev = shift(&line, 5.0, 0.0, "r");
    rev.points.reverse();
    for motion in [MotionMode::Global, MotionMode::Linear] {
        let mut f = fixture(&[(1, CelRole::Key, vec![line.clone()]), (2, CelRole::Key, vec![rev.clone()])], &[(0, 1), (8, 2)]);
        let mut r = req(&f.cut, &f.layer, 4);
        r.motion = motion;
        let p = ibp::preview(&mut f.s, &r).unwrap();
        assert!(codes(&p.refusals).contains(&NoticeCode::SenseMismatch), "{motion:?}: {:?}", p.refusals);
        assert!(p.refusals.iter().any(|n| n.text.contains("sentido diferente entre A e B")));
    }
    let ra = circle("ca", 300.0, 200.0, 40.0, 12, 0);
    let rb = circle("cb", 310.0, 200.0, 40.0, 12, 3);
    let mut f = fixture(&[(1, CelRole::Key, vec![ra]), (2, CelRole::Key, vec![rb])], &[(0, 1), (8, 2)]);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::SenseMismatch), "{:?}", p.refusals);
    // o mesmo contorno no mesmo sentido passa
    let mut g = two_keys();
    let p = ibp::preview(&mut g.s, &req(&g.cut, &g.layer, 4)).unwrap();
    assert!(p.refusals.is_empty(), "{:?}", p.refusals);
}

#[test]
fn manual_pairs_errors_are_explicit() {
    // AT-18
    let mut f = two_keys();
    let mut r = req(&f.cut, &f.layer, 4);
    r.manual_pairs = vec![(Id::from("sumiu"), Id::from("lb"))];
    let e = ibp::preview(&mut f.s, &r).unwrap_err().to_string();
    assert!(e.contains("sumiu"), "{e}");
    r.manual_pairs = vec![(Id::from("la"), Id::from("lb")), (Id::from("la"), Id::from("rb"))];
    let e = ibp::preview(&mut f.s, &r).unwrap_err().to_string();
    assert!(e.contains("mais de um par"), "{e}");
    r.manual_pairs = vec![(Id::from("la"), Id::from("rb"))];
    let e = ibp::preview(&mut f.s, &r).unwrap_err().to_string();
    assert!(e.contains("incompatível") && e.contains("fechado"), "{e}");
    // pares por índice no pedido: o pedido não tem esse campo
    let bad = serde_json::from_value::<InbetweenPreviewRequest>(json!({"cut": f.cut, "layer": f.layer, "frame": 4, "pairs": [[0, 1]]}));
    assert!(bad.is_err());
    // par manual válido troca a correspondência e aparece marcado
    r.manual_pairs = vec![(Id::from("la"), Id::from("lb"))];
    let p = ibp::preview(&mut f.s, &r).unwrap();
    assert!(p.pairs.iter().any(|pv| pv.manual && pv.a.as_ref().map(|i| i.0.as_str()) == Some("la")));
}

#[test]
fn limits_refuse_before_expansion() {
    // AT-19: acima dos limites globais; par fechado com contagens muito desiguais dentro do limite global
    let many: Vec<Stroke> = (0..(ibp::MAX_STROKES_PER_KEY + 1)).map(|i| st(&format!("s{i}"), &[[i as f32, 0.0], [i as f32, 10.0]], false)).collect();
    let mut f = fixture(&[(1, CelRole::Key, many.clone()), (2, CelRole::Key, many)], &[(0, 1), (8, 2)]);
    let t0 = std::time::Instant::now();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge));
    assert!(p.refusals[0].text.contains("grande demais para a prévia"));
    assert!(t0.elapsed().as_millis() < 2000);
    let small = circle("a", 300.0, 200.0, 40.0, 10, 0);
    let big = circle("b", 300.0, 200.0, 40.0, 900, 0);
    let mut g = fixture(&[(1, CelRole::Key, vec![small]), (2, CelRole::Key, vec![big])], &[(0, 1), (8, 2)]);
    let t0 = std::time::Instant::now();
    let p = ibp::preview(&mut g.s, &req(&g.cut, &g.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", p.refusals);
    assert!(t0.elapsed().as_millis() < 2000, "recusou antes de alinhar 1800 pontos");
}

#[test]
fn origin_survives_save_reopen_undo_redo_and_old_cels_have_none() {
    // AT-20 (e AT-15 no núcleo: aceite pelo MCP = DirectToolCall)
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 5)).unwrap();
    let acc = ibp::accept(&mut f.s, p.id, "agent:claude").unwrap();
    assert_eq!(f.s.log().last().unwrap().source, InputSource::DirectToolCall);
    let origin = |s: &CoreSession, id: &Id| match &s.project.node(id).unwrap().data {
        NodeKind::Cel(c) => c.inbetween_origin.clone(),
        _ => unreachable!(),
    };
    let o = origin(&f.s, &acc.cel).expect("registro");
    assert_eq!((o.source, o.mechanism.as_str(), o.engine.as_str()), (InputSource::DirectToolCall, "inbetween", ibp::ENGINE_BASE));
    assert_eq!((o.key_a, o.key_b, o.frame_a, o.frame_b, o.frame), (1, 2, 0, 8, 5));
    assert_eq!(o.strokes_a, vec![Id::from("la"), Id::from("ra")]);
    assert!((o.t - 5.0 / 8.0).abs() < 1e-6);
    assert_eq!(o.revision, p.revision);
    let dir = std::env::temp_dir().join(format!("aiai-ibp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    persist::save(&mut f.s, &dir).unwrap();
    let (re, _) = persist::open(&dir).unwrap();
    assert_eq!(origin(&re, &acc.cel), Some(o.clone()), "reabre igual");
    drop(re);
    f.s.undo().unwrap();
    assert!(f.s.project.node(&acc.cel).is_none());
    f.s.redo().unwrap();
    assert_eq!(origin(&f.s, &acc.cel), Some(o));
    // cel antigo sem o campo
    let key = f.s.project.cel_by_number(&f.layer, 1).unwrap().id.clone();
    assert!(origin(&f.s, &key).is_none());
    let mut v = serde_json::to_value(match &f.s.project.node(&key).unwrap().data { NodeKind::Cel(c) => c, _ => unreachable!() }).unwrap();
    v.as_object_mut().unwrap().remove("inbetween_origin");
    let old: CelData = serde_json::from_value(v).unwrap();
    assert!(old.inbetween_origin.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_pairs_block_accept_and_old_generation_is_dropped() {
    // AT-21
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    ibp::set_manual_pairs(&mut f.s, p.id, vec![(Id::from("la"), Id::from("lb"))]).unwrap();
    assert!(ibp::current(&f.s).unwrap().stale);
    let e = ibp::accept(&mut f.s, p.id, "a").unwrap_err().to_string();
    assert!(e.contains("Atualizar"), "{e}");
    let q = ibp::refresh(&mut f.s, p.id).unwrap();
    assert_ne!(q.id, p.id, "a proposta atualizada tem id novo");
    assert!(!q.stale);
    let e = ibp::accept(&mut f.s, p.id, "a").unwrap_err().to_string();
    assert!(e.contains("substituída"), "{e}");
    ibp::accept(&mut f.s, q.id, "a").unwrap();
}

#[test]
fn key_change_and_document_swap_invalidate() {
    // AT-03, AT-04, AT-10
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    let key = f.s.project.cel_by_number(&f.layer, 2).unwrap().id.clone();
    f.s.apply(Command::SetProps { id: key, name: Some("A2b".into()), data: json!({}) }).unwrap();
    let why = ibp::invalidate_if_changed(&mut f.s).expect("invalidou");
    assert!(why.contains("mudou"), "{why}");
    assert!(ibp::current(&f.s).is_none());
    let e = ibp::accept(&mut f.s, p.id, "a").unwrap_err().to_string();
    assert!(e.contains("não vale mais"), "{e}");
    // troca de documento
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    let proj = f.s.project.clone();
    let blobs = f.s.blobs.clone();
    f.s.replace_project(proj, blobs);
    assert!(ibp::accept(&mut f.s, p.id, "a").is_err(), "outra época");
    // camada travada
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    let mut locked = f.s.project.clone();
    if let Some(NodeKind::Layer(l)) = locked.nodes.get_mut(&f.layer).map(|n| &mut n.data) {
        l.locked = true;
    }
    let e = ibp::inbetween_frame_command(&locked, &mut f.s.ids, &p, InputSource::DirectToolCall).unwrap_err().to_string();
    assert!(e.contains("travada"), "{e}");
}

// ---------------------------------------------------------------------------------------------- rodada 7 (A06, CX10)

#[test]
fn huge_coordinates_are_refused_before_any_expansion() {
    // C10-02: dois pontos a 1 000 000 px passavam pelas contagens e geravam milhões de amostras na sobreposição
    // polilinha: ~1 amostra por px, sem teto por segmento (na spline o teto é 20 000 por segmento)
    let mut far = st("f", &[[0.0, 0.0], [1_000_000.0, 0.0]], false);
    far.geometry = StrokeGeometry::Polyline;
    let mut f = fixture(&[(1, CelRole::Key, vec![far.clone()]), (2, CelRole::Key, vec![shift(&far, 5.0, 0.0, "g")])], &[(0, 1), (8, 2)]);
    let t0 = std::time::Instant::now();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", p.refusals);
    assert!(p.refusals[0].text.contains("amostras de traço"), "{}", p.refusals[0].text);
    assert!(p.candidate.is_empty());
    assert!(t0.elapsed().as_millis() < 500);
}

#[test]
fn summed_alignment_work_is_bounded() {
    // A6-02: muitos pares fechados grandes, cada um abaixo do limite por par, passavam juntos
    let a: Vec<Stroke> = (0..12).map(|i| circle(&format!("a{i}"), 100.0 + 40.0 * i as f32, 200.0, 15.0, 400, 0)).collect();
    let b: Vec<Stroke> = (0..12).map(|i| circle(&format!("b{i}"), 110.0 + 40.0 * i as f32, 200.0, 15.0, 410, 0)).collect();
    let mut f = fixture(&[(1, CelRole::Key, a), (2, CelRole::Key, b)], &[(0, 1), (8, 2)]);
    let t0 = std::time::Instant::now();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", codes(&p.refusals));
    assert!(t0.elapsed().as_millis() < 3000, "{:?}", t0.elapsed());
}

#[test]
fn group_one_to_many_uses_the_same_support_table() {
    // C10-03 / A6-05: o traço que nasce do par do grupo passa pelas mesmas recusas dos pares
    let mut a1 = st("a1", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0]], false);
    a1.group = "g".into();
    let mut b1 = shift(&a1, 50.0, 0.0, "b1");
    b1.group = "g".into();
    let mut b2 = shift(&a1, 50.0, 40.0, "b2");
    b2.group = "g".into();
    b2.profile = WidthProfile(vec![[0.0, 1.0], [1.0, 0.2]]);
    let mut f = fixture(&[(1, CelRole::Key, vec![a1]), (2, CelRole::Key, vec![b1, b2])], &[(0, 1), (8, 2)]);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::ProfileDiffers), "{:?}", p.refusals);
    let morph = p.pairs.iter().find(|x| x.kind == ibp::PairKind::GroupMorph).expect("1 para N");
    assert_eq!((morph.a.as_ref().map(|i| i.0.as_str()), morph.b.as_ref().map(|i| i.0.as_str())), (Some("a1"), Some("b2")), "a ligação mostra de onde nasce");
    assert_eq!(morph.keeps.as_ref().map(|i| i.0.as_str()), Some("b2"));
}

#[test]
fn breakdowns_bound_the_gap_and_key_frames_are_protected() {
    // A6-06: breakdown é fronteira; o quadro de uma chave não vira intercalação; substituir um desenho é dito no botão
    let l = st("l", &[[100.0, 100.0], [180.0, 100.0]], false);
    let mut f = fixture(
        &[(1, CelRole::Key, vec![l.clone()]), (2, CelRole::Breakdown, vec![shift(&l, 30.0, 0.0, "m")]), (3, CelRole::Key, vec![shift(&l, 60.0, 0.0, "n")]), (4, CelRole::Inbetween, vec![shift(&l, 10.0, 0.0, "o")])],
        &[(0, 1), (2, 4), (6, 2), (12, 3)],
    );
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 2)).unwrap();
    assert_eq!((p.frame_a, p.frame_b, p.key_b), (0, 6, 2), "o breakdown no quadro 7 fecha o vão");
    let scope = ibp::accept_scope(&f.s.project, &p).unwrap();
    assert_eq!(scope.button_text("A"), "Criar desenho A5 no quadro 3 no lugar de A4 (o quadro 4 continua mostrando A4)");
    let mut r = req(&f.cut, &f.layer, 6);
    r.frame_a = Some(0);
    r.frame_b = Some(12);
    let e = ibp::preview(&mut f.s, &r).unwrap_err().to_string();
    assert!(e.contains("não substitui chave nem breakdown"), "{e}");
}

#[test]
fn locked_layer_or_locked_parent_folder_blocks_with_the_same_reason_everywhere() {
    // A6-03 / C10-07: uma regra só; a trava da pasta acima também vale
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert_eq!(ibp::blocked_reason(&f.s.project, &p), None);
    let mut locked = f.s.project.clone();
    if let Some(NodeKind::Layer(l)) = locked.nodes.get_mut(&f.layer).map(|n| &mut n.data) {
        l.locked = true;
    }
    assert!(ibp::blocked_reason(&locked, &p).is_some_and(|w| w.contains("camada está travada")));
    // pasta travada acima da camada
    let folder = Id::from("pasta");
    let mut in_folder = f.s.project.clone();
    let mut fd = LayerData::new(LayerType::Group);
    fd.locked = true;
    in_folder.nodes.insert(folder.clone(), Node { id: folder.clone(), name: "Pasta".into(), parent: Some(f.cut.clone()), children: vec![f.layer.clone()], data: NodeKind::Layer(fd), provenance: Default::default() });
    in_folder.nodes.get_mut(&f.layer).unwrap().parent = Some(folder.clone());
    let why = ibp::blocked_reason(&in_folder, &p).unwrap_or_default();
    assert!(why.contains("pasta \"Pasta\" está travada"), "{why}");
    let e = ibp::inbetween_frame_command(&in_folder, &mut f.s.ids, &p, InputSource::DirectToolCall).unwrap_err().to_string();
    assert!(e.contains("travada"), "{e}");
}

#[test]
fn engine_options_make_the_proposal_stale_until_refresh() {
    // A6-08
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    ibp::set_options(&mut f.s, p.id, MotionMode::Linear, 0.3, 150.0).unwrap();
    assert!(ibp::current(&f.s).unwrap().stale);
    assert!(ibp::accept(&mut f.s, p.id, "a").is_err());
    let q = ibp::refresh(&mut f.s, p.id).unwrap();
    assert_eq!((q.options.motion, q.options.bend, q.options.max_cost), (MotionMode::Linear, 0.3, 150.0));
    assert!(ibp::set_options(&mut f.s, q.id, MotionMode::Global, f32::NAN, 1.0).is_err());
}

// ---------------------------------------------------------------------------------------------- rodada 8 (A07, CX11)

#[test]
fn manual_pair_in_same_structure_keeps_the_displaced_strokes_paired() {
    // A7-03: com a mesma estrutura, um par manual i→j deixava A[j] e B[i] sem par (cortados ao meio)
    let eye_l = st("aL", &[[100.0, 100.0], [110.0, 95.0], [120.0, 100.0]], false);
    let eye_r = st("aR", &[[150.0, 100.0], [160.0, 95.0], [170.0, 100.0]], false);
    let a = vec![eye_l.clone(), eye_r.clone()];
    let b = vec![shift(&eye_l, 10.0, 0.0, "bL"), shift(&eye_r, 10.0, 0.0, "bR")];
    let mut f = fixture(&[(1, CelRole::Key, a), (2, CelRole::Key, b)], &[(0, 1), (8, 2)]);
    let mut r = req(&f.cut, &f.layer, 4);
    r.manual_pairs = vec![(Id::from("aL"), Id::from("bR"))];
    let p = ibp::preview(&mut f.s, &r).unwrap();
    assert!(!codes(&p.approximations).contains(&NoticeCode::Unmatched), "{:?}", p.approximations);
    assert!(p.pairs.iter().any(|x| x.a.as_ref().map(|i| i.0.as_str()) == Some("aR") && x.b.as_ref().map(|i| i.0.as_str()) == Some("bL")));
}

#[test]
fn incompatible_manual_pair_is_refused_at_once_and_refresh_errors_keep_the_proposal() {
    // A7-01: o par inválido era aceito e o Atualizar apagava a prévia e os pares de antes
    let mut f = two_keys();
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    ibp::set_manual_pairs(&mut f.s, p.id, vec![(Id::from("la"), Id::from("lb"))]).unwrap();
    let e = ibp::set_manual_pairs(&mut f.s, p.id, vec![(Id::from("la"), Id::from("lb")), (Id::from("ra"), Id::from("lb"))]).unwrap_err().to_string();
    assert!(e.contains("mais de um par") || e.contains("incompatível"), "{e}");
    let e = ibp::set_manual_pairs(&mut f.s, p.id, vec![(Id::from("la"), Id::from("rb"))]).unwrap_err().to_string();
    assert!(e.contains("fechado"), "{e}");
    let cur = ibp::current(&f.s).unwrap();
    assert_eq!(cur.request.manual_pairs, vec![(Id::from("la"), Id::from("lb"))], "os pares de antes ficaram");
    // um pedido que falha não apaga a proposta viva e ela continua aceitável depois de atualizar
    let mut bad = req(&f.cut, &f.layer, 4);
    bad.manual_pairs = vec![(Id::from("sumiu"), Id::from("lb"))];
    assert!(ibp::preview(&mut f.s, &bad).is_err());
    let q = ibp::refresh(&mut f.s, p.id).unwrap();
    ibp::accept(&mut f.s, q.id, "a").unwrap();
}

#[test]
fn sample_budget_counts_handles_and_closing_segments() {
    // A7-04 / C11-02: alça longa com pontos de controle próximos passava pelo comprimento da poligonal
    // 50 pontos a 2 px com alça enorme em cada um: 49 segmentos × 20 000 amostras (exemplo do A07)
    let pts: Vec<[f32; 2]> = (0..50).map(|i| [100.0 + 2.0 * i as f32, 100.0]).collect();
    let mut h = st("h", &pts, false);
    for q in &mut h.points {
        q.handle = Some([100_000.0, 0.0]);
    }
    let mut f = fixture(&[(1, CelRole::Key, vec![h.clone()]), (2, CelRole::Key, vec![shift(&h, 5.0, 0.0, "h2")])], &[(0, 1), (8, 2)]);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", p.refusals);
    // o contraexemplo do CX11 como polilinha (A = B, 180 000 px cada): passa do teto de amostras
    let mut c = st("c", &[[0.0, 0.0], [75_000.0, 50_000.0], [150_000.0, 0.0]], false);
    c.geometry = StrokeGeometry::Polyline;
    let mut g = fixture(&[(1, CelRole::Key, vec![c.clone()]), (2, CelRole::Key, vec![shift(&c, 0.0, 0.0, "c2")])], &[(0, 1), (8, 2)]);
    let p = ibp::preview(&mut g.s, &req(&g.cut, &g.layer, 4)).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", p.refusals);
    assert!(p.candidate.is_empty(), "nada de candidato para mostrar simplificado");
}

#[test]
fn group_morphs_count_both_alignments() {
    // C11-03: 1 para N em fechados grandes contava só metade do alinhamento
    let mk = |id: &str, dx: f32| {
        let mut c = circle(id, 200.0 + dx, 200.0, 30.0, 450, 0);
        c.group = "g".into();
        c
    };
    let a = vec![mk("a0", 0.0)];
    let b: Vec<Stroke> = (0..11).map(|i| mk(&format!("b{i}"), 3.0 * i as f32)).collect();
    let mut f = fixture(&[(1, CelRole::Key, a), (2, CelRole::Key, b)], &[(0, 1), (8, 2)]);
    let mut r = req(&f.cut, &f.layer, 4);
    r.manual_pairs = vec![(Id::from("a0"), Id::from("b0"))];
    let t0 = std::time::Instant::now();
    let p = ibp::preview(&mut f.s, &r).unwrap();
    assert!(codes(&p.refusals).contains(&NoticeCode::TooLarge), "{:?}", codes(&p.refusals));
    assert!(t0.elapsed().as_millis() < 3000);
}

#[test]
fn accept_refused_by_a_cut_lock_leaves_the_whole_project_log_and_history_untouched() {
    // CX13 C13-01: projeto dividido por corte, outro processo com a trava do corte; o aceite volta inteiro (conteúdo,
    // proveniência, revisão, log, histórico) e a prévia continua aceitável para quando a trava sair
    let mut f = two_keys();
    f.s.apply(Command::SetProjectProps { patch: json!({"settings": {"split_cuts": true}}) }).unwrap();
    let dir = std::env::temp_dir().join(format!("aiai-ibp-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    persist::save(&mut f.s, &dir).unwrap();
    let (cut, layer) = (f.cut.clone(), f.layer.clone());
    drop(f);
    let (mut s, rep) = persist::open(&dir).unwrap();
    assert!(!rep.read_only());
    let other = json!({"pid": 4, "host": aiai_core::lock::host_name(), "started_ms": 0, "exe": "outro.exe", "token": "x"});
    std::fs::write(aiai_core::lock::cut_lock_path(&dir, &cut), other.to_string()).unwrap();
    let p = ibp::preview(&mut s, &req(&cut, &layer, 4)).unwrap();
    let before = (serde_json::to_string(&s.project).unwrap(), s.project.revision, s.log().len(), s.history());
    let e = ibp::accept(&mut s, p.id, "agent:claude").unwrap_err().to_string();
    assert!(e.contains("sendo editado em outro lugar"), "{e}");
    let after = (serde_json::to_string(&s.project).unwrap(), s.project.revision, s.log().len(), s.history());
    assert_eq!(after, before, "a recusa não deixa rastro");
    // a trava sai: a mesma proposta é aceita
    std::fs::remove_file(aiai_core::lock::cut_lock_path(&dir, &cut)).unwrap();
    ibp::accept(&mut s, p.id, "agent:claude").unwrap();
    assert_eq!(s.project.revision, before.1 + 1);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_of_an_accept_recovered_from_the_journal_respects_the_cut_lock() {
    // CX14 C14-01: aceite que só está no diário (fechou sem Salvar), reaberto; outro processo pega o corte; desfazer e
    // refazer recusam sem rastro (projeto inteiro, log, histórico, diário) e valem quando a trava sai
    let mut f = two_keys();
    f.s.apply(Command::SetProjectProps { patch: json!({"settings": {"split_cuts": true}}) }).unwrap();
    let dir = std::env::temp_dir().join(format!("aiai-ibp-undo-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    persist::save(&mut f.s, &dir).unwrap();
    let (cut, layer) = (f.cut.clone(), f.layer.clone());
    drop(f);
    let (mut s, _) = persist::open(&dir).unwrap();
    let p = ibp::preview(&mut s, &req(&cut, &layer, 4)).unwrap();
    let acc = ibp::accept(&mut s, p.id, "agent:claude").unwrap();
    drop(s); // sem Salvar: o aceite está só no diário
    let (mut s, rep) = persist::open(&dir).unwrap();
    assert_eq!(rep.replayed, 1);
    assert!(s.project.node(&acc.cel).is_some(), "aceite recuperado");
    assert!(s.claimed_cuts().any(|c| *c == cut), "a recuperação travou o corte");
    let other = json!({"pid": 4, "host": aiai_core::lock::host_name(), "started_ms": 0, "exe": "outro.exe", "token": "x"});
    let wal = dir.join("wal.ndjson");
    let state = |s: &CoreSession| (serde_json::to_string(&s.project).unwrap(), s.log().len(), s.history(), std::fs::read(&wal).unwrap());
    s.release_cut_locks();
    std::fs::write(aiai_core::lock::cut_lock_path(&dir, &cut), other.to_string()).unwrap();
    let before = state(&s);
    let e = s.undo().unwrap_err().to_string();
    assert!(e.contains("outro lugar"), "{e}");
    assert_eq!(state(&s), before, "desfazer recusado não deixa rastro");
    std::fs::remove_file(aiai_core::lock::cut_lock_path(&dir, &cut)).unwrap();
    s.undo().unwrap();
    assert!(s.project.node(&acc.cel).is_none());
    s.release_cut_locks();
    std::fs::write(aiai_core::lock::cut_lock_path(&dir, &cut), other.to_string()).unwrap();
    let before = state(&s);
    assert!(s.redo().is_err());
    assert_eq!(state(&s), before, "refazer recusado não deixa rastro");
    std::fs::remove_file(aiai_core::lock::cut_lock_path(&dir, &cut)).unwrap();
    s.redo().unwrap();
    assert!(s.project.node(&acc.cel).is_some());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn proposal_ids_of_different_sessions_never_open_each_others_proposals() {
    // CX16 C16-07: cada sessão sorteia o começo dos seus ids (não depende do relógio); o id de outra sessão não aceita,
    // não descarta e não acha nada aqui
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..300 {
        let mut f = two_keys();
        let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
        assert!(p.id < (1u64 << 53), "cabe exato em JSON");
        assert!(ids.insert(p.id), "id repetido entre sessões: {}", p.id);
    }
    let mut a = two_keys();
    let mut b = two_keys();
    let pa = ibp::preview(&mut a.s, &req(&a.cut, &a.layer, 4)).unwrap();
    let pb = ibp::preview(&mut b.s, &req(&b.cut, &b.layer, 4)).unwrap();
    assert_ne!(pa.id, pb.id);
    let rev = b.s.project.revision;
    assert!(ibp::accept(&mut b.s, pa.id, "a").is_err(), "aceitar o id da outra sessão falha");
    assert!(ibp::discard(&mut b.s, pa.id).is_err(), "descartar o id da outra sessão falha");
    assert_eq!(b.s.project.revision, rev);
    ibp::accept(&mut b.s, pb.id, "a").unwrap();
}

#[test]
fn accept_on_a_layer_that_already_uses_the_largest_cel_number_is_refused_without_panic() {
    // CX16 C16-08
    let line = st("la", &[[100.0, 100.0], [140.0, 90.0], [180.0, 100.0]], false);
    let mut f = fixture(&[(1, CelRole::Key, vec![line.clone()]), (u32::MAX, CelRole::Key, vec![shift(&line, 80.0, 0.0, "lb")])], &[(0, 1), (8, u32::MAX)]);
    let p = ibp::preview(&mut f.s, &req(&f.cut, &f.layer, 4)).unwrap();
    let rev = f.s.project.revision;
    let e = ibp::accept(&mut f.s, p.id, "a").unwrap_err().to_string();
    assert!(e.contains("maior possível"), "{e}");
    assert_eq!(f.s.project.revision, rev);
}

//! E3 (R16 §11, CX04 §4): a prévia e o aceite preservam o suporte declarado? Para cada caso: a prévia classifica
//! (preserva / aproxima / recusa); quando aceita, o cel gravado é exatamente o candidato, cada atributo dos traços de saída
//! é o declarado (nada muda sem aviso) e o render do cel aceito é, pixel a pixel, o desenho da prévia.
//!
//! `cargo test -p aiai-tools --test inbetween_e3 -- --nocapture` imprime a tabela do relatório.

use aiai_core::command::CellEdit;
use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::inbetween_preview::{self as ibp, InbetweenPreviewRequest, NoticeCode, Support};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn pt(x: f32, y: f32) -> StrokePoint {
    StrokePoint { x, y, pressure: 0.8, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
}

fn st(id: &str, p: &[[f32; 2]], closed: bool) -> Stroke {
    Stroke {
        id: Id::from(id),
        points: p.iter().map(|q| pt(q[0], q[1])).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::GPen,
        width: 4.0,
        profile: Default::default(),
        color: ColorRef::Direct { rgba: Rgba([0.1, 0.1, 0.1, 1.0]) },
        opacity: 1.0,
        closed,
        aliased: false,
        color_trace: false,
        group: String::new(),
    }
}

fn moved(s: &Stroke, dx: f32, id: &str) -> Stroke {
    let mut o = s.clone();
    o.id = Id::from(id);
    for q in &mut o.points {
        q.x += dx;
    }
    o
}

fn arc(id: &str, n: usize, closed: bool) -> Stroke {
    let p: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * if closed { std::f32::consts::TAU } else { 2.5 };
            [200.0 + 60.0 * a.cos(), 180.0 + 60.0 * a.sin()]
        })
        .collect();
    st(id, &p, closed)
}

struct Case {
    name: &'static str,
    a: Vec<Stroke>,
    b: Vec<Stroke>,
}

fn session(case: &Case) -> (CoreSession, Id, Id) {
    let mut s = CoreSession::for_tests("e3");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 640, "height": 360}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let layer = create(&mut s, Some(&cut), "A", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    for (n, strokes) in [(1u32, &case.a), (2, &case.b)] {
        let mut c = CelData::new(n);
        c.strokes = strokes.clone();
        create(&mut s, Some(&layer), &format!("A{n}"), NodeKind::Cel(c));
    }
    let track = Id::from("t");
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(track.clone(), "A", TrackKind::Drawing, Some(layer.clone())) }).unwrap();
    s.apply(Command::SetCells { cut: cut.clone(), track, cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }, CellEdit { frame: 8, cell: Some(CellRef::Cel { number: 2 }) }] }).unwrap();
    (s, cut, layer)
}

/// Atributos da saída que mudaram sem aviso que os declare (perda silenciosa).
fn silent_losses(p: &ibp::InbetweenProposal, a: &[Stroke], out: &[Stroke]) -> Vec<String> {
    let has = |c: NoticeCode| p.approximations.iter().chain(&p.refusals).chain(&p.notes).any(|n| n.code == c);
    let mut lost = Vec::new();
    // a ordem da saída segue os pares (A primeiro); casa cada saída com o traço de A do par
    let mut k = 0;
    for pv in &p.pairs {
        let Some(s) = out.get(k) else { break };
        k += 1;
        let Some(src) = pv.a.as_ref().and_then(|id| a.iter().find(|x| &x.id == id)) else { continue };
        if s.tip != src.tip && !has(NoticeCode::TipDiffers) {
            lost.push(format!("{}: ponta", src.id));
        }
        if s.profile != src.profile {
            lost.push(format!("{}: perfil", src.id));
        }
        if s.closed != src.closed && !has(NoticeCode::Unmatched) {
            lost.push(format!("{}: fechado", src.id));
        }
        if (s.aliased, s.color_trace, &s.group) != (src.aliased, src.color_trace, &src.group) {
            lost.push(format!("{}: aliased/linha de cor/grupo", src.id));
        }
        if s.tension != src.tension && !has(NoticeCode::TensionDiffers) {
            lost.push(format!("{}: tensão", src.id));
        }
        if s.geometry != src.geometry && !has(NoticeCode::Resampled) && !has(NoticeCode::GroupMorph) && !has(NoticeCode::Unmatched) {
            lost.push(format!("{}: geometria {:?}→{:?}", src.id, src.geometry, s.geometry));
        }
        if s.points.iter().any(|q| q.width != 1.0 || q.corner || q.handle.is_some()) {
            lost.push(format!("{}: atributo de ponto", src.id));
        }
    }
    lost
}

fn layer_png(s: &CoreSession, cut: &Id, layer: &Id, frame: u32) -> Vec<[f32; 4]> {
    let o = RenderOptions { only_layers: Some(vec![layer.clone()]), camera: false, ..Default::default() };
    render_frame(&s.project, &s.blobs, cut, frame, &o).unwrap().image.data
}

#[test]
fn e3_preview_and_accept_keep_the_declared_support() {
    let open = arc("o", 9, false);
    let ring = arc("r", 16, true);
    let mut cases = vec![
        Case { name: "aberto, direto, cores diretas diferentes", a: vec![open.clone()], b: vec![{ let mut x = moved(&open, 60.0, "o2"); x.color = ColorRef::Direct { rgba: Rgba([0.8, 0.1, 0.1, 1.0]) }; x }] },
        Case { name: "fechado, direto, mesmo sentido", a: vec![ring.clone()], b: vec![moved(&ring, 60.0, "r2")] },
        Case { name: "contagens diferentes (reamostrado)", a: vec![open.clone()], b: vec![{ let mut x = arc("o3", 14, false); for q in &mut x.points { q.x += 60.0; } x }] },
        Case { name: "perfil igual, direto", a: vec![{ let mut x = open.clone(); x.profile = WidthProfile(vec![[0.0, 0.3], [0.5, 1.0], [1.0, 0.3]]); x }], b: vec![{ let mut x = moved(&open, 60.0, "p2"); x.profile = WidthProfile(vec![[0.0, 0.3], [0.5, 1.0], [1.0, 0.3]]); x }] },
        Case { name: "perfil igual, reamostrado e invertido", a: vec![{ let mut x = open.clone(); x.profile = WidthProfile(vec![[0.0, 0.2], [1.0, 1.0]]); x }], b: vec![{ let mut x = arc("p3", 14, false); x.points.reverse(); x.profile = WidthProfile(vec![[0.0, 0.2], [1.0, 1.0]]); x }] },
        Case { name: "largura variável por ponto", a: vec![{ let mut x = open.clone(); x.points[3].width = 1.6; x }], b: vec![moved(&open, 60.0, "w2")] },
        Case { name: "alça (handle)", a: vec![{ let mut x = open.clone(); x.points[2].handle = Some([3.0, 2.0]); x }], b: vec![moved(&open, 60.0, "h2")] },
        Case { name: "Bézier, direto", a: vec![{ let mut x = st("bz", &[[100.0, 100.0], [130.0, 60.0], [170.0, 60.0], [200.0, 100.0]], false); x.geometry = StrokeGeometry::Bezier; x }], b: vec![{ let mut x = st("bz2", &[[160.0, 100.0], [190.0, 70.0], [230.0, 70.0], [260.0, 100.0]], false); x.geometry = StrokeGeometry::Bezier; x }] },
        Case { name: "Bézier, reamostrado", a: vec![{ let mut x = st("bz", &[[100.0, 100.0], [130.0, 60.0], [170.0, 60.0], [200.0, 100.0]], false); x.geometry = StrokeGeometry::Bezier; x }], b: vec![{ let mut x = st("bz3", &[[160.0, 100.0], [180.0, 70.0], [200.0, 70.0], [220.0, 100.0], [240.0, 130.0], [260.0, 130.0], [280.0, 100.0]], false); x.geometry = StrokeGeometry::Bezier; x }] },
        Case { name: "cores de paleta diferentes", a: vec![{ let mut x = open.clone(); x.color = ColorRef::Palette { palette: Id::from("pal"), color: Id::from("ca") }; x }], b: vec![{ let mut x = moved(&open, 60.0, "c2"); x.color = ColorRef::Palette { palette: Id::from("pal"), color: Id::from("cb") }; x }] },
        Case { name: "ponta quadrada igual", a: vec![{ let mut x = open.clone(); x.tip = VectorTip::Square; x }], b: vec![{ let mut x = moved(&open, 60.0, "q2"); x.tip = VectorTip::Square; x }] },
        Case { name: "pontas diferentes", a: vec![open.clone()], b: vec![{ let mut x = moved(&open, 60.0, "t2"); x.tip = VectorTip::Marker; x }] },
        Case { name: "traço sem par (some)", a: vec![open.clone(), st("solo", &[[400.0, 300.0], [450.0, 300.0], [500.0, 310.0]], false)], b: vec![moved(&open, 60.0, "u2")] },
    ];
    // paleta só existe para os casos que a usam
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for case in cases.drain(..) {
        let (mut s, cut, layer) = session(&case);
        if case.name.contains("paleta") {
            let named = |id: &str, c: [f32; 4]| NamedColor { id: Id::from(id), name: id.into(), group: String::new(), role: String::new(), rgba: Rgba(c), variants: Default::default() };
            let pal = PaletteData { colors: vec![named("ca", [0.1, 0.1, 0.1, 1.0]), named("cb", [0.2, 0.3, 0.9, 1.0])], ..Default::default() };
            s.apply(Command::CreateNode { id: Some(Id::from("pal")), parent: None, index: None, name: "pal".into(), data: NodeKind::Palette(pal) }).unwrap();
        }
        let p = ibp::preview(&mut s, &InbetweenPreviewRequest::new(cut.clone(), layer.clone(), 4)).unwrap();
        let class = p.pairs.iter().map(|x| x.support).max_by_key(|x| match x { Support::Preserve => 0, Support::Approximate => 1, Support::Refuse => 2 }).unwrap_or(Support::Preserve);
        let codes: Vec<String> = p.approximations.iter().chain(&p.refusals).chain(&p.notes).map(|n| format!("{:?}", n.code)).collect();
        let verdict;
        if p.refusals.is_empty() {
            let cand = p.candidate.clone();
            // render da prévia: o candidato num cel provisório no quadro (o mesmo caminho do render com prévia)
            let mut tmp = s.project.clone();
            let mut c = CelData::new(99);
            c.strokes = cand.clone();
            let tid = Id::from("tmp-cel");
            tmp.nodes.insert(tid.clone(), Node { id: tid.clone(), name: "t".into(), parent: Some(layer.clone()), children: vec![], data: NodeKind::Cel(c), provenance: Default::default() });
            tmp.nodes.get_mut(&layer).unwrap().children.push(tid);
            if let Some(NodeKind::Cut(cd)) = tmp.nodes.get_mut(&cut).map(|n| &mut n.data) {
                cd.xsheet.tracks[0].cells.insert(4, CellRef::Cel { number: 99 });
            }
            let o = RenderOptions { only_layers: Some(vec![layer.clone()]), camera: false, ..Default::default() };
            let prev_px = render_frame(&tmp, &s.blobs, &cut, 4, &o).unwrap().image.data;
            let acc = ibp::accept(&mut s, p.id, "e3").unwrap();
            let saved = match &s.project.node(&acc.cel).unwrap().data {
                NodeKind::Cel(c) => c.strokes.clone(),
                _ => unreachable!(),
            };
            let same_strokes = saved.len() == cand.len() && saved.iter().zip(&cand).all(|(x, y)| x.points == y.points && x.width == y.width && x.color == y.color && x.profile == y.profile && x.tip == y.tip && x.geometry == y.geometry);
            let acc_px = layer_png(&s, &cut, &layer, 4);
            let diff = prev_px.iter().zip(&acc_px).filter(|(a, b)| a.iter().zip(b.iter()).any(|(u, v)| (u - v).abs() > 1e-4)).count();
            let lost = silent_losses(&p, &case.a, &saved);
            verdict = format!("aceito; traços = candidato: {}; pixels diferentes prévia × aceito: {diff}; perda silenciosa: {}", if same_strokes { "sim" } else { "NÃO" }, if lost.is_empty() { "nenhuma".to_owned() } else { lost.join(", ") });
            if !same_strokes || diff != 0 || !lost.is_empty() {
                failures.push(format!("{}: {verdict}", case.name));
            }
        } else {
            verdict = "recusado antes de gravar (Aceitar desligado)".into();
        }
        rows.push(format!("| {} | {:?} | {} | {} |", case.name, class, codes.join(", "), verdict));
    }
    // t = 0 e t = 1: a prévia só existe entre as chaves
    let (mut s, cut, layer) = session(&Case { name: "", a: vec![open.clone()], b: vec![moved(&open, 60.0, "z")] });
    for f in [0u32, 8] {
        let e = ibp::preview(&mut s, &InbetweenPreviewRequest::new(cut.clone(), layer.clone(), f)).unwrap_err().to_string();
        rows.push(format!("| quadro {f} (t = {}) | — | — | não oferecido: {e} |", if f == 0 { 0 } else { 1 }));
    }
    println!("| caso | classe do par | avisos/recusas | resultado |\n|---|---|---|---|");
    for r in &rows {
        println!("{r}");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

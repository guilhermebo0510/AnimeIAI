//! Fluxo completo de ferramenta sem UI: desenhar linha por curvas → balde com paleta → lint → render.

use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::draw::{CurveSpec, DrawRequest, draw_command};
use aiai_tools::fill::{FillRequest, FillSeed, fill_command, lint};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn curve(points: &[[f32; 3]], width: f32, color: &ColorRef, closed: bool) -> CurveSpec {
    serde_json::from_value(json!({
        "points": points.iter().map(|p| p.to_vec()).collect::<Vec<_>>(),
        "width": width, "tip": "g_pen", "color": color, "closed": closed
    }))
    .unwrap()
}

#[test]
fn draw_fill_lint_render() {
    let mut s = CoreSession::for_tests("tools");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 640, "height": 360}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(24)));
    let pal = create(
        &mut s,
        None,
        "Settei",
        NodeKind::Palette(PaletteData {
            colors: [("linha", "#40302c"), ("pele", "#fbe3d3"), ("cabelo", "#f2d36b"), ("olho", "#3d6fd1"), ("branco", "#ffffff")]
                .iter()
                .map(|(id, hex)| NamedColor { id: Id::from(*id), name: id.to_string(), group: id.to_string(), role: "normal".into(), rgba: Rgba::from_hex(hex).unwrap(), variants: Default::default() })
                .collect(),
            active_variant: String::new(),
        }),
    );
    let c = |id: &str| ColorRef::Palette { palette: pal.clone(), color: Id::from(id) };
    let fill_layer = create(&mut s, Some(&cut), "A-cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line_layer = create(&mut s, Some(&cut), "A-linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    create(&mut s, Some(&fill_layer), "A-cor1", NodeKind::Cel(CelData::new(1)));
    create(&mut s, Some(&line_layer), "A-linha1", NodeKind::Cel(CelData::new(1)));
    s.apply(Command::SetProps { id: fill_layer.clone(), name: None, data: json!({"line_refs": [line_layer.0]}) }).unwrap();

    // Rosto simples: contorno, franja (com um buraco de ~2 px proposital), dois olhos (simetria).
    let face = curve(&[[250.0, 120.0, 0.8], [240.0, 200.0, 1.0], [320.0, 280.0, 1.0], [400.0, 200.0, 1.0], [390.0, 120.0, 0.8]], 4.0, &c("linha"), false);
    let hair = curve(&[[248.0, 128.0, 0.6], [270.0, 70.0, 1.0], [320.0, 55.0, 1.0], [370.0, 70.0, 1.0], [393.0, 128.0, 0.6]], 4.0, &c("linha"), false);
    let fringe = curve(&[[250.0, 125.0, 1.0], [285.0, 150.0, 1.0], [320.0, 128.0, 1.0], [355.0, 150.0, 1.0], [388.0, 124.0, 1.0]], 3.0, &c("linha"), false);
    let eye = curve(&[[285.0, 185.0, 1.0], [300.0, 175.0, 1.0], [300.0, 205.0, 1.0], [285.0, 200.0, 1.0]], 3.0, &c("linha"), true);
    let mut draw = DrawRequest { cut: cut.clone(), frame: 0, layer: line_layer.clone(), curves: vec![face, hair, fringe], mirror_x: None };
    s.apply(draw_command(&s.project, &draw).unwrap()).unwrap();
    draw.curves = vec![eye];
    draw.mirror_x = Some(320.0);
    s.apply(draw_command(&s.project, &draw).unwrap()).unwrap();

    // Antes de pintar: o lint aponta regiões fechadas sem cor.
    let before = lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap();
    assert!(before.unpainted.len() >= 4, "rosto, franja e dois olhos: {:?}", before.unpainted.len());

    // Balde em cada região (sementes do próprio lint, como o agente faria).
    let seeds: Vec<FillSeed> = before
        .unpainted
        .iter()
        .map(|r| {
            let (x, y) = (r.point[0] as f32, r.point[1] as f32);
            let color = if y < 125.0 { c("cabelo") } else if r.area < 600 { c("olho") } else { c("pele") };
            FillSeed { x, y, color }
        })
        .collect();
    let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds, line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35 };
    let cmd = fill_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(cmd).unwrap();
    let after = lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap();
    assert!(after.unpainted.is_empty(), "tudo pintado: {:?}", after.unpainted);

    let out = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions { background: Some([1.0, 1.0, 1.0]), ..Default::default() }).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        out.image.save_png(&std::path::Path::new(&dir).join("rosto_linha_balde.png")).unwrap();
    }
    // a pele dentro do rosto tem a cor do settei, sem halo branco junto da linha
    let p = out.image.get(320, 240);
    assert!((p[0] - 0xfb as f32 / 255.0).abs() < 0.02 && (p[2] - 0xd3 as f32 / 255.0).abs() < 0.02, "pele: {p:?}");
    // undo do balde volta a ter regiões sem cor
    s.undo().unwrap();
    assert!(!lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap().unpainted.is_empty());
}

#[test]
fn paint_sky_study_with_brushes() {
    use aiai_paint::brush::{BrushPreset, GradientKind};
    use aiai_tools::paint::{GradientRequest, PaintRequest, PaintStrokeSpec, gradient_command, paint_command};
    let mut s = CoreSession::for_tests("ceu");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 960, "height": 540}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let sky = create(&mut s, Some(&cut), "BG-ceu", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(&mut s, Some(&sky), "BG1", NodeKind::Cel(CelData::new(1)));
    let clouds = create(&mut s, Some(&cut), "BG-nuvens", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(&mut s, Some(&clouds), "N1", NodeKind::Cel(CelData::new(1)));
    // céu: degradê vertical azul profundo → azul claro perto do horizonte
    let g = GradientRequest {
        cut: cut.clone(), frame: 0, layer: sky.clone(), kind: GradientKind::Linear, from: [0.0, 0.0], to: [0.0, 540.0],
        stops: vec![(0.0, "#3c7fd9ff".into()), (0.7, "#8ec5f2ff".into()), (1.0, "#d8ecfaff".into())], blend: BlendMode::Normal, opacity: 1.0,
    };
    { let c = gradient_command(&s.project, &mut s.blobs, &g).unwrap(); s.apply(c).unwrap(); }
    // nuvens: massa macia branca + borda de aquarela + sombra azulada na base
    let presets = BrushPreset::defaults();
    let soft = presets.iter().find(|p| p.name == "Redondo macio").unwrap().clone();
    let wc = presets.iter().find(|p| p.name == "Aquarela").unwrap().clone();
    let mut big_soft = soft.clone();
    big_soft.size = 110.0;
    big_soft.flow = 0.25;
    let white = ColorRef::Direct { rgba: Rgba::from_hex("#ffffff").unwrap() };
    let shade = ColorRef::Direct { rgba: Rgba::from_hex("#b7c8e6").unwrap() };
    let mass = |pts: &[[f32; 3]]| PaintStrokeSpec { points: pts.iter().map(|p| p.to_vec()).collect(), smooth: true };
    let r = PaintRequest {
        cut: cut.clone(), frame: 0, layer: clouds.clone(), preset: big_soft, color: white.clone(),
        strokes: vec![
            mass(&[[180.0, 300.0, 0.8], [260.0, 240.0, 1.0], [360.0, 230.0, 1.0], [460.0, 260.0, 0.9], [540.0, 300.0, 0.7]]),
            mass(&[[600.0, 200.0, 0.6], [680.0, 160.0, 1.0], [780.0, 170.0, 0.8]]),
        ],
        seed: 1,
    };
    { let c = paint_command(&s.project, &mut s.blobs, &r).unwrap(); s.apply(c).unwrap(); }
    let mut wc_white = wc.clone();
    wc_white.size = 70.0;
    let r2 = PaintRequest {
        cut: cut.clone(), frame: 0, layer: clouds.clone(), preset: wc_white, color: white,
        strokes: vec![mass(&[[220.0, 250.0, 1.0], [300.0, 205.0, 1.0], [380.0, 200.0, 1.0], [430.0, 225.0, 0.8]])], seed: 2,
    };
    { let c = paint_command(&s.project, &mut s.blobs, &r2).unwrap(); s.apply(c).unwrap(); }
    let mut wc_shade = wc;
    wc_shade.size = 60.0;
    wc_shade.opacity = 0.5;
    let r3 = PaintRequest {
        cut: cut.clone(), frame: 0, layer: clouds, preset: wc_shade, color: shade,
        strokes: vec![mass(&[[200.0, 320.0, 0.7], [320.0, 300.0, 1.0], [450.0, 305.0, 1.0], [530.0, 320.0, 0.6]])], seed: 3,
    };
    { let c = paint_command(&s.project, &mut s.blobs, &r3).unwrap(); s.apply(c).unwrap(); }
    let out = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap();
    assert!(out.warnings.is_empty());
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        out.image.save_png(&std::path::Path::new(&dir).join("estudo_ceu.png")).unwrap();
    }
    let top = out.image.get(480, 10);
    let cloud = out.image.get(310, 236); // sobre a massa macia + aquarela
    assert!(top[2] > top[0] + 0.3, "topo azul: {top:?}");
    let sky_same_h = out.image.get(60, 236);
    assert!(cloud[0] > sky_same_h[0] + 0.15, "nuvem clareia o céu na mesma altura: {cloud:?} vs {sky_same_h:?}");
    // undo da última pincelada volta exatamente
    let before = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image.content_hash();
    s.undo().unwrap();
    s.redo().unwrap();
    assert_eq!(render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image.content_hash(), before);
}

#[test]
fn transform_vector_raster_and_repeat() {
    use aiai_tools::transform::{Deform, RepeatRequest, TransformRequest, repeat_command, transform_command};
    let mut s = CoreSession::for_tests("tr");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 400, "height": 200}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let vl = create(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    let vcel = create(&mut s, Some(&vl), "V1", NodeKind::Cel(CelData::new(1)));
    let black = ColorRef::Direct { rgba: Rgba::BLACK };
    let col = curve(&[[20.0, 150.0, 1.0], [20.0, 60.0, 1.0]], 6.0, &black, false);
    let req = DrawRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), curves: vec![col], mirror_x: None };
    s.apply(draw_command(&s.project, &req).unwrap()).unwrap();
    let sid = match &s.project.node(&vcel).unwrap().data { NodeKind::Cel(c) => c.strokes[0].id.clone(), _ => unreachable!() };
    // repetir a "coluna" ao longo de uma linha diminuindo (perspectiva simples)
    let rep = RepeatRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), strokes: vec![sid.clone()], origin: [20.0, 150.0], path: vec![[60.0, 150.0], [380.0, 130.0]], spacing: 40.0, offset: 0.0, follow: false, scale_range: [1.0, 0.6] };
    s.apply(repeat_command(&s.project, &rep).unwrap()).unwrap();
    let n = match &s.project.node(&vcel).unwrap().data { NodeKind::Cel(c) => c.strokes.len(), _ => 0 };
    assert_eq!(n, 1 + 9, "motivo + 9 cópias");
    // girar 90° a coluna original em torno da base
    let tr = TransformRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), deform: Deform::Affine { translate: [0.0, 0.0], scale: [1.0, 1.0], rotate: 90.0, shear: [0.0, 0.0], pivot: [20.0, 150.0] }, selection: None, scale_width: true, strokes: vec![sid.clone()] };
    { let c = transform_command(&s.project, &mut s.blobs, &tr).unwrap(); s.apply(c).unwrap(); }
    let pts = match &s.project.node(&vcel).unwrap().data { NodeKind::Cel(c) => c.strokes[0].points.clone(), _ => unreachable!() };
    assert!((pts[1].x - 110.0).abs() < 0.01 && (pts[1].y - 150.0).abs() < 0.01, "y para baixo: +90° = horário, (20,60) → (110,150): {:?}", pts[1]);
    // raster: retângulo pintado, seleção à esquerda, empurrar 50 px
    use aiai_paint::brush::{BrushPreset};
    use aiai_paint::select::SelectShape;
    use aiai_tools::paint::{PaintRequest, PaintStrokeSpec, paint_command};
    let rl = create(&mut s, Some(&cut), "R", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(&mut s, Some(&rl), "R1", NodeKind::Cel(CelData::new(1)));
    let pr = PaintRequest { cut: cut.clone(), frame: 0, layer: rl.clone(), preset: BrushPreset::round("d", 20.0, 1.0), color: ColorRef::Direct { rgba: Rgba::from_hex("#ff0000").unwrap() }, strokes: vec![PaintStrokeSpec { points: vec![vec![100.0, 100.0], vec![300.0, 100.0]], smooth: false }], seed: 0 };
    { let c = paint_command(&s.project, &mut s.blobs, &pr).unwrap(); s.apply(c).unwrap(); }
    let tr2 = TransformRequest { cut: cut.clone(), frame: 0, layer: rl.clone(), deform: Deform::Affine { translate: [0.0, 50.0], scale: [1.0, 1.0], rotate: 0.0, shear: [0.0, 0.0], pivot: [0.0, 0.0] }, selection: Some(SelectShape::Rect { rect: [0.0, 0.0, 200.0, 200.0] }), scale_width: true, strokes: vec![] };
    { let c = transform_command(&s.project, &mut s.blobs, &tr2).unwrap(); s.apply(c).unwrap(); }
    let only = RenderOptions { only_layers: Some(vec![rl.clone()]), ..Default::default() };
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &only).unwrap().image;
    assert!(img.get(150, 100)[3] < 0.05 && img.get(150, 150)[3] > 0.9, "metade esquerda desceu 50 px");
    assert!(img.get(250, 100)[3] > 0.9, "metade direita ficou");
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        let all = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions { background: Some([1.0; 3]), ..Default::default() }).unwrap().image;
        all.save_png(&std::path::Path::new(&dir).join("transformar_repetir.png")).unwrap();
    }
}

#[test]
fn inbetween_with_timesheet_and_fill_propagation() {
    use aiai_tools::anim::{InbetweenRequest, PropagateFillsRequest, inbetween_command, propagate_fills};
    use aiai_tools::transform::{Deform, TransformRequest, transform_command};
    let mut s = CoreSession::for_tests("anim");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 480, "height": 270}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let fill = create(&mut s, Some(&cut), "A-cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line = create(&mut s, Some(&cut), "A", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    s.apply(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [line.0]}) }).unwrap();
    for n in [1u32, 2] {
        create(&mut s, Some(&line), &format!("A{n}"), NodeKind::Cel(CelData::new(n)));
        create(&mut s, Some(&fill), &format!("C{n}"), NodeKind::Cel(CelData::new(n)));
    }
    let tl = s.ids.next();
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(tl.clone(), "A", TrackKind::Drawing, Some(line.clone())) }).unwrap();
    let tf = s.ids.next();
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(tf.clone(), "A-cor", TrackKind::Drawing, Some(fill.clone())) }).unwrap();
    // A1 no quadro 0, A2 no quadro 8 (temporário, para desenhar/pintar cada chave)
    for t in [&tl, &tf] {
        s.apply(Command::SetCells { cut: cut.clone(), track: t.clone(), cells: vec![
            aiai_core::command::CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) },
            aiai_core::command::CellEdit { frame: 8, cell: Some(CellRef::Cel { number: 2 }) },
        ] }).unwrap();
    }
    let ink = ColorRef::Direct { rgba: Rgba::from_hex("#2b2020").unwrap() };
    let skin = ColorRef::Direct { rgba: Rgba::from_hex("#f7d9c4").unwrap() };
    // chave A: cabeça (contorno fechado) + olho
    let head = curve(&[[200.0, 80.0, 1.0], [260.0, 90.0, 1.0], [270.0, 160.0, 1.0], [230.0, 200.0, 1.0], [190.0, 160.0, 1.0]], 4.0, &ink, true);
    let eye = curve(&[[225.0, 130.0, 1.0], [232.0, 120.0, 1.0], [239.0, 130.0, 1.0]], 3.0, &ink, false);
    let d = DrawRequest { cut: cut.clone(), frame: 0, layer: line.clone(), curves: vec![head.clone(), eye.clone()], mirror_x: None };
    s.apply(draw_command(&s.project, &d).unwrap()).unwrap();
    // chave B: mesma cabeça girada 25° e deslocada (como um animador copiaria e moveria a chave)
    let d2 = DrawRequest { cut: cut.clone(), frame: 8, layer: line.clone(), curves: vec![head, eye], mirror_x: None };
    s.apply(draw_command(&s.project, &d2).unwrap()).unwrap();
    let tr = TransformRequest { cut: cut.clone(), frame: 8, layer: line.clone(), deform: Deform::Affine { translate: [80.0, 10.0], scale: [1.0, 1.0], rotate: 25.0, shear: [0.0, 0.0], pivot: [230.0, 140.0] }, selection: None, scale_width: true, strokes: vec![] };
    { let c = transform_command(&s.project, &mut s.blobs, &tr).unwrap(); s.apply(c).unwrap(); }
    // pintar as duas chaves
    for (f, x, y) in [(0u32, 215.0f32, 165.0f32), (8, 0.0, 0.0)] {
        let (sx, sy) = if f == 0 { (x, y) } else {
            let r = lint(&s.project, &s.blobs, &cut, 8, &fill, 3.0, 200).unwrap();
            let big = r.unpainted.iter().max_by_key(|g| g.area).unwrap();
            (big.point[0] as f32, big.point[1] as f32)
        };
        let req = FillRequest { cut: cut.clone(), frame: f, layer: fill.clone(), seeds: vec![FillSeed { x: sx, y: sy, color: skin.clone() }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35 };
        let c = fill_command(&s.project, &mut s.blobs, &req).unwrap();
        s.apply(c).unwrap();
    }
    // 3 intercalações (metades para B), A em 0, intercalações em 2, 4, 6, B em 8 — em 2s
    let ib = InbetweenRequest { cut: cut.clone(), layer: line.clone(), key_a: 1, key_b: 2, count: 3, times: vec![], ease: None, halves_toward_b: Some(true), first_number: None, start_frame: Some(0), hold: 2, options: Default::default(), fill_layer: Some(fill.clone()) };
    let cmd = inbetween_command(&s.project, &mut s.ids, &ib).unwrap();
    s.apply(cmd).unwrap();
    let changes = Query::ChangeFrames { cut: cut.clone() }.run(&s).unwrap();
    assert_eq!(changes, json!([0, 2, 4, 6, 8]), "timesheet em 2s");
    let pf = PropagateFillsRequest { cut: cut.clone(), fill_layer: fill.clone(), key_frame_a: 0, key_frame_b: 8, frames: vec![2, 4, 6], gap: 3.0 };
    for c in propagate_fills(&s.project, &mut s.blobs, &pf).unwrap() {
        s.apply(c).unwrap();
    }
    let mut strip = aiai_paint::Image::new(240 * 5, 135);
    for (k, f) in [0u32, 2, 4, 6, 8].iter().enumerate() {
        let r = lint(&s.project, &s.blobs, &cut, *f, &fill, 3.0, 200).unwrap();
        assert!(r.unpainted.is_empty(), "quadro {f} sem cor: {:?}", r.unpainted);
        let img = render_frame(&s.project, &s.blobs, &cut, *f, &RenderOptions { background: Some([1.0; 3]), ..Default::default() }).unwrap().image;
        let half = aiai_paint::transform_image(&img, aiai_paint::Affine([0.5, 0.0, 0.0, 0.0, 0.5, 0.0]), 240, 135);
        strip.draw_over(&half, k as i32 * 240, 0, 1.0);
    }
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        strip.save_png(&std::path::Path::new(&dir).join("intercalacao_tira.png")).unwrap();
    }
}

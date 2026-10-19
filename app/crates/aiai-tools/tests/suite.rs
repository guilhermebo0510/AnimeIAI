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
    let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds, line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None };
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
fn fill_ignores_animated_layer_offset() {
    // camadas que entram deslizando (chaves offset_x): o balde trabalha no cel cru, não na posição animada do quadro
    let mut s = CoreSession::for_tests("tools-offset");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 640, "height": 360}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(24)));
    let fill_layer = create(&mut s, Some(&cut), "cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line_layer = create(&mut s, Some(&cut), "linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    create(&mut s, Some(&fill_layer), "cor1", NodeKind::Cel(CelData::new(1)));
    create(&mut s, Some(&line_layer), "linha1", NodeKind::Cel(CelData::new(1)));
    s.apply(Command::SetProps { id: fill_layer.clone(), name: None, data: json!({"line_refs": [line_layer.0]}) }).unwrap();
    let ink = ColorRef::Direct { rgba: Rgba::from_hex("#202020").unwrap() };
    let square = curve(&[[270.0, 130.0, 1.0], [370.0, 130.0, 1.0], [370.0, 230.0, 1.0], [270.0, 230.0, 1.0]], 3.0, &ink, true);
    let mut sq = square;
    sq.geometry = serde_json::from_value(json!("polyline")).unwrap();
    s.apply(draw_command(&s.project, &DrawRequest { cut: cut.clone(), frame: 0, layer: line_layer.clone(), curves: vec![sq], mirror_x: None }).unwrap()).unwrap();
    for l in [&fill_layer, &line_layer] {
        s.apply(Command::SetProps { id: l.clone(), name: None, data: json!({"keys": {"offset_x": [{"frame": 0, "value": -300.0, "ease": "ease_out"}, {"frame": 12, "value": 0.0, "ease": "hold"}]}}) }).unwrap();
    }
    let red = ColorRef::Direct { rgba: Rgba::from_hex("#d03030").unwrap() };
    let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds: vec![FillSeed { x: 320.0, y: 180.0, color: red }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None };
    let cmd = fill_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(cmd).unwrap();
    let raw = RenderOptions { only_layers: Some(vec![fill_layer.clone()]), camera: false, layer_motion: false, ..Default::default() };
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &raw).unwrap().image;
    assert!(img.get(320, 180)[3] > 0.99, "dentro do quadrado: pintado");
    assert!(img.get(50, 50)[3] < 0.01 && img.get(600, 300)[3] < 0.01, "fora do quadrado: vazio (o balde não pode ler a linha deslocada)");
    assert!(lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap().unpainted.is_empty());
    // no quadro 0 a camada aparece deslocada 300 px para a esquerda; no 12, no lugar
    let moved = RenderOptions { only_layers: Some(vec![fill_layer.clone()]), camera: false, ..Default::default() };
    assert!(render_frame(&s.project, &s.blobs, &cut, 0, &moved).unwrap().image.get(20, 180)[3] > 0.99);
    assert!(render_frame(&s.project, &s.blobs, &cut, 12, &moved).unwrap().image.get(320, 180)[3] > 0.99);
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
fn moving_partial_vector_selection_keeps_other_strokes() {
    use aiai_paint::select::SelectShape;
    use aiai_tools::transform::{Deform, TransformRequest, transform_command};
    let mut s = CoreSession::for_tests("trsel");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 400, "height": 200}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let vl = create(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    let vcel = create(&mut s, Some(&vl), "V1", NodeKind::Cel(CelData::new(1)));
    let black = ColorRef::Direct { rgba: Rgba::BLACK };
    // traço pequeno (a "unha"), traço grande que passa pela seleção só numa ponta e um traço longe
    let small = curve(&[[100.0, 100.0, 1.0], [110.0, 104.0, 1.0], [118.0, 100.0, 1.0]], 3.0, &black, false);
    let big = curve(&[[60.0, 40.0, 1.0], [300.0, 40.0, 1.0], [300.0, 180.0, 1.0], [60.0, 180.0, 1.0], [105.0, 110.0, 1.0]], 3.0, &black, false);
    let far = curve(&[[320.0, 20.0, 1.0], [380.0, 60.0, 1.0]], 3.0, &black, false);
    let req = DrawRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), curves: vec![small, big, far], mirror_x: None };
    s.apply(draw_command(&s.project, &req).unwrap()).unwrap();
    let before = match &s.project.node(&vcel).unwrap().data { NodeKind::Cel(c) => c.strokes.clone(), _ => unreachable!() };
    let lasso = SelectShape::Lasso { points: vec![[90.0, 90.0], [130.0, 90.0], [130.0, 115.0], [90.0, 115.0]] };
    let tr = TransformRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), deform: Deform::Affine { translate: [0.0, 30.0], scale: [1.0, 1.0], rotate: 0.0, shear: [0.0, 0.0], pivot: [0.0, 0.0] }, selection: Some(lasso), scale_width: true, strokes: vec![] };
    { let c = transform_command(&s.project, &mut s.blobs, &tr).unwrap(); s.apply(c).unwrap(); }
    let after = match &s.project.node(&vcel).unwrap().data { NodeKind::Cel(c) => c.strokes.clone(), _ => unreachable!() };
    assert_eq!(after.len(), 3, "nenhum traço some ao mover uma seleção parcial");
    assert!((after[0].points[0].y - before[0].points[0].y - 30.0).abs() < 0.01, "o traço selecionado desceu 30 px");
    assert_eq!(after[1].points, before[1].points, "o traço grande que só encosta na seleção fica");
    assert_eq!(after[2].points, before[2].points, "o traço longe fica");
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
        let req = FillRequest { cut: cut.clone(), frame: f, layer: fill.clone(), seeds: vec![FillSeed { x: sx, y: sy, color: skin.clone() }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None };
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

#[test]
fn close_leftovers_fills_only_small_gaps_with_the_surrounding_color() {
    let mut s = CoreSession::for_tests("frestas");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 400, "height": 240}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let fill_layer = create(&mut s, Some(&cut), "cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line_layer = create(&mut s, Some(&cut), "linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    create(&mut s, Some(&fill_layer), "cor1", NodeKind::Cel(CelData::new(1)));
    create(&mut s, Some(&line_layer), "linha1", NodeKind::Cel(CelData::new(1)));
    s.apply(Command::SetProps { id: fill_layer.clone(), name: None, data: json!({"line_refs": [line_layer.0]}) }).unwrap();
    let ink = ColorRef::Direct { rgba: Rgba::from_hex("#202020").unwrap() };
    let poly = |pts: &[[f32; 3]]| {
        let mut c = curve(pts, 2.0, &ink, true);
        c.geometry = serde_json::from_value(json!("polyline")).unwrap();
        c
    };
    // dois quadrados vermelhos separados por uma fresta fechada de ~4 px; um quadrado grande sem tinta à direita
    let a = poly(&[[40.0, 40.0, 1.0], [140.0, 40.0, 1.0], [140.0, 140.0, 1.0], [40.0, 140.0, 1.0]]);
    let b = poly(&[[146.0, 40.0, 1.0], [246.0, 40.0, 1.0], [246.0, 140.0, 1.0], [146.0, 140.0, 1.0]]);
    let big = poly(&[[270.0, 40.0, 1.0], [370.0, 40.0, 1.0], [370.0, 200.0, 1.0], [270.0, 200.0, 1.0]]);
    let mut top = curve(&[[139.0, 40.0, 1.0], [147.0, 40.0, 1.0]], 2.0, &ink, false);
    let mut bot = curve(&[[139.0, 140.0, 1.0], [147.0, 140.0, 1.0]], 2.0, &ink, false);
    top.tip = serde_json::from_value(json!("marker")).unwrap();
    bot.tip = serde_json::from_value(json!("marker")).unwrap();
    s.apply(draw_command(&s.project, &DrawRequest { cut: cut.clone(), frame: 0, layer: line_layer.clone(), curves: vec![a, b, big, top, bot], mirror_x: None }).unwrap()).unwrap();
    let red = ColorRef::Direct { rgba: Rgba::from_hex("#d03030").unwrap() };
    let seeds = vec![FillSeed { x: 90.0, y: 90.0, color: red.clone() }, FillSeed { x: 196.0, y: 90.0, color: red.clone() }];
    let before = {
        let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds: seeds.clone(), line_layers: vec![], gap: 0.0, expand: 2, line_threshold: 0.35, close_leftovers: None };
        let c = fill_command(&s.project, &mut s.blobs, &req).unwrap();
        let mut t = CoreSession::for_tests("x");
        t.project = s.project.clone();
        t.blobs = s.blobs.clone();
        t.apply(c).unwrap();
        lint(&t.project, &t.blobs, &cut, 0, &fill_layer, 0.0, 1).unwrap()
    };
    assert!(before.unpainted.iter().any(|r| r.area < 600), "sem fechar, a fresta aparece no lint: {:?}", before.unpainted);
    let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds, line_layers: vec![], gap: 0.0, expand: 2, line_threshold: 0.35, close_leftovers: Some(600) };
    let c = fill_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(c).unwrap();
    let raw = RenderOptions { only_layers: Some(vec![fill_layer.clone()]), camera: false, ..Default::default() };
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &raw).unwrap().image;
    let p = img.get(143, 90);
    assert!(p[3] > 0.99 && p[0] > 0.7 && p[1] < 0.3, "a fresta ficou vermelha (cor em volta): {p:?}");
    assert!(img.get(320, 120)[3] < 0.01, "a região grande sem semente continua sem tinta");
    let after = lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 0.0, 1).unwrap();
    assert!(after.unpainted.iter().all(|r| r.area > 600), "só a grande sobra no lint: {:?}", after.unpainted);
    assert_eq!(after.unpainted.len(), 1);
}

#[test]
fn local_tile_painting_matches_full_canvas_painting() {
    use aiai_paint::brush::{BrushPoint, BrushPreset, paint_stroke};
    use aiai_tools::paint::{PaintRequest, PaintStrokeSpec, cel_raster, paint_command};
    let mut s = CoreSession::for_tests("local-paint");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 900, "height": 500}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
    let rl = create(&mut s, Some(&cut), "R", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    let cel = create(&mut s, Some(&rl), "1", NodeKind::Cel(CelData::new(1)));
    let red = ColorRef::Direct { rgba: Rgba::from_hex("#c02020").unwrap() };
    let base = PaintRequest { cut: cut.clone(), frame: 0, layer: rl.clone(), preset: BrushPreset::round("b", 80.0, 0.9), color: red,
        strokes: vec![PaintStrokeSpec { points: vec![vec![20.0, 250.0], vec![880.0, 260.0]], smooth: false }], seed: 3 };
    let c = paint_command(&s.project, &mut s.blobs, &base).unwrap();
    s.apply(c).unwrap();
    for name in ["Aquarela", "Borrador", "Guache seco", "Folhagem"] {
        let preset = BrushPreset::defaults().into_iter().find(|p| p.name == name).unwrap();
        let pts = vec![vec![300.0, 200.0, 0.8], vec![360.0, 260.0, 1.0], vec![420.0, 300.0, 0.6]];
        // esperado: o motor no canvas inteiro
        let cd = match &s.project.node(&cel).unwrap().data { NodeKind::Cel(c) => c.clone(), _ => unreachable!() };
        let mut expect = cel_raster(&s.blobs, &cd, 900, 500).unwrap();
        let path: Vec<BrushPoint> = pts.iter().map(|q| BrushPoint { x: q[0], y: q[1], pressure: q[2] }).collect();
        paint_stroke(&mut expect, &preset, &path, [0.1, 0.4, 0.2, 1.0], 11);
        // pelo comando (recorte nos tiles tocados)
        let req = PaintRequest { cut: cut.clone(), frame: 0, layer: rl.clone(), preset, color: ColorRef::Direct { rgba: Rgba([0.1, 0.4, 0.2, 1.0]) },
            strokes: vec![PaintStrokeSpec { points: pts, smooth: false }], seed: 11 };
        let c = paint_command(&s.project, &mut s.blobs, &req).unwrap();
        s.apply(c).unwrap();
        let cd = match &s.project.node(&cel).unwrap().data { NodeKind::Cel(c) => c.clone(), _ => unreachable!() };
        let got = cel_raster(&s.blobs, &cd, 900, 500).unwrap();
        let max = got.data.iter().zip(&expect.data).map(|(a, b)| (0..4).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
        assert!(max < 2e-3, "{name}: recorte local = canvas inteiro (diferença máx {max})");
    }
}

#[test]
fn import_image_fits_into_reference_and_raster_cels() {
    use aiai_tools::import::{ImageFit, ImportImageRequest, import_image_command};
    let dir = std::env::temp_dir().join(format!("aiai-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // imagem 200x100 vermelha com faixa azul em cima, salva em PNG e em BMP
    let mut src = image::RgbaImage::from_pixel(200, 100, image::Rgba([220, 30, 30, 255]));
    for y in 0..20 {
        for x in 0..200 {
            src.put_pixel(x, y, image::Rgba([30, 30, 220, 255]));
        }
    }
    let png = dir.join("ref.png");
    let bmp = dir.join("ref.bmp");
    src.save(&png).unwrap();
    src.save(&bmp).unwrap();
    let mut s = CoreSession::for_tests("import");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 400, "height": 400}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
    let rf = create(&mut s, Some(&cut), "ref", NodeKind::Layer(LayerData::new(LayerType::Reference)));
    create(&mut s, Some(&rf), "1", NodeKind::Cel(CelData::new(1)));
    let rs = create(&mut s, Some(&cut), "R", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(&mut s, Some(&rs), "1", NodeKind::Cel(CelData::new(1)));
    // contain: 200x100 → 400x200 centralizada (y 100..300)
    let c = import_image_command(&s.project, &mut s.blobs, &ImportImageRequest { cut: cut.clone(), layer: rf.clone(), frame: 0, path: png.display().to_string(), fit: ImageFit::Contain }).unwrap();
    s.apply(c).unwrap();
    let opts = RenderOptions { only_layers: Some(vec![rf.clone()]), include_reference: true, camera: false, ..Default::default() };
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &opts).unwrap().image;
    assert!(img.get(200, 50)[3] < 0.01, "acima da imagem: vazio");
    let top = img.get(200, 110);
    let mid = img.get(200, 250);
    assert!(top[2] > 0.7 && top[0] < 0.3, "faixa azul no alto da imagem ajustada: {top:?}");
    assert!(mid[0] > 0.7 && mid[3] > 0.99, "vermelho no meio: {mid:?}");
    // raster: vira tiles (BMP também abre)
    let c = import_image_command(&s.project, &mut s.blobs, &ImportImageRequest { cut: cut.clone(), layer: rs.clone(), frame: 0, path: bmp.display().to_string(), fit: ImageFit::Actual }).unwrap();
    s.apply(c).unwrap();
    let opts = RenderOptions { only_layers: Some(vec![rs.clone()]), camera: false, ..Default::default() };
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &opts).unwrap().image;
    assert!(img.get(10, 10)[2] > 0.7 && img.get(150, 80)[0] > 0.7 && img.get(300, 300)[3] < 0.01);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn vector_eraser_whole_segment_and_up_to_intersection() {
    use aiai_tools::erase::{EraseMode, EraseVectorRequest, erase_vector_command};
    let setup = || {
        let mut s = CoreSession::for_tests("erase");
        s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 400, "height": 200}}) }).unwrap();
        let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
        let vl = create(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
        let cel = create(&mut s, Some(&vl), "1", NodeKind::Cel(CelData::new(1)));
        let ink = ColorRef::Direct { rgba: Rgba::BLACK };
        let mut line = |pts: &[[f32; 3]]| {
            let mut c = curve(pts, 3.0, &ink, false);
            c.geometry = serde_json::from_value(json!("polyline")).unwrap();
            c
        };
        let h = line(&[[20.0, 100.0, 1.0], [380.0, 100.0, 1.0]]);
        let v1 = line(&[[150.0, 40.0, 1.0], [150.0, 160.0, 1.0]]);
        let v2 = line(&[[250.0, 40.0, 1.0], [250.0, 160.0, 1.0]]);
        s.apply(draw_command(&s.project, &DrawRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), curves: vec![h, v1, v2], mirror_x: None }).unwrap()).unwrap();
        (s, cut, vl, cel)
    };
    let strokes = |s: &CoreSession, cel: &Id| match &s.project.node(cel).unwrap().data { NodeKind::Cel(c) => c.strokes.clone(), _ => unreachable!() };
    let span = |st: &Stroke| (st.points.iter().map(|p| p.x).fold(f32::MAX, f32::min), st.points.iter().map(|p| p.x).fold(f32::MIN, f32::max));
    for mode in [EraseMode::Whole, EraseMode::Segment, EraseMode::Intersection] {
        let (mut s, cut, vl, cel) = setup();
        // borracha passa em x = 200 pela linha horizontal (entre as duas verticais)
        let req = EraseVectorRequest { cut: cut.clone(), frame: 0, layer: vl.clone(), path: vec![[200.0, 90.0], [200.0, 110.0]], radius: 6.0, mode };
        let c = erase_vector_command(&s.project, &req).unwrap();
        s.apply(c).unwrap();
        let st = strokes(&s, &cel);
        let horiz: Vec<&Stroke> = st.iter().filter(|x| { let (a, b) = span(x); b - a > 20.0 }).collect();
        match mode {
            EraseMode::Whole => assert_eq!((st.len(), horiz.len()), (2, 0), "só as verticais sobram"),
            EraseMode::Segment => {
                assert_eq!(horiz.len(), 2, "a horizontal vira dois pedaços");
                let mut spans: Vec<(f32, f32)> = horiz.iter().map(|x| span(x)).collect();
                spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                assert!(spans[0].1 < 196.0 && spans[0].1 > 185.0 && spans[1].0 > 204.0 && spans[1].0 < 215.0, "buraco só onde a borracha passou: {spans:?}");
            }
            EraseMode::Intersection => {
                assert_eq!(horiz.len(), 2);
                let mut spans: Vec<(f32, f32)> = horiz.iter().map(|x| span(x)).collect();
                spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                assert!((spans[0].1 - 150.0).abs() < 1.5 && (spans[1].0 - 250.0).abs() < 1.5, "apaga até as interseções: {spans:?}");
                assert!((spans[0].0 - 20.0).abs() < 1.0 && (spans[1].1 - 380.0).abs() < 1.0);
            }
        }
        // desfazer volta ao original
        s.undo().unwrap();
        assert_eq!(strokes(&s, &cel).len(), 3);
    }
}

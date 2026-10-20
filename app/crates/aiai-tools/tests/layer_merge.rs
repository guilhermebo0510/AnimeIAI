//! Mesclar camadas (Ctrl+E): raster+raster com opacidade e modo, vetor+vetor (traços juntos), vetor+raster (rasteriza),
//! vários cels no tempo, seleção e "visíveis", recusas e undo de um passo só.

use aiai_core::*;
use aiai_paint::Image;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::context::ToolError;
use aiai_tools::draw::{CurveSpec, DrawRequest, draw_command};
use aiai_tools::layer_merge::{LayerMergeRequest, MergeOp, layer_merge_command};
use aiai_tools::paint::tiles_for_region;
use serde_json::json;

struct Doc {
    s: CoreSession,
    cut: Id,
}

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn doc(duration: u32) -> Doc {
    let mut s = CoreSession::for_tests("merge");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 320, "height": 180}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(duration)));
    Doc { s, cut }
}

impl Doc {
    fn layer(&mut self, name: &str, kind: LayerType, props: serde_json::Value) -> Id {
        let id = create(&mut self.s, Some(&self.cut), name, NodeKind::Layer(LayerData::new(kind)));
        if !props.is_null() {
            self.s.apply(Command::SetProps { id: id.clone(), name: None, data: props }).unwrap();
        }
        id
    }

    fn cel(&mut self, layer: &Id, number: u32) -> Id {
        create(&mut self.s, Some(layer), &number.to_string(), NodeKind::Cel(CelData::new(number)))
    }

    /// Retângulo [x0, y0, x1, y1) de cor straight `rgba` nos tiles do cel.
    fn rect(&mut self, cel: &Id, r: [u32; 4], rgba: [f32; 4]) {
        let mut img = Image::new(320, 180);
        for y in r[1]..r[3] {
            for x in r[0]..r[2] {
                let i = img.idx(x, y);
                img.data[i] = [rgba[0] * rgba[3], rgba[1] * rgba[3], rgba[2] * rgba[3], rgba[3]];
            }
        }
        let tiles = tiles_for_region(&mut self.s.blobs, &img, [0, 0, 320, 180]);
        self.s.apply(Command::SetTiles { cel: cel.clone(), tiles }).unwrap();
    }

    fn line(&mut self, layer: &Id, y: f32, color: [f32; 4]) {
        let c: CurveSpec = serde_json::from_value(json!({
            "points": [[20.0, y, 1.0], [300.0, y, 1.0]], "width": 8.0, "tip": "g_pen", "color": {"type": "direct", "rgba": color}, "closed": false
        }))
        .unwrap();
        let cmd = draw_command(&self.s.project, &DrawRequest { cut: self.cut.clone(), frame: 0, layer: layer.clone(), curves: vec![c], mirror_x: None }).unwrap();
        self.s.apply(cmd).unwrap();
    }

    fn frame(&self, f: u32) -> Image {
        render_frame(&self.s.project, &self.s.blobs, &self.cut, f, &RenderOptions { camera: false, ..Default::default() }).unwrap().image
    }

    fn merge(&mut self, req: LayerMergeRequest) -> Result<aiai_tools::layer_merge::MergePlan, ToolError> {
        let plan = layer_merge_command(&self.s.project, &mut self.s.blobs, &mut self.s.ids, &req)?;
        self.s.apply(plan.command.clone()).unwrap();
        Ok(plan)
    }

    fn layers(&self) -> Vec<String> {
        self.s.project.node(&self.cut).unwrap().children.iter().filter_map(|c| self.s.project.node(c)).filter(|n| matches!(n.data, NodeKind::Layer(_))).map(|n| n.name.clone()).collect()
    }

    fn cels(&self, layer: &Id) -> Vec<CelData> {
        let mut v: Vec<CelData> = self.s.project.node(layer).unwrap().children.iter().filter_map(|c| match &self.s.project.node(c)?.data { NodeKind::Cel(c) => Some(c.clone()), _ => None }).collect();
        v.sort_by_key(|c| c.number);
        v
    }

    fn ldata(&self, layer: &Id) -> LayerData {
        match &self.s.project.node(layer).unwrap().data {
            NodeKind::Layer(l) => l.clone(),
            _ => panic!("não é camada"),
        }
    }
}

fn down(layer: &Id) -> LayerMergeRequest {
    LayerMergeRequest { layer: Some(layer.clone()), ..Default::default() }
}

fn same(a: &Image, b: &Image, tol: f32, what: &str) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let worst = a.data.iter().zip(&b.data).map(|(p, q)| (0..4).map(|c| (p[c] - q[c]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
    assert!(worst <= tol, "{what}: diferença máxima {worst}");
}

fn err(r: Result<aiai_tools::layer_merge::MergePlan, ToolError>) -> String {
    match r {
        Err(ToolError::Invalid(m)) => m,
        other => panic!("devia recusar: {other:?}"),
    }
}

#[test]
fn raster_down_composites_with_opacity_and_blend_in_one_undo_step() {
    let mut d = doc(12);
    let bottom = d.layer("Fundo", LayerType::Raster, json!(null));
    let top = d.layer("Tinta", LayerType::Raster, json!({"opacity": 0.5}));
    let (cb, ct) = (d.cel(&bottom, 1), d.cel(&top, 1));
    d.rect(&cb, [20, 20, 200, 120], [1.0, 0.1, 0.1, 1.0]);
    d.rect(&ct, [100, 60, 280, 160], [0.1, 0.2, 1.0, 1.0]);
    let before = d.frame(0);
    assert!(before.get(150, 90)[2] > 0.2, "o azul meio transparente aparece sobre o vermelho");
    let plan = d.merge(down(&top)).unwrap();
    assert_eq!((plan.layer.clone(), plan.kind, plan.cels_changed, plan.cels_created), (bottom.clone(), LayerType::Raster, 1, 0));
    assert_eq!(d.layers(), vec!["Fundo"], "a de cima saiu, a de baixo guarda o nome");
    same(&before, &d.frame(0), 2e-3, "raster normal");
    assert_eq!(d.cels(&bottom).len(), 1, "o cel de baixo recebeu o resultado no lugar");
    // um passo de undo devolve as duas camadas e o desenho
    d.s.undo().unwrap();
    assert_eq!(d.layers(), vec!["Fundo", "Tinta"]);
    same(&before, &d.frame(0), 2e-3, "depois do undo");
    d.s.redo().unwrap();
    assert_eq!(d.layers(), vec!["Fundo"]);
    // a camada de baixo é o que o undo anterior (criar o último rect) deixou: um só passo desfeito a mais, não vários
    d.s.undo().unwrap();
    assert_eq!(d.layers(), vec!["Fundo", "Tinta"], "a mesclagem inteira foi UM passo");
}

#[test]
fn raster_blend_modes_and_pixels_past_the_canvas_edge_survive() {
    let mut d = doc(6);
    let bottom = d.layer("A", LayerType::Raster, json!(null));
    let top = d.layer("B", LayerType::Raster, json!({"blend": "multiply", "opacity": 0.8}));
    let (cb, ct) = (d.cel(&bottom, 1), d.cel(&top, 1));
    d.rect(&cb, [0, 0, 320, 180], [0.9, 0.8, 0.5, 1.0]);
    d.rect(&ct, [40, 40, 160, 140], [0.3, 0.6, 0.9, 1.0]);
    // um tile fora do quadro (cenário maior que o canvas) na camada de baixo
    let mut off = Image::new(256, 256);
    off.data.iter_mut().for_each(|p| *p = [0.0, 0.5, 0.0, 1.0]);
    let tiles = aiai_paint::tile::image_to_tiles(&off, [-256, 0]);
    let edits: Vec<_> = tiles.into_iter().filter_map(|(x, y, px)| px.map(|px| aiai_core::command::TileEdit { x, y, blob: Some(d.s.blobs.put(aiai_paint::tile::encode_tile(&px))) })).collect();
    assert_eq!(edits.len(), 1);
    d.s.apply(Command::SetTiles { cel: cb.clone(), tiles: edits }).unwrap();
    let before = d.frame(0);
    d.merge(down(&top)).unwrap();
    same(&before, &d.frame(0), 3e-3, "multiply 80%");
    let c = &d.cels(&bottom)[0];
    assert!(c.tiles.iter().any(|t| t.x == -1), "o tile fora do quadro continua lá");
}

#[test]
fn vector_plus_vector_keeps_geometry_and_opacity() {
    let mut d = doc(6);
    let bottom = d.layer("Linha 1", LayerType::Vector, json!(null));
    let top = d.layer("Linha 2", LayerType::Vector, json!({"opacity": 0.5}));
    d.cel(&bottom, 1);
    d.cel(&top, 1);
    d.line(&bottom, 60.0, [0.0, 0.0, 0.0, 1.0]);
    d.line(&bottom, 90.0, [1.0, 0.0, 0.0, 1.0]);
    d.line(&top, 120.0, [0.0, 0.0, 1.0, 1.0]);
    // a de baixo tem opacidade 1 e a de cima 0,5: o traço leva a diferença
    let before = d.frame(0);
    let top_stroke = d.cels(&top)[0].strokes[0].clone();
    let plan = d.merge(down(&top)).unwrap();
    assert_eq!(plan.kind, LayerType::Vector);
    let cel = &d.cels(&bottom)[0];
    assert_eq!(cel.strokes.len(), 3, "os traços de cima entram no cel de baixo");
    let moved = &cel.strokes[2];
    assert_eq!((moved.points.clone(), moved.width, moved.geometry.clone()), (top_stroke.points.clone(), top_stroke.width, top_stroke.geometry.clone()), "geometria intacta");
    assert!((moved.opacity - 0.5).abs() < 1e-6, "opacidade da camada virou opacidade do traço: {}", moved.opacity);
    let ids: std::collections::BTreeSet<_> = cel.strokes.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids.len(), 3, "ids únicos");
    same(&before, &d.frame(0), 2e-3, "vetor + vetor");
    d.s.undo().unwrap();
    assert_eq!(d.cels(&bottom)[0].strokes.len(), 2);
    assert_eq!(d.layers().len(), 2);
}

#[test]
fn vector_and_raster_rasterize_into_a_raster_layer() {
    for vector_below in [true, false] {
        let mut d = doc(6);
        let (bottom, top);
        if vector_below {
            bottom = d.layer("V", LayerType::Vector, json!(null));
            top = d.layer("R", LayerType::Raster, json!({"opacity": 0.7}));
        } else {
            bottom = d.layer("R", LayerType::Raster, json!(null));
            top = d.layer("V", LayerType::Vector, json!({"opacity": 0.7}));
        }
        let (cb, ct) = (d.cel(&bottom, 1), d.cel(&top, 1));
        let _ = (&cb, &ct);
        if vector_below {
            d.line(&bottom, 80.0, [0.1, 0.1, 0.1, 1.0]);
            d.rect(&ct, [60, 40, 260, 140], [0.9, 0.2, 0.3, 1.0]);
        } else {
            d.rect(&cb, [60, 40, 260, 140], [0.9, 0.2, 0.3, 1.0]);
            d.line(&top, 80.0, [0.1, 0.1, 0.1, 1.0]);
        }
        let before = d.frame(0);
        let plan = d.merge(down(&top)).unwrap();
        assert_eq!(plan.kind, LayerType::Raster, "vector_below={vector_below}");
        assert_eq!(d.ldata(&bottom).kind, LayerType::Raster);
        let cel = &d.cels(&bottom)[0];
        assert!(cel.strokes.is_empty() && !cel.tiles.is_empty(), "traços viram pixels");
        same(&before, &d.frame(0), 4e-3, if vector_below { "vetor embaixo" } else { "raster embaixo" });
        d.s.undo().unwrap();
        assert_eq!(d.ldata(&bottom).kind, if vector_below { LayerType::Vector } else { LayerType::Raster });
        same(&before, &d.frame(0), 4e-3, "undo");
    }
}

#[test]
fn animated_top_over_a_still_bottom_makes_one_cel_per_combination() {
    use aiai_core::xsheet::{CellRef, Track, TrackKind};
    let mut d = doc(12);
    let bottom = d.layer("Fundo", LayerType::Raster, json!(null));
    let top = d.layer("Anim", LayerType::Raster, json!(null));
    let cb = d.cel(&bottom, 1);
    let (t1, t2) = (d.cel(&top, 1), d.cel(&top, 2));
    d.rect(&cb, [0, 0, 320, 180], [1.0, 1.0, 0.8, 1.0]);
    d.rect(&t1, [20, 20, 80, 80], [1.0, 0.0, 0.0, 1.0]);
    d.rect(&t2, [220, 90, 300, 160], [0.0, 0.0, 1.0, 1.0]);
    let mut tr = Track::new(d.s.ids.next(), "Anim", TrackKind::Drawing, Some(top.clone()));
    tr.cells.insert(0, CellRef::Cel { number: 1 });
    tr.cells.insert(6, CellRef::Cel { number: 2 });
    d.s.apply(Command::AddTrack { cut: d.cut.clone(), index: None, track: tr }).unwrap();
    let before: Vec<Image> = (0..12).map(|f| d.frame(f)).collect();
    assert!(before[0].get(40, 40)[0] > 0.9 && before[8].get(260, 120)[2] > 0.9);
    let plan = d.merge(down(&top)).unwrap();
    assert_eq!((plan.cels_changed, plan.cels_created), (1, 1), "o cel de baixo fica com a 1ª combinação, a 2ª ganha cel novo");
    assert_eq!(d.cels(&bottom).iter().map(|c| c.number).collect::<Vec<_>>(), vec![1, 2]);
    let cut = match &d.s.project.node(&d.cut).unwrap().data {
        NodeKind::Cut(c) => c.clone(),
        _ => unreachable!(),
    };
    let track = cut.xsheet.track_for_layer(&bottom).expect("a trilha da camada de baixo foi criada");
    assert_eq!(track.cells.iter().map(|(f, c)| (*f, *c)).collect::<Vec<_>>(), vec![(0, CellRef::Cel { number: 1 }), (6, CellRef::Cel { number: 2 })]);
    assert!(cut.xsheet.track_for_layer(&top).is_none(), "a trilha da camada que saiu sumiu");
    for f in 0..12 {
        same(&before[f as usize], &d.frame(f), 2e-3, &format!("quadro {f}"));
    }
    d.s.undo().unwrap();
    assert_eq!(d.cels(&bottom).len(), 1);
    for f in 0..12 {
        same(&before[f as usize], &d.frame(f), 2e-3, &format!("undo, quadro {f}"));
    }
}

#[test]
fn bottom_keeps_its_own_drawing_where_nothing_is_on_top() {
    use aiai_core::xsheet::{CellRef, Track, TrackKind};
    let mut d = doc(10);
    let bottom = d.layer("Fundo", LayerType::Raster, json!(null));
    let top = d.layer("Cima", LayerType::Raster, json!(null));
    let cb = d.cel(&bottom, 1);
    let ct = d.cel(&top, 1);
    d.rect(&cb, [0, 0, 320, 180], [0.2, 0.8, 0.2, 1.0]);
    d.rect(&ct, [100, 50, 220, 130], [0.9, 0.1, 0.1, 1.0]);
    // a de cima só aparece nos quadros 3 a 6
    let mut tr = Track::new(d.s.ids.next(), "Cima", TrackKind::Drawing, Some(top.clone()));
    tr.cells.insert(0, CellRef::Empty);
    tr.cells.insert(3, CellRef::Cel { number: 1 });
    tr.cells.insert(7, CellRef::Empty);
    d.s.apply(Command::AddTrack { cut: d.cut.clone(), index: None, track: tr }).unwrap();
    let before: Vec<Image> = (0..10).map(|f| d.frame(f)).collect();
    assert!(before[0].get(150, 90)[0] < 0.5 && before[4].get(150, 90)[0] > 0.8);
    let plan = d.merge(down(&top)).unwrap();
    assert_eq!((plan.cels_changed, plan.cels_created), (0, 1), "o cel 1 segue puro nos quadros sem nada por cima");
    for f in 0..10 {
        same(&before[f as usize], &d.frame(f), 2e-3, &format!("quadro {f}"));
    }
}

#[test]
fn refusals_change_nothing() {
    let mut d = doc(6);
    let a = d.layer("A", LayerType::Raster, json!(null));
    let ca = d.cel(&a, 1);
    d.rect(&ca, [10, 10, 100, 100], [1.0, 0.0, 0.0, 1.0]);
    let before = d.s.change_counter;
    for (kind, props, what) in [
        (LayerType::Fill, json!(null), "preenchimento"),
        (LayerType::Group, json!(null), "pasta"),
        (LayerType::Text, json!(null), "texto"),
        (LayerType::Raster, json!({"locked": true}), "travada"),
        (LayerType::Raster, json!({"visible": false}), "oculta"),
    ] {
        let top = d.layer("Cima", kind, props);
        let m = err(d.merge(down(&top)));
        assert!(m.contains(what), "{what}: {m}");
        d.s.apply(Command::DeleteNode { id: top }).unwrap();
    }
    // a de baixo oculta
    d.s.apply(Command::SetProps { id: a.clone(), name: None, data: json!({"visible": false}) }).unwrap();
    let top = d.layer("Cima", LayerType::Raster, json!(null));
    assert!(err(d.merge(down(&top))).contains("\"A\" está oculta"));
    d.s.apply(Command::SetProps { id: a.clone(), name: None, data: json!({"visible": true}) }).unwrap();
    // sem camada abaixo
    d.s.apply(Command::DeleteNode { id: a.clone() }).unwrap();
    assert!(err(d.merge(down(&top))).contains("não há camada abaixo"));
    assert!(d.s.change_counter > before);
    // as recusas em si não aplicaram nada: um contador só sobe por comando aplicado
    let n = d.s.change_counter;
    let _ = d.merge(down(&top));
    assert_eq!(d.s.change_counter, n);
    // uma visível de outro tipo no meio muda de ordem
    let b = d.layer("B", LayerType::Raster, json!(null));
    let c = d.layer("Cor", LayerType::Fill, json!(null));
    let e = d.layer("E", LayerType::Raster, json!(null));
    let _ = (&b, &c);
    let m = err(d.merge(LayerMergeRequest { op: Some(MergeOp::Selected), layers: vec![b.clone(), e.clone()], ..Default::default() }));
    assert!(m.contains("entre as que seriam mescladas"), "{m}");
    // `visible` com menos de duas
    let m = err(d.merge(LayerMergeRequest { op: Some(MergeOp::Visible), parent: Some(top.clone()), ..Default::default() }));
    assert!(!m.is_empty());
}

#[test]
fn selected_and_visible_merge_into_the_lowest() {
    let mut d = doc(6);
    let a = d.layer("A", LayerType::Raster, json!(null));
    let b = d.layer("B", LayerType::Raster, json!({"opacity": 0.6}));
    let c = d.layer("C", LayerType::Raster, json!({"blend": "screen"}));
    let hidden = d.layer("Oculta", LayerType::Raster, json!({"visible": false}));
    for (l, r, col) in [(&a, [10, 10, 150, 100], [0.8, 0.1, 0.1, 1.0]), (&b, [60, 40, 220, 140], [0.1, 0.8, 0.1, 1.0]), (&c, [120, 70, 300, 170], [0.1, 0.1, 0.8, 1.0]), (&hidden, [0, 0, 50, 50], [1.0, 1.0, 0.0, 1.0])] {
        let cel = d.cel(l, 1);
        d.rect(&cel, r, col);
    }
    let before = d.frame(0);
    assert!(err(d.merge(LayerMergeRequest { op: Some(MergeOp::Selected), layers: vec![a.clone(), hidden.clone()], ..Default::default() })).contains("oculta"));
    // as três juntas (A embaixo, B e C por cima; a oculta fica onde está)
    let plan = d.merge(LayerMergeRequest { op: Some(MergeOp::Selected), layers: vec![c.clone(), a.clone(), b.clone()], ..Default::default() }).unwrap();
    assert_eq!((plan.layer.clone(), plan.merged.len()), (a.clone(), 2));
    assert_eq!(d.layers(), vec!["A", "Oculta"]);
    same(&before, &d.frame(0), 3e-3, "A+B+C");
    d.s.undo().unwrap();
    assert_eq!(d.layers(), vec!["A", "B", "C", "Oculta"]);
    d.s.redo().unwrap();

    // "visíveis": a mesclada e uma nova (a oculta no meio não atrapalha)
    let extra = d.layer("Extra", LayerType::Raster, json!(null));
    let ce = d.cel(&extra, 1);
    d.rect(&ce, [200, 10, 300, 60], [1.0, 1.0, 1.0, 1.0]);
    let before2 = d.frame(0);
    let plan = d.merge(LayerMergeRequest { op: Some(MergeOp::Visible), cut: Some(d.cut.clone()), ..Default::default() }).unwrap();
    assert_eq!(plan.layer, a);
    assert_eq!(d.layers(), vec!["A", "Oculta"]);
    same(&before2, &d.frame(0), 3e-3, "visíveis");
}

#[test]
fn masks_and_bucket_borders_pointing_at_a_merged_layer_follow_the_bottom() {
    let mut d = doc(6);
    let line_a = d.layer("Linha A", LayerType::Vector, json!(null));
    let line_b = d.layer("Linha B", LayerType::Vector, json!(null));
    let fill = d.layer("Cor", LayerType::Fill, json!(null));
    d.cel(&line_a, 1);
    d.cel(&line_b, 1);
    d.cel(&fill, 1);
    d.s.apply(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [line_b.0, line_a.0]}) }).unwrap();
    // o Cor está acima das duas linhas: nada visível entre elas
    d.merge(down(&line_b)).unwrap();
    assert_eq!(d.ldata(&fill).line_refs, vec![line_a.clone()], "a fronteira do balde aponta para a que ficou, sem duplicar");
    assert_eq!(d.layers(), vec!["Linha A", "Cor"]);
}

#[test]
fn different_motion_and_clipping_are_refused() {
    let mut d = doc(6);
    let a = d.layer("A", LayerType::Raster, json!(null));
    let b = d.layer("B", LayerType::Raster, json!({"offset": [30.0, 0.0]}));
    d.cel(&a, 1);
    d.cel(&b, 1);
    assert!(err(d.merge(down(&b))).contains("posição"));
    d.s.apply(Command::SetProps { id: b.clone(), name: None, data: json!({"offset": [0.0, 0.0], "clip_to_below": true}) }).unwrap();
    assert!(err(d.merge(down(&b))).contains("recorte"));
    d.s.apply(Command::SetProps { id: b.clone(), name: None, data: json!({"clip_to_below": false}) }).unwrap();
    d.merge(down(&b)).unwrap();
    assert_eq!(d.layers(), vec!["A"]);
}

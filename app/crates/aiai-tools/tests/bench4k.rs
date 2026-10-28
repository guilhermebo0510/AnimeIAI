//! Medida de desempenho no caso da auditoria (4K, 24 camadas): pincelada, clique do balde e render do quadro.
//! `cargo test -p aiai-tools --release --test bench4k -- --ignored --nocapture` (grava `bench4k.json` em
//! `AIAI_EVIDENCE_DIR`, se definido).

use aiai_core::*;
use aiai_paint::brush::BrushPreset;
use aiai_render::{FrameCache, RenderOptions, render_frame, render_frame_cached};
use aiai_tools::draw::{CurveSpec, DrawRequest, draw_command};
use aiai_tools::fill::{FillRequest, FillSeed, fill_command};
use aiai_tools::paint::{PaintRequest, PaintStrokeSpec, paint_command};
use serde_json::json;
use std::time::Instant;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

#[test]
#[ignore]
fn bench_4k_24_layers() {
    let (w, h) = (3840u32, 2160u32);
    let mut s = CoreSession::for_tests("bench");
    s.full_validate = false;
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": w, "height": h}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let blue = ColorRef::Direct { rgba: Rgba::from_hex("#3060c0").unwrap() };
    // 22 camadas raster com uma faixa larga pintada cada (como cenário/sombras), 1 de linha, 1 de cor
    let mut rasters = Vec::new();
    for i in 0..22 {
        let l = create(&mut s, Some(&cut), &format!("R{i}"), NodeKind::Layer(LayerData::new(LayerType::Raster)));
        create(&mut s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
        let y = 100.0 + i as f32 * 90.0;
        let req = PaintRequest { cut: cut.clone(), frame: 0, layer: l.clone(), preset: BrushPreset::round("r", 120.0, 0.8), color: blue.clone(),
            strokes: vec![PaintStrokeSpec { points: vec![vec![100.0, y], vec![3700.0, y + 40.0]], smooth: false }], seed: i };
        let c = paint_command(&s.project, &mut s.blobs, &req).unwrap();
        s.apply(c).unwrap();
        rasters.push(l);
    }
    let fill = create(&mut s, Some(&cut), "cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line = create(&mut s, Some(&cut), "linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    create(&mut s, Some(&fill), "1", NodeKind::Cel(CelData::new(1)));
    create(&mut s, Some(&line), "1", NodeKind::Cel(CelData::new(1)));
    s.apply(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [line.0]}) }).unwrap();
    let ink = ColorRef::Direct { rgba: Rgba::BLACK };
    let mut curves = Vec::new();
    for k in 0..30 {
        let (cx, cy) = (300.0 + (k % 10) as f32 * 340.0, 400.0 + (k / 10) as f32 * 600.0);
        let pts: Vec<Vec<f32>> = (0..12).map(|i| { let a = i as f32 / 12.0 * std::f32::consts::TAU; vec![cx + 130.0 * a.cos(), cy + 200.0 * a.sin(), 1.0] }).collect();
        curves.push(serde_json::from_value::<CurveSpec>(json!({"points": pts, "width": 5.0, "tip": "g_pen", "color": ink, "closed": true})).unwrap());
    }
    let c = draw_command(&s.project, &DrawRequest { cut: cut.clone(), frame: 0, layer: line.clone(), curves, mirror_x: None }).unwrap();
    s.apply(c).unwrap();

    // pincelada curta (como um retoque de 60 px) na última camada raster
    let t = Instant::now();
    let req = PaintRequest { cut: cut.clone(), frame: 0, layer: rasters[21].clone(), preset: BrushPreset::defaults().into_iter().find(|p| p.name == "Aquarela").unwrap(), color: blue.clone(),
        strokes: vec![PaintStrokeSpec { points: vec![vec![1900.0, 1000.0], vec![1940.0, 1030.0], vec![1960.0, 1060.0]], smooth: true }], seed: 1 };
    let c = paint_command(&s.project, &mut s.blobs, &req).unwrap();
    let paint_ms = ms(t);
    s.apply(c).unwrap();
    // dois cliques de balde seguidos (o segundo pode usar cache)
    let mut fill_ms = Vec::new();
    for (x, y) in [(300.0, 400.0), (640.0, 400.0)] {
        let t = Instant::now();
        let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill.clone(), seeds: vec![FillSeed { x, y, color: blue.clone() }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None, skip_seeds_on_line: false, only_unpainted: false };
        let c = fill_command(&s.project, &mut s.blobs, &req).unwrap();
        fill_ms.push(ms(t));
        s.apply(c).unwrap();
    }
    let t = Instant::now();
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let render_ms = ms(t);
    assert_eq!((img.width, img.height), (w, h));
    // canvas do editor: render com a foto do composto abaixo; depois de um retoque na camada 22, re-render
    let opts = RenderOptions { camera: false, ..Default::default() };
    let mut cache = FrameCache::default();
    let first = render_frame_cached(&s.project, &s.blobs, &cut, 0, &opts, &mut cache).unwrap();
    // como o app: o quadro que já foi para a tela volta para o cache (a memória é reaproveitada)
    cache.recycle(first.image);
    let mut cached_ms = Vec::new();
    for k in 0..2 {
        let req = PaintRequest { cut: cut.clone(), frame: 0, layer: rasters[21].clone(), preset: BrushPreset::round("r", 30.0, 0.8), color: blue.clone(),
            strokes: vec![PaintStrokeSpec { points: vec![vec![1000.0 + k as f32 * 50.0, 900.0], vec![1060.0 + k as f32 * 50.0, 940.0]], smooth: false }], seed: 9 };
        let c = paint_command(&s.project, &mut s.blobs, &req).unwrap();
        s.apply(c).unwrap();
        let t = Instant::now();
        let out = render_frame_cached(&s.project, &s.blobs, &cut, 0, &opts, &mut cache).unwrap();
        cached_ms.push(ms(t));
        cache.recycle(out.image);
    }
    let out = json!({"canvas": [w, h], "layers": 24, "paint_ms": paint_ms, "fill_ms": fill_ms, "render_ms": render_ms, "render_cached_after_edit_ms": cached_ms, "skipped_layers": cache.last_skipped});
    println!("{out}");
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("bench4k.json"), serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}

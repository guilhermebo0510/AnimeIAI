//! Golden tests do compositor (render CPU determinístico). Os hashes ficam em `tests/golden/*.hash`;
//! para atualizar: `AIAI_UPDATE_GOLDEN=1 cargo test -p aiai-render --test golden` e revisar os PNGs gerados.
//! `AIAI_EVIDENCE_DIR=<pasta>` grava os PNGs para inspeção visual.

use aiai_core::command::{IndexedStroke, TileEdit};
use aiai_core::*;
use aiai_paint::{Image, Mask, poly, tile};
use aiai_render::{RenderOptions, render_frame};
use serde_json::json;
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

fn check_golden(name: &str, img: &Image) {
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        img.save_png(&PathBuf::from(&dir).join(format!("{name}.png"))).unwrap();
    }
    let hash = img.content_hash();
    let path = golden_dir().join(format!("{name}.hash"));
    if std::env::var("AIAI_UPDATE_GOLDEN").is_ok() || !path.exists() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        std::fs::write(&path, &hash).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(hash, expected.trim(), "golden '{name}' mudou (rode com AIAI_UPDATE_GOLDEN=1 e revise o PNG)");
}

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn layer(s: &mut CoreSession, parent: &Id, name: &str, kind: LayerType, patch: serde_json::Value) -> (Id, Id) {
    let l = create(s, Some(parent), name, NodeKind::Layer(LayerData::new(kind)));
    if !patch.is_null() {
        s.apply(Command::SetProps { id: l.clone(), name: None, data: patch }).unwrap();
    }
    if kind == LayerType::Group {
        return (l, Id::from(""));
    }
    let c = create(s, Some(&l), &format!("{name}1"), NodeKind::Cel(CelData::new(1)));
    (l, c)
}

fn gpen(points: &[(f32, f32, f32)], width: f32, color: ColorRef) -> Stroke {
    Stroke {
        id: Id::from(""),
        points: points.iter().map(|&(x, y, p)| StrokePoint { x, y, pressure: p, tilt: None, t: None }).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::GPen,
        width,
        profile: WidthProfile::default(),
        color,
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
    }
}

/// Cena de teste: fundo raster em degradê (tiles), pasta isolada com personagem (preenchimento + sombra multiplicada
/// recortada + linha G-pen), máscara, e camadas com vários modos de mistura.
fn build_scene() -> (CoreSession, Id) {
    let mut s = CoreSession::for_tests("golden");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 512, "height": 288}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(24)));
    let pal = create(
        &mut s,
        None,
        "Settei",
        NodeKind::Palette(PaletteData {
            colors: vec![
                NamedColor { id: Id::from("pele"), name: "Pele".into(), group: "pele".into(), role: "normal".into(), rgba: Rgba::from_hex("#f6dcc8").unwrap(), variants: Default::default() },
                NamedColor { id: Id::from("pele_s"), name: "Pele sombra".into(), group: "pele".into(), role: "sombra".into(), rgba: Rgba::from_hex("#d9a993").unwrap(), variants: Default::default() },
                NamedColor { id: Id::from("linha"), name: "Linha".into(), group: "linha".into(), role: "linha".into(), rgba: Rgba::from_hex("#3a2a2a").unwrap(), variants: Default::default() },
            ],
            active_variant: String::new(),
        }),
    );
    let pref = |c: &str| ColorRef::Palette { palette: pal.clone(), color: Id::from(c) };

    // BG: degradê vertical céu em tiles
    let (_bg, bg_cel) = layer(&mut s, &cut, "BG", LayerType::Raster, json!({"role": "background"}));
    let mut sky = Image::new(512, 288);
    for y in 0..288 {
        let t = y as f32 / 287.0;
        let c = [0.45 + 0.4 * t, 0.7 + 0.2 * t, 0.95, 1.0];
        for x in 0..512 {
            let i = sky.idx(x, y);
            sky.data[i] = c;
        }
    }
    let mut edits = Vec::new();
    for (tx, ty, px) in tile::image_to_tiles(&sky, [0, 0]) {
        if let Some(px) = px {
            let h = s.blobs.put(tile::encode_tile(&px));
            edits.push(TileEdit { x: tx, y: ty, blob: Some(h) });
        }
    }
    s.apply(Command::SetTiles { cel: bg_cel, tiles: edits }).unwrap();

    // Personagem (pasta isolada, opacidade 0,9)
    let (chara, _) = layer(&mut s, &cut, "Personagem", LayerType::Group, json!({"blend": "normal", "opacity": 0.9}));
    // preenchimento de pele: círculo como polígono → máscara
    let (_fill, fill_cel) = layer(&mut s, &chara, "A-cor", LayerType::Fill, json!({"role": "shiage"}));
    let circle: Vec<[f32; 2]> = (0..64).map(|i| {
        let a = i as f32 / 64.0 * std::f32::consts::TAU;
        [256.0 + 70.0 * a.cos(), 150.0 + 80.0 * a.sin()]
    }).collect();
    let cov = poly::fill_polygons(&[circle], true, 512, 288).unwrap();
    let mask = Mask::from_coverage(cov.width, cov.height, &cov.cov);
    let mh = s.blobs.put(mask.encode());
    s.apply(Command::ReplaceFills {
        cel: fill_cel,
        fills: vec![FillRegion { id: Id::from(""), seed: [256.0, 150.0], color: pref("pele"), mask: Some(mh), bounds: [cov.x0, cov.y0, cov.width as i32, cov.height as i32] }],
    })
    .unwrap();
    // sombra multiplicada recortada pela cor
    let (_sh, sh_cel) = layer(&mut s, &chara, "A-sombra", LayerType::Raster, json!({"blend": "multiply", "clip_to_below": true, "role": "shadow"}));
    let mut shadow = Image::new(512, 288);
    for y in 0..288 {
        for x in 300..512 {
            let i = shadow.idx(x, y);
            shadow.data[i] = [0.85, 0.66, 0.6, 1.0];
        }
    }
    let mut edits = Vec::new();
    for (tx, ty, px) in tile::image_to_tiles(&shadow, [0, 0]) {
        if let Some(px) = px {
            edits.push(TileEdit { x: tx, y: ty, blob: Some(s.blobs.put(tile::encode_tile(&px))) });
        }
    }
    s.apply(Command::SetTiles { cel: sh_cel, tiles: edits }).unwrap();
    // linha G-pen
    let (_ln, ln_cel) = layer(&mut s, &chara, "A-linha", LayerType::Vector, json!({"role": "douga"}));
    let outline: Vec<(f32, f32, f32)> = (0..=16).map(|i| {
        let a = i as f32 / 16.0 * std::f32::consts::TAU;
        (256.0 + 70.0 * a.cos(), 150.0 + 80.0 * a.sin(), 0.6 + 0.4 * (a * 2.0).sin().abs())
    }).collect();
    let mut st = gpen(&outline, 5.0, pref("linha"));
    st.closed = true;
    let eye = gpen(&[(225.0, 140.0, 0.3), (232.0, 128.0, 1.0), (240.0, 140.0, 0.3)], 6.0, pref("linha"));
    s.apply(Command::InsertStrokes { cel: ln_cel, items: vec![IndexedStroke { index: 0, stroke: st }, IndexedStroke { index: 1, stroke: eye }] }).unwrap();

    // Faixas com modos de mistura sobre tudo (tela, sobrepor, cor, diferença) — uma faixa por modo
    for (i, mode) in ["screen", "overlay", "color", "difference"].iter().enumerate() {
        let (_l, c) = layer(&mut s, &cut, mode, LayerType::Vector, json!({"blend": mode}));
        let y = 30.0 + i as f32 * 65.0;
        let mut m = gpen(&[(20.0, y, 1.0), (140.0, y + 10.0, 1.0)], 22.0, ColorRef::Direct { rgba: Rgba::from_hex("#e0603080").unwrap() });
        m.tip = VectorTip::Marker;
        s.apply(Command::InsertStrokes { cel: c, items: vec![IndexedStroke { index: 0, stroke: m }] }).unwrap();
    }
    // Camada com máscara (degradê horizontal de luminância) — vinheta quente
    let (warm, warm_cel) = layer(&mut s, &cut, "Luz", LayerType::Raster, json!({"blend": "add", "opacity": 0.5}));
    let (mask_l, mask_cel) = layer(&mut s, &cut, "Luz-mascara", LayerType::Raster, json!({"visible": false}));
    let mut warm_img = Image::new(512, 288);
    let mut mask_img = Image::new(512, 288);
    for y in 0..288 {
        for x in 0..512 {
            let i = warm_img.idx(x, y);
            warm_img.data[i] = [0.4, 0.25, 0.05, 1.0];
            let v = x as f32 / 511.0;
            mask_img.data[i] = [v, v, v, 1.0];
        }
    }
    for (cel, img) in [(warm_cel, &warm_img), (mask_cel, &mask_img)] {
        let mut edits = Vec::new();
        for (tx, ty, px) in tile::image_to_tiles(img, [0, 0]) {
            if let Some(px) = px {
                edits.push(TileEdit { x: tx, y: ty, blob: Some(s.blobs.put(tile::encode_tile(&px))) });
            }
        }
        s.apply(Command::SetTiles { cel, tiles: edits }).unwrap();
    }
    s.apply(Command::SetProps { id: warm, name: None, data: json!({"mask": mask_l.0}) }).unwrap();
    (s, cut)
}

#[test]
fn scene_renders_deterministically_and_matches_golden() {
    let (s, cut) = build_scene();
    let opts = RenderOptions::default();
    let a = render_frame(&s.project, &s.blobs, &cut, 0, &opts).unwrap();
    assert!(a.warnings.is_empty(), "avisos: {:?}", a.warnings);
    let b = render_frame(&s.project, &s.blobs, &cut, 5, &opts).unwrap();
    assert_eq!(a.image.content_hash(), b.image.content_hash(), "mesmo conteúdo em quadros sem mudança");
    check_golden("cena_basica", &a.image);

    // Conferências pontuais
    let px = |x: u32, y: u32| {
        let p = a.image.get(x, y);
        [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
    };
    let skin = px(240, 170); // pele normal à esquerda da sombra
    let shade = px(310, 170); // pele com sombra multiplicada
    assert!(shade[0] < skin[0] && shade[1] < skin[1], "sombra multiplicada escurece: {skin:?} {shade:?}");
    let outside = px(500, 20); // fora do personagem: a sombra recortada não aparece
    assert!(outside[2] > 0.8, "recorte impede a sombra fora da pele: {outside:?}");
}

#[test]
fn palette_change_recolors_and_hidden_layer_disappears() {
    let (mut s, cut) = build_scene();
    let before = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let pal = s.project.roots.palettes[0].clone();
    let mut data = match &s.project.node(&pal).unwrap().data { NodeKind::Palette(p) => p.clone(), _ => unreachable!() };
    data.colors[0].rgba = Rgba::from_hex("#80c080").unwrap();
    s.apply(Command::ReplaceData { id: pal, data: NodeKind::Palette(data) }).unwrap();
    let after = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let p = after.get(240, 170);
    assert!(p[1] / p[3] > p[0] / p[3], "pele virou verde pela paleta: {p:?}");
    assert_ne!(before.content_hash(), after.content_hash());
    check_golden("paleta_trocada", &after);
}

#[test]
fn xsheet_exposure_switches_cels() {
    let mut s = CoreSession::for_tests("x");
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(12)));
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 64, "height": 64}}) }).unwrap();
    let (l, c1) = layer(&mut s, &cut, "A", LayerType::Vector, serde_json::Value::Null);
    let c2 = create(&mut s, Some(&l), "A2", NodeKind::Cel(CelData::new(2)));
    let red = ColorRef::Direct { rgba: Rgba::from_hex("#ff0000").unwrap() };
    let mut a = gpen(&[(10.0, 10.0, 1.0), (20.0, 10.0, 1.0)], 6.0, red.clone());
    a.tip = VectorTip::Marker;
    let mut b = gpen(&[(40.0, 50.0, 1.0), (50.0, 50.0, 1.0)], 6.0, red);
    b.tip = VectorTip::Marker;
    s.apply(Command::InsertStrokes { cel: c1, items: vec![IndexedStroke { index: 0, stroke: a }] }).unwrap();
    s.apply(Command::InsertStrokes { cel: c2, items: vec![IndexedStroke { index: 0, stroke: b }] }).unwrap();
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from(""), "A", TrackKind::Drawing, Some(l.clone())) }).unwrap();
    let tid = match &s.project.node(&cut).unwrap().data { NodeKind::Cut(c) => c.xsheet.tracks[0].id.clone(), _ => unreachable!() };
    let cells = exposure_entries(&[1, 2], 2, 0).into_iter().map(|(f, c)| aiai_core::command::CellEdit { frame: f, cell: Some(c) }).collect();
    s.apply(Command::SetCells { cut: cut.clone(), track: tid, cells }).unwrap();
    let f0 = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let f2 = render_frame(&s.project, &s.blobs, &cut, 2, &RenderOptions::default()).unwrap().image;
    let f9 = render_frame(&s.project, &s.blobs, &cut, 9, &RenderOptions::default()).unwrap().image;
    assert!(f0.get(15, 10)[3] > 0.9 && f0.get(45, 50)[3] == 0.0, "quadro 0 = A1");
    assert!(f2.get(15, 10)[3] == 0.0 && f2.get(45, 50)[3] > 0.9, "quadro 2 = A2");
    assert_eq!(f2.content_hash(), f9.content_hash(), "A2 segurado até o fim");
}

#[test]
fn camera_pan_with_multiplane_parallax_and_layer_keys() {
    let mut s = CoreSession::for_tests("cam");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 200, "height": 100}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let mut cd = CutData::new(25);
    cd.canvas = Some([400, 100]); // cenário mais largo que o quadro
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(cd));
    // câmera: pan de x=100 a x=300 em 24 quadros
    s.apply(Command::SetProps { id: cut.clone(), name: None, data: json!({"camera": {"keys": {"center_x": [{"frame": 0, "value": 100.0}, {"frame": 24, "value": 300.0}]}}}) }).unwrap();
    let mark = |s: &mut CoreSession, name: &str, x: f32, depth: f32| {
        let (l, c) = layer(s, &cut, name, LayerType::Vector, json!({"depth": depth}));
        let mut m = gpen(&[(x, 40.0, 1.0), (x, 60.0, 1.0)], 4.0, ColorRef::Direct { rgba: Rgba::BLACK });
        m.tip = VectorTip::Marker;
        s.apply(Command::InsertStrokes { cel: c, items: vec![IndexedStroke { index: 0, stroke: m }] }).unwrap();
        l
    };
    mark(&mut s, "BG", 150.0, 2.0); // plano distante
    let near = mark(&mut s, "Personagem", 150.0, 0.0);
    let col_of = |img: &Image, y: u32| -> Vec<u32> { (0..img.width).filter(|&x| img.get(x, y)[3] > 0.5).collect() };
    let f0 = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    assert_eq!((f0.width, f0.height), (200, 100), "saída no tamanho do projeto");
    let f24 = render_frame(&s.project, &s.blobs, &cut, 24, &RenderOptions::default()).unwrap().image;
    // quadro 0: câmera em 100 → x=150 do canvas aparece em 150 (os dois juntos)
    let c0 = col_of(&f0, 50);
    assert!(c0.iter().all(|&x| (147..=153).contains(&x)), "{c0:?}");
    // quadro 24: plano 0 deslocou 200 px (sai do quadro); plano de profundidade 2 deslocou 200/3 ≈ 67 px
    let c24 = col_of(&f24, 50);
    assert!(!c24.is_empty() && c24.iter().all(|&x| (80..=86).contains(&x)), "só o fundo aparece, em ~83: {c24:?}");
    // chaves de camada: opacidade 0 no quadro 0 some com o personagem
    s.apply(Command::SetProps { id: near, name: None, data: json!({"keys": {"opacity": [{"frame": 0, "value": 0.0}, {"frame": 10, "value": 1.0}]}}) }).unwrap();
    let f0b = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let max_a = (140..160).map(|x| f0b.get(x, 50)[3]).fold(0.0f32, f32::max);
    assert!(max_a > 0.5, "o fundo continua lá");
    // sem câmera: canvas inteiro 400 px
    let raw = render_frame(&s.project, &s.blobs, &cut, 24, &RenderOptions { camera: false, ..Default::default() }).unwrap().image;
    assert_eq!(raw.width, 400);
}

#[test]
fn vector_lines_stay_sharp_under_camera_zoom() {
    // linha vertical de 2 px com câmera em zoom 4×: a borda tem de continuar com ~1 px de transição (vetor
    // rasterizado no lugar), não ~4 px (bitmap do canvas ampliado)
    let mut s = CoreSession::for_tests("zoom");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 200, "height": 100}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
    s.apply(Command::SetProps { id: cut.clone(), name: None, data: json!({"camera": {"zoom": 4.0}}) }).unwrap();
    let (_l, c) = layer(&mut s, &cut, "L", LayerType::Vector, json!({}));
    let mut m = gpen(&[(100.3, 20.0, 1.0), (100.3, 80.0, 1.0)], 2.0, ColorRef::Direct { rgba: Rgba::BLACK });
    m.tip = VectorTip::Marker;
    s.apply(Command::InsertStrokes { cel: c, items: vec![IndexedStroke { index: 0, stroke: m }] }).unwrap();
    let img = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap().image;
    let row: Vec<f32> = (0..img.width).map(|x| img.get(x, 50)[3]).collect();
    let partial = row.iter().filter(|&&a| a > 0.05 && a < 0.95).count();
    let solid = row.iter().filter(|&&a| a >= 0.95).count();
    assert!(solid >= 6, "a linha ampliada 4× tem ~8 px cheios: {solid}");
    assert!(partial <= 2, "borda nítida (no máximo 1 px de transição de cada lado): {partial} px parciais em {row:?}");
}

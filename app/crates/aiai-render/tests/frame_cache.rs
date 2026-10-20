//! Cache do canvas (`render_frame_cached`): foto do composto de baixo + imagem guardada por camada + tiles
//! decodificados. Em toda edição o resultado tem de ser o do render completo, pixel a pixel, com recorte, máscara,
//! modos de mistura, camada de efeito, light wrap, pasta isolada, camada vazia e ajuste por cima.

use aiai_core::command::{IndexedStroke, TileEdit};
use aiai_core::*;
use aiai_paint::{Image, tile};
use aiai_render::{FrameCache, RenderOptions, render_frame, render_frame_cached};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn layer(s: &mut CoreSession, parent: &Id, name: &str, kind: LayerType, patch: serde_json::Value) -> (Id, Id) {
    let l = create(s, Some(parent), name, NodeKind::Layer(LayerData::new(kind)));
    if !patch.is_null() {
        s.apply(Command::SetProps { id: l.clone(), name: None, data: patch }).unwrap();
    }
    if matches!(kind, LayerType::Group | LayerType::Effect | LayerType::Adjustment) {
        return (l, Id::from(""));
    }
    let c = create(s, Some(&l), &format!("{name}1"), NodeKind::Cel(CelData::new(1)));
    (l, c)
}

fn gpen(points: &[(f32, f32)], width: f32, hex: &str) -> Stroke {
    Stroke {
        id: Id::from(""),
        points: points.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::GPen,
        width,
        profile: WidthProfile::default(),
        color: ColorRef::Direct { rgba: Rgba::from_hex(hex).unwrap() },
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
    }
}

fn stroke(s: &mut CoreSession, cel: &Id, st: Stroke) {
    s.apply(Command::InsertStrokes { cel: cel.clone(), items: vec![IndexedStroke { index: 0, stroke: st }] }).unwrap();
}

/// Pinta um retângulo (x0..x1, y0..y1) em tiles no cel, por cima do que já tem.
fn paint_rect(s: &mut CoreSession, cel: &Id, canvas: (u32, u32), rect: (u32, u32, u32, u32), rgba: [f32; 4]) {
    let NodeKind::Cel(c) = &s.project.node(cel).unwrap().data else { panic!() };
    let mut img = Image::new(canvas.0, canvas.1);
    for t in &c.tiles {
        let px = tile::decode_tile(&s.blobs.get(&t.blob).unwrap()).unwrap();
        tile::blit_tile(&mut img, &px, t.x, t.y, [0, 0]);
    }
    for y in rect.1..rect.3 {
        for x in rect.0..rect.2 {
            let i = img.idx(x, y);
            img.data[i] = rgba;
        }
    }
    let mut edits = Vec::new();
    for (tx, ty, px) in tile::image_to_tiles(&img, [0, 0]) {
        edits.push(TileEdit { x: tx, y: ty, blob: px.map(|px| s.blobs.put(tile::encode_tile(&px))) });
    }
    s.apply(Command::SetTiles { cel: cel.clone(), tiles: edits }).unwrap();
}

#[test]
fn cached_canvas_render_matches_the_full_render_through_edits_on_every_kind_of_layer() {
    let (w, h) = (300u32, 200u32); // 2×1 tiles, o segundo cortado pela borda do canvas
    let mut s = CoreSession::for_tests("cache");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": w, "height": h}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));

    let (_bg, bg_cel) = layer(&mut s, &cut, "bg", LayerType::Raster, json!(null));
    paint_rect(&mut s, &bg_cel, (w, h), (0, 0, w, h), [0.2, 0.3, 0.5, 1.0]);
    paint_rect(&mut s, &bg_cel, (w, h), (40, 120, 260, 190), [0.1, 0.5, 0.2, 1.0]);
    let (_blur, _) = layer(&mut s, &cut, "blur", LayerType::Effect, json!({"params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": 2.0}}]}}));
    // camada que o artista pinta (só tiles: atalho do cel de tiles)
    let (_paint, paint_cel) = layer(&mut s, &cut, "pintura", LayerType::Raster, json!(null));
    paint_rect(&mut s, &paint_cel, (w, h), (60, 40, 140, 100), [0.8, 0.2, 0.2, 1.0]);
    // linha recortada pela pintura
    let (_clip, clip_cel) = layer(&mut s, &cut, "recorte", LayerType::Vector, json!({"clip_to_below": true}));
    stroke(&mut s, &clip_cel, gpen(&[(20.0, 70.0), (280.0, 70.0)], 14.0, "#ffffff"));
    // multiplicar com máscara e opacidade
    let (mask_l, mask_cel) = layer(&mut s, &cut, "mascara", LayerType::Raster, json!({"visible": false}));
    paint_rect(&mut s, &mask_cel, (w, h), (150, 0, 300, 200), [1.0, 1.0, 1.0, 1.0]);
    let (_mul, mul_cel) = layer(&mut s, &cut, "sombra", LayerType::Vector, json!({"blend": "multiply", "opacity": 0.6, "mask": mask_l.0}));
    stroke(&mut s, &mul_cel, gpen(&[(100.0, 20.0), (220.0, 180.0)], 30.0, "#4040a0"));
    // linha com contorno (efeito na própria camada) e outra com light wrap (lê o composto de baixo)
    let (_ol, ol_cel) = layer(&mut s, &cut, "contorno", LayerType::Vector, json!({"params": {"effects": [{"effect": {"type": "outline", "width": 3.0, "color": [0.0, 0.0, 0.0, 1.0]}}]}}));
    stroke(&mut s, &ol_cel, gpen(&[(30.0, 150.0), (120.0, 160.0)], 8.0, "#f0e020"));
    let (_lw, lw_cel) = layer(&mut s, &cut, "luz", LayerType::Vector, json!({"params": {"effects": [{"effect": {"type": "light_wrap", "radius": 6.0, "amount": 0.8}}]}}));
    stroke(&mut s, &lw_cel, gpen(&[(180.0, 30.0), (190.0, 170.0)], 20.0, "#202020"));
    // pasta isolada
    let (grp, _) = layer(&mut s, &cut, "pasta", LayerType::Group, json!({"blend": "screen", "opacity": 0.8}));
    let (_gc, gc_cel) = layer(&mut s, &grp, "dentro", LayerType::Vector, json!(null));
    stroke(&mut s, &gc_cel, gpen(&[(10.0, 10.0), (290.0, 190.0)], 6.0, "#30c0c0"));
    // camada vazia como base e uma linha recortada por ela (some inteira)
    let (_empty, _) = layer(&mut s, &cut, "vazia", LayerType::Raster, json!(null));
    let (hid, hid_cel) = layer(&mut s, &cut, "escondida", LayerType::Vector, json!({"clip_to_below": true}));
    stroke(&mut s, &hid_cel, gpen(&[(10.0, 100.0), (290.0, 100.0)], 10.0, "#ff00ff"));
    // linha comum e ajuste recortado por ela por cima de tudo
    let (_top, top_cel) = layer(&mut s, &cut, "topo", LayerType::Vector, json!(null));
    stroke(&mut s, &top_cel, gpen(&[(200.0, 20.0), (280.0, 60.0)], 12.0, "#e0e0e0"));
    let (_adj, _) = layer(&mut s, &cut, "ajuste", LayerType::Adjustment, json!({"clip_to_below": true, "params": {"adjustment": {"type": "hue_saturation", "hue": 90.0, "saturation": 0.0, "lightness": 0.0}}}));

    let opts = RenderOptions { camera: false, include_guides: true, include_reference: true, ..Default::default() };
    let mut cache = FrameCache::default();
    let check = |s: &CoreSession, cache: &mut FrameCache, frame: u32, what: &str| {
        let a = render_frame_cached(&s.project, &s.blobs, &cut, frame, &opts, cache).unwrap();
        let b = render_frame(&s.project, &s.blobs, &cut, frame, &opts).unwrap();
        assert_eq!(a.image.content_hash(), b.image.content_hash(), "cache = render completo ({what})");
        assert_eq!(a.warnings, b.warnings, "mesmos avisos ({what})");
    };
    check(&s, &mut cache, 0, "primeiro render");
    check(&s, &mut cache, 0, "sem mudança");
    // pinceladas na camada de pintura: parte da foto e mistura as de cima guardadas
    for k in 0..3u32 {
        paint_rect(&mut s, &paint_cel, (w, h), (150 + 30 * k, 50, 170 + 30 * k, 90), [0.9, 0.6, 0.1, 1.0]);
        check(&s, &mut cache, 0, &format!("pincelada {k}"));
        // a primeira tira a foto antes da pintura; as seguintes partem dela
        assert_eq!(cache.last_skipped, if k == 0 { 0 } else { 2 });
        // de cima, só o light wrap e o ajuste (leem o que está embaixo) são refeitos
        assert_eq!(cache.last_reused, if k == 0 { 8 } else { 7 }, "camadas misturadas da imagem guardada");
    }
    // pincelada que sai pela borda do canvas (tile cortado)
    paint_rect(&mut s, &paint_cel, (w, h), (280, 150, 300, 200), [0.1, 0.1, 0.9, 1.0]);
    check(&s, &mut cache, 0, "tile na borda");
    // mexer numa camada de cima, na máscara, na pasta e embaixo do efeito
    stroke(&mut s, &ol_cel, gpen(&[(200.0, 120.0), (260.0, 140.0)], 8.0, "#f0e020"));
    check(&s, &mut cache, 0, "contorno");
    paint_rect(&mut s, &mask_cel, (w, h), (0, 0, 60, 60), [0.5, 0.5, 0.5, 1.0]);
    check(&s, &mut cache, 0, "máscara");
    stroke(&mut s, &gc_cel, gpen(&[(290.0, 10.0), (10.0, 190.0)], 6.0, "#c030c0"));
    check(&s, &mut cache, 0, "dentro da pasta");
    paint_rect(&mut s, &bg_cel, (w, h), (0, 0, 50, 50), [1.0, 1.0, 1.0, 1.0]);
    check(&s, &mut cache, 0, "embaixo do efeito");
    // esconder, desfazer e outro quadro
    s.apply(Command::SetProps { id: _mul, name: None, data: json!({"visible": false}) }).unwrap();
    check(&s, &mut cache, 0, "camada escondida");
    s.undo().unwrap();
    check(&s, &mut cache, 0, "desfazer");
    s.undo().unwrap();
    check(&s, &mut cache, 0, "desfazer 2");
    check(&s, &mut cache, 1, "outro quadro");
    check(&s, &mut cache, 0, "volta ao quadro");
    // a linha recortada pela camada vazia não aparece: escondê-la não muda o quadro
    let with = render_frame_cached(&s.project, &s.blobs, &cut, 0, &opts, &mut cache).unwrap().image.content_hash();
    s.apply(Command::SetProps { id: hid, name: None, data: json!({"visible": false}) }).unwrap();
    let without = render_frame_cached(&s.project, &s.blobs, &cut, 0, &opts, &mut cache).unwrap().image.content_hash();
    assert_eq!(with, without, "recortada por camada vazia = invisível");
}

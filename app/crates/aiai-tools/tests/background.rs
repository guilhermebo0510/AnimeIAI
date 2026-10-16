//! Cenário externo pintado só com ferramentas: céu procedural, prédio em perspectiva com textura de tijolo mapeada a
//! partir de uma câmera 3D, telhado, janelas, chão de grama em perspectiva, árvores e canteiro com pincéis de cenário.

use aiai_core::*;
use aiai_guide3d::{Camera3d, vanishing_points};
use aiai_paint::brush::BrushPreset;
use aiai_paint::select::SelectShape;
use aiai_paint::texture::{SkySpec, TextureParams, TextureSpec};
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::background::{PatternFillRequest, SkyRequest, TextureMapping, pattern_fill_command, sky_command};
use aiai_tools::paint::{PaintRequest, PaintStrokeSpec, paint_command};
use glam::Vec3;
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn raster(s: &mut CoreSession, cut: &Id, name: &str) -> Id {
    let l = create(s, Some(cut), name, NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
    l
}

fn rgb(hex: &str) -> [f32; 3] {
    let r = Rgba::from_hex(hex).unwrap().0;
    [r[0], r[1], r[2]]
}

fn tex(spec: TextureSpec, dark: &str, light: &str, accent: Option<&str>, seed: u32) -> TextureParams {
    TextureParams { spec, dark: rgb(dark), light: rgb(light), accent: accent.map(rgb), seed, contrast: 1.0, opacity: 1.0 }
}

fn apply(s: &mut CoreSession, c: Command) {
    s.apply(c).unwrap();
}

#[test]
fn exterior_background_painted_with_tools() {
    let (w, h) = (960u32, 540u32);
    let mut s = CoreSession::for_tests("bg");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": w, "height": h}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let sky_l = raster(&mut s, &cut, "BG-ceu");
    let ground_l = raster(&mut s, &cut, "BG-chao");
    let bld_l = raster(&mut s, &cut, "BG-predio");
    let trees_l = raster(&mut s, &cut, "BG-arvores");
    let book_l = raster(&mut s, &cut, "BOOK-canteiro");

    let cam = Camera3d { position: [9.0, 1.6, 14.0], target: [0.0, 2.5, 0.0], focal_mm: 30.0, roll: 0.0, ortho_height: None };
    let proj = cam.projector(w, h);
    let pr = |p: [f32; 3]| {
        let (x, y, _) = proj.project(Vec3::from(p)).expect("na frente da câmera");
        [x, y]
    };
    let horizon = vanishing_points(&cam, w, h).horizon_y.unwrap();

    // céu de fim de tarde com nuvens
    let sky = SkyRequest { cut: cut.clone(), frame: 0, layer: sky_l.clone(), sky: SkySpec { time_of_day: "sunset".into(), zenith: None, horizon: None, horizon_y: horizon, clouds: 0.45, cloud_scale: 200.0, seed: 4 } };
    let c = sky_command(&s.project, &mut s.blobs, &sky).unwrap();
    apply(&mut s, c);

    // chão de grama em perspectiva (plano y=0)
    let gq = [pr([-25.0, 0.0, -40.0]), pr([15.0, 0.0, -40.0]), pr([15.0, 0.0, 6.0]), pr([-25.0, 0.0, 6.0])];
    let ground = PatternFillRequest {
        cut: cut.clone(), frame: 0, layer: ground_l.clone(),
        texture: tex(TextureSpec::Grass { scale: 6.0, density: 0.9 }, "#3f6b2a", "#9cc65a", None, 2),
        mapping: TextureMapping::Perspective { quad: gq, size: [4000.0, 4600.0] },
        region: Some(SelectShape::Rect { rect: [0.0, horizon, w as f32, h as f32 - horizon] }),
        blend: BlendMode::Normal, opacity: 1.0,
    };
    let c = pattern_fill_command(&s.project, &mut s.blobs, &ground).unwrap();
    apply(&mut s, c);

    // prédio 10 × 5 × 6 m: frente e lateral com tijolo (lateral mais escura)
    let (x0, x1, z0, z1, top) = (-5.0f32, 5.0f32, -3.0f32, 3.0f32, 5.0f32);
    let front = [pr([x0, top, z1]), pr([x1, top, z1]), pr([x1, 0.0, z1]), pr([x0, 0.0, z1])];
    let side = [pr([x1, top, z1]), pr([x1, top, z0]), pr([x1, 0.0, z0]), pr([x1, 0.0, z1])];
    let brick = tex(TextureSpec::Brick { brick: [36.0, 14.0], mortar: 2.5, variation: 0.25 }, "#9a5a44", "#c98a6a", Some("#e8dccb"), 7);
    for (quad, size, shade) in [(front, [1000.0f32, 500.0], 1.0f32), (side, [600.0, 500.0], 0.78)] {
        let mut t = brick.clone();
        for k in 0..3 {
            t.dark[k] *= shade;
            t.light[k] *= shade;
        }
        let r = PatternFillRequest { cut: cut.clone(), frame: 0, layer: bld_l.clone(), texture: t, mapping: TextureMapping::Perspective { quad, size }, region: None, blend: BlendMode::Normal, opacity: 1.0 };
        let c = pattern_fill_command(&s.project, &mut s.blobs, &r).unwrap();
        apply(&mut s, c);
    }
    // telhado inclinado com telhas
    let roof = [pr([x0 - 0.4, top + 1.6, z1 - 1.5]), pr([x1 + 0.4, top + 1.6, z1 - 1.5]), pr([x1 + 0.4, top, z1 + 0.4]), pr([x0 - 0.4, top, z1 + 0.4])];
    let r = PatternFillRequest {
        cut: cut.clone(), frame: 0, layer: bld_l.clone(),
        texture: tex(TextureSpec::RoofTiles { tile: [26.0, 18.0], variation: 0.25 }, "#3a4a6a", "#6e86b2", Some("#26324a"), 9),
        mapping: TextureMapping::Perspective { quad: roof, size: [1080.0, 260.0] }, region: None, blend: BlendMode::Normal, opacity: 1.0,
    };
    let c = pattern_fill_command(&s.project, &mut s.blobs, &r).unwrap();
    apply(&mut s, c);
    // janelas 4 × 2 na frente
    for row in 0..2 {
        for col in 0..4u32 {
            let (wx, wy) = (x0 + 1.0 + col as f32 * 2.3, 1.0 + row as f32 * 2.0);
            let q = [pr([wx, wy + 1.3, z1 + 0.01]), pr([wx + 1.1, wy + 1.3, z1 + 0.01]), pr([wx + 1.1, wy, z1 + 0.01]), pr([wx, wy, z1 + 0.01])];
            let r = PatternFillRequest {
                cut: cut.clone(), frame: 0, layer: bld_l.clone(),
                texture: tex(TextureSpec::Noise { scale: 30.0, octaves: 3 }, "#27365a", "#7fa6d6", None, 11 + col),
                mapping: TextureMapping::Perspective { quad: q, size: [110.0, 130.0] }, region: None, blend: BlendMode::Normal, opacity: 1.0,
            };
            let c = pattern_fill_command(&s.project, &mut s.blobs, &r).unwrap();
            apply(&mut s, c);
        }
    }
    // árvore: tronco + copa com pincel de folhas em três tons
    let presets = BrushPreset::defaults();
    let get = |n: &str| presets.iter().find(|p| p.name == n).unwrap().clone();
    let pts = |v: &[[f32; 3]]| PaintStrokeSpec { points: v.iter().map(|p| p.to_vec()).collect(), smooth: true };
    let (tb, tt) = (pr([-8.0, 0.0, 4.0]), pr([-8.0, 3.0, 4.0]));
    let mut hard = get("Redondo duro");
    hard.size = 12.0;
    let r = PaintRequest { cut: cut.clone(), frame: 0, layer: trees_l.clone(), preset: hard, color: ColorRef::Direct { rgba: Rgba::from_hex("#5a3f2c").unwrap() }, strokes: vec![pts(&[[tb[0], tb[1], 1.0], [tt[0], tt[1], 0.6]])], seed: 1 };
    let c = paint_command(&s.project, &mut s.blobs, &r).unwrap();
    apply(&mut s, c);
    let crown = pr([-8.0, 4.2, 4.0]);
    let mut leaves = get("Folhagem");
    leaves.size = 28.0;
    for (k, (col, dy)) in [("#2f5a2a", 12.0f32), ("#4f8a3a", 0.0), ("#86b85a", -14.0)].iter().enumerate() {
        let (cx, cy) = (crown[0], crown[1] + dy);
        let ring: Vec<[f32; 3]> = (0..14).map(|i| {
            let a = i as f32 / 13.0 * std::f32::consts::TAU;
            [cx + 55.0 * a.cos(), cy + 45.0 * a.sin(), 1.0]
        }).collect();
        let r = PaintRequest {
            cut: cut.clone(), frame: 0, layer: trees_l.clone(), preset: leaves.clone(), color: ColorRef::Direct { rgba: Rgba::from_hex(col).unwrap() },
            strokes: vec![pts(&ring), pts(&[[cx - 30.0, cy, 1.0], [cx + 30.0, cy + 5.0, 1.0]])], seed: 10 + k as u64,
        };
        let c = paint_command(&s.project, &mut s.blobs, &r).unwrap();
        apply(&mut s, c);
    }
    // canteiro de rosas em primeiro plano (BOOK)
    let (bl, br) = (pr([-2.0, 0.0, 9.0]), pr([6.0, 0.0, 9.0]));
    let mut bush = get("Folhagem");
    bush.size = 22.0;
    let r = PaintRequest {
        cut: cut.clone(), frame: 0, layer: book_l.clone(), preset: bush, color: ColorRef::Direct { rgba: Rgba::from_hex("#2e5e2c").unwrap() },
        strokes: vec![pts(&[[bl[0], bl[1], 1.0], [(bl[0] + br[0]) / 2.0, bl[1] - 10.0, 1.0], [br[0], br[1], 1.0]])], seed: 21,
    };
    let c = paint_command(&s.project, &mut s.blobs, &r).unwrap();
    apply(&mut s, c);
    let mut roses = get("Flores/pétalas");
    roses.size = 16.0;
    let r = PaintRequest {
        cut: cut.clone(), frame: 0, layer: book_l.clone(), preset: roses, color: ColorRef::Direct { rgba: Rgba::from_hex("#e0405a").unwrap() },
        strokes: vec![pts(&[[bl[0], bl[1] - 8.0, 1.0], [br[0], br[1] - 8.0, 1.0]])], seed: 22,
    };
    let c = paint_command(&s.project, &mut s.blobs, &r).unwrap();
    apply(&mut s, c);

    let out = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        out.image.save_png(&std::path::Path::new(&dir).join("cenario_externo.png")).unwrap();
    }
    let top = out.image.get(20, 10);
    let bottom = out.image.get(20, h - 5);
    assert!(top[2] > 0.3 && bottom[1] > bottom[2], "céu {top:?} / grama {bottom:?}");
}

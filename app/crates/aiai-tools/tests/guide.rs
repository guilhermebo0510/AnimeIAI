//! Layout com guia 3D: sala em blocos + manequim sentado + grade, régua de perspectiva gerada pela câmera, e linha de
//! layout desenhada por cima seguindo a guia.

use aiai_core::*;
use aiai_guide3d::{Camera3d, Mannequin, Object3d, Scene3d, Shape};
use aiai_guide3d::scene::FloorGrid;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::guide::{Guide3dRequest, PerspectiveRequest, guide3d_command, perspective_command, perspective_info};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn obj(name: &str, shape: Shape, pos: [f32; 3], rot_y: f32) -> Object3d {
    Object3d { name: name.into(), shape, position: pos, rotation: [0.0, rot_y, 0.0], scale: [1.0; 3] }
}

#[test]
fn room_layout_with_mannequin_and_perspective() {
    let mut s = CoreSession::for_tests("layout");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 960, "height": 540}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
    let shaded = create(&mut s, Some(&cut), "guia-sombra", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    let g3d = create(&mut s, Some(&cut), "guia-3d", NodeKind::Layer(LayerData::new(LayerType::Guide)));
    let ruler = create(&mut s, Some(&cut), "regua", NodeKind::Layer(LayerData::new(LayerType::Guide)));
    for l in [&shaded, &g3d, &ruler] {
        create(&mut s, Some(l), "1", NodeKind::Cel(CelData::new(1)));
    }
    s.apply(Command::SetProps { id: shaded.clone(), name: None, data: json!({"opacity": 0.35, "role": "construction"}) }).unwrap();
    let scene = Scene3d {
        objects: vec![
            obj("piso", Shape::Plane { size: [6.0, 6.0] }, [0.0, 0.0, 0.0], 0.0),
            obj("parede-fundo", Shape::Box { size: [6.0, 2.8, 0.1] }, [0.0, 1.4, -3.0], 0.0),
            obj("parede-esq", Shape::Box { size: [0.1, 2.8, 6.0] }, [-3.0, 1.4, 0.0], 0.0),
            obj("janela", Shape::Box { size: [1.6, 1.2, 0.12] }, [0.6, 1.5, -2.95], 0.0),
            obj("mesa", Shape::Box { size: [1.2, 0.05, 0.8] }, [0.3, 0.74, -0.8], 0.0),
            obj("perna-mesa", Shape::Cylinder { radius: 0.05, height: 0.72 }, [0.3, 0.36, -0.8], 0.0),
            obj("cadeira", Shape::Box { size: [0.45, 0.45, 0.45] }, [-0.5, 0.225, -0.6], 15.0),
            obj("encosto", Shape::Box { size: [0.45, 0.5, 0.06] }, [-0.62, 0.7, -0.6], 105.0),
        ],
        mannequins: vec![Mannequin { position: [-0.55, 0.0, -0.6], facing: 90.0, pose: Some("sitting".into()), ..Default::default() }],
        floor_grid: Some(FloorGrid { size: 6.0, step: 0.5 }),
    };
    let camera = Camera3d { position: [2.2, 1.4, 2.8], target: [-0.2, 0.9, -0.8], focal_mm: 28.0, roll: 0.0, ortho_height: None };
    let req = Guide3dRequest { cut: cut.clone(), frame: 0, layer: g3d.clone(), scene, camera: camera.clone(), style: Default::default(), shaded_layer: Some(shaded.clone()), light_dir: [-0.3, -1.0, -0.6] };
    let c = guide3d_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(c).unwrap();
    let pr = PerspectiveRequest { cut: cut.clone(), frame: 0, layer: ruler.clone(), camera: Some(camera), points: vec![], horizon_y: None, lines_per_point: 16, color: "#f07a2f66".into() };
    let info = perspective_info(&s.project, &pr).unwrap();
    assert!(info.points.len() >= 2, "dois pontos de fuga (X e Z) numa vista em ângulo: {:?}", info.points);
    s.apply(perspective_command(&s.project, &pr).unwrap()).unwrap();
    // sem guias: nada aparece (guias não saem no render final)
    let final_img = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap();
    let opaque = final_img.image.data.iter().filter(|p| p[3] > 0.5).count();
    // só a camada de sombra (raster comum, 35%) aparece — ela é "construction"; as de guia ficam de fora
    assert!(opaque == 0, "guias fora do render final (sombra tem 35%): {opaque}");
    let with_guides = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions { include_guides: true, background: Some([1.0; 3]), ..Default::default() }).unwrap();
    assert!(with_guides.warnings.is_empty(), "{:?}", with_guides.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        with_guides.image.save_png(&std::path::Path::new(&dir).join("layout_guia3d.png")).unwrap();
    }
}

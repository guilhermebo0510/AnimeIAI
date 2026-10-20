//! Camada 3D no compositor (S24 3D): o modelo aparece no canvas pelo rasterizador CPU (sem GPU registrada), fica fora
//! do render final quando é referência, muda com a transformação e o cache de rasters acerta/erra pelo estado avaliado.

use aiai_3d_model::model::{Model, ModelInfo, Part};
use aiai_core::model3d::{Model3dParams, OrbitCam3d, Render3d, RenderMode3d, Transform3d};
use aiai_core::*;
use aiai_render::raster3d::{Layer3dJob, cache_key, cache_stats};
use aiai_render::{RenderOptions, render_frame};
use serde_json::json;

fn cube() -> Model {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
        ([0.0, 0.0, -1.0], [[0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5]]),
        ([0.0, 1.0, 0.0], [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]]),
        ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
        ([1.0, 0.0, 0.0], [[0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5]]),
        ([-1.0, 0.0, 0.0], [[-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5], [-0.5, 0.5, -0.5]]),
    ];
    for (n, c) in faces {
        let b = positions.len() as u32;
        positions.extend(c);
        normals.extend([n; 4]);
        indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    let mut m = Model { parts: vec![Part { name: "cubo".into(), positions, normals, indices, color: [0.7, 0.3, 0.2, 1.0], material: "m".into() }], info: ModelInfo::default(), skin: None };
    m.refresh_info();
    m
}

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

/// Projeto 64x36 com um corte, um ativo 3D (cubo) e uma camada 3D.
fn setup(reference: bool) -> (CoreSession, Id, Id) {
    let mut s = CoreSession::for_tests("m3d");
    s.project.settings.width = 64;
    s.project.settings.height = 36;
    let ep = create(&mut s, None, "ep", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "sc", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "c1", NodeKind::Cut(CutData::new(4)));
    let glb = aiai_3d_model::glb::write_glb(&cube());
    let hash = s.blobs.put(glb);
    let asset = create(&mut s, None, "cubo", NodeKind::Asset(AssetData { asset: AssetType::Model3d, blob: Some(hash.clone()), params: json!({}), source: "teste".into() }));
    let p = Model3dParams {
        version: 1,
        asset,
        model_hash: hash,
        transform: Transform3d::default(),
        camera: OrbitCam3d { target: [0.0; 3], yaw_deg: 30.0, pitch_deg: 20.0, distance: 3.0, fov_deg: 40.0 },
        render: Render3d { mode: RenderMode3d::Toon, reference, ..Render3d::default() },
        pose: None,
    };
    let mut ld = LayerData::new(LayerType::Model3d);
    ld.params = json!({ "model3d": p });
    let layer = create(&mut s, Some(&cut), "3D", NodeKind::Layer(ld));
    (s, cut, layer)
}

fn covered(img: &aiai_paint::Image) -> usize {
    img.data.iter().filter(|p| p[3] > 0.5).count()
}

#[test]
fn model3d_layer_renders_and_reference_stays_out_of_the_final_render() {
    let (s, cut, _) = setup(true);
    let canvas = RenderOptions { include_reference: true, camera: false, ..Default::default() };
    let out = render_frame(&s.project, &s.blobs, &cut, 0, &canvas).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(covered(&out.image) > 100, "o cubo aparece no canvas");
    // render final (sem referências): a camada 3D de referência não sai
    let fin = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions::default()).unwrap();
    assert_eq!(covered(&fin.image), 0);
    // desligada a referência, sai no render final também
    let (s2, cut2, _) = setup(false);
    let fin2 = render_frame(&s2.project, &s2.blobs, &cut2, 0, &RenderOptions::default()).unwrap();
    assert!(covered(&fin2.image) > 100);
}

#[test]
fn opacity_comes_from_the_layer_and_keys_move_the_model() {
    let (mut s, cut, layer) = setup(true);
    let canvas = RenderOptions { include_reference: true, camera: false, ..Default::default() };
    let full = render_frame(&s.project, &s.blobs, &cut, 0, &canvas).unwrap().image;
    s.apply(Command::SetProps { id: layer.clone(), name: None, data: json!({ "opacity": 0.3 }) }).unwrap();
    let faint = render_frame(&s.project, &s.blobs, &cut, 0, &canvas).unwrap().image;
    let (i, _) = full.data.iter().enumerate().max_by(|a, b| a.1[3].total_cmp(&b.1[3])).unwrap();
    assert!((faint.data[i][3] - full.data[i][3] * 0.3).abs() < 0.02, "{} vs {}", faint.data[i][3], full.data[i][3]);
    // chave de posição: no quadro 3 o cubo está fora do quadro
    s.apply(Command::SetProps { id: layer.clone(), name: None, data: json!({ "opacity": 1.0, "keys": { "m3d.pos.x": [{ "frame": 0, "value": 0.0 }, { "frame": 3, "value": 20.0 }] } }) }).unwrap();
    let f0 = render_frame(&s.project, &s.blobs, &cut, 0, &canvas).unwrap().image;
    let f3 = render_frame(&s.project, &s.blobs, &cut, 3, &canvas).unwrap().image;
    assert!(covered(&f0) > 100);
    assert_eq!(covered(&f3), 0);
    // chave m3d.* desconhecida é recusada pela validação do documento
    let bad = s.apply(Command::SetProps { id: layer, name: None, data: json!({ "keys": { "m3d.pos.w": [{ "frame": 0, "value": 1.0 }] } }) });
    assert!(bad.is_err());
}

#[test]
fn raster_cache_hits_on_same_state_and_misses_when_it_changes() {
    let (s, cut, layer) = setup(true);
    let canvas = RenderOptions { include_reference: true, camera: false, ..Default::default() };
    let p = match s.project.node(&layer).map(|n| &n.data) {
        Some(NodeKind::Layer(l)) => Model3dParams::from_layer_params(&l.params).unwrap().unwrap(),
        _ => unreachable!(),
    };
    let st = p.eval(&KeyTracks::default(), 0.0);
    let mut moved = st.clone();
    moved.transform.rot_deg[1] = 45.0;
    let job = |state| Layer3dJob { model_hash: &p.model_hash, state, blobs: &s.blobs, width: 64, height: 36 };
    let k0 = cache_key("cpu", &job(&st));
    let st2 = st.clone();
    assert_eq!(k0, cache_key("cpu", &job(&st2)), "chave determinística");
    assert_ne!(k0, cache_key("cpu", &job(&moved)), "outro estado, outra chave");
    assert_ne!(k0, cache_key("gpu", &job(&st)), "outro renderizador, outra chave");
    assert_ne!(k0, cache_key("cpu", &Layer3dJob { width: 32, ..job(&st) }), "outro tamanho, outra chave");
    // dois renders iguais: o segundo vem do cache (o contador global só cresce)
    let a = render_frame(&s.project, &s.blobs, &cut, 1, &canvas).unwrap().image;
    let (h0, _, _) = cache_stats();
    let b = render_frame(&s.project, &s.blobs, &cut, 1, &canvas).unwrap().image;
    let (h1, _, _) = cache_stats();
    assert!(h1 > h0, "acerto no cache");
    assert_eq!(a.data, b.data);
}

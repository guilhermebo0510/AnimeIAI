//! Spike da fase 0 (S24 3D): mede o renderer cel do ANIGO offscreen nesta máquina.
//!
//! Roda com `cargo test -p aiai-3d-render --test spike -- --nocapture --ignored`. Fica `#[ignore]` porque depende de GPU
//! e escreve arquivos; os números vão para `sprints/registro/S24-pesquisa/3d-spike.md`.

use std::time::Instant;

use aiai_3d_core::mesh::Mesh;
use aiai_3d_core::scene::{Scene, SceneNode, StylizedMaterial};
use aiai_3d_render::HeadlessRenderer;
use glam::{Quat, Vec3};

fn material(base: [f32; 4], outline: f32) -> StylizedMaterial {
    let mut m = StylizedMaterial::default();
    m.base_color = base;
    m.shade_color = [base[0] * 0.6, base[1] * 0.6, base[2] * 0.75, 1.0];
    m.outline_width = outline;
    m.outline_color = [0.05, 0.05, 0.08, 1.0];
    m
}

fn scene() -> Scene {
    let mut scene = Scene::new_empty();
    scene.background_color = [0.0, 0.0, 0.0, 0.0];
    let mut figure = SceneNode::new("figura", "Figura").with_mesh(Mesh::create_mannequin_proxy());
    figure.material = Some(material([0.85, 0.75, 0.68, 1.0], 0.004));
    scene.add_node(figure);
    let mut ball = SceneNode::new("bola", "Bola").with_mesh(Mesh::create_uv_sphere_at(0.35, 32, 48, [0.9, 0.35, 0.0]));
    ball.material = Some(material([0.35, 0.55, 0.9, 1.0], 0.006));
    scene.add_node(ball);
    let mut cube = SceneNode::new("caixa", "Caixa").with_mesh(Mesh::create_cube_at(0.6, [-0.9, 0.3, 0.2]));
    cube.material = Some(material([0.9, 0.4, 0.35, 1.0], 0.006));
    cube.transform.rotation = Quat::from_rotation_y(0.6);
    scene.add_node(cube);
    scene.camera.eye = Vec3::new(0.4, 1.3, 3.6);
    scene.camera.target = Vec3::new(0.0, 0.85, 0.0);
    scene.camera.aspect = 16.0 / 9.0;
    scene.rebuild_children();
    scene
}

fn allocator_mb(device: &wgpu::Device) -> Option<(f64, f64)> {
    device.generate_allocator_report().map(|r| {
        (r.total_allocated_bytes as f64 / 1048576.0, r.total_reserved_bytes as f64 / 1048576.0)
    })
}

#[test]
#[ignore]
fn spike_1080p() {
    let renderer = match pollster::block_on(HeadlessRenderer::new()) {
        Ok(r) => r,
        Err(e) => panic!("sem GPU para o spike: {e}"),
    };
    let scene = scene();
    let tris: usize = scene.nodes.iter().filter_map(|n| n.mesh.as_ref()).map(|m| m.indices.len() / 3).sum();
    println!("spike: triângulos = {tris}");
    println!("spike: alocador antes = {:?}", allocator_mb(&renderer.device));

    for (w, h) in [(1920u32, 1080u32), (960, 540), (3840, 2160)] {
        let t0 = Instant::now();
        let (img, m) = pollster::block_on(renderer.render_scene(&scene, w, h)).expect("render");
        let first = t0.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for _ in 0..10 {
            let t = Instant::now();
            let _ = pollster::block_on(renderer.render_scene(&scene, w, h)).expect("render");
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = times[times.len() / 2];
        println!(
            "spike: {w}x{h} primeiro = {first:.1} ms, mediana(10) = {median:.1} ms, min = {:.1}, max = {:.1}; adaptador = {} ({}); passes = {:?}",
            times[0],
            times[times.len() - 1],
            m.adapter_name,
            m.backend,
            m.executed_passes
        );
        println!("spike: alocador depois de {w}x{h} = {:?}", allocator_mb(&renderer.device));

        if w == 1920 {
            // Alfa: canto vazio deve ser 0; nenhum pixel pode ter cor acima do alfa (pré-multiplicado); a borda do contorno
            // precisa ter alfa parcial (MSAA resolvido sobre transparente).
            let px = img.as_raw();
            let mut transparent = 0usize;
            let mut partial = 0usize;
            let mut over = 0usize;
            let mut max_over = 0u8;
            for p in px.chunks_exact(4) {
                let a = p[3];
                if a == 0 {
                    transparent += 1;
                } else if a < 255 {
                    partial += 1;
                }
                let mx = p[0].max(p[1]).max(p[2]);
                if mx > a {
                    over += 1;
                    max_over = max_over.max(mx - a);
                }
            }
            let total = (w * h) as usize;
            println!(
                "spike: alfa — transparente {transparent}/{total}, parcial {partial}, cor>alfa {over} (máx excesso {max_over}); canto = {:?}",
                img.get_pixel(0, 0)
            );
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../sprints/registro/evidencias/S24/3d_spike_1080p.png");
            if let Some(dir) = out.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            img.save(&out).expect("salvar png");
            println!("spike: imagem em {}", out.display());
        }
    }
}

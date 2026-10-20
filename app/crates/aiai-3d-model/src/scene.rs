//! Cena do renderer cel (`aiai-3d-render`, vendorizado do ANIGO) a partir do modelo e do estado da camada num quadro.
//! Sem wgpu aqui: só monta a [`aiai_3d_core::Scene`]. A malha de cada parte é convertida uma vez ([`base_scene`]); por
//! quadro só mudam câmera, transformação, luz e materiais ([`update_scene`]).

use crate::camera::rotation_quat;
use crate::model::Model;
use crate::raster::{SHADE_MUL, SILHOUETTE_DEFAULT, frame_setup, linear_to_srgb, srgb_to_linear};
use aiai_3d_core::mesh::{Mesh, Vertex};
use aiai_3d_core::scene::{Scene, SceneNode, StylizedMaterial};
use aiai_core::model3d::{Model3dState, RenderMode3d};
use glam::Vec3;

/// Cena com uma parte por nó (malhas convertidas para o `Vertex` de 72 B do renderer; cor de vértice neutra: AO 1,
/// sem desvio de sombra, contorno inteiro, especular ligado).
pub fn base_scene(model: &Model) -> Scene {
    let mut scene = Scene::new_empty();
    scene.background_color = [0.0, 0.0, 0.0, 0.0];
    for (i, p) in model.parts.iter().enumerate() {
        let vertices: Vec<Vertex> = p.positions.iter().zip(&p.normals).map(|(q, n)| Vertex::new(*q, *n, [0.0, 0.0])).collect();
        let mesh = Mesh::new(p.name.clone(), vertices, p.indices.clone());
        let mut node = SceneNode::new(format!("parte_{i}"), p.name.clone()).with_mesh(mesh);
        node.material = Some(StylizedMaterial::default());
        scene.add_node(node);
    }
    scene.rebuild_children();
    scene
}

/// Exposição fixa do shader do ANIGO (`exp2(ambient_sky.w)`, com `w = 1` no empacotamento do headless).
const SHADER_EXPOSURE: f32 = 2.0;

/// Inversa do tonemap ACES (ajuste de Narkowicz) do `cel_shading.wgsl`: x tal que aces(x) = c.
fn aces_inverse(c: f32) -> f32 {
    let c = c.clamp(0.0, 0.999);
    let (a, b) = (2.51 - 2.43 * c, 0.03 - 0.59 * c);
    ((-b + (b * b + 4.0 * a * 0.14 * c).sqrt()) / (2.0 * a)).max(0.0)
}

/// Cor sRGB desejada na saída → cor sRGB do material: o shader lineariza, multiplica pela luz (1), pela exposição e
/// passa pelo ACES; aqui se desfaz isso para a referência ter as cores do modelo (e não lavadas pelo tonemap).
pub fn precompensate(srgb: [f32; 3]) -> [f32; 3] {
    srgb.map(|v| linear_to_srgb((aces_inverse(srgb_to_linear(v)) / SHADER_EXPOSURE).clamp(0.0, 1.0)))
}

/// Atualiza câmera, transformação, luz e materiais da cena para o quadro (`w`×`h` = canvas do corte). O modo `wire` não
/// passa por aqui (o renderer cel não desenha arestas; o serviço usa o rasterizador CPU).
pub fn update_scene(scene: &mut Scene, model: &Model, s: &Model3dState, w: u32, h: u32) {
    let fs = frame_setup(model, s, w, h);
    scene.camera.eye = fs.eye;
    scene.camera.target = fs.target;
    scene.camera.up = fs.up;
    scene.camera.fov_y = fs.fov_y;
    scene.camera.aspect = w as f32 / h.max(1) as f32;
    scene.camera.z_near = fs.near;
    scene.camera.z_far = fs.far;
    scene.camera.orthographic = None;
    let shaded = matches!(s.mode, RenderMode3d::Toon);
    // toon: luz presa à câmera, de cima e da esquerda; chapado/silhueta: luz do lado da câmera (toda face visível fica
    // do lado iluminado, sem banda de sombra)
    let back = (fs.eye - fs.target).normalize_or(Vec3::Z);
    scene.light.direction = if shaded { fs.light_dir.to_array() } else { back.to_array() };
    // luz e ambiente brancos, intensidade 1: a cor que chega ao tonemap é a do material (ver `precompensate`)
    scene.light.color = [1.0, 1.0, 1.0];
    scene.light.intensity = 1.0;
    scene.light.ambient_intensity = 1.0;
    scene.light.ambient_sky = [1.0, 1.0, 1.0];
    scene.light.ambient_ground = [1.0, 1.0, 1.0];
    scene.light.shadow_color = [1.0, 1.0, 1.0];
    scene.background_color = [0.0, 0.0, 0.0, 0.0];
    let t = &s.transform;
    // contorno: o casco invertido usa espessura em NDC (constante em px sem o falloff por distância): 2·px/altura
    let outline_ndc = 2.0 * s.outline_px / h.max(1) as f32;
    let oc = s.outline_color;
    for (node, p) in scene.nodes.iter_mut().zip(&model.parts) {
        node.transform.translation = Vec3::from(t.pos);
        node.transform.rotation = rotation_quat(t.rot_deg);
        node.transform.scale = Vec3::from(t.scale);
        // o shader do ANIGO recebe as cores do material em sRGB (converte para linear dentro dele)
        // cor desejada na saída (sRGB), igual à do rasterizador CPU: luz = base, sombra = base × SHADE_MUL
        let base: [f32; 3] = match (s.mode, s.color) {
            (RenderMode3d::Silhouette, c) => c.unwrap_or(SILHOUETTE_DEFAULT),
            (_, Some(c)) => c,
            _ => [linear_to_srgb(p.color[0]), linear_to_srgb(p.color[1]), linear_to_srgb(p.color[2])],
        };
        let shade = [base[0] * SHADE_MUL[0], base[1] * SHADE_MUL[1], base[2] * SHADE_MUL[2]];
        let lit = precompensate(base);
        let sh = if shaded { precompensate(shade) } else { lit };
        let m = node.material.get_or_insert_with(StylizedMaterial::default);
        m.base_color = [lit[0], lit[1], lit[2], 1.0];
        m.shade_color = [sh[0], sh[1], sh[2], 1.0];
        // referência limpa: sem brilho especular nem luz de recorte (só luz e sombra)
        m.spec_intensity = 0.0;
        m.rim_intensity = 0.0;
        m.outline_color = [oc[0], oc[1], oc[2], 1.0];
        m.outline_width = outline_ndc;
        m.outline_opacity = if s.outline_px > 0.0 { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::tests::{cube_model, state};

    #[test]
    fn scene_follows_the_layer_state() {
        let m = cube_model();
        let mut sc = base_scene(&m);
        assert_eq!(sc.nodes.len(), 1);
        assert_eq!(sc.nodes[0].mesh.as_ref().unwrap().indices.len(), 36);
        let mut st = state(RenderMode3d::Silhouette);
        st.transform.pos = [1.0, 2.0, 3.0];
        st.outline_px = 4.0;
        update_scene(&mut sc, &m, &st, 1920, 1080);
        let n = &sc.nodes[0];
        assert_eq!(n.transform.translation, Vec3::new(1.0, 2.0, 3.0));
        let mat = n.material.as_ref().unwrap();
        assert_eq!(mat.base_color, mat.shade_color, "silhueta: cor única");
        // o material compensa exposição e ACES do shader: aplicar o caminho do shader devolve a cor pedida
        let aces = |x: f32| ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0);
        for k in 0..3 {
            let out = linear_to_srgb(aces(srgb_to_linear(mat.base_color[k]) * SHADER_EXPOSURE));
            assert!((out - SILHOUETTE_DEFAULT[k]).abs() < 0.01, "{out} vs {}", SILHOUETTE_DEFAULT[k]);
        }
        assert_eq!(sc.light.direction, (sc.camera.eye - sc.camera.target).normalize().to_array(), "luz do lado da câmera");
        assert!((mat.outline_width - 8.0 / 1080.0).abs() < 1e-6);
        assert_eq!(sc.background_color, [0.0; 4]);
        assert!((sc.camera.aspect - 16.0 / 9.0).abs() < 1e-5);
    }
}

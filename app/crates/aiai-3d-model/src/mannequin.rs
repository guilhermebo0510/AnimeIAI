//! Manequim posável (S24 3D, fase 8): o corpo da base de personagem anime do ANIGO (`aiai-3d-charbase`, neutra entre
//! masculino e feminino) com o esqueleto humanoide VRM e os pesos de skin, como um [`Model`] com [`Skin`]. Vai para o
//! projeto como um ativo 3D comum (GLB normalizado com pele), então renderiza, salva e abre em outro PC igual a qualquer
//! modelo importado. Determinístico: o mesmo código gera os mesmos bytes.

use crate::model::{LicenseInfo, Model, ModelInfo, Part, PartSkin, Skin, SkinBone};
use aiai_3d_charbase::{Gender, generate};
use std::sync::{Arc, OnceLock};

/// Nome do ativo e da camada.
pub const NAME: &str = "Manequim";
/// Marca do ativo embutido (`params.builtin` do nó `Asset`): reusa o mesmo ativo no projeto em vez de duplicar.
pub const BUILTIN: &str = "manequim";
/// Versão da geração (sobe quando a malha do Manequim mudar; entra em `params.builtin_version`).
pub const VERSION: u32 = 1;
/// Cor do Manequim (linear; ≈ cinza-claro azulado em sRGB, o "boneco de madeira" neutro de referência).
pub const COLOR: [f32; 4] = [0.56, 0.58, 0.64, 1.0];

static MODEL: OnceLock<Arc<Model>> = OnceLock::new();

/// O Manequim (gerado uma vez por processo).
pub fn model() -> Arc<Model> {
    Arc::clone(MODEL.get_or_init(|| Arc::new(build())))
}

fn build() -> Model {
    let c = generate(Gender::Blend(0.5));
    let topo = c.topology();
    let positions: Vec<[f32; 3]> = c.positions.iter().map(|p| p.to_array()).collect();
    let normals: Vec<[f32; 3]> = c.normals.iter().map(|n| n.normalize_or(glam::Vec3::Y).to_array()).collect();
    let mut indices = Vec::with_capacity(topo.faces.len() * 6);
    for f in &topo.faces {
        indices.extend([f[0], f[1], f[2], f[0], f[2], f[3]]);
    }
    let joints: Vec<[u16; 4]> = topo.weights.iter().map(|w| [w[0].0, w[1].0, w[2].0, w[3].0]).collect();
    let weights: Vec<[f32; 4]> = topo
        .weights
        .iter()
        .map(|w| {
            let s: f32 = w.iter().map(|x| x.1.max(0.0)).sum();
            let k = if s > 1e-6 { 1.0 / s } else { 0.0 };
            [w[0].1.max(0.0) * k, w[1].1.max(0.0) * k, w[2].1.max(0.0) * k, w[3].1.max(0.0) * k]
        })
        .collect();
    let bones = c
        .skeleton
        .bones
        .iter()
        .map(|b| SkinBone { name: b.name.clone(), parent: b.parent, rest_pos: b.world, rest_rot: [0.0, 0.0, 0.0, 1.0] })
        .collect();
    let part = Part { name: "corpo".into(), positions, normals, indices, color: COLOR, material: "manequim".into() };
    let license = LicenseInfo {
        title: NAME.into(),
        author: "AnimeIAI".into(),
        license: "gerado pelo AnimeIAI (base procedural do ANIGO)".into(),
        copyright: String::new(),
        source: "procedural".into(),
    };
    let mut m = Model {
        parts: vec![part],
        info: ModelInfo { format: "aiai".into(), file_name: "manequim".into(), license, ..Default::default() },
        skin: Some(Skin { bones, parts: vec![PartSkin { joints, weights }] }),
    };
    m.refresh_info();
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose;

    #[test]
    fn mannequin_has_a_humanoid_skin_and_stands_on_the_floor() {
        let m = model();
        let s = m.skin.as_ref().unwrap();
        s.validate(&m.parts).unwrap();
        for n in pose::POSE_JOINTS {
            assert!(s.index(n).is_some(), "falta o osso {n}");
        }
        let [lo, hi] = m.info.bbox;
        assert!(lo[1].abs() < 0.08, "pés perto do chão: {lo:?}");
        assert!((1.2..2.2).contains(&(hi[1] - lo[1])), "altura {}", hi[1] - lo[1]);
        // em pé (braços baixados): a mão esquerda fica abaixo do ombro e do lado esquerdo (+X)
        let stand = pose::preset("stand", s).unwrap();
        let g = pose::globals(s, &stand);
        let (hand, shoulder) = (g[s.index("leftHand").unwrap()].pos, g[s.index("leftUpperArm").unwrap()].pos);
        assert!(hand.y < shoulder.y - 0.3 && hand.x > 0.0, "mão {hand:?} ombro {shoulder:?}");
        // a malha posada fica mais estreita que em T (braços para baixo)
        let posed = pose::apply(&m, &stand);
        let w_t = hi[0] - lo[0];
        let w_s = posed.info.bbox[1][0] - posed.info.bbox[0][0];
        assert!(w_s < w_t * 0.7, "largura em T {w_t}, em pé {w_s}");
        // sentado: o quadril desce
        let sit = pose::preset("sit", s).unwrap();
        assert!(pose::apply(&m, &sit).info.bbox[1][1] < hi[1] - 0.2);
    }

    #[test]
    fn mannequin_glb_round_trips_with_its_skin() {
        let m = model();
        let bytes = crate::glb::write_glb(&m);
        assert_eq!(crate::glb::write_glb(&model()), bytes, "determinístico");
        let back = crate::glb::read_glb(&bytes).unwrap();
        let (a, b) = (back.skin.as_ref().unwrap(), m.skin.as_ref().unwrap());
        assert_eq!(a.bones.len(), b.bones.len());
        assert_eq!(back.info.triangles, m.info.triangles);
        // a mesma pose dá a mesma malha depois de relida
        let p = pose::preset("walk", b).unwrap();
        let (pa, pb) = (pose::apply(&back, &p), pose::apply(&m, &p));
        let max = pa.parts[0].positions.iter().zip(&pb.parts[0].positions).map(|(x, y)| (0..3).map(|k| (x[k] - y[k]).abs()).fold(0.0, f32::max)).fold(0.0, f32::max);
        assert!(max < 1e-4, "diferença {max}");
    }
}

//! Manequim articulado (cinemática direta) com proporções de anime ajustáveis (altura em cabeças, chibi).
//! Cada osso é desenhado por um volume simples (elipsoide/caixa), como os manequins de referência do Clip Studio.

use crate::mesh::Mesh;
use glam::{EulerRot, Mat4, Quat, Vec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Proportions {
    /// Altura total (m).
    #[serde(default = "d_height")]
    pub height: f32,
    /// Altura em cabeças (realista 7,5; anime 6–7; adolescente 6; chibi 2–3).
    #[serde(default = "d_heads")]
    pub heads_tall: f32,
    /// Largura dos ombros em cabeças.
    #[serde(default = "d_shoulders")]
    pub shoulder_heads: f32,
    /// Largura do quadril em cabeças.
    #[serde(default = "d_hips")]
    pub hip_heads: f32,
}

fn d_height() -> f32 {
    1.6
}
fn d_heads() -> f32 {
    6.5
}
fn d_shoulders() -> f32 {
    1.5
}
fn d_hips() -> f32 {
    1.3
}

impl Default for Proportions {
    fn default() -> Self {
        Proportions { height: d_height(), heads_tall: d_heads(), shoulder_heads: d_shoulders(), hip_heads: d_hips() }
    }
}

/// Ossos do manequim (nomes estáveis usados nas poses).
pub const BONES: &[&str] = &[
    "pelvis", "spine", "chest", "neck", "head", "shoulder_l", "upper_arm_l", "forearm_l", "hand_l", "shoulder_r", "upper_arm_r", "forearm_r",
    "hand_r", "thigh_l", "shin_l", "foot_l", "thigh_r", "shin_r", "foot_r",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Mannequin {
    #[serde(default)]
    pub proportions: Proportions,
    /// Posição do pé/quadril no mundo (a pelve fica na altura certa acima disto).
    #[serde(default)]
    pub position: [f32; 3],
    /// Rotação do corpo inteiro em torno de Y (graus).
    #[serde(default)]
    pub facing: f32,
    /// Pose nomeada de partida ("standing", "walking", "sitting", "pointing", "arms_up", "t_pose").
    #[serde(default)]
    pub pose: Option<String>,
    /// Ajustes por osso: graus [x, y, z] (aplicados sobre a pose nomeada).
    #[serde(default)]
    pub bones: BTreeMap<String, [f32; 3]>,
}

impl Default for Mannequin {
    fn default() -> Self {
        Mannequin { proportions: Proportions::default(), position: [0.0; 3], facing: 0.0, pose: Some("standing".into()), bones: BTreeMap::new() }
    }
}

fn named_pose(name: &str) -> BTreeMap<&'static str, [f32; 3]> {
    let mut m = BTreeMap::new();
    match name {
        "t_pose" => {
            m.insert("upper_arm_l", [0.0, 0.0, 90.0]);
            m.insert("upper_arm_r", [0.0, 0.0, -90.0]);
        }
        "walking" => {
            m.insert("thigh_l", [-25.0, 0.0, 0.0]);
            m.insert("shin_l", [10.0, 0.0, 0.0]);
            m.insert("thigh_r", [20.0, 0.0, 0.0]);
            m.insert("shin_r", [30.0, 0.0, 0.0]);
            m.insert("upper_arm_l", [20.0, 0.0, 8.0]);
            m.insert("upper_arm_r", [-20.0, 0.0, -8.0]);
            m.insert("forearm_l", [-15.0, 0.0, 0.0]);
            m.insert("forearm_r", [-25.0, 0.0, 0.0]);
        }
        "sitting" => {
            m.insert("thigh_l", [-90.0, 0.0, 5.0]);
            m.insert("thigh_r", [-90.0, 0.0, -5.0]);
            m.insert("shin_l", [90.0, 0.0, 0.0]);
            m.insert("shin_r", [90.0, 0.0, 0.0]);
            m.insert("upper_arm_l", [-20.0, 0.0, 10.0]);
            m.insert("upper_arm_r", [-20.0, 0.0, -10.0]);
            m.insert("forearm_l", [-60.0, 0.0, 0.0]);
            m.insert("forearm_r", [-60.0, 0.0, 0.0]);
        }
        "pointing" => {
            m.insert("upper_arm_r", [-80.0, 0.0, -10.0]);
            m.insert("upper_arm_l", [0.0, 0.0, 10.0]);
            m.insert("head", [0.0, -15.0, 0.0]);
        }
        "arms_up" => {
            m.insert("upper_arm_l", [0.0, 0.0, 160.0]);
            m.insert("upper_arm_r", [0.0, 0.0, -160.0]);
        }
        _ => {
            // standing: braços relaxados levemente abertos
            m.insert("upper_arm_l", [0.0, 0.0, 8.0]);
            m.insert("upper_arm_r", [0.0, 0.0, -8.0]);
            m.insert("forearm_l", [-8.0, 0.0, 0.0]);
            m.insert("forearm_r", [-8.0, 0.0, 0.0]);
        }
    }
    m
}

struct BoneDef {
    name: &'static str,
    parent: Option<&'static str>,
    /// Offset do início do osso em relação ao FIM do pai (em cabeças), no espaço do pai.
    offset: [f32; 3],
    /// Comprimento do osso (cabeças) ao longo de -Y (ossos "para baixo") ou +Y.
    length: f32,
    dir_up: bool,
    /// Raio do volume (cabeças).
    radius: f32,
}

impl Mannequin {
    pub fn validate(&self) -> Result<(), String> {
        let p = &self.proportions;
        if !(1.5..=10.0).contains(&p.heads_tall) || !(0.3..=3.0).contains(&p.height) {
            return Err("proporções fora do intervalo (cabeças 1,5..10, altura 0,3..3 m)".into());
        }
        for k in self.bones.keys() {
            if !BONES.contains(&k.as_str()) {
                return Err(format!("osso desconhecido: {k} (válidos: {})", BONES.join(", ")));
            }
        }
        Ok(())
    }

    fn defs(&self) -> Vec<BoneDef> {
        let p = &self.proportions;
        let ht = p.heads_tall;
        // Distribuição vertical em cabeças: cabeça 1, pescoço 0,25; tronco ~ (ht*0.36); pernas ~ (ht*0.47)
        let chibi = ht < 4.0;
        let leg = if chibi { ht * 0.30 } else { ht * 0.47 };
        let torso = (ht - 1.0 - leg - 0.15).max(0.4);
        let arm = if chibi { torso * 1.1 } else { ht * 0.42 };
        let sh = p.shoulder_heads / 2.0;
        let hp = p.hip_heads / 4.0;
        let limb_r = if chibi { 0.16 } else { 0.11 };
        vec![
            BoneDef { name: "pelvis", parent: None, offset: [0.0, leg, 0.0], length: torso * 0.25, dir_up: true, radius: p.hip_heads * 0.38 },
            BoneDef { name: "spine", parent: Some("pelvis"), offset: [0.0, 0.0, 0.0], length: torso * 0.35, dir_up: true, radius: p.hip_heads * 0.32 },
            BoneDef { name: "chest", parent: Some("spine"), offset: [0.0, 0.0, 0.0], length: torso * 0.40, dir_up: true, radius: p.shoulder_heads * 0.3 },
            BoneDef { name: "neck", parent: Some("chest"), offset: [0.0, 0.0, 0.0], length: 0.15, dir_up: true, radius: 0.16 },
            BoneDef { name: "head", parent: Some("neck"), offset: [0.0, 0.0, 0.0], length: 1.0, dir_up: true, radius: 0.42 },
            BoneDef { name: "shoulder_l", parent: Some("chest"), offset: [sh * 0.3, -0.05, 0.0], length: 0.0, dir_up: false, radius: 0.0 },
            BoneDef { name: "upper_arm_l", parent: Some("shoulder_l"), offset: [sh * 0.7, 0.0, 0.0], length: arm * 0.5, dir_up: false, radius: limb_r },
            BoneDef { name: "forearm_l", parent: Some("upper_arm_l"), offset: [0.0, 0.0, 0.0], length: arm * 0.42, dir_up: false, radius: limb_r * 0.85 },
            BoneDef { name: "hand_l", parent: Some("forearm_l"), offset: [0.0, 0.0, 0.0], length: arm * 0.2, dir_up: false, radius: limb_r * 0.9 },
            BoneDef { name: "shoulder_r", parent: Some("chest"), offset: [-sh * 0.3, -0.05, 0.0], length: 0.0, dir_up: false, radius: 0.0 },
            BoneDef { name: "upper_arm_r", parent: Some("shoulder_r"), offset: [-sh * 0.7, 0.0, 0.0], length: arm * 0.5, dir_up: false, radius: limb_r },
            BoneDef { name: "forearm_r", parent: Some("upper_arm_r"), offset: [0.0, 0.0, 0.0], length: arm * 0.42, dir_up: false, radius: limb_r * 0.85 },
            BoneDef { name: "hand_r", parent: Some("forearm_r"), offset: [0.0, 0.0, 0.0], length: arm * 0.2, dir_up: false, radius: limb_r * 0.9 },
            BoneDef { name: "thigh_l", parent: Some("pelvis"), offset: [hp, -torso * 0.25, 0.0], length: leg * 0.5, dir_up: false, radius: limb_r * 1.4 },
            BoneDef { name: "shin_l", parent: Some("thigh_l"), offset: [0.0, 0.0, 0.0], length: leg * 0.46, dir_up: false, radius: limb_r * 1.05 },
            BoneDef { name: "foot_l", parent: Some("shin_l"), offset: [0.0, 0.0, 0.0], length: leg * 0.06, dir_up: false, radius: limb_r * 1.1 },
            BoneDef { name: "thigh_r", parent: Some("pelvis"), offset: [-hp, -torso * 0.25, 0.0], length: leg * 0.5, dir_up: false, radius: limb_r * 1.4 },
            BoneDef { name: "shin_r", parent: Some("thigh_r"), offset: [0.0, 0.0, 0.0], length: leg * 0.46, dir_up: false, radius: limb_r * 1.05 },
            BoneDef { name: "foot_r", parent: Some("shin_r"), offset: [0.0, 0.0, 0.0], length: leg * 0.06, dir_up: false, radius: limb_r * 1.1 },
        ]
    }

    /// Transformações mundiais (início do osso) e pontos final de cada osso.
    pub fn joints(&self) -> BTreeMap<&'static str, (Mat4, Vec3, Vec3)> {
        let head = self.proportions.height / self.proportions.heads_tall;
        let mut angles: BTreeMap<String, [f32; 3]> = named_pose(self.pose.as_deref().unwrap_or("standing")).into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        for (k, v) in &self.bones {
            let e = angles.entry(k.clone()).or_insert([0.0; 3]);
            for i in 0..3 {
                e[i] += v[i];
            }
        }
        // Poses que dobram as pernas abaixam a pelve: sentado = pelve na altura do assento (≈ comprimento da canela).
        let chibi = self.proportions.heads_tall < 4.0;
        let leg = if chibi { self.proportions.heads_tall * 0.30 } else { self.proportions.heads_tall * 0.47 };
        let drop = match self.pose.as_deref() {
            Some("sitting") => leg * 0.5 * head,
            _ => 0.0,
        };
        let root = Mat4::from_translation(Vec3::from(self.position) - Vec3::Y * drop) * Mat4::from_rotation_y(self.facing.to_radians());
        let mut out: BTreeMap<&'static str, (Mat4, Vec3, Vec3)> = BTreeMap::new();
        // "fim" de cada osso no espaço do mundo, como matriz para os filhos
        let mut ends: BTreeMap<&'static str, Mat4> = BTreeMap::new();
        for d in self.defs() {
            let parent_end = match d.parent {
                Some(p) => ends[p],
                None => root,
            };
            let a = angles.get(d.name).copied().unwrap_or([0.0; 3]);
            let rot = Quat::from_euler(EulerRot::XYZ, a[0].to_radians(), a[1].to_radians(), a[2].to_radians());
            let start = parent_end * Mat4::from_translation(Vec3::from(d.offset) * head) * Mat4::from_quat(rot);
            let dir = if d.dir_up { Vec3::Y } else { -Vec3::Y };
            let end = start * Mat4::from_translation(dir * d.length * head);
            let p0 = start.transform_point3(Vec3::ZERO);
            let p1 = end.transform_point3(Vec3::ZERO);
            out.insert(d.name, (start, p0, p1));
            ends.insert(d.name, end);
        }
        out
    }

    /// Malha do manequim: elipsoide por osso (cabeça esfera, tronco caixas arredondadas aproximadas por elipsoides).
    pub fn mesh(&self) -> Mesh {
        let head = self.proportions.height / self.proportions.heads_tall;
        let joints = self.joints();
        let mut m = Mesh::default();
        for d in self.defs() {
            if d.length <= 0.0 || d.radius <= 0.0 {
                continue;
            }
            let (start, _, _) = joints[d.name];
            let len = d.length * head;
            let r = d.radius * head;
            let dir = if d.dir_up { 1.0 } else { -1.0 };
            let (rings, segs) = if d.name == "head" { (10, 16) } else { (6, 10) };
            let sx = if matches!(d.name, "pelvis" | "spine" | "chest") { 1.0 } else { 1.0 };
            let sz = if matches!(d.name, "pelvis" | "spine" | "chest") { 0.6 } else { 1.0 };
            let sphere = Mesh::sphere(1.0, rings, segs);
            let shape = Mat4::from_translation(Vec3::new(0.0, dir * len * 0.5, 0.0))
                * Mat4::from_scale(Vec3::new(r * sx, (len * 0.5).max(r * 0.6), r * sz));
            m.append(&sphere.transformed(&(start * shape)));
            if d.name == "head" {
                // marcador do rosto (nariz) para a direção da cabeça aparecer nas vistas de 3/4 e perfil
                let nose = Mesh::cuboid(Vec3::new(r * 0.22, r * 0.3, r * 0.35));
                let at = Mat4::from_translation(Vec3::new(0.0, len * 0.42, r * 0.98));
                m.append(&nose.transformed(&(start * at)));
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_height_matches_proportions() {
        let man = Mannequin::default();
        let j = man.joints();
        let top = j["head"].2.y;
        let foot = j["foot_l"].2.y;
        assert!((top - foot - 1.6).abs() < 0.12, "altura ≈ 1,6 m: {}", top - foot);
        let head_len = j["head"].2.y - j["head"].1.y;
        assert!((head_len - 1.6 / 6.5).abs() < 0.01);
        let chibi = Mannequin { proportions: Proportions { heads_tall: 2.5, height: 0.9, ..Default::default() }, ..Default::default() };
        let jc = chibi.joints();
        let ch = jc["head"].2.y - jc["head"].1.y;
        assert!(ch > 0.3, "cabeça grande no chibi: {ch}");
        assert!(man.validate().is_ok());
    }
}


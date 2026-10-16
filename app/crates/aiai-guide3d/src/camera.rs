//! Câmera de guia: posição, alvo, lente (mm em sensor 35 mm) ou ortográfica.

use glam::{Mat4, Vec3, Vec4};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Camera3d {
    pub position: [f32; 3],
    pub target: [f32; 3],
    /// Distância focal em mm (sensor full-frame 36 mm de largura). 35 mm ≈ grande-angular, 50 normal, 85 retrato.
    #[serde(default = "d_focal")]
    pub focal_mm: f32,
    /// Rolagem em graus (inclinação "dutch").
    #[serde(default)]
    pub roll: f32,
    /// Se definido, câmera ortográfica com esta altura visível (m).
    #[serde(default)]
    pub ortho_height: Option<f32>,
}

fn d_focal() -> f32 {
    35.0
}

impl Default for Camera3d {
    fn default() -> Self {
        Camera3d { position: [0.0, 1.5, 4.0], target: [0.0, 1.0, 0.0], focal_mm: 35.0, roll: 0.0, ortho_height: None }
    }
}

/// Projeção pronta para um canvas.
pub struct Projector {
    pub view: Mat4,
    pub proj: Mat4,
    pub width: f32,
    pub height: f32,
    pub ortho: bool,
}

impl Camera3d {
    pub fn validate(&self) -> Result<(), String> {
        let p = Vec3::from(self.position);
        let t = Vec3::from(self.target);
        if (p - t).length() < 1e-4 {
            return Err("câmera e alvo no mesmo lugar".into());
        }
        if !(1.0..=2000.0).contains(&self.focal_mm) {
            return Err("lente fora de 1..2000 mm".into());
        }
        Ok(())
    }

    /// Campo de visão horizontal (rad) pela lente.
    pub fn hfov(&self) -> f32 {
        2.0 * (18.0 / self.focal_mm).atan()
    }

    pub fn projector(&self, width: u32, height: u32) -> Projector {
        let eye = Vec3::from(self.position);
        let target = Vec3::from(self.target);
        let fwd = (target - eye).normalize();
        let mut up = if fwd.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
        if self.roll != 0.0 {
            up = glam::Quat::from_axis_angle(fwd, -self.roll.to_radians()) * up;
        }
        let view = Mat4::look_at_rh(eye, target, up);
        let aspect = width as f32 / height as f32;
        let (proj, ortho) = match self.ortho_height {
            Some(h) => (Mat4::orthographic_rh(-h * aspect / 2.0, h * aspect / 2.0, -h / 2.0, h / 2.0, 0.01, 1000.0), true),
            None => {
                let vfov = 2.0 * ((self.hfov() / 2.0).tan() / aspect).atan();
                (Mat4::perspective_rh(vfov, aspect, 0.01, 1000.0), false)
            }
        };
        Projector { view, proj, width: width as f32, height: height as f32, ortho }
    }
}

impl Projector {
    /// Mundo → (x, y em px, profundidade linear positiva em m). `None` atrás da câmera.
    pub fn project(&self, p: Vec3) -> Option<(f32, f32, f32)> {
        let v = self.view * Vec4::new(p.x, p.y, p.z, 1.0);
        let depth = -v.z;
        if depth <= 0.01 {
            return None;
        }
        let c = self.proj * v;
        let (nx, ny) = (c.x / c.w, c.y / c.w);
        Some(((nx * 0.5 + 0.5) * self.width, (0.5 - ny * 0.5) * self.height, depth))
    }

    /// Ponto de fuga de uma direção do mundo (perspectiva); `None` se paralela ao plano da imagem ou ortográfica.
    pub fn vanishing_point(&self, dir: Vec3) -> Option<[f32; 2]> {
        if self.ortho {
            return None;
        }
        let d = self.view.transform_vector3(dir.normalize());
        if d.z.abs() < 1e-4 {
            return None;
        }
        // ponto no infinito na direção d (ou oposta, se aponta para trás)
        let d = if d.z > 0.0 { -d } else { d };
        let c = self.proj * Vec4::new(d.x, d.y, d.z, 0.0);
        if c.w.abs() < 1e-8 {
            return None;
        }
        Some([(c.x / c.w * 0.5 + 0.5) * self.width, (0.5 - c.y / c.w * 0.5) * self.height])
    }
}

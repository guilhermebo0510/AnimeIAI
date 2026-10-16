//! Cena de guia: blocos (primitivas), manequins e grade de chão.

use crate::mannequin::Mannequin;
use crate::mesh::Mesh;
use glam::{EulerRot, Mat4, Quat, Vec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Box { size: [f32; 3] },
    Cylinder { radius: f32, height: f32 },
    Cone { radius: f32, height: f32 },
    Sphere { radius: f32 },
    Plane { size: [f32; 2] },
    Stairs { steps: u32, width: f32, depth: f32, height: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Object3d {
    #[serde(default)]
    pub name: String,
    pub shape: Shape,
    /// Posição do centro (caixa/esfera/cilindro) ou da base (plano, escada).
    #[serde(default)]
    pub position: [f32; 3],
    /// Graus (X, Y, Z).
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "unit3")]
    pub scale: [f32; 3],
}

fn unit3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorGrid {
    /// Tamanho (m) do quadrado da grade centrado na origem.
    pub size: f32,
    /// Espaçamento (m).
    pub step: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Scene3d {
    #[serde(default)]
    pub objects: Vec<Object3d>,
    #[serde(default)]
    pub mannequins: Vec<Mannequin>,
    #[serde(default)]
    pub floor_grid: Option<FloorGrid>,
}

impl Object3d {
    pub fn mesh(&self) -> Mesh {
        let base = match &self.shape {
            Shape::Box { size } => Mesh::cuboid(Vec3::from(*size)),
            Shape::Cylinder { radius, height } => Mesh::cylinder(*radius, *height, 24),
            Shape::Cone { radius, height } => Mesh::frustum(*radius, 0.0001, *height, 24),
            Shape::Sphere { radius } => Mesh::sphere(*radius, 12, 20),
            Shape::Plane { size } => Mesh::plane(size[0], size[1]),
            Shape::Stairs { steps, width, depth, height } => Mesh::stairs(*steps, *width, *depth, *height),
        };
        let r = Quat::from_euler(EulerRot::XYZ, self.rotation[0].to_radians(), self.rotation[1].to_radians(), self.rotation[2].to_radians());
        let m = Mat4::from_scale_rotation_translation(Vec3::from(self.scale), r, Vec3::from(self.position));
        base.transformed(&m)
    }
}

impl Scene3d {
    pub fn validate(&self) -> Result<(), String> {
        for o in &self.objects {
            let bad = match &o.shape {
                Shape::Box { size } => size.iter().any(|v| *v <= 0.0),
                Shape::Cylinder { radius, height } | Shape::Cone { radius, height } => *radius <= 0.0 || *height <= 0.0,
                Shape::Sphere { radius } => *radius <= 0.0,
                Shape::Plane { size } => size.iter().any(|v| *v <= 0.0),
                Shape::Stairs { steps, width, depth, height } => *steps == 0 || *steps > 200 || *width <= 0.0 || *depth <= 0.0 || *height <= 0.0,
            };
            if bad {
                return Err(format!("objeto '{}' com dimensões inválidas", o.name));
            }
        }
        for m in &self.mannequins {
            m.validate()?;
        }
        if let Some(g) = &self.floor_grid {
            if g.step <= 0.0 || g.size <= 0.0 || g.size / g.step > 400.0 {
                return Err("grade de chão inválida (passo > 0, no máx. 400 linhas)".into());
            }
        }
        Ok(())
    }

    /// Malhas (uma por objeto/manequim), para contorno e profundidade.
    pub fn meshes(&self) -> Vec<Mesh> {
        self.objects.iter().map(|o| o.mesh()).chain(self.mannequins.iter().map(|m| m.mesh())).collect()
    }
}

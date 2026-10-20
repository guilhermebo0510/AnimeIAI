//! Subconjunto de `anigo-core/src/level.rs`: só o [`LightShape`], usado por `scene::RenderLight`.

use serde::{Deserialize, Serialize};

/// Tipo de luz de um nó `Light`: o renderer cel desenha até [`crate::scene::MAX_RENDER_LIGHTS`] delas, somadas à luz
/// principal. A luz aponta para -Z local do nó (como no Blender): girar o nó gira o feixe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightShape {
    /// Direcional extra (luz de preenchimento/recorte): só a direção importa, sem queda com a distância.
    Directional,
    /// Ponto: irradia em todas as direções, com queda suave até o alcance.
    Point,
    /// Holofote: cone com borda suave.
    Spot,
    /// Painel retangular de área (softbox): ilumina de um lado só, a partir do ponto mais próximo do retângulo.
    Rect,
}

impl LightShape {
    pub const ALL: [LightShape; 4] = [LightShape::Point, LightShape::Spot, LightShape::Rect, LightShape::Directional];

    pub fn as_str(self) -> &'static str {
        match self {
            LightShape::Directional => "directional",
            LightShape::Point => "point",
            LightShape::Spot => "spot",
            LightShape::Rect => "rect",
        }
    }
}

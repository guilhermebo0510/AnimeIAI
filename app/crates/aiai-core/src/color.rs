use crate::ids::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Cor sRGB não linear com alfa, componentes 0..=1 (o que o usuário vê e o que o settei guarda).
/// A conversão para linear acontece no render.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rgba(pub [f32; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0.0, 0.0, 0.0, 1.0]);
    pub const WHITE: Rgba = Rgba([1.0, 1.0, 1.0, 1.0]);
    pub const TRANSPARENT: Rgba = Rgba([0.0, 0.0, 0.0, 0.0]);

    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Rgba([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    }

    /// Aceita `#rrggbb` ou `#rrggbbaa`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.strip_prefix('#').unwrap_or(hex);
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Rgba([byte(0)? as f32 / 255.0, byte(2)? as f32 / 255.0, byte(4)? as f32 / 255.0, 1.0])),
            8 => Some(Rgba([
                byte(0)? as f32 / 255.0,
                byte(2)? as f32 / 255.0,
                byte(4)? as f32 / 255.0,
                byte(6)? as f32 / 255.0,
            ])),
            _ => None,
        }
    }

    pub fn to_hex(&self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}{:02x}", c(self.0[0]), c(self.0[1]), c(self.0[2]), c(self.0[3]))
    }

    pub fn is_valid(&self) -> bool {
        self.0.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }
}

/// Referência de cor: direta ou por índice de paleta (trocar a cor na paleta recolore tudo que a referencia).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColorRef {
    Direct { rgba: Rgba },
    Palette { palette: Id, color: Id },
}

impl Default for ColorRef {
    fn default() -> Self {
        ColorRef::Direct { rgba: Rgba::BLACK }
    }
}

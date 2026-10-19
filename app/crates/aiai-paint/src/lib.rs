//! Motor de desenho (CPU, determinístico). Convenções:
//! - Cor de trabalho: sRGB **codificado** (não linear), alfa **pré-multiplicado**, f32 — como o After Effects e o Photoshop
//!   no modo padrão; é o que faz multiplicar/tela/sobrepor baterem com a referência de anime.
//! - Coordenadas em pixels do canvas do corte; o centro do pixel (x, y) é (x + 0.5, y + 0.5).

pub mod adjust;
pub mod blend;
pub mod brush;
pub mod curve;
pub mod fill;
pub mod image;
pub mod mask;
pub mod noise;
pub mod poly;
pub mod select;
pub mod stroke_raster;
pub mod text;
pub mod texture;
pub mod tile;
pub mod transform;
pub mod warp;

pub use crate::image::Image;
pub use curve::{StrokeSample, flatten_points, sample_stroke};
pub use mask::Mask;
pub use transform::{Affine, transform_image};
pub use stroke_raster::{rasterize_stroke, StrokeCoverage};

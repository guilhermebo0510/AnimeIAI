//! Guias 3D (S09): o agente monta o espaço em blocos e posa um manequim, escolhe câmera e lente, e o programa gera
//! **linhas de guia** (contornos e vincos visíveis, com remoção de linhas ocultas) como traços vetoriais numa camada de
//! guia, um sombreado simples de referência de luz, e os pontos de fuga da perspectiva — o desenho final é feito por
//! cima, como no Clip Studio com modelos 3D.
//!
//! Unidades do mundo: metros; Y para cima; a câmera olha para `target`. Canvas: pixels, origem no canto superior
//! esquerdo, y para baixo.

pub mod camera;
pub mod mannequin;
pub mod mesh;
pub mod render;
pub mod scene;

pub use camera::Camera3d;
pub use mannequin::{Mannequin, Proportions};
pub use mesh::Mesh;
pub use render::{GuideStyle, render_lines, render_shaded, vanishing_points, VanishingPoints};
pub use scene::{Object3d, Scene3d, Shape};

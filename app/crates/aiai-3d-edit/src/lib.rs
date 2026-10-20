//! Modelagem do AnimeIAI (S24 3D, fase 12): subconjunto vendorizado do `anigo-modeling` (malha editável n-gon, primitivas,
//! operadores do modo Edição e modificadores; procedência em `VENDOR.md`) e o documento da Modelagem ([`doc`]): objetos,
//! modificadores, material toon, histórico e a conversão para o modelo normalizado (GLB) da camada 3D e da biblioteca.
//! Rust puro, sem GPU: tudo testável.

pub mod doc;
pub mod error;
pub mod mesh;
pub mod modifiers;
pub mod normals;
pub mod ops;
pub mod primitives;
pub mod select;
pub mod subdiv;
pub mod topology;

pub use doc::{History, ModelDoc, ModifierSpec, Object3d, Primitive, Shading};
pub use error::{ModelError, ModelResult};
pub use mesh::{edge_key, Corner, EVert, EdgeAttr, EdgeKey, EditMesh, Face, Remap};
pub use select::{SelectMode, Selection};
pub use topology::Topology;

//! Modelagem do AnimeIAI (S24 3D, fase 12): o motor vendorizado do `anigo-modeling` (malha editável n-gon, primitivas,
//! os operadores do modo Edição com o catálogo da interface, os modificadores e o histórico; procedência em `VENDOR.md`) e o documento da Modelagem ([`doc`]): objetos,
//! modificadores, material toon, histórico e a conversão para o modelo normalizado (GLB) da camada 3D e da biblioteca.
//! Rust puro, sem GPU: tudo testável.

pub mod bvh;
pub mod convert;
pub mod doc;
pub mod error;
pub mod history;
pub mod mesh;
pub mod modifiers;
pub mod normals;
pub mod normals_tools;
pub mod ops;
pub mod primitives;
pub mod sculpt;
pub mod select;
pub mod subdiv;
pub mod topology;
pub mod uv;

pub use doc::{History, ModelDoc, ModifierSpec, Object3d, Primitive, Shading};
pub use error::{ModelError, ModelResult};
pub use mesh::{edge_key, Corner, EVert, EdgeAttr, EdgeKey, EditMesh, Face, Remap};
pub use select::{SelectMode, Selection};
pub use topology::Topology;

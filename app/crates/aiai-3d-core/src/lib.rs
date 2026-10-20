//! Núcleo 3D do AnimeIAI: subconjunto vendorizado do `anigo-core` (ANIGO), só o que o renderer cel precisa
//! (malha, matemática, cena, NPR, sombra, morphs). Procedência e alterações em `VENDOR.md`.

pub mod hierarchy;
pub mod level;
pub mod math;
pub mod mesh;
pub mod morph;
pub mod npr;
pub mod scene;
pub mod shadow;
pub mod snapshot;

pub use hierarchy::{HierarchyError, NodeKind};
pub use level::LightShape;
pub use math::{
    Aabb, BoundingSphere, Camera, Frustum, FrustumPlane, OrthographicBounds, ProjectionError,
    ProjectionMode, Transform,
};
pub use mesh::{BaseGender, Mesh, Vertex};
pub use morph::{MorphChannel, MorphTarget, SparseMorphDelta, SparseMorphHeader, SparseMorphSet};
pub use npr::{NprParams, RampCustom};
pub use scene::{LightShadow, Scene, SceneNode, StylizedLight, StylizedMaterial};
pub use snapshot::SkinPayload;

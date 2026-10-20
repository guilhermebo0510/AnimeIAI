//! Base de personagem anime posável vendorizada do ANIGO (`anigo-charbase`, subconjunto): gaiola procedural, subdivisão
//! Catmull-Clark, topologia de referência, esqueleto humanoide VRM (55 ossos + twist + busto) e pesos de skin. É o
//! "Manequim" da camada 3D do AnimeIAI (S24 3D, fase 8). Procedência e mudanças em `VENDOR.md`.

pub mod cage;
pub mod character;
pub mod dims;
pub mod params;
pub mod qmesh;
pub mod skeleton;
pub mod subdiv;
pub mod topology;
pub mod uv;
pub mod weights;

pub use character::{generate, generate_with, BaseCharacter};
pub use params::{FaceParams, Gender, Params};

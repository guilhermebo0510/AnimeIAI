//! Subconjunto de `anigo-core/src/snapshot.rs`: só o [`SkinPayload`], que a [`crate::scene::Scene`] carrega.
//!
//! AnimeIAI: o restante do snapshot (geometria estática, morphs, catálogo) fica no ANIGO; aqui a paleta canônica é sempre
//! a identidade de 24 ossos, igual ao ANIGO enquanto `PROPORTIONS_BAKED_INTO_BASE_MESH` for `true`.

use serde::{Deserialize, Serialize};

/// Ossos do esqueleto canônico do ANIGO (VRM 1.0, `BondSyncManager`).
pub const CANONICAL_JOINT_COUNT: usize = 24;

/// Skinning entregue junto da geometria estática (P1-04).
///
/// A paleta é `world × inverse bind` por osso, 16 floats por osso (ordem de
/// colunas), pronta para o bloco `bones` do contrato. A malha carrega até 4
/// influências por vértice nos atributos 4 e 5 do layout de 72 B.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkinPayload {
    /// Ossos na paleta (24 no esqueleto canônico).
    pub bone_count: u32,
    /// Matrizes de skinning: `bone_count * 16` floats.
    pub palette: Vec<f32>,
    /// Nome da pose de bind (`canonical_rest`).
    pub bind_pose: String,
    /// As proporções estão assadas na malha base (o bake está ligado).
    pub proportions_baked: bool,
    /// A paleta entregue é a identidade — skinning não deforma duas vezes.
    pub palette_is_identity: bool,
}

impl SkinPayload {
    /// Paleta canônica (identidade de 24 ossos).
    pub fn canonical_base() -> Self {
        Self {
            bone_count: CANONICAL_JOINT_COUNT as u32,
            palette: identity_palette_floats(CANONICAL_JOINT_COUNT),
            bind_pose: "canonical_rest".to_string(),
            proportions_baked: true,
            palette_is_identity: true,
        }
    }
}

/// Paleta identidade (`anigo-core/src/skinning.rs`).
pub fn identity_palette_floats(bone_count: usize) -> Vec<f32> {
    let mut palette = Vec::with_capacity(bone_count * 16);
    for _ in 0..bone_count {
        palette.extend_from_slice(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
    }
    palette
}

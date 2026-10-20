//! Hierarquia de cena (Fase 1, issue #12): árvore de transformações
//! parent-child com propagação matricial, tipos de entidade e rejeição de
//! ciclos.
//!
//! Por que existe um módulo só para isto: numa cena de anime, acessórios
//! **seguem** partes do personagem (laço na cabeça, espada na mão, jaqueta no
//! tronco). Com uma lista plana de nós, cada um calculando apenas a própria
//! transformação local, essa vinculação é impossível de expressar. A regra é a
//! mesma de qualquer motor: a transformação mundial é a composição da cadeia
//! até a raiz,
//!
//! ```text
//! W(child) = W(parent) × T(local)
//! ```
//!
//! e a ordem de avaliação tem de ser topológica (pai antes de filho). O
//! algoritmo é o de Kahn sobre os índices dos nós, com ordem determinística
//! (ordem de declaração), o que torna o resultado reproduzível — requisito de
//! contrato: dois renderizadores (headless e viewport) têm de chegar à mesma
//! matriz.
//!
//! Um `parent_id` que aponte para um nó inexistente, para si mesmo ou que feche
//! um ciclo é **erro de dados**, não um caso a tratar silenciosamente: o
//! projeto não valida e o comando é revertido (nada de cena corrompida).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use glam::Mat4;
use serde::{Deserialize, Serialize};
use thiserror::Error;


use crate::math::Transform;


/// Tipo especializado de um nó da cena (issue #12).
///
/// O tipo é *dado* do núcleo (serializado no `ProjectState`), não uma etiqueta
/// de UI: é ele que diz se o nó é raiz de personagem, osso do esqueleto, peça de
/// vestuário (que segue a armature), cabelo, acessório, luz ou câmera. A
/// contraparte TypeScript espelha exatamente estes nomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Raiz do personagem: dono da malha deformada e do material canônico.
    CharacterRoot,
    /// Osso do esqueleto humanoide (o vértice segue a paleta de skinning).
    HumanoidBone,
    /// Peça de vestuário — acompanha os ossos do corpo.
    Clothing,
    /// Mechas/cabelo — acompanham a cabeça e possuem dinâmica secundária.
    Hair,
    /// Acessório preso a um osso ou nó (laço, óculos, espada).
    Accessory,
    /// Malha solta/estática: cenário, props, blocagem.
    #[default]
    Mesh,
    /// Luz de cena.
    Light,
    /// Câmera de cena.
    Camera,
    /// Agrupador sem geometria própria (nó intermediário de organização).
    Group,
}

impl NodeKind {
    /// Todos os tipos, em ordem canônica (telemetria, UI e testes).
    pub const ALL: [Self; 9] = [
        Self::CharacterRoot,
        Self::HumanoidBone,
        Self::Clothing,
        Self::Hair,
        Self::Accessory,
        Self::Mesh,
        Self::Light,
        Self::Camera,
        Self::Group,
    ];

    /// Nome canônico em `snake_case` (o mesmo do JSON e do TypeScript).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CharacterRoot => "character_root",
            Self::HumanoidBone => "humanoid_bone",
            Self::Clothing => "clothing",
            Self::Hair => "hair",
            Self::Accessory => "accessory",
            Self::Mesh => "mesh",
            Self::Light => "light",
            Self::Camera => "camera",
            Self::Group => "group",
        }
    }

    /// Parse tolerante (case-insensitive, `-`/espaço aceitos como `_`).
    pub fn parse(value: &str) -> Option<Self> {
        // `&[char; N]` implementa `Pattern`; o array por valor, não.
        let normalized = value.trim().to_ascii_lowercase().replace(&['-', ' '], "_");
        Self::ALL.into_iter().find(|kind| kind.as_str() == normalized)
    }

    /// `true` quando o nó é vestido/preso ao corpo (acompanha a armature e
    /// participa da deformação secundária).
    pub fn is_attachment(self) -> bool {
        matches!(self, Self::Clothing | Self::Hair | Self::Accessory)
    }

    /// `true` quando o nó carrega geometria própria (relevante para culling e
    /// para o passe de pele/cabelo).
    pub fn carries_geometry(self) -> bool {
        matches!(
            self,
            Self::CharacterRoot | Self::HumanoidBone | Self::Clothing | Self::Hair | Self::Accessory | Self::Mesh
        )
    }
}

/// Falha de estrutura da árvore de cena (issue #12).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HierarchyError {
    /// Dois nós declaram o mesmo id.
    #[error("duplicated scene node id '{0}'")]
    DuplicateNodeId(String),
    /// O nó citado não existe na cena.
    #[error("unknown scene node '{node}'")]
    UnknownNode { node: String },
    /// `parent_id` aponta para o próprio nó.
    #[error("node '{node}' cannot be its own parent")]
    SelfParent { node: String },
    /// `parent_id` aponta para um nó que não existe na cena.
    #[error("node '{node}' references unknown parent '{parent}'")]
    UnknownParent { node: String, parent: String },
    /// A cadeia de pais fecha um ciclo (`a → b → c → a`).
    #[error("cyclic scene hierarchy: {path}")]
    Cycle { path: String },
}

/// Resolve as transformações mundiais de uma lista de nós **por índice**.
///
/// `ids[i]`, `parents[i]` e `locals[i]` descrevem o nó `i`; o resultado é
/// `world[i] = world[parent] × locals[i]` (raízes usam só `locals[i]`).
///
/// Uma implementação, dois consumidores: `Scene` (nós do renderer, ids
/// `String`) e a lista de `NodeSlot` do `ProjectState` (ids tipados). Duplicar
/// o algoritmo seria duplicar a chance de divergir.
pub fn resolve_indexed(
    ids: &[&str],
    parents: &[Option<&str>],
    locals: &[Mat4],
) -> Result<Vec<Mat4>, HierarchyError> {
    if ids.len() != parents.len() || ids.len() != locals.len() {
        return Err(HierarchyError::DuplicateNodeId(format!(
            "resolve_indexed: {} ids, {} parents, {} transforms",
            ids.len(),
            parents.len(),
            locals.len()
        )));
    }

    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, id) in ids.iter().enumerate() {
        if index_of.insert(id, index).is_some() {
            return Err(HierarchyError::DuplicateNodeId((*id).to_string()));
        }
    }

    // Pai resolvido para índice (ou raiz) + validações locais.
    let mut parent_index: Vec<Option<usize>> = vec![None; ids.len()];
    for (index, parent) in parents.iter().enumerate() {
        let Some(parent) = parent else { continue };
        if *parent == ids[index] {
            return Err(HierarchyError::SelfParent {
                node: ids[index].to_string(),
            });
        }
        let resolved = index_of
            .get(parent)
            .copied()
            .ok_or_else(|| HierarchyError::UnknownParent {
                node: ids[index].to_string(),
                parent: (*parent).to_string(),
            })?;
        parent_index[index] = Some(resolved);
    }

    // Filhos em ordem de declaração (determinismo do resultado).
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    for (index, parent) in parent_index.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(index);
        }
    }

    // Kahn: raízes primeiro (ordem de declaração), depois os filhos.
    let mut world: Vec<Option<Mat4>> = vec![None; ids.len()];
    let mut queue: VecDeque<usize> = (0..ids.len())
        .filter(|index| parent_index[*index].is_none())
        .collect();
    let mut resolved = 0usize;
    while let Some(index) = queue.pop_front() {
        let parent_world = parent_index[index]
            .and_then(|parent| world[parent])
            .unwrap_or(Mat4::IDENTITY);
        world[index] = Some(parent_world * locals[index]);
        resolved += 1;
        for child in &children[index] {
            queue.push_back(*child);
        }
    }

    if resolved != ids.len() {
        // Sobrou nó sem pai alcançado ⇒ existe ciclo: percorre a cadeia para
        // nomear o caminho no diagnóstico (nunca um "erro desconhecido").
        let start = world
            .iter()
            .position(|matrix| matrix.is_none())
            .unwrap_or_default();
        let mut path: Vec<&str> = Vec::new();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut cursor = Some(start);
        while let Some(index) = cursor {
            if !seen.insert(index) {
                path.push(ids[index]);
                break;
            }
            path.push(ids[index]);
            cursor = parent_index[index];
            if path.len() > ids.len() {
                break;
            }
        }
        path.reverse();
        return Err(HierarchyError::Cycle {
            path: path.join(" -> "),
        });
    }

    Ok(world
        .into_iter()
        .map(|matrix| matrix.unwrap_or(Mat4::IDENTITY))
        .collect())
}
/// Transformação local de um `Transform` (atalho com documentação de intenção).
pub fn local_matrix(transform: Transform) -> Mat4 {
    transform.to_matrix()
}


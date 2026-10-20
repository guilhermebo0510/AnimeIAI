//! Modelo 3D normalizado: lista de partes (uma por primitiva de malha), já com as transformações dos nós aplicadas, em
//! triângulos indexados, com normais e uma cor base por parte. É o que o importador produz, o que vai para o blob do
//! projeto (como GLB, ver [`crate::glb`]) e o que os renderizadores (CPU e GPU) consomem.

use serde::{Deserialize, Serialize};

/// Uma parte do modelo: geometria em espaço do modelo e cor base **linear** (convenção do glTF).
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Cor base linear RGBA (fator do material; texturas não são lidas na v1).
    pub color: [f32; 4],
    /// Nome do material de origem (informativo).
    pub material: String,
}

impl Part {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Licença e autoria declaradas no arquivo (glTF `asset.copyright`/`asset.extras`, metadados do OBJ). Guardadas e
/// mostradas como vieram; vazias quando o arquivo não diz nada (o que também é informação: origem desconhecida).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LicenseInfo {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub copyright: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

impl LicenseInfo {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.author.is_empty() && self.license.is_empty() && self.copyright.is_empty() && self.source.is_empty()
    }

    /// Texto curto para o campo `source` do ativo e para a interface.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.license.is_empty() {
            parts.push(format!("licença: {}", self.license));
        }
        if !self.author.is_empty() {
            parts.push(format!("autor: {}", self.author));
        }
        if !self.copyright.is_empty() {
            parts.push(format!("© {}", self.copyright));
        }
        if !self.source.is_empty() {
            parts.push(format!("origem: {}", self.source));
        }
        if parts.is_empty() { "licença não declarada no arquivo".into() } else { parts.join("; ") }
    }
}

/// Informações do modelo (vão para `params.model3d` do nó `Asset` e para a aba 3D das Propriedades).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelInfo {
    /// Formato do arquivo de origem: `glb`, `gltf` ou `obj`.
    pub format: String,
    /// Nome do arquivo de origem (sem pasta).
    #[serde(default)]
    pub file_name: String,
    pub triangles: usize,
    pub vertices: usize,
    pub parts: usize,
    pub materials: usize,
    /// Caixa envolvente em espaço do modelo: [mín, máx].
    pub bbox: [[f32; 3]; 2],
    #[serde(default)]
    pub license: LicenseInfo,
    /// Avisos da importação (pele/animação ignoradas, primitiva não triangular pulada...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Ossos do esqueleto (0 = modelo rígido, sem pose).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bones: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Osso do esqueleto do modelo: nome, pai e o repouso no espaço do modelo (posição da junta e rotação global).
#[derive(Debug, Clone, PartialEq)]
pub struct SkinBone {
    pub name: String,
    pub parent: Option<usize>,
    pub rest_pos: [f32; 3],
    /// Rotação global de repouso `[x, y, z, w]` (identidade no Manequim).
    pub rest_rot: [f32; 4],
}

/// Influências de uma parte: até 4 ossos por vértice (índices em [`Skin::bones`]) e os pesos (somam 1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartSkin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// Pele do modelo (S24 3D, fase 8): esqueleto em ordem de pai antes de filho e as influências por parte (mesma ordem
/// de [`Model::parts`]). As posições das partes estão no repouso; a pose deforma por skinning linear na CPU
/// ([`crate::pose`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skin {
    pub bones: Vec<SkinBone>,
    pub parts: Vec<PartSkin>,
}

impl Skin {
    pub fn index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    /// Esqueleto bem formado: pais antes dos filhos, influências com o tamanho das partes e índices válidos.
    pub fn validate(&self, parts: &[Part]) -> Result<(), String> {
        for (i, b) in self.bones.iter().enumerate() {
            if b.parent.is_some_and(|p| p >= i) {
                return Err(format!("osso '{}' vem antes do pai", b.name));
            }
        }
        if self.parts.len() != parts.len() {
            return Err(format!("pele com {} partes para {} partes do modelo", self.parts.len(), parts.len()));
        }
        for (ps, p) in self.parts.iter().zip(parts) {
            if ps.joints.len() != p.positions.len() || ps.weights.len() != p.positions.len() {
                return Err(format!("parte '{}': influências fora do tamanho", p.name));
            }
            if ps.joints.iter().flatten().any(|&j| j as usize >= self.bones.len()) {
                return Err(format!("parte '{}': osso inexistente nas influências", p.name));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parts: Vec<Part>,
    pub info: ModelInfo,
    /// Esqueleto e pesos (Manequim, modelos importados com pele). `None` = modelo rígido.
    pub skin: Option<Skin>,
}

impl Model {
    /// Recalcula contagens e caixa a partir das partes.
    pub fn refresh_info(&mut self) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut verts = 0;
        let mut tris = 0;
        let mut mats = std::collections::BTreeSet::new();
        for p in &self.parts {
            verts += p.positions.len();
            tris += p.triangles();
            mats.insert(p.material.clone());
            for v in &p.positions {
                for i in 0..3 {
                    lo[i] = lo[i].min(v[i]);
                    hi[i] = hi[i].max(v[i]);
                }
            }
        }
        if verts == 0 {
            lo = [0.0; 3];
            hi = [0.0; 3];
        }
        self.info.vertices = verts;
        self.info.triangles = tris;
        self.info.parts = self.parts.len();
        self.info.materials = mats.len();
        self.info.bbox = [lo, hi];
        self.info.bones = self.skin.as_ref().map(|s| s.bones.len()).unwrap_or(0);
    }

    pub fn center(&self) -> [f32; 3] {
        let [lo, hi] = self.info.bbox;
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]
    }

    /// Raio da esfera que contém a caixa.
    pub fn radius(&self) -> f32 {
        let [lo, hi] = self.info.bbox;
        let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 0.5).max(1e-3)
    }
}

/// Normais suaves por vértice (média ponderada por área das faces) para partes sem normais.
pub fn smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut acc = vec![[0.0f32; 3]; positions.len()];
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        if a >= positions.len() || b >= positions.len() || c >= positions.len() {
            continue;
        }
        let (pa, pb, pc) = (positions[a], positions[b], positions[c]);
        let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in [a, b, c] {
            for k in 0..3 {
                acc[i][k] += n[k];
            }
        }
    }
    acc.into_iter()
        .map(|n| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 1e-12 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 1.0, 0.0] }
        })
        .collect()
}

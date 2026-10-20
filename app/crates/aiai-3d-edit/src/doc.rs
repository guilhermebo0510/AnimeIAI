//! Documento da Modelagem (S24 3D, fase 12): objetos (malha editável `EditMesh` em espaço local, transformação, cor toon,
//! sombreamento, pilha de modificadores), as operações que a interface e a MCP usam (primitivas, extrude, inset, loop
//! cut, bevel, merge, dissolve, apagar, transformar elementos) e a conversão para o [`Model`] normalizado (o que vira GLB
//! na biblioteca e na camada 3D). O histórico (desfazer/refazer) guarda o documento inteiro por passo: os objetos de
//! adereço são pequenos e a cópia é barata.
//!
//! Convenções: metros, Y para cima, enrolamento anti-horário visto de fora (as do ANIGO e do glTF).

use crate::error::{ModelError, ModelResult};
use crate::mesh::{EdgeKey, EditMesh};
use crate::modifiers::{array, mirror};
use crate::normals::{compute_corner_normals, NormalParams};
use crate::ops::{self, OpOutput};
use crate::primitives;
use crate::select::{SelectMode, Selection};
use crate::subdiv;
use crate::topology::Topology;
use aiai_3d_model::camera::{model_matrix, rotation_quat};
use aiai_3d_model::model::{LicenseInfo, Model, ModelInfo, Part};
use aiai_core::model3d::Transform3d;
use glam::{Mat3, Mat4, Vec3};
use serde::{Deserialize, Serialize};

/// Versão do esquema do documento (guardado em `params.modeling` do ativo 3D).
pub const DOC_VERSION: u32 = 1;
/// Teto de objetos por documento e de faces avaliadas por objeto (a Modelagem é para adereços e blocagem).
pub const MAX_OBJECTS: usize = 256;
pub const MAX_EVAL_FACES: usize = 400_000;

/// Primitivas da v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    Cube,
    Sphere,
    Cylinder,
    Cone,
    Plane,
    Torus,
}

impl Primitive {
    pub const ALL: [Primitive; 6] = [Primitive::Cube, Primitive::Sphere, Primitive::Cylinder, Primitive::Cone, Primitive::Plane, Primitive::Torus];

    pub fn key(self) -> &'static str {
        match self {
            Primitive::Cube => "cube",
            Primitive::Sphere => "sphere",
            Primitive::Cylinder => "cylinder",
            Primitive::Cone => "cone",
            Primitive::Plane => "plane",
            Primitive::Torus => "torus",
        }
    }

    pub fn from_key(k: &str) -> Option<Primitive> {
        Primitive::ALL.into_iter().find(|p| p.key() == k)
    }

    /// Malha da primitiva com tamanho `size` (aresta do cubo, diâmetro da esfera, altura do cilindro...), centrada na
    /// origem (o plano fica em Y = 0).
    pub fn mesh(self, size: f32) -> EditMesh {
        let s = if size.is_finite() && size > 1e-4 { size } else { 1.0 };
        let mut m = match self {
            Primitive::Cube => primitives::cube(s),
            Primitive::Sphere => primitives::uv_sphere(s * 0.5, 12, 24),
            Primitive::Cylinder => primitives::cylinder(s * 0.5, s, 24, 1, true),
            Primitive::Cone => primitives::cone(s * 0.5, 0.0, s, 24, true),
            Primitive::Plane => primitives::plane(s),
            Primitive::Torus => primitives::torus(s * 0.36, s * 0.14, 32, 12),
        };
        // centra na origem (o cilindro e o cone do ANIGO podem nascer apoiados em Y = 0)
        if self != Primitive::Plane {
            if let Some((lo, hi)) = m.bounds() {
                let c = (lo + hi) * 0.5;
                if c.length() > 1e-6 {
                    m.transform_positions(|p| p - c);
                }
            }
        }
        m
    }
}

/// Sombreamento do objeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Shading {
    /// Liso com arestas vivas acima de 30° (o melhor para cel shading: cubo chapado, esfera lisa).
    #[default]
    Auto,
    Flat,
    Smooth,
}

impl Shading {
    pub const ALL: [Shading; 3] = [Shading::Auto, Shading::Flat, Shading::Smooth];

    pub fn key(self) -> &'static str {
        match self {
            Shading::Auto => "auto",
            Shading::Flat => "flat",
            Shading::Smooth => "smooth",
        }
    }
}

/// Modificador da pilha (aplicados em ordem, sem mudar a malha editável).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModifierSpec {
    /// Espelho nos eixos marcados (no espaço do objeto, plano pela origem), soldando no plano.
    Mirror {
        axes: [bool; 3],
        #[serde(default = "yes")]
        merge: bool,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Subdivisão Catmull-Clark (níveis 1 a 3).
    Subsurf {
        levels: u32,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Cópias em fila: `count` cópias deslocadas por `offset` (fração do tamanho da malha em cada eixo).
    Array {
        count: u32,
        offset: [f32; 3],
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Casca com espessura (`offset` -1..1: onde fica a superfície original), fechando as bordas.
    Solidify {
        thickness: f32,
        #[serde(default = "minus_one")]
        offset: f32,
        #[serde(default = "yes")]
        even: bool,
        #[serde(default = "yes")]
        rim: bool,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Solda vértices mais próximos que `distance`.
    Weld {
        distance: f32,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Separa as arestas mais agudas que `angle_deg` (e as marcadas como duras): quina dura no cel shading.
    EdgeSplit {
        angle_deg: f32,
        #[serde(default = "yes")]
        sharp: bool,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Reduz as faces (colapso quádrico; `ratio` 0..1 das faces que ficam).
    Decimate {
        ratio: f32,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Desloca os vértices pela normal (ou eixo `axis` 0/1/2) com ruído (`noise_scale` > 0) ou constante.
    Displace {
        strength: f32,
        #[serde(default)]
        noise_scale: f32,
        #[serde(default)]
        seed: u32,
        #[serde(default)]
        axis: Option<u32>,
        #[serde(default = "yes")]
        enabled: bool,
    },
    /// Triangula quads e n-gons.
    Triangulate {
        #[serde(default = "yes")]
        enabled: bool,
    },
}

fn minus_one() -> f32 {
    -1.0
}

fn yes() -> bool {
    true
}

impl ModifierSpec {
    /// Chaves na ordem do menu "Adicionar".
    pub const KEYS: [&'static str; 9] = ["mirror", "subsurf", "array", "solidify", "weld", "edge_split", "decimate", "displace", "triangulate"];

    pub fn key(&self) -> &'static str {
        match self {
            ModifierSpec::Mirror { .. } => "mirror",
            ModifierSpec::Subsurf { .. } => "subsurf",
            ModifierSpec::Array { .. } => "array",
            ModifierSpec::Solidify { .. } => "solidify",
            ModifierSpec::Weld { .. } => "weld",
            ModifierSpec::EdgeSplit { .. } => "edge_split",
            ModifierSpec::Decimate { .. } => "decimate",
            ModifierSpec::Displace { .. } => "displace",
            ModifierSpec::Triangulate { .. } => "triangulate",
        }
    }

    /// Modificador novo com os valores padrão.
    pub fn new(key: &str) -> Option<ModifierSpec> {
        Some(match key {
            "mirror" => ModifierSpec::Mirror { axes: [true, false, false], merge: true, enabled: true },
            "subsurf" => ModifierSpec::Subsurf { levels: 1, enabled: true },
            "array" => ModifierSpec::Array { count: 3, offset: [1.1, 0.0, 0.0], enabled: true },
            "solidify" => ModifierSpec::Solidify { thickness: 0.05, offset: -1.0, even: true, rim: true, enabled: true },
            "weld" => ModifierSpec::Weld { distance: 0.001, enabled: true },
            "edge_split" => ModifierSpec::EdgeSplit { angle_deg: 30.0, sharp: true, enabled: true },
            "decimate" => ModifierSpec::Decimate { ratio: 0.5, enabled: true },
            "displace" => ModifierSpec::Displace { strength: 0.1, noise_scale: 0.5, seed: 1, axis: None, enabled: true },
            "triangulate" => ModifierSpec::Triangulate { enabled: true },
            _ => return None,
        })
    }

    fn enabled_mut(&mut self) -> &mut bool {
        match self {
            ModifierSpec::Mirror { enabled, .. }
            | ModifierSpec::Subsurf { enabled, .. }
            | ModifierSpec::Array { enabled, .. }
            | ModifierSpec::Solidify { enabled, .. }
            | ModifierSpec::Weld { enabled, .. }
            | ModifierSpec::EdgeSplit { enabled, .. }
            | ModifierSpec::Decimate { enabled, .. }
            | ModifierSpec::Displace { enabled, .. }
            | ModifierSpec::Triangulate { enabled } => enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            ModifierSpec::Mirror { enabled, .. }
            | ModifierSpec::Subsurf { enabled, .. }
            | ModifierSpec::Array { enabled, .. }
            | ModifierSpec::Solidify { enabled, .. }
            | ModifierSpec::Weld { enabled, .. }
            | ModifierSpec::EdgeSplit { enabled, .. }
            | ModifierSpec::Decimate { enabled, .. }
            | ModifierSpec::Displace { enabled, .. }
            | ModifierSpec::Triangulate { enabled } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        *self.enabled_mut() = on;
    }

    pub fn validate(&self) -> Result<(), String> {
        let fin = |x: f32, lim: f32| x.is_finite() && x.abs() <= lim;
        match self {
            ModifierSpec::Mirror { .. } | ModifierSpec::Triangulate { .. } => Ok(()),
            ModifierSpec::Subsurf { levels, .. } if (1..=3).contains(levels) => Ok(()),
            ModifierSpec::Subsurf { levels, .. } => Err(format!("subdivisão: níveis {levels} fora de 1..3")),
            ModifierSpec::Array { count, offset, .. } => {
                if !(1..=100).contains(count) {
                    return Err(format!("array: {count} cópias fora de 1..100"));
                }
                if !offset.iter().all(|x| fin(*x, 100.0)) {
                    return Err("array: deslocamento inválido".into());
                }
                Ok(())
            }
            ModifierSpec::Solidify { thickness, offset, .. } => {
                if !fin(*thickness, 100.0) || thickness.abs() < 1e-5 {
                    return Err("solidify: espessura tem de ser diferente de zero e até ±100".into());
                }
                if !(offset.is_finite() && (-1.0..=1.0).contains(offset)) {
                    return Err("solidify: deslocamento fora de -1..1".into());
                }
                Ok(())
            }
            ModifierSpec::Weld { distance, .. } if distance.is_finite() && (0.0..=10.0).contains(distance) => Ok(()),
            ModifierSpec::Weld { .. } => Err("weld: distância fora de 0..10".into()),
            ModifierSpec::EdgeSplit { angle_deg, .. } if angle_deg.is_finite() && (0.0..=180.0).contains(angle_deg) => Ok(()),
            ModifierSpec::EdgeSplit { .. } => Err("edge split: ângulo fora de 0..180°".into()),
            ModifierSpec::Decimate { ratio, .. } if ratio.is_finite() && *ratio > 0.0 && *ratio <= 1.0 => Ok(()),
            ModifierSpec::Decimate { .. } => Err("decimate: proporção fora de (0, 1]".into()),
            ModifierSpec::Displace { strength, noise_scale, axis, .. } => {
                if !fin(*strength, 100.0) || !noise_scale.is_finite() || *noise_scale < 0.0 || *noise_scale > 1000.0 {
                    return Err("displace: intensidade ou escala do ruído inválida".into());
                }
                if axis.is_some_and(|a| a > 2) {
                    return Err("displace: eixo 0, 1 ou 2".into());
                }
                Ok(())
            }
        }
    }

    fn apply(&self, m: &EditMesh) -> ModelResult<EditMesh> {
        use crate::modifiers::{decimate, displace, edge_split, solidify, weld};
        match self {
            ModifierSpec::Mirror { axes, merge, .. } => mirror::mirror(m, &mirror::MirrorParams { axes: *axes, merge: *merge, ..Default::default() }),
            ModifierSpec::Subsurf { levels, .. } => subdiv::catmull_clark(m, &subdiv::SubdivParams { levels: (*levels).min(3), ..Default::default() }),
            ModifierSpec::Array { count, offset, .. } => array::array(m, &array::ArrayParams { count: (*count).clamp(1, 100), relative_offset: Vec3::from(*offset), ..Default::default() }),
            ModifierSpec::Solidify { thickness, offset, even, rim, .. } => {
                solidify::solidify(m, &solidify::SolidifyParams { thickness: *thickness, offset: *offset, even_thickness: *even, rim_fill: *rim, ..Default::default() })
            }
            ModifierSpec::Weld { distance, .. } => weld::weld(m, *distance, false),
            ModifierSpec::EdgeSplit { angle_deg, sharp, .. } => edge_split::edge_split(m, Some(angle_deg.to_radians()), *sharp),
            ModifierSpec::Decimate { ratio, .. } => decimate::decimate(m, &decimate::DecimateSettings::Collapse { ratio: *ratio, params: Default::default() }),
            ModifierSpec::Displace { strength, noise_scale, seed, axis, .. } => {
                let direction = match axis {
                    Some(0) => displace::DisplaceDirection::X,
                    Some(1) => displace::DisplaceDirection::Y,
                    Some(_) => displace::DisplaceDirection::Z,
                    None => displace::DisplaceDirection::Normal,
                };
                let texture = if *noise_scale > 0.0 { displace::DisplaceSource::Noise { scale: *noise_scale, seed: *seed } } else { displace::DisplaceSource::Constant };
                displace::displace(m, &displace::DisplaceParams { strength: *strength, direction, texture, ..Default::default() })
            }
            ModifierSpec::Triangulate { .. } => {
                let mut out = m.clone();
                let faces: Vec<u32> = (0..out.face_count() as u32).filter(|f| out.faces[*f as usize].corners.len() > 3).collect();
                if !faces.is_empty() {
                    crate::ops::triangulate::triangulate_faces(&mut out, &faces, crate::ops::triangulate::QuadMethod::Beauty, crate::ops::triangulate::NgonMethod::Beauty)?;
                }
                Ok(out)
            }
        }
    }
}

/// Um objeto do documento.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object3d {
    pub name: String,
    pub mesh: EditMesh,
    #[serde(default)]
    pub transform: Transform3d,
    /// Cor toon (sRGB 0..1).
    pub color: [f32; 3],
    #[serde(default)]
    pub shading: Shading,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<ModifierSpec>,
    #[serde(default = "yes")]
    pub visible: bool,
}

impl Object3d {
    pub fn new(name: &str, mesh: EditMesh, color: [f32; 3]) -> Object3d {
        Object3d { name: name.to_string(), mesh, transform: Transform3d::default(), color, shading: Shading::Auto, modifiers: Vec::new(), visible: true }
    }

    pub fn matrix(&self) -> Mat4 {
        model_matrix(&self.transform)
    }

    /// Malha com os modificadores ligados aplicados (espaço do objeto).
    pub fn evaluated(&self) -> ModelResult<EditMesh> {
        let mut m = self.mesh.clone();
        for md in self.modifiers.iter().filter(|m| m.enabled()) {
            md.validate().map_err(ModelError::InvalidParameter)?;
            m = md.apply(&m)?;
            if m.face_count() > MAX_EVAL_FACES {
                return Err(ModelError::InvalidParameter(format!("o resultado dos modificadores passa de {MAX_EVAL_FACES} faces")));
            }
        }
        Ok(m)
    }
}

/// Paleta das cores iniciais dos objetos novos (sRGB): tons claros que leem bem como referência.
pub const COLORS: [[f32; 3]; 6] = [
    [0.80, 0.80, 0.84],
    [0.86, 0.66, 0.52],
    [0.58, 0.72, 0.86],
    [0.72, 0.82, 0.60],
    [0.88, 0.78, 0.50],
    [0.80, 0.62, 0.74],
];

/// O documento da Modelagem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDoc {
    #[serde(default = "doc_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub objects: Vec<Object3d>,
}

fn doc_version() -> u32 {
    DOC_VERSION
}

impl Default for ModelDoc {
    fn default() -> Self {
        ModelDoc { version: DOC_VERSION, name: "Modelo".into(), objects: Vec::new() }
    }
}

fn op_err(e: ModelError) -> String {
    e.to_string()
}

impl ModelDoc {
    pub fn new(name: &str) -> ModelDoc {
        ModelDoc { name: name.to_string(), ..Default::default() }
    }

    /// Lê e confere (versão, limites, malhas válidas, modificadores).
    pub fn from_json(v: &serde_json::Value) -> Result<ModelDoc, String> {
        let d: ModelDoc = serde_json::from_value(v.clone()).map_err(|e| format!("documento de modelagem inválido: {e}"))?;
        d.validate()?;
        Ok(d)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != DOC_VERSION {
            return Err(format!("documento de modelagem versão {} (este app entende a {DOC_VERSION})", self.version));
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err(format!("mais de {MAX_OBJECTS} objetos"));
        }
        for o in &self.objects {
            o.mesh.validate().map_err(|e| format!("objeto '{}': {e}", o.name))?;
            for m in &o.modifiers {
                m.validate().map_err(|e| format!("objeto '{}': {e}", o.name))?;
            }
            let t = &o.transform;
            if !t.pos.iter().chain(&t.rot_deg).chain(&t.scale).all(|x| x.is_finite()) || t.scale.iter().any(|s| s.abs() < 1e-4) {
                return Err(format!("objeto '{}': transformação inválida", o.name));
            }
            if !o.color.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)) {
                return Err(format!("objeto '{}': cor fora de 0..1", o.name));
            }
        }
        Ok(())
    }

    /// Nome livre para um objeto novo ("Cubo", "Cubo.001"...).
    pub fn unique_name(&self, base: &str) -> String {
        if !self.objects.iter().any(|o| o.name == base) {
            return base.to_string();
        }
        (1..).map(|i| format!("{base}.{i:03}")).find(|n| !self.objects.iter().any(|o| &o.name == n)).unwrap_or_else(|| base.to_string())
    }

    /// Objeto por índice ou nome.
    pub fn find(&self, key: &str) -> Option<usize> {
        key.parse::<usize>().ok().filter(|i| *i < self.objects.len()).or_else(|| self.objects.iter().position(|o| o.name == key))
    }

    /// Acrescenta uma primitiva em `pos` (devolve o índice do objeto).
    pub fn add_primitive(&mut self, kind: Primitive, size: f32, pos: [f32; 3], name: Option<&str>) -> Result<usize, String> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(format!("limite de {MAX_OBJECTS} objetos"));
        }
        let base = name.map(str::to_string).unwrap_or_else(|| default_name(kind).to_string());
        let mut o = Object3d::new(&self.unique_name(&base), kind.mesh(size), COLORS[self.objects.len() % COLORS.len()]);
        o.transform.pos = pos;
        if kind == Primitive::Sphere || kind == Primitive::Torus {
            o.shading = Shading::Smooth;
        }
        self.objects.push(o);
        Ok(self.objects.len() - 1)
    }

    /// Modelo normalizado: uma parte por objeto visível, na posição do mundo, com a cor do objeto.
    pub fn to_model(&self) -> Result<Model, String> {
        let mut parts = Vec::new();
        for o in self.objects.iter().filter(|o| o.visible) {
            let m = o.evaluated().map_err(|e| format!("objeto '{}': {e}", o.name))?;
            if m.face_count() == 0 {
                continue;
            }
            parts.push(object_part(o, &m));
        }
        if parts.is_empty() {
            return Err("nada para salvar: o documento não tem objetos visíveis com faces".into());
        }
        let mut model = Model {
            parts,
            info: ModelInfo {
                format: "aiai".into(),
                file_name: format!("{}.glb", self.name),
                license: LicenseInfo { title: self.name.clone(), license: "modelado no AnimeIAI".into(), source: "Modelagem do AnimeIAI".into(), ..Default::default() },
                ..Default::default()
            },
            skin: None,
        };
        model.refresh_info();
        Ok(model)
    }

    /// GLB normalizado do documento (o que vai para o blob do ativo e para `Documentos\AnimeIAI\Modelos 3D`).
    pub fn to_glb(&self) -> Result<Vec<u8>, String> {
        Ok(aiai_3d_model::glb::write_glb(&self.to_model()?))
    }

    /// Assinatura do conteúdo (chave de cache da prévia).
    pub fn content_hash(&self) -> aiai_core::BlobHash {
        aiai_core::BlobHash::of(serde_json::to_string(self).unwrap_or_default().as_bytes())
    }
}

fn default_name(kind: Primitive) -> &'static str {
    match kind {
        Primitive::Cube => "Cubo",
        Primitive::Sphere => "Esfera",
        Primitive::Cylinder => "Cilindro",
        Primitive::Cone => "Cone",
        Primitive::Plane => "Plano",
        Primitive::Torus => "Toro",
    }
}

/// Parte do modelo de um objeto avaliado: triângulos com vértices por canto (normal do sombreamento escolhido), no mundo.
pub fn object_part(o: &Object3d, m: &EditMesh) -> Part {
    part_with_matrix(o, m, o.matrix())
}

/// Como [`object_part`], no espaço do objeto (o viewport da Modelagem põe a transformação no nó da cena).
pub fn object_part_local(o: &Object3d, m: &EditMesh) -> Part {
    part_with_matrix(o, m, if o.matrix().determinant() < 0.0 { o.matrix() } else { Mat4::IDENTITY })
}

/// Normais do sombreamento escolhido.
pub fn normal_params(s: Shading) -> NormalParams {
    match s {
        Shading::Auto => NormalParams::default(),
        Shading::Flat => NormalParams { auto_smooth_angle: Some(0.0), use_custom: false, ..NormalParams::default() },
        Shading::Smooth => NormalParams { auto_smooth_angle: None, use_custom: false, ..NormalParams::default() },
    }
}

fn part_with_matrix(o: &Object3d, m: &EditMesh, mat: Mat4) -> Part {
    let params = normal_params(o.shading);
    let mut mesh = m.clone();
    if o.shading == Shading::Smooth {
        for f in &mut mesh.faces {
            f.smooth = true;
        }
    }
    let cn = compute_corner_normals(&mesh, &params);
    let nm = Mat3::from_mat4(mat).inverse().transpose();
    let flip = mat.determinant() < 0.0;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        let pts: Vec<Vec3> = f.corners.iter().map(|c| mesh.co(c.vert)).collect();
        let base = positions.len() as u32;
        for (ci, p) in pts.iter().enumerate() {
            positions.push(mat.transform_point3(*p).to_array());
            let n = cn.get(fi).and_then(|v| v.get(ci)).copied().unwrap_or(Vec3::Y);
            normals.push((nm * n).normalize_or(Vec3::Y).to_array());
        }
        for t in triangulate_polygon(&pts) {
            if flip {
                indices.extend([base + t[0] as u32, base + t[2] as u32, base + t[1] as u32]);
            } else {
                indices.extend([base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]);
            }
        }
    }
    let lin = o.color.map(aiai_3d_model::raster::srgb_to_linear);
    Part { name: o.name.clone(), positions, normals, indices, color: [lin[0], lin[1], lin[2], 1.0], material: o.name.clone() }
}

/// Triangulação de polígono do `convert.rs` do ANIGO (ear clipping no plano de Newell).
pub use crate::convert::triangulate_polygon;

// ------------------------------------------------------------------------------------------- operações de elementos

/// Faces cuja normal (espaço do objeto) aponta para `dir` (cosseno ≥ `min_cos`): "as faces de cima" = `+Y`.
pub fn faces_facing(m: &EditMesh, dir: Vec3, min_cos: f32) -> Vec<u32> {
    let d = dir.normalize_or(Vec3::Y);
    (0..m.face_count() as u32).filter(|&f| m.face_normal(f).dot(d) >= min_cos).collect()
}

/// Direção por nome (`+x`, `-y`, `top`...).
pub fn direction(name: &str) -> Option<Vec3> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "+x" | "x" | "right" => Vec3::X,
        "-x" | "left" => -Vec3::X,
        "+y" | "y" | "top" | "up" => Vec3::Y,
        "-y" | "bottom" | "down" => -Vec3::Y,
        "+z" | "z" | "front" => Vec3::Z,
        "-z" | "back" => -Vec3::Z,
        _ => return None,
    })
}

/// Todas as arestas da malha.
pub fn all_edges(m: &EditMesh) -> Vec<EdgeKey> {
    Topology::build(m).edges.iter().map(|e| e.verts).collect()
}

/// Vértices de uma seleção no modo dado (faces → os vértices delas; arestas → os extremos).
pub fn selection_verts(m: &EditMesh, sel: &Selection, mode: SelectMode) -> Vec<u32> {
    let mut out: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    match mode {
        SelectMode::Vertex => out.extend(sel.verts.iter().copied()),
        SelectMode::Edge => {
            for (a, b) in &sel.edges {
                out.insert(*a);
                out.insert(*b);
            }
        }
        SelectMode::Face => {
            for f in &sel.faces {
                if let Some(face) = m.faces.get(*f as usize) {
                    out.extend(face.verts());
                }
            }
        }
    }
    out.into_iter().filter(|v| (*v as usize) < m.vert_count()).collect()
}

/// Extrude das faces ao longo da normal média (região conectada).
pub fn extrude(m: &mut EditMesh, faces: &[u32], distance: f32) -> Result<OpOutput, String> {
    ops::extrude::extrude_faces_region_along_normal(m, faces, distance).map_err(op_err)
}

/// Inset da região de faces.
pub fn inset(m: &mut EditMesh, faces: &[u32], thickness: f32, depth: f32) -> Result<OpOutput, String> {
    ops::inset::inset_faces_region(m, faces, thickness, depth, false, true, true).map_err(op_err)
}

/// Loop cut no anel da aresta `edge` (`cuts` cortes, no meio).
pub fn loop_cut(m: &mut EditMesh, edge: EdgeKey, cuts: u32) -> Result<OpOutput, String> {
    ops::loops::loop_cut(m, edge, cuts.clamp(1, 32), 0.0).map(Into::into).map_err(op_err)
}

/// Bevel das arestas (`segments` segmentos, perfil redondo).
pub fn bevel(m: &mut EditMesh, edges: &[EdgeKey], width: f32, segments: u32) -> Result<OpOutput, String> {
    ops::bevel::bevel_edges(m, edges, width, segments.clamp(1, 16), 0.5, true).map_err(op_err)
}

/// Funde os vértices no centro.
pub fn merge_center(m: &mut EditMesh, verts: &[u32]) -> Result<OpOutput, String> {
    ops::merge::merge_verts(m, verts, ops::merge::MergeMode::Center).map_err(op_err)
}

/// Funde vértices próximos (remove duplicados) entre os dados, ou na malha toda.
pub fn merge_by_distance(m: &mut EditMesh, verts: Option<&[u32]>, distance: f32) -> Result<OpOutput, String> {
    ops::merge::merge_by_distance(m, verts, distance, Default::default()).map_err(op_err)
}

/// Dissolve no modo da seleção (vértices, arestas ou faces).
pub fn dissolve(m: &mut EditMesh, sel: &Selection, mode: SelectMode) -> Result<OpOutput, String> {
    match mode {
        SelectMode::Vertex => ops::dissolve::dissolve_verts(m, &sel.verts.iter().copied().collect::<Vec<_>>(), true, false),
        SelectMode::Edge => ops::dissolve::dissolve_edges(m, &sel.edges.iter().copied().collect::<Vec<_>>(), true),
        SelectMode::Face => ops::dissolve::dissolve_faces(m, &sel.faces.iter().copied().collect::<Vec<_>>()),
    }
    .map_err(op_err)
}

/// Apaga no modo da seleção.
pub fn delete(m: &mut EditMesh, sel: &Selection, mode: SelectMode) -> Result<OpOutput, String> {
    match mode {
        SelectMode::Vertex => ops::delete::delete_verts(m, &sel.verts.iter().copied().collect::<Vec<_>>()),
        SelectMode::Edge => ops::delete::delete_edges(m, &sel.edges.iter().copied().collect::<Vec<_>>()),
        SelectMode::Face => ops::delete::delete_faces(m, &sel.faces.iter().copied().collect::<Vec<_>>()),
    }
    .map_err(op_err)
}

/// Move vértices (espaço do objeto).
pub fn translate(m: &mut EditMesh, verts: &[u32], delta: Vec3) -> Result<OpOutput, String> {
    ops::transform::translate_verts(m, verts, delta).map_err(op_err)
}

/// Gira vértices em volta da média deles.
pub fn rotate(m: &mut EditMesh, verts: &[u32], axis: Vec3, angle: f32) -> Result<OpOutput, String> {
    ops::transform::rotate_verts(m, verts, ops::transform::Pivot::MedianPoint, axis, angle).map_err(op_err)
}

/// Escala vértices em volta da média deles (eixos do objeto).
pub fn scale(m: &mut EditMesh, verts: &[u32], s: Vec3) -> Result<OpOutput, String> {
    ops::transform::scale_verts(m, verts, ops::transform::Pivot::MedianPoint, s, Mat3::IDENTITY).map_err(op_err)
}

/// Rotação Euler (graus) da transformação de um objeto como quatérnio (para girar em volta de eixos do mundo).
pub fn object_rotation(o: &Object3d) -> glam::Quat {
    rotation_quat(o.transform.rot_deg)
}

// ---------------------------------------------------------------------------------------------------- histórico

/// Desfazer/refazer por documento inteiro, com rótulo por passo (o `History<T>` do ANIGO, ver `crate::history`).
pub type History = crate::history::History<ModelDoc>;

/// Passos guardados.
pub const HISTORY_CAP: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;

    fn house() -> ModelDoc {
        // casa de caixa com telhado: cubo, extrude da face de cima e a tampa nova achatada em X (cumeeira)
        let mut d = ModelDoc::new("Casa");
        let i = d.add_primitive(Primitive::Cube, 1.0, [0.0, 0.5, 0.0], Some("Casa")).unwrap();
        let m = &mut d.objects[i].mesh;
        let top = faces_facing(m, Vec3::Y, 0.9);
        assert_eq!(top.len(), 1);
        let out = extrude(m, &top, 0.6).unwrap();
        let cap: Vec<u32> = out.selection.faces.iter().copied().collect();
        let verts = selection_verts(m, &Selection::from_faces(m, cap.iter().copied()), SelectMode::Face);
        scale(m, &verts, Vec3::new(0.0, 1.0, 1.0)).unwrap();
        merge_by_distance(m, None, 1e-4).unwrap();
        d
    }

    #[test]
    fn extrude_adds_faces_and_the_house_roof_closes() {
        let mut m = Primitive::Cube.mesh(1.0);
        assert_eq!((m.vert_count(), m.face_count()), (8, 6));
        let top = faces_facing(&m, Vec3::Y, 0.9);
        let out = extrude(&mut m, &top, 0.5).unwrap();
        assert_eq!(m.face_count(), 6 + 4, "a tampa sobe e nascem 4 laterais");
        assert_eq!(m.vert_count(), 12);
        assert_eq!(out.selection.faces.len(), 1);
        let cap = *out.selection.faces.iter().next().unwrap();
        assert!((m.face_center(cap).y - 1.0).abs() < 1e-5);
        // a casa: o telhado vira um prisma (a tampa colapsa numa cumeeira)
        let d = house();
        let m = &d.objects[0].mesh;
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi.y - 1.1).abs() < 1e-4 && (lo.y + 0.5).abs() < 1e-4, "{lo} {hi}");
        let ridge: Vec<Vec3> = (0..m.vert_count() as u32).map(|v| m.co(v)).filter(|p| (p.y - 1.1).abs() < 1e-4).collect();
        assert_eq!(ridge.len(), 2, "cumeeira com dois vértices: {ridge:?}");
        assert!(ridge.iter().all(|p| p.x.abs() < 1e-5));
        m.validate().unwrap();
        let model = d.to_model().unwrap();
        assert_eq!(model.parts.len(), 1);
        assert!(model.info.triangles >= 12);
    }

    #[test]
    fn bevel_inset_loop_cut_and_dissolve_counts() {
        // bevel de uma aresta do cubo: 1 face nova, 2 vértices a mais
        let mut m = Primitive::Cube.mesh(1.0);
        let e = all_edges(&m)[0];
        bevel(&mut m, &[e], 0.1, 1).unwrap();
        assert_eq!(m.face_count(), 7);
        assert_eq!(m.vert_count(), 10);
        // bevel de todas as arestas: 6 + 12 + 8 faces
        let mut m = Primitive::Cube.mesh(1.0);
        let edges = all_edges(&m);
        bevel(&mut m, &edges, 0.1, 1).unwrap();
        assert_eq!(m.face_count(), 26);
        // inset da face de cima: 4 faces de borda novas
        let mut m = Primitive::Cube.mesh(1.0);
        let top = faces_facing(&m, Vec3::Y, 0.9);
        inset(&mut m, &top, 0.1, 0.0).unwrap();
        assert_eq!(m.face_count(), 10);
        // loop cut: corta o anel de 4 faces em 8
        let mut m = Primitive::Cube.mesh(1.0);
        let e = all_edges(&m)[0];
        loop_cut(&mut m, e, 1).unwrap();
        assert_eq!(m.face_count(), 10);
        assert_eq!(m.vert_count(), 12);
        // dissolver as arestas novas volta ao cubo de 6 faces
        let new_edges: Vec<EdgeKey> = all_edges(&m).into_iter().filter(|(a, b)| *a >= 8 && *b >= 8).collect();
        let sel = Selection::from_edges(new_edges);
        dissolve(&mut m, &sel, SelectMode::Edge).unwrap();
        assert_eq!(m.face_count(), 6);
    }

    #[test]
    fn modifiers_mirror_is_symmetric_array_copies_and_subsurf_refines() {
        let mut d = ModelDoc::new("m");
        let i = d.add_primitive(Primitive::Cube, 1.0, [0.0; 3], None).unwrap();
        // meio cubo deslocado em +X: o espelho em X cria o outro lado, simétrico
        d.objects[i].mesh.transform_positions(|p| p + Vec3::new(1.0, 0.0, 0.0));
        d.objects[i].modifiers.push(ModifierSpec::new("mirror").unwrap());
        let e = d.objects[i].evaluated().unwrap();
        assert_eq!(e.face_count(), 12);
        let mut xs: Vec<i32> = (0..e.vert_count() as u32).map(|v| (e.co(v).x * 1000.0).round() as i32).collect();
        xs.sort();
        let mut neg: Vec<i32> = xs.iter().map(|x| -x).collect();
        neg.sort();
        assert_eq!(xs, neg, "simetria em X");
        // array de 3: três vezes as faces
        d.objects[i].modifiers = vec![ModifierSpec::Array { count: 3, offset: [1.5, 0.0, 0.0], enabled: true }];
        assert_eq!(d.objects[i].evaluated().unwrap().face_count(), 18);
        // subdivisão 1: cada quad vira 4
        d.objects[i].modifiers = vec![ModifierSpec::Subsurf { levels: 1, enabled: true }];
        assert_eq!(d.objects[i].evaluated().unwrap().face_count(), 24);
        // desligado não conta
        d.objects[i].modifiers[0].set_enabled(false);
        assert_eq!(d.objects[i].evaluated().unwrap().face_count(), 6);
        assert!(ModifierSpec::Subsurf { levels: 9, enabled: true }.validate().is_err());
    }

    #[test]
    fn primitives_are_valid_centered_and_named() {
        let mut d = ModelDoc::new("p");
        for k in Primitive::ALL {
            let i = d.add_primitive(k, 1.0, [0.0; 3], None).unwrap();
            let m = &d.objects[i].mesh;
            m.validate().unwrap();
            assert!(m.face_count() > 0, "{k:?}");
            let (lo, hi) = m.bounds().unwrap();
            assert!(((lo + hi) * 0.5).length() < 1e-4, "{k:?} centrado: {lo} {hi}");
            assert_eq!(Primitive::from_key(k.key()), Some(k));
        }
        assert_eq!(d.objects[0].name, "Cubo");
        d.add_primitive(Primitive::Cube, 1.0, [0.0; 3], None).unwrap();
        assert_eq!(d.objects.last().unwrap().name, "Cubo.001");
        assert_eq!(d.find("Cubo.001"), Some(6));
        assert_eq!(d.find("2"), Some(2));
    }

    #[test]
    fn glb_export_round_trips_and_doc_json_round_trips() {
        let mut d = house();
        d.objects[0].color = [0.9, 0.2, 0.1];
        let j = d.add_primitive(Primitive::Cylinder, 0.4, [1.2, 0.2, 0.0], Some("Poste")).unwrap();
        d.objects[j].transform.rot_deg = [0.0, 0.0, 90.0];
        let glb = d.to_glb().unwrap();
        assert_eq!(d.to_glb().unwrap(), glb, "determinístico");
        let back = aiai_3d_model::glb::read_glb(&glb).unwrap();
        assert_eq!(back.parts.len(), 2);
        assert_eq!(back.info.triangles, d.to_model().unwrap().info.triangles);
        // a cor do objeto volta (linear no GLB → sRGB igual)
        let c = back.parts[0].color;
        assert!((aiai_3d_model::raster::linear_to_srgb(c[0]) - 0.9).abs() < 1e-3);
        // o poste girado 90° em Z fica deitado (mais largo em X que alto)
        let p = &back.parts[1];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        assert!(hi[0] - lo[0] > hi[1] - lo[1]);
        // o documento volta igual do JSON (é o que vai em params.modeling)
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(ModelDoc::from_json(&v).unwrap(), d);
        assert!(ModelDoc::from_json(&serde_json::json!({ "version": 9, "name": "x" })).is_err());
    }

    #[test]
    fn history_undoes_and_redoes_whole_documents() {
        let mut d = ModelDoc::new("h");
        let mut h = History::default();
        h.record("cubo", &d);
        d.add_primitive(Primitive::Cube, 1.0, [0.0; 3], None).unwrap();
        h.record("esfera", &d);
        d.add_primitive(Primitive::Sphere, 1.0, [0.0; 3], None).unwrap();
        assert_eq!(h.labels(), ["cubo", "esfera"]);
        assert_eq!(h.undo(&mut d).as_deref(), Some("esfera"));
        assert_eq!(d.objects.len(), 1);
        assert!(h.undo(&mut d).is_some());
        assert_eq!(d.objects.len(), 0);
        assert!(h.undo(&mut d).is_none());
        assert!(h.redo(&mut d).is_some());
        assert_eq!(d.objects.len(), 1);
    }
}

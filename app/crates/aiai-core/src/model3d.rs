//! Camada 3D de referência (S24 3D): o esquema de `params.model3d` de uma camada `model3d`, a avaliação por quadro
//! (valores-base + chaves `m3d.*` do `KeyTracks` da camada) e a validação. Sem GPU e sem matemática 3D aqui: quem
//! renderiza (`aiai-3d-model` na CPU, o serviço 3D do app na GPU) recebe o [`Model3dState`] já avaliado.
//!
//! Convenções: metros, Y para cima, mão direita (as mesmas do `aiai-guide3d` e do ANIGO). A câmera orbita em volta de
//! `target`: `yaw` 0 olha para -Z a partir de +Z, `pitch` positivo sobe a câmera.

use crate::ids::Id;
use crate::blob::BlobHash;
use crate::keys::KeyTracks;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Versão do esquema de `params.model3d`.
pub const MODEL3D_VERSION: u32 = 1;

/// Limites de segurança dos valores (evitam matriz degenerada e câmera dentro do infinito).
pub const MIN_SCALE: f32 = 1e-4;
pub const MAX_ABS: f32 = 1e6;
pub const MIN_FOV_DEG: f32 = 1.0;
pub const MAX_FOV_DEG: f32 = 150.0;
pub const MIN_DISTANCE: f32 = 1e-3;
pub const MAX_OUTLINE_PX: f32 = 64.0;

/// Propriedade das chaves de pose (S24 3D, fase 8): o valor de cada chave é o índice da pose em `pose.keyed`; entre
/// duas chaves a pose é misturada (slerp por osso) com o easing do trecho (`hold` segura a pose até a próxima chave).
pub const POSE_KEY: &str = "m3d.pose";

/// Propriedades animáveis (chaves do `KeyTracks` da camada). Fora desta lista, um nome começado por `m3d.` é erro.
pub const KEY_PROPS: [&str; 17] = [
    "m3d.pos.x",
    "m3d.pos.y",
    "m3d.pos.z",
    "m3d.rot.x",
    "m3d.rot.y",
    "m3d.rot.z",
    "m3d.scale.x",
    "m3d.scale.y",
    "m3d.scale.z",
    "m3d.cam.yaw",
    "m3d.cam.pitch",
    "m3d.cam.dist",
    "m3d.cam.fov",
    "m3d.cam.target.x",
    "m3d.cam.target.y",
    "m3d.cam.target.z",
    POSE_KEY,
];

/// Transformação do modelo no mundo: posição (m), rotação em graus (Euler X→Y→Z, eixos do mundo) e escala por eixo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transform3d {
    #[serde(default)]
    pub pos: [f32; 3],
    #[serde(default)]
    pub rot_deg: [f32; 3],
    #[serde(default = "ones3")]
    pub scale: [f32; 3],
}

impl Default for Transform3d {
    fn default() -> Self {
        Transform3d { pos: [0.0; 3], rot_deg: [0.0; 3], scale: [1.0; 3] }
    }
}

fn ones3() -> [f32; 3] {
    [1.0; 3]
}

/// Câmera orbital da camada: olha para `target` de `distance` metros, com `yaw`/`pitch` em graus e abertura vertical
/// `fov_deg`. O quadro da câmera é o canvas do corte (mesma proporção).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrbitCam3d {
    #[serde(default)]
    pub target: [f32; 3],
    #[serde(default)]
    pub yaw_deg: f32,
    #[serde(default)]
    pub pitch_deg: f32,
    #[serde(default = "default_distance")]
    pub distance: f32,
    #[serde(default = "default_fov")]
    pub fov_deg: f32,
}

fn default_distance() -> f32 {
    4.0
}
fn default_fov() -> f32 {
    35.0
}

impl Default for OrbitCam3d {
    fn default() -> Self {
        OrbitCam3d { target: [0.0, 0.9, 0.0], yaw_deg: 20.0, pitch_deg: 8.0, distance: default_distance(), fov_deg: default_fov() }
    }
}

/// Modo de desenho do modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode3d {
    /// Cel shading de dois tons (luz e sombra) com contorno.
    #[default]
    Toon,
    /// Cor chapada por material, sem sombra.
    Flat,
    /// Arestas dos triângulos visíveis (remoção de linhas ocultas), sem preenchimento.
    Wire,
    /// Silhueta de uma cor só (`color`, ou cinza-azulado).
    Silhouette,
}

impl RenderMode3d {
    pub const ALL: [RenderMode3d; 4] = [RenderMode3d::Toon, RenderMode3d::Flat, RenderMode3d::Wire, RenderMode3d::Silhouette];

    pub fn key(self) -> &'static str {
        match self {
            RenderMode3d::Toon => "toon",
            RenderMode3d::Flat => "flat",
            RenderMode3d::Wire => "wire",
            RenderMode3d::Silhouette => "silhouette",
        }
    }
}

/// Aparência e papel da camada no render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Render3d {
    #[serde(default)]
    pub mode: RenderMode3d,
    /// Largura do contorno em px do canvas (0 = sem contorno). No modo `wire` é a largura das arestas.
    #[serde(default = "default_outline_px")]
    pub outline_px: f32,
    /// Cor do contorno (e das arestas no `wire`), "#rrggbb".
    #[serde(default = "default_outline_color")]
    pub outline_color: String,
    /// Cor única no lugar das cores dos materiais ("#rrggbb"); `None` = materiais do modelo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Referência: aparece no canvas e na mesa de luz, mas NÃO sai no render final nem na exportação (padrão ligado).
    #[serde(default = "yes")]
    pub reference: bool,
}

fn default_outline_px() -> f32 {
    2.0
}
fn default_outline_color() -> String {
    "#1c1e26".into()
}
fn yes() -> bool {
    true
}

impl Default for Render3d {
    fn default() -> Self {
        Render3d { mode: RenderMode3d::Toon, outline_px: default_outline_px(), outline_color: default_outline_color(), color: None, reference: true }
    }
}

/// Pose de um modelo com esqueleto (Manequim ou modelo importado com pele): por nome de osso, a rotação **delta** sobre o
/// repouso (quatérnio `[x, y, z, w]`, lida nos eixos do mundo quando os pais estão em repouso, a convenção do Posing do
/// ANIGO) e o deslocamento do osso raiz (quadril) em metros. Osso ausente = repouso. Nomes que o esqueleto não tem são
/// ignorados ao aplicar (a mesma pose serve para esqueletos com os mesmos nomes humanoides).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct Pose3d {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rotations: BTreeMap<String, [f32; 4]>,
    #[serde(default, skip_serializing_if = "is_zero3")]
    pub hips: [f32; 3],
}

fn is_zero3(v: &[f32; 3]) -> bool {
    v.iter().all(|c| c.abs() <= 1e-7)
}

/// Limite do deslocamento do quadril (m).
pub const MAX_HIPS: f32 = 50.0;

/// Quatérnio normalizado com `w >= 0` (q e -q são a mesma rotação); `None` se não finito ou nulo.
pub fn quat_canonical(q: [f32; 4]) -> Option<[f32; 4]> {
    let l = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if !l.is_finite() || l < 1e-6 {
        return None;
    }
    let s = if q[3] < 0.0 { -1.0 / l } else { 1.0 / l };
    Some(q.map(|c| c * s))
}

fn quat_is_identity(q: [f32; 4]) -> bool {
    (q[3].abs() - 1.0).abs() <= 1e-6 && q[0].abs() <= 1e-6 && q[1].abs() <= 1e-6 && q[2].abs() <= 1e-6
}

/// Slerp pelo caminho curto entre dois quatérnios unitários.
pub fn quat_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let b = if dot < 0.0 {
        dot = -dot;
        b.map(|c| -c)
    } else {
        b
    };
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - t, t)
    } else {
        let th = dot.clamp(-1.0, 1.0).acos();
        let s = th.sin();
        (((1.0 - t) * th).sin() / s, (t * th).sin() / s)
    };
    let q = [0, 1, 2, 3].map(|i| a[i] * wa + b[i] * wb);
    quat_canonical(q).unwrap_or([0.0, 0.0, 0.0, 1.0])
}

impl Pose3d {
    /// Rotação delta do osso (identidade se ausente).
    pub fn rotation(&self, bone: &str) -> [f32; 4] {
        self.rotations.get(bone).copied().and_then(quat_canonical).unwrap_or([0.0, 0.0, 0.0, 1.0])
    }

    /// Define a rotação delta (identidade remove a entrada). Normaliza.
    pub fn set_rotation(&mut self, bone: &str, q: [f32; 4]) {
        match quat_canonical(q) {
            Some(q) if !quat_is_identity(q) => {
                self.rotations.insert(bone.to_string(), q);
            }
            _ => {
                self.rotations.remove(bone);
            }
        }
    }

    pub fn is_rest(&self) -> bool {
        self.rotations.is_empty() && is_zero3(&self.hips)
    }

    /// Mistura `a → b` em `t` (slerp por osso, lerp do quadril).
    pub fn blend(a: &Pose3d, b: &Pose3d, t: f32) -> Pose3d {
        // nas pontas, a pose exata (sem o ruído da renormalização): o "hold" devolve a própria pose da chave
        if t <= 0.0 {
            return a.clone();
        }
        if t >= 1.0 {
            return b.clone();
        }
        let mut out = Pose3d::default();
        let names: std::collections::BTreeSet<&String> = a.rotations.keys().chain(b.rotations.keys()).collect();
        for n in names {
            out.set_rotation(n, quat_slerp(a.rotation(n), b.rotation(n), t));
        }
        out.hips = [0, 1, 2].map(|i| a.hips[i] + (b.hips[i] - a.hips[i]) * t);
        out
    }

    /// Espelha esquerda ↔ direita (nomes `left*`/`right*`, como no VRM): `q' = (x, -y, -z, w)`, quadril com `x → -x`.
    pub fn mirrored(&self) -> Pose3d {
        let mut out = Pose3d::default();
        for (n, q) in &self.rotations {
            out.set_rotation(&mirror_bone_name(n), [q[0], -q[1], -q[2], q[3]]);
        }
        out.hips = [-self.hips[0], self.hips[1], self.hips[2]];
        out
    }

    pub fn validate(&self) -> Result<(), String> {
        for (n, q) in &self.rotations {
            if n.is_empty() || n.len() > 128 {
                return Err("pose: nome de osso vazio ou longo demais".into());
            }
            if q.iter().any(|c| !c.is_finite()) {
                return Err(format!("pose: rotação não finita em '{n}'"));
            }
            let l2: f32 = q.iter().map(|c| c * c).sum();
            if !(0.25..=4.0).contains(&l2) {
                return Err(format!("pose: quatérnio de '{n}' não é unitário (|q|² = {l2})"));
            }
        }
        if self.hips.iter().any(|c| !c.is_finite() || c.abs() > MAX_HIPS) {
            return Err(format!("pose: quadril fora de ±{MAX_HIPS} m"));
        }
        Ok(())
    }
}

/// Nome do osso do outro lado (`leftHand` ↔ `rightHand`, `tw_leftUpperArm` ↔ `tw_rightUpperArm`, `bust_L` ↔ `bust_R`);
/// ossos do centro voltam iguais.
pub fn mirror_bone_name(n: &str) -> String {
    for (a, b) in [("left", "right"), ("right", "left"), ("Left", "Right"), ("Right", "Left")] {
        if let Some(rest) = n.strip_prefix(a) {
            return format!("{b}{rest}");
        }
        if let Some(i) = n.find(a) {
            let before = &n[..i];
            if before.ends_with('_') || before.ends_with(':') || before.ends_with('.') {
                return format!("{before}{b}{}", &n[i + a.len()..]);
            }
        }
    }
    if let Some(s) = n.strip_suffix("_L") {
        return format!("{s}_R");
    }
    if let Some(s) = n.strip_suffix("_R") {
        return format!("{s}_L");
    }
    n.to_string()
}

/// Pose da camada: a pose-base (sem chaves) e as poses guardadas pelas chaves `m3d.pose` (o valor da chave é o índice).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct PoseParams {
    #[serde(default)]
    pub base: Pose3d,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyed: Vec<Pose3d>,
}

impl PoseParams {
    /// Pose no quadro: sem chaves `m3d.pose`, a base; com chaves, a mistura das poses das duas chaves vizinhas com o
    /// easing do trecho (índice inválido = base).
    pub fn eval(&self, keys: &KeyTracks, frame: f32) -> Pose3d {
        let Some(k) = keys.0.get(POSE_KEY).filter(|k| !k.is_empty()) else { return self.base.clone() };
        let pose_at = |v: f32| -> Option<&Pose3d> {
            let i = v.round();
            (i >= 0.0 && (i as usize) < self.keyed.len()).then(|| &self.keyed[i as usize])
        };
        let one = |v: f32| pose_at(v).cloned().unwrap_or_else(|| self.base.clone());
        let (first, last) = (&k[0], &k[k.len() - 1]);
        if frame <= first.frame as f32 {
            return one(first.value);
        }
        if frame >= last.frame as f32 {
            return one(last.value);
        }
        let Some(i) = k.windows(2).position(|w| frame >= w[0].frame as f32 && frame < w[1].frame as f32) else { return one(last.value) };
        let (a, b) = (&k[i], &k[i + 1]);
        let t = a.ease.apply((frame - a.frame as f32) / (b.frame - a.frame) as f32);
        match (pose_at(a.value), pose_at(b.value)) {
            (Some(pa), Some(pb)) => Pose3d::blend(pa, pb, t),
            _ => one(a.value),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.base.validate()?;
        for p in &self.keyed {
            p.validate()?;
        }
        Ok(())
    }
}

/// `params.model3d` de uma camada `model3d`. Chave desconhecida é erro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Model3dParams {
    #[serde(default = "version")]
    pub version: u32,
    /// Nó `Asset` (tipo `model3d`) com o GLB normalizado.
    pub asset: Id,
    /// Hash do blob do modelo no momento em que a camada foi ligada a ele (detecta troca do ativo).
    pub model_hash: BlobHash,
    #[serde(default)]
    pub transform: Transform3d,
    #[serde(default)]
    pub camera: OrbitCam3d,
    #[serde(default)]
    pub render: Render3d,
    /// Pose (só para modelos com esqueleto: o Manequim e modelos importados com pele). `None` = sem pose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<PoseParams>,
}

fn version() -> u32 {
    MODEL3D_VERSION
}

/// Estado avaliado num quadro: o que o renderizador precisa (valores-base com as chaves aplicadas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model3dState {
    pub model_hash: BlobHash,
    pub transform: Transform3d,
    pub camera: OrbitCam3d,
    pub mode: RenderMode3d,
    pub outline_px: f32,
    /// sRGB 0..1.
    pub outline_color: [f32; 3],
    /// sRGB 0..1.
    pub color: Option<[f32; 3]>,
    /// Pose avaliada no quadro (fora da serialização quando não há: a chave do cache das camadas sem pose não muda).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<Pose3d>,
}

/// "#rrggbb" → sRGB 0..1.
pub fn parse_hex_rgb(s: &str) -> Result<[f32; 3], String> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("cor '{s}' inválida (use \"#rrggbb\")"));
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|x| x as f32 / 255.0).map_err(|e| e.to_string());
    Ok([v(0)?, v(2)?, v(4)?])
}

/// sRGB 0..1 → "#rrggbb".
pub fn hex_rgb(c: [f32; 3]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

fn finite3(v: &[f32; 3]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= MAX_ABS)
}

impl Model3dParams {
    /// Lê `params.model3d` de uma camada. `None` se a chave não existe.
    pub fn from_layer_params(params: &serde_json::Value) -> Option<Result<Model3dParams, String>> {
        let v = params.get("model3d")?;
        Some(serde_json::from_value::<Model3dParams>(v.clone()).map_err(|e| format!("params.model3d inválido: {e}")).and_then(|p| p.validate().map(|_| p)))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != MODEL3D_VERSION {
            return Err(format!("params.model3d.version {} não suportada (esperado {MODEL3D_VERSION})", self.version));
        }
        if !self.model_hash.is_well_formed() {
            return Err("params.model3d.model_hash inválido".into());
        }
        let t = &self.transform;
        if !(finite3(&t.pos) && finite3(&t.rot_deg) && finite3(&t.scale)) {
            return Err("params.model3d.transform com valor não finito ou fora da faixa".into());
        }
        if t.scale.iter().any(|s| s.abs() < MIN_SCALE) {
            return Err(format!("params.model3d.transform.scale abaixo de {MIN_SCALE}"));
        }
        let c = &self.camera;
        if !(finite3(&c.target) && c.yaw_deg.is_finite() && c.pitch_deg.is_finite()) {
            return Err("params.model3d.camera com valor não finito".into());
        }
        if !(c.distance.is_finite() && (MIN_DISTANCE..=MAX_ABS).contains(&c.distance)) {
            return Err(format!("params.model3d.camera.distance fora de {MIN_DISTANCE}..{MAX_ABS}"));
        }
        if !(MIN_FOV_DEG..=MAX_FOV_DEG).contains(&c.fov_deg) {
            return Err(format!("params.model3d.camera.fov_deg fora de {MIN_FOV_DEG}..{MAX_FOV_DEG}"));
        }
        let r = &self.render;
        if !(r.outline_px.is_finite() && (0.0..=MAX_OUTLINE_PX).contains(&r.outline_px)) {
            return Err(format!("params.model3d.render.outline_px fora de 0..{MAX_OUTLINE_PX}"));
        }
        parse_hex_rgb(&r.outline_color).map_err(|e| format!("params.model3d.render.outline_color: {e}"))?;
        if let Some(c) = &r.color {
            parse_hex_rgb(c).map_err(|e| format!("params.model3d.render.color: {e}"))?;
        }
        if let Some(p) = &self.pose {
            p.validate().map_err(|e| format!("params.model3d.{e}"))?;
        }
        Ok(())
    }

    /// Avalia no quadro (fracionário permitido): valores-base, e por cima as chaves `m3d.*` que existirem.
    pub fn eval(&self, keys: &KeyTracks, frame: f32) -> Model3dState {
        let k = |prop: &str, base: f32| keys.eval(prop, frame).unwrap_or(base);
        let t = &self.transform;
        let c = &self.camera;
        let transform = Transform3d {
            pos: [k("m3d.pos.x", t.pos[0]), k("m3d.pos.y", t.pos[1]), k("m3d.pos.z", t.pos[2])],
            rot_deg: [k("m3d.rot.x", t.rot_deg[0]), k("m3d.rot.y", t.rot_deg[1]), k("m3d.rot.z", t.rot_deg[2])],
            scale: [k("m3d.scale.x", t.scale[0]), k("m3d.scale.y", t.scale[1]), k("m3d.scale.z", t.scale[2])].map(|s| if s.abs() < MIN_SCALE { MIN_SCALE } else { s }),
        };
        let camera = OrbitCam3d {
            target: [k("m3d.cam.target.x", c.target[0]), k("m3d.cam.target.y", c.target[1]), k("m3d.cam.target.z", c.target[2])],
            yaw_deg: k("m3d.cam.yaw", c.yaw_deg),
            pitch_deg: k("m3d.cam.pitch", c.pitch_deg),
            distance: k("m3d.cam.dist", c.distance).max(MIN_DISTANCE),
            fov_deg: k("m3d.cam.fov", c.fov_deg).clamp(MIN_FOV_DEG, MAX_FOV_DEG),
        };
        let r = &self.render;
        Model3dState {
            model_hash: self.model_hash.clone(),
            transform,
            camera,
            mode: r.mode,
            outline_px: r.outline_px,
            outline_color: parse_hex_rgb(&r.outline_color).unwrap_or([0.1, 0.1, 0.12]),
            color: r.color.as_deref().and_then(|c| parse_hex_rgb(c).ok()),
            pose: self.pose.as_ref().map(|p| p.eval(keys, frame)),
        }
    }

    /// Grava os valores de um estado avaliado como valores-base (transformação e câmera).
    pub fn set_base_from(&mut self, s: &Model3dState) {
        self.transform = s.transform;
        self.camera = s.camera;
    }
}

/// Valor de uma propriedade animável num estado avaliado (para gravar chaves a partir de um gesto).
pub fn state_prop(s: &Model3dState, prop: &str) -> Option<f32> {
    let t = &s.transform;
    let c = &s.camera;
    Some(match prop {
        "m3d.pos.x" => t.pos[0],
        "m3d.pos.y" => t.pos[1],
        "m3d.pos.z" => t.pos[2],
        "m3d.rot.x" => t.rot_deg[0],
        "m3d.rot.y" => t.rot_deg[1],
        "m3d.rot.z" => t.rot_deg[2],
        "m3d.scale.x" => t.scale[0],
        "m3d.scale.y" => t.scale[1],
        "m3d.scale.z" => t.scale[2],
        "m3d.cam.yaw" => c.yaw_deg,
        "m3d.cam.pitch" => c.pitch_deg,
        "m3d.cam.dist" => c.distance,
        "m3d.cam.fov" => c.fov_deg,
        "m3d.cam.target.x" => c.target[0],
        "m3d.cam.target.y" => c.target[1],
        "m3d.cam.target.z" => c.target[2],
        _ => return None,
    })
}

/// Chaves `m3d.*` com nome desconhecido são erro (as demais propriedades da camada seguem as regras de sempre).
pub fn validate_keys(keys: &KeyTracks) -> Result<(), String> {
    for prop in keys.0.keys() {
        if prop.starts_with("m3d.") && !KEY_PROPS.contains(&prop.as_str()) {
            return Err(format!("chave '{prop}' desconhecida (use: {})", KEY_PROPS.join(", ")));
        }
    }
    Ok(())
}

/// A camada é referência (não sai no render final)? Sem params válidos, conta como referência (o caso seguro).
pub fn is_reference(params: &serde_json::Value) -> bool {
    match Model3dParams::from_layer_params(params) {
        Some(Ok(p)) => p.render.reference,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;

    fn sample() -> Model3dParams {
        Model3dParams {
            version: 1,
            asset: Id("a1".into()),
            model_hash: BlobHash::of(b"glb"),
            transform: Transform3d::default(),
            camera: OrbitCam3d::default(),
            render: Render3d::default(),
            pose: None,
        }
    }

    #[test]
    fn serde_round_trip_and_defaults() {
        let p = sample();
        let v = serde_json::to_value(&p).unwrap();
        let back: Model3dParams = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back, p);
        // só o obrigatório: o resto vem dos padrões (referência ligada, toon, contorno de 2 px)
        let min = serde_json::json!({ "asset": "a1", "model_hash": p.model_hash.0 });
        let m: Model3dParams = serde_json::from_value(min).unwrap();
        assert!(m.render.reference);
        assert_eq!(m.render.mode, RenderMode3d::Toon);
        assert_eq!(m.transform.scale, [1.0; 3]);
        m.validate().unwrap();
    }

    #[test]
    fn unknown_keys_and_bad_values_are_errors() {
        let p = sample();
        let mut v = serde_json::to_value(&p).unwrap();
        v["render"]["glow"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Model3dParams>(v).is_err());
        let mut q = sample();
        q.camera.fov_deg = 0.0;
        assert!(q.validate().is_err());
        let mut q = sample();
        q.transform.scale = [0.0, 1.0, 1.0];
        assert!(q.validate().is_err());
        let mut q = sample();
        q.render.outline_color = "azul".into();
        assert!(q.validate().is_err());
        let mut keys = KeyTracks::default();
        keys.set_key("m3d.pos.w", 0, 1.0);
        assert!(validate_keys(&keys).is_err());
    }

    #[test]
    fn keys_override_base_values_per_frame() {
        let p = sample();
        let mut keys = KeyTracks::default();
        keys.0.insert("m3d.pos.x".into(), vec![Key { frame: 0, value: 0.0, ease: Default::default() }, Key { frame: 10, value: 2.0, ease: Default::default() }]);
        keys.0.insert("m3d.cam.yaw".into(), vec![Key { frame: 0, value: 90.0, ease: Default::default() }]);
        let s = p.eval(&keys, 5.0);
        assert!((s.transform.pos[0] - 1.0).abs() < 1e-5);
        assert_eq!(s.camera.yaw_deg, 90.0);
        assert_eq!(s.camera.pitch_deg, p.camera.pitch_deg);
        assert_eq!(state_prop(&s, "m3d.pos.x"), Some(s.transform.pos[0]));
        validate_keys(&keys).unwrap();
    }

    fn q_axis(axis: [f32; 3], deg: f32) -> [f32; 4] {
        let h = deg.to_radians() * 0.5;
        [axis[0] * h.sin(), axis[1] * h.sin(), axis[2] * h.sin(), h.cos()]
    }

    #[test]
    fn pose_keys_blend_between_poses_and_hold_holds() {
        let mut p = sample();
        let mut a = Pose3d::default();
        a.set_rotation("leftUpperArm", q_axis([0.0, 0.0, 1.0], -80.0));
        let mut b = Pose3d::default();
        b.hips = [0.0, -0.4, 0.0];
        p.pose = Some(PoseParams { base: Pose3d::default(), keyed: vec![a.clone(), b.clone()] });
        p.validate().unwrap();
        let mut keys = KeyTracks::default();
        // sem chaves: a base (repouso)
        assert!(p.eval(&keys, 3.0).pose.unwrap().is_rest());
        keys.set_key(POSE_KEY, 0, 0.0);
        keys.set_key(POSE_KEY, 10, 1.0);
        validate_keys(&keys).unwrap();
        let mid = p.eval(&keys, 5.0).pose.unwrap();
        // metade do caminho: -40° em volta de Z
        let q = mid.rotation("leftUpperArm");
        let ang = 2.0 * q[3].clamp(-1.0, 1.0).acos().to_degrees();
        assert!((ang - 40.0).abs() < 0.05, "{ang}");
        assert!((mid.hips[1] + 0.2).abs() < 1e-5);
        assert_eq!(p.eval(&keys, 12.0).pose.unwrap(), b, "depois da última chave, a última pose");
        // hold: segura a primeira até a segunda chave
        keys.set_ease(POSE_KEY, Some(&[0]), crate::keys::Ease::Hold);
        assert_eq!(p.eval(&keys, 9.0).pose.unwrap(), a);
        // índice inválido cai na base, sem pânico
        keys.set_key(POSE_KEY, 20, 7.0);
        assert!(p.eval(&keys, 25.0).pose.unwrap().is_rest());
        // serde: a camada sem pose não grava a chave `pose`, e o estado sem pose também não (cache estável)
        let none = sample();
        assert!(serde_json::to_value(&none).unwrap().get("pose").is_none());
        assert!(serde_json::to_value(none.eval(&KeyTracks::default(), 0.0)).unwrap().get("pose").is_none());
        let back: Model3dParams = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn pose_mirror_and_validation() {
        let mut a = Pose3d::default();
        a.set_rotation("leftUpperArm", q_axis([0.0, 0.0, 1.0], -70.0));
        a.hips = [0.1, 0.0, 0.0];
        let m = a.mirrored();
        assert!(m.rotations.contains_key("rightUpperArm") && !m.rotations.contains_key("leftUpperArm"));
        assert_eq!(m.hips, [-0.1, 0.0, 0.0]);
        assert_eq!(m.mirrored(), a);
        assert_eq!(mirror_bone_name("tw_leftLowerArm"), "tw_rightLowerArm");
        assert_eq!(mirror_bone_name("bust_L"), "bust_R");
        assert_eq!(mirror_bone_name("spine"), "spine");
        let mut bad = Pose3d::default();
        bad.rotations.insert("head".into(), [0.0, 0.0, 0.0, 9.0]);
        assert!(bad.validate().is_err());
        // identidade não fica guardada
        let mut z = Pose3d::default();
        z.set_rotation("head", [0.0, 0.0, 0.0, -1.0]);
        assert!(z.is_rest());
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex_rgb(parse_hex_rgb("#1c1e26").unwrap()), "#1c1e26");
        assert!(parse_hex_rgb("#12345").is_err());
    }
}

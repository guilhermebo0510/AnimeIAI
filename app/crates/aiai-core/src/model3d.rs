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

/// Versão do esquema de `params.model3d`.
pub const MODEL3D_VERSION: u32 = 1;

/// Limites de segurança dos valores (evitam matriz degenerada e câmera dentro do infinito).
pub const MIN_SCALE: f32 = 1e-4;
pub const MAX_ABS: f32 = 1e6;
pub const MIN_FOV_DEG: f32 = 1.0;
pub const MAX_FOV_DEG: f32 = 150.0;
pub const MIN_DISTANCE: f32 = 1e-3;
pub const MAX_OUTLINE_PX: f32 = 64.0;

/// Propriedades animáveis (chaves do `KeyTracks` da camada). Fora desta lista, um nome começado por `m3d.` é erro.
pub const KEY_PROPS: [&str; 16] = [
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

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex_rgb(parse_hex_rgb("#1c1e26").unwrap()), "#1c1e26");
        assert!(parse_hex_rgb("#12345").is_err());
    }
}

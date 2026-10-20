//! Efeitos de composição (satsuei). Contrato com o compositor (`aiai-render`):
//!
//! - Pilha por camada/pasta: `LayerData.params.effects = [EffectInstance, ...]`, aplicada à imagem da camada (já no
//!   espaço de saída, depois da câmera) antes da mistura.
//! - Camada de efeito (`LayerType::Effect`): `params.effects` aplicado ao que já foi composto abaixo dela na mesma pasta
//!   (como uma camada de ajuste), respeitando opacidade, máscara e recorte.
//! - Parâmetros animáveis: `EffectInstance.keys` (caminho do parâmetro numérico → chaves: `radius`, `center.0`,
//!   `adjustment.hue`); sem chave o efeito é estático (o JSON de sempre). `posterize_time` faz o efeito "andar" em 2s/3s.
//! - Determinismo: tudo que é aleatório usa `FxContext.seed` e o quadro.

use aiai_core::{BlobStore, KeyTracks};
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod blur;
pub mod color;
pub mod effects;
pub mod light;
pub mod lut;
pub mod scopes;
pub mod style;
pub mod util;

pub use effects::{Effect, bake_lut};
pub use lut::Lut;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectInstance {
    pub effect: Effect,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Segura o efeito por N quadros (1 = todo quadro). Anime costuma animar efeitos em 2s/3s.
    #[serde(default = "one")]
    pub posterize_time: u32,
    /// Chaves por nome de campo numérico do efeito (ex.: "radius", "intensity", "amount").
    #[serde(default, skip_serializing_if = "KeyTracks::is_empty")]
    pub keys: KeyTracks,
}

fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}

pub struct FxContext<'a> {
    pub frame: u32,
    /// O que está composto abaixo (light wrap, camada de efeito).
    pub below: Option<&'a Image>,
    pub blobs: &'a BlobStore,
    pub seed: u64,
}

impl EffectInstance {
    /// Efeito com os parâmetros animados avaliados no quadro (com posterização temporal). Cada trilha de `keys` é um
    /// caminho de parâmetro numérico (`param_get`): `radius`, `center.0`, `color.2`, `adjustment.hue`. Trilha que não
    /// aponta para um número do efeito é erro (vira aviso no render), nunca é ignorada.
    pub fn at_frame(&self, frame: u32) -> Result<Effect, String> {
        if self.keys.is_empty() {
            return Ok(self.effect.clone());
        }
        let n = self.posterize_time.max(1);
        let f = (frame / n * n) as f32;
        let mut v = serde_json::to_value(&self.effect).map_err(|e| e.to_string())?;
        for prop in self.keys.0.keys() {
            if let Some(val) = self.keys.eval(prop, f) {
                if !param_set(&mut v, prop, val as f64) {
                    return Err(format!("chave animada '{prop}' não é um parâmetro numérico de '{}'", self.effect.name()));
                }
            }
        }
        serde_json::from_value(v).map_err(|e| format!("chave animada inválida: {e}"))
    }

    /// Chaves em ordem, finitas e cada trilha num parâmetro numérico que o efeito tem.
    pub fn validate(&self) -> Result<(), String> {
        self.keys.validate()?;
        let v = serde_json::to_value(&self.effect).map_err(|e| e.to_string())?;
        for prop in self.keys.0.keys() {
            if param_get(&v, prop).is_none() {
                return Err(format!("'{prop}' não é um parâmetro numérico de '{}' (use {})", self.effect.name(), numeric_params(&self.effect).join(", ")));
            }
        }
        Ok(())
    }
}

/// Valor numérico no caminho `a.b.0` do JSON de um efeito (objeto por nome, lista por índice); `type` não é parâmetro.
pub fn param_get(v: &serde_json::Value, path: &str) -> Option<f64> {
    if path.is_empty() || path.split('.').any(|p| p == "type") {
        return None;
    }
    let mut cur = v;
    for part in path.split('.') {
        cur = match cur {
            serde_json::Value::Object(o) => o.get(part)?,
            serde_json::Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    cur.as_f64()
}

/// Troca o número no caminho (só se ali já há um número). Devolve se trocou.
pub fn param_set(v: &mut serde_json::Value, path: &str, x: f64) -> bool {
    if path.is_empty() || path.split('.').any(|p| p == "type") || !x.is_finite() {
        return false;
    }
    let mut cur = v;
    for part in path.split('.') {
        cur = match cur {
            serde_json::Value::Object(o) => match o.get_mut(part) {
                Some(c) => c,
                None => return false,
            },
            serde_json::Value::Array(a) => match part.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                Some(c) => c,
                None => return false,
            },
            _ => return false,
        };
    }
    if !cur.is_number() {
        return false;
    }
    *cur = serde_json::json!(x);
    true
}

/// Todos os caminhos de parâmetro numérico (animáveis) de um efeito, na ordem do JSON.
pub fn numeric_params(e: &Effect) -> Vec<String> {
    fn walk(v: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Number(_) => out.push(prefix.to_owned()),
            serde_json::Value::Object(o) => {
                // `type` marca o efeito e os ajustes de cor ("adjustment.type"): não é parâmetro
                for (k, c) in o.iter().filter(|(k, _)| k.as_str() != "type") {
                    walk(c, &if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, c) in a.iter().enumerate() {
                    walk(c, &format!("{prefix}.{i}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::to_value(e) {
        walk(&v, "", &mut out);
    }
    out
}

/// Aplica a pilha na imagem. Devolve avisos (efeito não implementado, parâmetro inválido...) — nunca falha calado.
pub fn apply_stack(img: &mut Image, stack: &[EffectInstance], ctx: &FxContext) -> Vec<String> {
    let mut warnings = Vec::new();
    for inst in stack.iter().filter(|i| i.enabled) {
        let n = inst.posterize_time.max(1);
        let frame = ctx.frame / n * n;
        match inst.at_frame(frame) {
            Ok(e) => {
                let c = FxContext { frame, below: ctx.below, blobs: ctx.blobs, seed: ctx.seed };
                if let Err(w) = e.apply(img, &c) {
                    warnings.push(w);
                }
            }
            Err(w) => warnings.push(w),
        }
    }
    warnings
}

/// Lê a pilha de efeitos de `params.effects` (ou vazio).
pub fn stack_from_params(params: &serde_json::Value) -> Result<Vec<EffectInstance>, String> {
    match params.get("effects") {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("params.effects inválido: {e}")),
    }
}

#[cfg(test)]
mod keyed_tests {
    use super::*;
    use aiai_core::{Ease, Key};

    fn blur_keyed() -> EffectInstance {
        let mut keys = KeyTracks::default();
        keys.0.insert("radius".into(), vec![Key { frame: 0, value: 0.0, ease: Ease::Linear }, Key { frame: 10, value: 20.0, ease: Ease::Linear }]);
        EffectInstance { effect: Effect::GaussianBlur { radius: 4.0 }, enabled: true, posterize_time: 1, keys }
    }

    #[test]
    fn keyed_params_evaluate_per_frame_and_nested_paths_work() {
        let e = blur_keyed();
        assert_eq!(e.at_frame(5).unwrap(), Effect::GaussianBlur { radius: 10.0 });
        assert_eq!(e.at_frame(0).unwrap(), Effect::GaussianBlur { radius: 0.0 });
        // posterização: em 2s o quadro 5 segura o 4
        let mut p = e.clone();
        p.posterize_time = 2;
        assert_eq!(p.at_frame(5).unwrap(), Effect::GaussianBlur { radius: 8.0 });
        // caminho dentro de lista e de objeto aninhado
        let mut keys = KeyTracks::default();
        keys.set_key("center.1", 0, 0.0);
        keys.set_key("center.1", 4, 40.0);
        let r = EffectInstance { effect: Effect::RadialBlur { center: [5.0, 6.0], amount: 0.2 }, enabled: true, posterize_time: 1, keys };
        assert!(r.validate().is_ok());
        assert_eq!(r.at_frame(2).unwrap(), Effect::RadialBlur { center: [5.0, 20.0], amount: 0.2 });
        let adj = Effect::Adjust { adjustment: aiai_paint::adjust::Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0 } };
        let params = numeric_params(&adj);
        assert!(params.contains(&"adjustment.hue".to_string()) && !params.iter().any(|p| p.ends_with("type")), "{params:?}");
        let mut glow = numeric_params(&Effect::Glow { threshold: 0.5, radius: 3.0, intensity: 1.0, color: None, screen: false });
        glow.sort();
        assert_eq!(glow, vec!["intensity", "radius", "threshold"], "cor ausente e booleano não são parâmetros");
    }

    #[test]
    fn a_key_on_a_missing_param_is_an_error_not_ignored() {
        let mut e = blur_keyed();
        e.keys.set_key("raio", 0, 1.0);
        assert!(e.validate().unwrap_err().contains("raio"));
        assert!(e.at_frame(3).is_err());
        let mut img = aiai_paint::Image::new(4, 4);
        let blobs = BlobStore::default();
        let w = apply_stack(&mut img, &[e], &FxContext { frame: 3, below: None, blobs: &blobs, seed: 0 });
        assert_eq!(w.len(), 1, "vira aviso do render");
    }

    #[test]
    fn static_instances_round_trip_without_keys() {
        let e = EffectInstance { effect: Effect::Vignette { amount: 0.3, softness: 0.5, color: None }, enabled: true, posterize_time: 1, keys: KeyTracks::default() };
        let j = serde_json::to_value(&e).unwrap();
        assert!(j.get("keys").is_none(), "estático continua sem 'keys' no documento");
        let back: EffectInstance = serde_json::from_value(serde_json::json!({ "effect": { "type": "vignette", "amount": 0.3, "softness": 0.5 } })).unwrap();
        assert_eq!(back, e);
    }
}

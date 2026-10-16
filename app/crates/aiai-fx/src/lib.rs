//! Efeitos de composição (satsuei). Contrato com o compositor (`aiai-render`):
//!
//! - Pilha por camada/pasta: `LayerData.params.effects = [EffectInstance, ...]`, aplicada à imagem da camada (já no
//!   espaço de saída, depois da câmera) antes da mistura.
//! - Camada de efeito (`LayerType::Effect`): `params.effects` aplicado ao que já foi composto abaixo dela na mesma pasta
//!   (como uma camada de ajuste), respeitando opacidade, máscara e recorte.
//! - Parâmetros animáveis: `EffectInstance.keys` (nome do campo → chaves); `posterize_time` faz o efeito "andar" em 2s/3s.
//! - Determinismo: tudo que é aleatório usa `FxContext.seed` e o quadro.

use aiai_core::{BlobStore, KeyTracks};
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod effects;

pub use effects::Effect;

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
    /// Efeito com os parâmetros animados avaliados no quadro (com posterização temporal).
    pub fn at_frame(&self, frame: u32) -> Result<Effect, String> {
        if self.keys.is_empty() {
            return Ok(self.effect.clone());
        }
        let n = self.posterize_time.max(1);
        let f = (frame / n * n) as f32;
        let mut v = serde_json::to_value(&self.effect).map_err(|e| e.to_string())?;
        for prop in self.keys.0.keys() {
            if let Some(val) = self.keys.eval(prop, f) {
                if let Some(o) = v.as_object_mut() {
                    o.insert(prop.clone(), serde_json::json!(val));
                }
            }
        }
        serde_json::from_value(v).map_err(|e| format!("chave animada inválida: {e}"))
    }
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

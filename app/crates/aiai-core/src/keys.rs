//! Keyframes de propriedades numéricas (camada, câmera, efeitos) com easing. Quadros 0-based.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Aceita a forma marcada (`{"type":"ease_in_out"}`, `{"type":"bezier",...}`) e, para as curvas sem parâmetro, o nome
/// em texto (`"ease_in_out"`), como as descrições das ferramentas MCP mostram.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    /// Segura o valor até a próxima chave (animação "em degrau", típica de anime).
    Hold,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Curva de Bézier cúbica como no CSS/AE (x1, y1, x2, y2), x em 0..1.
    Bezier { x1: f32, y1: f32, x2: f32, y2: f32 },
}

impl<'de> Deserialize<'de> for Ease {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Tagged {
            Linear,
            Hold,
            EaseIn,
            EaseOut,
            EaseInOut,
            Bezier { x1: f32, y1: f32, x2: f32, y2: f32 },
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Name(String),
            Tagged(Tagged),
        }
        Ok(match Repr::deserialize(d).map_err(|_| {
            serde::de::Error::custom("ease: use \"linear\" | \"hold\" | \"ease_in\" | \"ease_out\" | \"ease_in_out\" ou {\"type\":\"bezier\",\"x1\",\"y1\",\"x2\",\"y2\"}")
        })? {
            Repr::Name(s) => match s.as_str() {
                "linear" => Ease::Linear,
                "hold" => Ease::Hold,
                "ease_in" => Ease::EaseIn,
                "ease_out" => Ease::EaseOut,
                "ease_in_out" => Ease::EaseInOut,
                other => return Err(serde::de::Error::custom(format!("ease desconhecido: {other}"))),
            },
            Repr::Tagged(t) => match t {
                Tagged::Linear => Ease::Linear,
                Tagged::Hold => Ease::Hold,
                Tagged::EaseIn => Ease::EaseIn,
                Tagged::EaseOut => Ease::EaseOut,
                Tagged::EaseInOut => Ease::EaseInOut,
                Tagged::Bezier { x1, y1, x2, y2 } => Ease::Bezier { x1, y1, x2, y2 },
            },
        })
    }
}

impl Ease {
    /// Progresso 0..1 → valor de interpolação (pode passar de 1 com overshoot na Bézier).
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Ease::Linear => t,
            Ease::Hold => 0.0,
            Ease::EaseIn => t * t * t,
            Ease::EaseOut => 1.0 - (1.0 - t).powi(3),
            Ease::EaseInOut => {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
            }
            Ease::Bezier { x1, y1, x2, y2 } => cubic_bezier(x1, y1, x2, y2, t),
        }
    }
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    // resolve t tal que Bx(t) = x (Newton + bisseção), devolve By(t)
    let bx = |t: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * x1 + 3.0 * (1.0 - t) * t * t * x2 + t * t * t;
    let by = |t: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t * t * t;
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..30 {
        let v = bx(t);
        if (v - x).abs() < 1e-5 {
            break;
        }
        if v < x { lo = t } else { hi = t }
        t = (lo + hi) / 2.0;
    }
    by(t)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    pub frame: u32,
    pub value: f32,
    /// Easing do trecho que COMEÇA nesta chave.
    #[serde(default)]
    pub ease: Ease,
}

/// Propriedade → chaves (ordenadas por quadro). Propriedades conhecidas da camada: offset_x, offset_y, opacity,
/// rotation, scale. Da câmera: center_x, center_y, zoom, rotation, shake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct KeyTracks(pub BTreeMap<String, Vec<Key>>);

impl KeyTracks {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Valor da propriedade no quadro (pode ser fracionário para motion blur); `None` se não há chaves.
    pub fn eval(&self, prop: &str, frame: f32) -> Option<f32> {
        let keys = self.0.get(prop)?;
        eval_keys(keys, frame)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (prop, keys) in &self.0 {
            if keys.windows(2).any(|w| w[0].frame >= w[1].frame) {
                return Err(format!("chaves de '{prop}' fora de ordem ou repetidas"));
            }
            if keys.iter().any(|k| !k.value.is_finite()) {
                return Err(format!("valor não finito em '{prop}'"));
            }
        }
        Ok(())
    }
}

pub fn eval_keys(keys: &[Key], frame: f32) -> Option<f32> {
    let first = keys.first()?;
    if frame <= first.frame as f32 {
        return Some(first.value);
    }
    let last = keys.last().expect("não vazio");
    if frame >= last.frame as f32 {
        return Some(last.value);
    }
    let i = keys.windows(2).position(|w| frame >= w[0].frame as f32 && frame < w[1].frame as f32)?;
    let (a, b) = (keys[i], keys[i + 1]);
    let t = (frame - a.frame as f32) / (b.frame - a.frame) as f32;
    Some(a.value + (b.value - a.value) * a.ease.apply(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_with_eases() {
        let keys = vec![
            Key { frame: 0, value: 0.0, ease: Ease::Linear },
            Key { frame: 10, value: 100.0, ease: Ease::Hold },
            Key { frame: 20, value: 0.0, ease: Ease::Linear },
        ];
        assert_eq!(eval_keys(&keys, 5.0), Some(50.0));
        assert_eq!(eval_keys(&keys, 15.0), Some(100.0), "hold segura");
        assert_eq!(eval_keys(&keys, 25.0), Some(0.0));
        let css_ease = Ease::Bezier { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1.0 };
        assert!((css_ease.apply(0.5) - 0.8024).abs() < 0.01, "ease do CSS em 0,5 ≈ 0,80: {}", css_ease.apply(0.5));
        assert!(Ease::EaseIn.apply(0.3) < 0.3 && Ease::EaseOut.apply(0.3) > 0.3);
    }

    #[test]
    fn ease_accepts_name_or_tagged() {
        let p = |s: &str| serde_json::from_str::<Ease>(s);
        assert_eq!(p(r#""ease_in_out""#).unwrap(), Ease::EaseInOut);
        assert_eq!(p(r#"{"type":"hold"}"#).unwrap(), Ease::Hold);
        assert_eq!(p(r#"{"type":"bezier","x1":0.1,"y1":0.2,"x2":0.3,"y2":0.4}"#).unwrap(), Ease::Bezier { x1: 0.1, y1: 0.2, x2: 0.3, y2: 0.4 });
        assert!(p(r#""zigzag""#).is_err());
        // a forma gravada continua marcada (documentos antigos e novos leem igual)
        assert_eq!(serde_json::to_string(&Ease::EaseInOut).unwrap(), r#"{"type":"ease_in_out"}"#);
    }
}

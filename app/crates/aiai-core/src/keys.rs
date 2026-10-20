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

/// Interpolação de uma chave como no After Effects (a UI da Composição e o MCP falam assim). O motor guarda o easing por
/// TRECHO (`Key::ease` vale do quadro desta chave até a próxima); aplicar uma interpolação numa chave mexe na metade de
/// saída do trecho que começa nela e, quando é o caso, na metade de chegada do trecho anterior (as alças da Bézier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyInterp {
    /// Linear nas duas metades (entrada e saída).
    Linear,
    /// Segura o valor até a próxima chave (saída em degrau).
    Hold,
    /// Easy Ease (F9): velocidade 0 com influência de 33,3% na chegada e na saída.
    EasyEase,
    /// Easy Ease só na chegada (Shift+F9).
    EasyEaseIn,
    /// Easy Ease só na saída (Ctrl+Shift+F9).
    EasyEaseOut,
}

/// Bézier equivalente (x1, y1, x2, y2) de um easing; `None` para o `Hold` (não tem curva).
pub fn ease_to_bezier(e: Ease) -> Option<[f32; 4]> {
    match e {
        Ease::Linear => Some([1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]),
        Ease::Hold => None,
        // as cúbicas polinomiais pelas Béziers do CSS mais próximas (easeInCubic/easeOutCubic/easeInOutCubic)
        Ease::EaseIn => Some([0.32, 0.0, 0.67, 0.0]),
        Ease::EaseOut => Some([0.33, 1.0, 0.68, 1.0]),
        Ease::EaseInOut => Some([0.65, 0.0, 0.35, 1.0]),
        Ease::Bezier { x1, y1, x2, y2 } => Some([x1, y1, x2, y2]),
    }
}

/// Bézier → easing, voltando a `Linear` quando as alças estão na reta (guardar `linear` mantém o documento limpo).
fn bezier_ease(b: [f32; 4]) -> Ease {
    let on_line = (b[0] - b[1]).abs() < 1e-4 && (b[2] - b[3]).abs() < 1e-4;
    if on_line { Ease::Linear } else { Ease::Bezier { x1: b[0], y1: b[1], x2: b[2], y2: b[3] } }
}

/// Influência do Easy Ease (33,33%), em fração do trecho.
pub const EASY_EASE: f32 = 1.0 / 3.0;

/// Aplica a interpolação à chave `i` de uma lista ordenada (só mexe nos easings).
pub fn apply_interp(keys: &mut [Key], i: usize, interp: KeyInterp) {
    if i >= keys.len() {
        return;
    }
    // metade de saída: trecho que começa em i
    let set_out = |k: &mut Key, x1: f32, y1: f32| {
        let b = ease_to_bezier(k.ease).unwrap_or([1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
        k.ease = bezier_ease([x1, y1, b[2], b[3]]);
    };
    // metade de chegada: trecho que termina em i (o que começa em i-1); um trecho em degrau não tem chegada
    let set_in = |k: &mut Key, x2: f32, y2: f32| {
        if let Some(b) = ease_to_bezier(k.ease) {
            k.ease = bezier_ease([b[0], b[1], x2, y2]);
        }
    };
    match interp {
        KeyInterp::Hold => keys[i].ease = Ease::Hold,
        KeyInterp::Linear => {
            set_out(&mut keys[i], 1.0 / 3.0, 1.0 / 3.0);
            if i > 0 {
                set_in(&mut keys[i - 1], 2.0 / 3.0, 2.0 / 3.0);
            }
        }
        KeyInterp::EasyEase | KeyInterp::EasyEaseIn | KeyInterp::EasyEaseOut => {
            if interp != KeyInterp::EasyEaseIn {
                set_out(&mut keys[i], EASY_EASE, 0.0);
            }
            if interp != KeyInterp::EasyEaseOut && i > 0 {
                set_in(&mut keys[i - 1], 1.0 - EASY_EASE, 1.0);
            }
        }
    }
}

impl KeyTracks {
    /// Grava (ou troca o valor de) uma chave no quadro; a chave que já existia mantém o easing.
    pub fn set_key(&mut self, prop: &str, frame: u32, value: f32) {
        let keys = self.0.entry(prop.to_owned()).or_default();
        match keys.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => keys[i].value = value,
            Err(i) => {
                // chave nova no meio de um trecho herda o easing do trecho (um degrau continua degrau)
                let ease = if i > 0 { keys[i - 1].ease } else { Ease::Linear };
                keys.insert(i, Key { frame, value, ease });
            }
        }
    }

    /// Tira a chave do quadro (a propriedade sem chave nenhuma sai do mapa). Devolve se havia.
    pub fn remove_key(&mut self, prop: &str, frame: u32) -> bool {
        let Some(keys) = self.0.get_mut(prop) else { return false };
        let before = keys.len();
        keys.retain(|k| k.frame != frame);
        let removed = keys.len() != before;
        if keys.is_empty() {
            self.0.remove(prop);
        }
        removed
    }

    /// Quadros com chave da propriedade.
    pub fn frames(&self, prop: &str) -> Vec<u32> {
        self.0.get(prop).map(|k| k.iter().map(|k| k.frame).collect()).unwrap_or_default()
    }

    pub fn has(&self, prop: &str) -> bool {
        self.0.get(prop).is_some_and(|k| !k.is_empty())
    }

    /// Desloca as chaves dos quadros `frames` em `delta` quadros (mínimo 0). Chave que cai em cima de outra que NÃO se
    /// move a substitui (como arrastar um losango sobre outro no After Effects). Erro se o deslocamento faria duas
    /// chaves movidas caírem no mesmo quadro (só acontece no limite 0).
    pub fn move_keys(&mut self, prop: &str, frames: &[u32], delta: i64) -> Result<(), String> {
        let Some(keys) = self.0.get_mut(prop) else { return Ok(()) };
        let moving: Vec<Key> = keys.iter().filter(|k| frames.contains(&k.frame)).copied().collect();
        if moving.is_empty() || delta == 0 {
            return Ok(());
        }
        let moved: Vec<Key> = moving.iter().map(|k| Key { frame: (k.frame as i64 + delta).max(0) as u32, ..*k }).collect();
        let mut seen = std::collections::BTreeSet::new();
        if !moved.iter().all(|k| seen.insert(k.frame)) {
            return Err(format!("'{prop}': chaves se sobrepõem no quadro 0"));
        }
        keys.retain(|k| !frames.contains(&k.frame) && !seen.contains(&k.frame));
        keys.extend(moved);
        keys.sort_by_key(|k| k.frame);
        Ok(())
    }

    /// Aplica a interpolação às chaves dos quadros `frames` (todas, se `None`).
    pub fn set_interp(&mut self, prop: &str, frames: Option<&[u32]>, interp: KeyInterp) {
        let Some(keys) = self.0.get_mut(prop) else { return };
        for i in 0..keys.len() {
            if frames.is_none_or(|f| f.contains(&keys[i].frame)) {
                apply_interp(keys, i, interp);
            }
        }
    }

    /// Troca o easing (do trecho que começa na chave) das chaves dos quadros `frames` (todas, se `None`).
    pub fn set_ease(&mut self, prop: &str, frames: Option<&[u32]>, ease: Ease) {
        if let Some(keys) = self.0.get_mut(prop) {
            for k in keys.iter_mut().filter(|k| frames.is_none_or(|f| f.contains(&k.frame))) {
                k.ease = ease;
            }
        }
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

    fn k(frame: u32, value: f32) -> Key {
        Key { frame, value, ease: Ease::Linear }
    }

    #[test]
    fn interpolation_linear_hold_and_easy_ease_evaluate_as_expected() {
        let mut t = KeyTracks::default();
        t.0.insert("x".into(), vec![k(0, 0.0), k(10, 100.0), k(20, 0.0)]);
        // linear: metade do caminho no meio do trecho
        assert_eq!(t.eval("x", 5.0), Some(50.0));
        // hold: segura até a próxima chave
        t.set_interp("x", Some(&[0]), KeyInterp::Hold);
        assert_eq!(t.eval("x", 9.0), Some(0.0));
        assert_eq!(t.eval("x", 10.0), Some(100.0));
        // de volta ao linear
        t.set_interp("x", Some(&[0]), KeyInterp::Linear);
        assert_eq!(t.0["x"][0].ease, Ease::Linear);
        assert!((t.eval("x", 5.0).unwrap() - 50.0).abs() < 1e-3);
        // Easy Ease na chave do meio: chega devagar (abaixo da reta perto do fim do 1º trecho) e sai devagar
        t.set_interp("x", Some(&[10]), KeyInterp::EasyEase);
        assert_eq!(t.0["x"][0].ease, Ease::Bezier { x1: 1.0 / 3.0, y1: 1.0 / 3.0, x2: 1.0 - EASY_EASE, y2: 1.0 });
        assert_eq!(t.0["x"][1].ease, Ease::Bezier { x1: EASY_EASE, y1: 0.0, x2: 2.0 / 3.0, y2: 2.0 / 3.0 });
        let near_in = t.eval("x", 9.0).unwrap();
        assert!(near_in > 90.0, "chegando devagar perto da chave: {near_in}");
        let near_out = t.eval("x", 11.0).unwrap();
        assert!(near_out > 90.0, "saindo devagar da chave: {near_out}");
        // Easy Ease nas duas pontas do 1º trecho = curva S simétrica: meio exato, começo lento
        t.set_interp("x", Some(&[0]), KeyInterp::EasyEase);
        assert!((t.eval("x", 5.0).unwrap() - 50.0).abs() < 0.5);
        assert!(t.eval("x", 1.0).unwrap() < 10.0 * 0.6);
        // linear de novo nas duas chaves desfaz as alças
        t.set_interp("x", None, KeyInterp::Linear);
        assert!(t.0["x"].iter().all(|k| k.ease == Ease::Linear), "{:?}", t.0["x"]);
        // só na chegada / só na saída
        t.set_interp("x", Some(&[10]), KeyInterp::EasyEaseIn);
        assert_eq!(t.0["x"][1].ease, Ease::Linear);
        assert!(matches!(t.0["x"][0].ease, Ease::Bezier { y2, .. } if y2 == 1.0));
        t.set_interp("x", Some(&[10]), KeyInterp::EasyEaseOut);
        assert!(matches!(t.0["x"][1].ease, Ease::Bezier { y1, .. } if y1 == 0.0));
    }

    #[test]
    fn set_move_and_remove_keys() {
        let mut t = KeyTracks::default();
        t.set_key("x", 10, 1.0);
        t.set_key("x", 0, 0.0);
        t.set_key("x", 5, 0.5);
        assert_eq!(t.frames("x"), vec![0, 5, 10]);
        t.set_ease("x", Some(&[0]), Ease::Hold);
        // chave nova no meio de um degrau herda o degrau
        t.set_key("x", 2, 0.2);
        assert_eq!(t.0["x"][1].ease, Ease::Hold);
        // mover para cima de outra substitui a outra
        t.move_keys("x", &[2], 3).unwrap();
        assert_eq!(t.frames("x"), vec![0, 5, 10]);
        assert_eq!(t.eval("x", 5.0), Some(0.2));
        // mover em bloco mantém a ordem; não passa do quadro 0
        t.move_keys("x", &[0, 5], -4).unwrap();
        assert_eq!(t.frames("x"), vec![0, 1, 10]);
        assert!(t.move_keys("x", &[0, 1], -5).is_err());
        assert!(t.validate().is_ok());
        assert!(t.remove_key("x", 1) && !t.remove_key("x", 1));
        t.remove_key("x", 0);
        t.remove_key("x", 10);
        assert!(t.is_empty(), "propriedade sem chaves sai do mapa");
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

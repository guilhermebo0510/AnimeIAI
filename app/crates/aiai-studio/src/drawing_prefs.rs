//! Preferências do lado esquerdo das áreas de desenho (S25): os tamanhos prontos da paleta Tamanho do pincel, quais
//! parâmetros cada sub-ferramenta mostra no painel Propriedades da ferramenta (o "olho" das configurações avançadas) e o
//! histórico de cores. Vão em `prefs.json` (campo `drawing`); o que faltar no arquivo volta ao padrão.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Tamanhos de fábrica da paleta (px), como os de um programa de pintura: finos para linha, grossos para cor.
pub const DEFAULT_SIZES: [f32; 28] = [0.7, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0, 15.0, 17.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 100.0, 120.0, 150.0, 170.0, 200.0];

/// Parâmetros à vista no painel Propriedades da ferramenta quando a sub-ferramenta ainda não tem escolha própria.
pub const DEFAULT_SHOWN: [&str; 4] = ["size", "opacity", "aa", "stabilizer"];

/// Quantas cores o histórico guarda.
pub const HISTORY_MAX: usize = 40;

/// Limites de um tamanho da paleta (px).
pub const SIZE_MIN: f32 = 0.1;
pub const SIZE_MAX: f32 = 2000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawingPrefs {
    /// Tamanhos prontos da paleta Tamanho do pincel (px, em ordem crescente, sem repetir).
    pub sizes: Vec<f32>,
    /// Parâmetros à vista por sub-ferramenta: chave da sub-ferramenta (id ou nome do pincel, `pen:g`...) → chaves dos
    /// parâmetros. Sem entrada = `DEFAULT_SHOWN`.
    pub shown: BTreeMap<String, Vec<String>>,
    /// Últimas cores usadas para desenhar, a mais recente primeiro ("#rrggbb").
    pub color_history: Vec<String>,
}

impl Default for DrawingPrefs {
    fn default() -> Self {
        DrawingPrefs { sizes: DEFAULT_SIZES.to_vec(), shown: BTreeMap::new(), color_history: Vec::new() }
    }
}

impl DrawingPrefs {
    /// Valores dentro dos limites (um JSON editado à mão nunca quebra a paleta).
    pub fn sanitized(mut self) -> DrawingPrefs {
        self.sizes = clean_sizes(self.sizes);
        if self.sizes.is_empty() {
            self.sizes = DEFAULT_SIZES.to_vec();
        }
        self.color_history.retain(|c| parse_hex(c).is_some());
        self.color_history.truncate(HISTORY_MAX);
        self
    }

    /// Parâmetros à vista da sub-ferramenta `key`.
    pub fn shown_for(&self, key: &str) -> Vec<String> {
        self.shown.get(key).cloned().unwrap_or_else(|| DEFAULT_SHOWN.iter().map(|s| (*s).to_owned()).collect())
    }

    pub fn is_shown(&self, key: &str, param: &str) -> bool {
        match self.shown.get(key) {
            Some(v) => v.iter().any(|p| p == param),
            None => DEFAULT_SHOWN.contains(&param),
        }
    }

    /// Liga/desliga o olho de um parâmetro. Voltar exatamente ao padrão tira a entrada (o arquivo fica pequeno).
    pub fn set_shown(&mut self, key: &str, param: &str, on: bool) {
        let mut v = self.shown_for(key);
        v.retain(|p| p != param);
        if on {
            v.push(param.to_owned());
        }
        let mut sorted = v.clone();
        sorted.sort();
        let mut def: Vec<String> = DEFAULT_SHOWN.iter().map(|s| (*s).to_owned()).collect();
        def.sort();
        if sorted == def {
            self.shown.remove(key);
        } else {
            self.shown.insert(key.to_owned(), v);
        }
    }

    /// Põe a cor no topo do histórico (sem repetir). Devolve se mudou.
    pub fn push_color(&mut self, rgb: [u8; 3]) -> bool {
        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        if self.color_history.first() == Some(&hex) {
            return false;
        }
        self.color_history.retain(|c| *c != hex);
        self.color_history.insert(0, hex);
        self.color_history.truncate(HISTORY_MAX);
        true
    }

    pub fn history_rgb(&self) -> Vec<[u8; 3]> {
        self.color_history.iter().filter_map(|c| parse_hex(c)).collect()
    }
}

/// Tamanhos válidos em ordem crescente e sem repetir (0,05 px de tolerância).
pub fn clean_sizes(mut v: Vec<f32>) -> Vec<f32> {
    v.retain(|s| s.is_finite() && (SIZE_MIN..=SIZE_MAX).contains(s));
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    v
}

/// Lê a lista editada pelo usuário ("0,7 1 1,5; 2 ..."): números separados por espaço ou ponto e vírgula, vírgula ou
/// ponto decimal. Erro diz qual pedaço não é tamanho.
pub fn parse_sizes(text: &str) -> Result<Vec<f32>, String> {
    let mut out = Vec::new();
    for part in text.split(|c: char| c.is_whitespace() || c == ';').filter(|p| !p.is_empty()) {
        let v: f32 = part.replace(',', ".").parse().map_err(|_| part.to_owned())?;
        if !v.is_finite() || !(SIZE_MIN..=SIZE_MAX).contains(&v) {
            return Err(part.to_owned());
        }
        out.push(v);
    }
    let out = clean_sizes(out);
    if out.is_empty() {
        return Err(text.trim().to_owned());
    }
    Ok(out)
}

/// Texto da lista para editar ("0,7 1 1,5 2"), com vírgula decimal.
pub fn sizes_text(v: &[f32]) -> String {
    v.iter().map(|s| format_size(*s)).collect::<Vec<_>>().join(" ")
}

/// "0,7", "10", "2,5".
pub fn format_size(s: f32) -> String {
    if (s - s.round()).abs() < 0.05 { format!("{}", s.round() as i64) } else { format!("{s:.1}").replace('.', ",") }
}

fn parse_hex(c: &str) -> Option<[u8; 3]> {
    let h = c.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eye_toggles_default_and_round_trip_through_json() {
        let mut p = DrawingPrefs::default();
        assert!(p.is_shown("Redondo duro", "size") && p.is_shown("x", "aa") && !p.is_shown("x", "spacing"));
        p.set_shown("Redondo duro", "spacing", true);
        p.set_shown("Redondo duro", "opacity", false);
        assert!(p.is_shown("Redondo duro", "spacing") && !p.is_shown("Redondo duro", "opacity"));
        // outra sub-ferramenta não muda
        assert!(p.is_shown("pen:g", "opacity"));
        let json = serde_json::to_string(&p).unwrap();
        let back: DrawingPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        // voltar ao padrão tira a entrada
        p.set_shown("Redondo duro", "spacing", false);
        p.set_shown("Redondo duro", "opacity", true);
        assert!(p.shown.is_empty(), "{:?}", p.shown);
        // arquivo antigo sem o campo: padrão (serde default)
        let old: DrawingPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(old, DrawingPrefs::default());
        let partial: DrawingPrefs = serde_json::from_str(r#"{"shown":{"a":["texture"]}}"#).unwrap();
        assert_eq!(partial.sizes, DEFAULT_SIZES.to_vec());
        assert!(partial.is_shown("a", "texture") && !partial.is_shown("a", "size"));
    }

    #[test]
    fn size_list_is_parsed_sorted_and_bounded() {
        assert_eq!(parse_sizes("3 1 2,5; 1").unwrap(), vec![1.0, 2.5, 3.0]);
        assert_eq!(parse_sizes("0.7 200").unwrap(), vec![0.7, 200.0]);
        assert_eq!(parse_sizes("1 abc").unwrap_err(), "abc");
        assert_eq!(parse_sizes("0 5").unwrap_err(), "0");
        assert!(parse_sizes("   ").is_err());
        assert_eq!(sizes_text(&[0.7, 1.0, 2.5, 200.0]), "0,7 1 2,5 200");
        assert_eq!(parse_sizes(&sizes_text(&DEFAULT_SIZES)).unwrap(), DEFAULT_SIZES.to_vec());
        let bad = DrawingPrefs { sizes: vec![f32::NAN, -1.0, 5000.0], ..Default::default() }.sanitized();
        assert_eq!(bad.sizes, DEFAULT_SIZES.to_vec(), "lista vazia depois de limpar volta à de fábrica");
    }

    #[test]
    fn color_history_keeps_the_latest_first_without_repeats() {
        let mut p = DrawingPrefs::default();
        assert!(p.push_color([255, 0, 0]));
        assert!(p.push_color([0, 0, 255]));
        assert!(!p.push_color([0, 0, 255]), "a mesma cor de novo não mexe");
        assert!(p.push_color([255, 0, 0]));
        assert_eq!(p.history_rgb(), vec![[255, 0, 0], [0, 0, 255]]);
        for i in 0..100u8 {
            p.push_color([i, i, i]);
        }
        assert_eq!(p.color_history.len(), HISTORY_MAX);
        p.color_history.push("lixo".into());
        assert_eq!(p.sanitized().color_history.len(), HISTORY_MAX);
    }
}

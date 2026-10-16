//! Tempos de fala: moras e fonemas com início/fim em segundos e a classe de vogal (base do lip-sync).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Classe de vogal de uma mora (alfabeto de fonemas do VOICEVOX: a i u e o N cl pau; maiúsculas = vogal desvozeada).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VowelClass {
    A,
    I,
    U,
    E,
    O,
    /// ん (boca fechada).
    N,
    /// っ (fechamento antes de consoante dupla).
    Cl,
    /// Pausa.
    Pau,
}

impl VowelClass {
    pub fn from_phoneme(p: &str) -> Option<(VowelClass, bool)> {
        let devoiced = p.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && p != "N";
        let c = match p {
            "a" | "A" => VowelClass::A,
            "i" | "I" => VowelClass::I,
            "u" | "U" => VowelClass::U,
            "e" | "E" => VowelClass::E,
            "o" | "O" => VowelClass::O,
            "N" => VowelClass::N,
            "cl" => VowelClass::Cl,
            "pau" => VowelClass::Pau,
            _ => return None,
        };
        Some((c, devoiced))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SegKind {
    Consonant,
    Vowel,
    Pause,
    /// Silêncio inicial/final da síntese (pre/post phoneme length).
    Silence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhonemeSeg {
    pub phoneme: String,
    pub kind: SegKind,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MoraTiming {
    /// Kana da mora ("ハ"); "、" etc. para pausas.
    pub text: String,
    pub consonant: Option<String>,
    pub vowel: String,
    pub class: VowelClass,
    pub devoiced: bool,
    /// Início da mora (início da consoante, se houver).
    pub start: f64,
    /// Início da vogal.
    pub vowel_start: f64,
    pub end: f64,
    /// Índice do sintagma acentual (frase) a que pertence.
    pub phrase: usize,
    /// Altura (log F0) prevista pelo engine; 0 em pausas.
    pub pitch: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Timings {
    pub moras: Vec<MoraTiming>,
    pub phonemes: Vec<PhonemeSeg>,
    /// Duração total prevista (s), incluindo silêncios inicial e final.
    pub duration: f64,
    /// Fala útil: do fim do silêncio inicial ao começo do final.
    pub speech_start: f64,
    pub speech_end: f64,
}

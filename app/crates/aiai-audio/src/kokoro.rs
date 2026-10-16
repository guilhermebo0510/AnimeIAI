//! Kokoro-82M (ONNX, Apache-2.0) para PT-BR/EN: chama `tools/kokoro/kokoro_tts.py` como PROCESSO separado via `uv run --script`
//! (o script carrega kokoro-onnx + espeak-ng GPL-3; o app nunca liga essas bibliotecas, só lê o WAV que o processo grava).
//! O modelo não expõe fonemas nem durações: o lip-sync dessas falas usa a energia do áudio.

use crate::buf::{AudioError, AudioResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct KokoroParams {
    /// Voz do Kokoro: `pf_dora`, `pm_alex`, `pm_santa` (PT-BR); `af_heart`, `am_adam`... (EN).
    #[serde(default = "d_voice")]
    pub voice: String,
    /// Idioma do fonemizador: `pt-br`, `en-us`, `en-gb`.
    #[serde(default = "d_lang")]
    pub lang: String,
    #[serde(default = "d_speed")]
    pub speed: f32,
}

fn d_voice() -> String {
    "pf_dora".into()
}
fn d_lang() -> String {
    "pt-br".into()
}
fn d_speed() -> f32 {
    1.0
}

impl Default for KokoroParams {
    fn default() -> Self {
        KokoroParams { voice: d_voice(), lang: d_lang(), speed: 1.0 }
    }
}

/// Procura `tools/kokoro/kokoro_tts.py` subindo a partir do diretório atual e do executável.
pub fn find_script() -> Option<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Ok(e) = std::env::current_exe() {
        if let Some(d) = e.parent() {
            starts.push(d.to_path_buf());
        }
    }
    for s in starts {
        for dir in s.ancestors() {
            let cand = dir.join("tools").join("kokoro").join("kokoro_tts.py");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Modelos presentes em `sidecars/kokoro` ao lado do repositório do script?
pub fn models_present(script: &Path) -> bool {
    script.ancestors().nth(3).map(|root| root.join("sidecars").join("kokoro").join("kokoro-v1.0.onnx").is_file() && root.join("sidecars").join("kokoro").join("voices-v1.0.bin").is_file()).unwrap_or(false)
}

/// Sintetiza e devolve o WAV (PCM16 mono 24 kHz).
pub fn synthesize(script: &Path, text: &str, p: &KokoroParams) -> AudioResult<Vec<u8>> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    if text.trim().is_empty() {
        return Err(AudioError::Invalid("texto vazio".into()));
    }
    let out = std::env::temp_dir().join(format!("aiai_kokoro_{}_{}.wav", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let o = Command::new("uv")
        .arg("run")
        .arg("--script")
        .arg(script)
        .args(["--text", text, "--voice", &p.voice, "--lang", &p.lang, "--speed", &p.speed.to_string(), "--out"])
        .arg(&out)
        .output()
        .map_err(|e| AudioError::Invalid(format!("não consegui executar `uv` (instale uv): {e}")))?;
    if !o.status.success() {
        return Err(AudioError::Invalid(format!("kokoro falhou ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or(""))));
    }
    let bytes = std::fs::read(&out).map_err(|e| AudioError::Invalid(format!("WAV do kokoro ausente: {e}")))?;
    let _ = std::fs::remove_file(&out);
    Ok(bytes)
}

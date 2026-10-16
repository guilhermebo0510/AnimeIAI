//! Preferências do usuário, fora do projeto: `%APPDATA%\AnimeIAI\prefs.json` (tema, idioma, último workspace, zoom da UI).
//! Com `--no-session` nada é lido nem gravado; `--session-file <json>` troca o arquivo.

use crate::i18n::Lang;
use crate::theme::ThemeKind;
use crate::workspace::WorkspaceId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_ZOOM: f32 = 0.75;
pub const MAX_ZOOM: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeKind,
    pub lang: Lang,
    pub workspace: WorkspaceId,
    pub ui_zoom: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: ThemeKind::Dark, lang: Lang::Pt, workspace: WorkspaceId::default(), ui_zoom: 1.0 }
    }
}

impl Prefs {
    /// Zoom dentro dos limites (um JSON editado à mão nunca deixa a UI ilegível).
    pub fn sanitized(mut self) -> Prefs {
        self.ui_zoom = if self.ui_zoom.is_finite() { self.ui_zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 };
        self
    }
}

/// `%APPDATA%\AnimeIAI\prefs.json`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(base).join("AnimeIAI").join("prefs.json"))
}

/// Lê as preferências. Arquivo ausente ou ilegível devolve o padrão (o ilegível é registrado, nunca derruba o app).
pub fn load(path: &Path) -> Prefs {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Prefs>(&s) {
            Ok(p) => p.sanitized(),
            Err(e) => {
                tracing::warn!("preferências ilegíveis em {}: {e}; usando o padrão", path.display());
                Prefs::default()
            }
        },
        Err(_) => Prefs::default(),
    }
}

/// Grava de forma atômica (arquivo temporário + rename).
pub fn save(path: &Path, prefs: &Prefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-prefs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("prefs.json")
    }

    #[test]
    fn round_trip() {
        let p = tmp("rt");
        let prefs = Prefs { theme: ThemeKind::Light, lang: Lang::En, workspace: WorkspaceId::Animacao, ui_zoom: 1.25 };
        save(&p, &prefs).unwrap();
        assert_eq!(load(&p), prefs);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn missing_and_corrupt_files_give_defaults() {
        let p = tmp("bad");
        assert_eq!(load(&p), Prefs::default());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ nao e json").unwrap();
        assert_eq!(load(&p), Prefs::default());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn partial_json_fills_defaults_and_zoom_is_clamped() {
        let p = tmp("partial");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"lang":"en","ui_zoom":9.0}"#).unwrap();
        let l = load(&p);
        assert_eq!((l.lang, l.theme, l.ui_zoom), (Lang::En, ThemeKind::Dark, MAX_ZOOM));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}

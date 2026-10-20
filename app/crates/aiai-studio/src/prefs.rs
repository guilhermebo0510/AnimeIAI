//! Preferências do usuário, fora do projeto: `%APPDATA%\AnimeIAI\prefs.json` (tema, idioma, último workspace, zoom da UI).
//! Com `--no-session` nada é lido nem gravado; `--session-file <json>` troca o arquivo.

use crate::i18n::Lang;
use crate::theme::ThemeKind;
use crate::workspace::WorkspaceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// Projetos abertos/salvos por último (mais recente primeiro, no máximo `MAX_RECENT`).
    pub recent: Vec<PathBuf>,
    /// Mostrar a tela inicial ao abrir o app sem `--project`.
    pub show_start: bool,
    /// Atalhos que diferem do padrão: id da ação → "Ctrl+Shift+Z" ("" = sem atalho). Ver `keymap.rs`.
    pub shortcuts: BTreeMap<String, String>,
    /// Pasta onde os diálogos Abrir/Salvar começam (vazio: `C:\AnimeIAI\producoes`, se existir).
    pub projects_dir: Option<PathBuf>,
    /// GPU usada pela interface e pelo viewport (vale ao abrir o programa de novo).
    pub gpu: GpuPref,
    /// Curva de pressão da caneta: pressão mínima (o traço nunca some) e expoente (< 1 = mão leve, > 1 = mão pesada).
    pub pen_pressure_min: f32,
    pub pen_pressure_gamma: f32,
    /// Autor e licença usados nos pincéis novos e preenchidos na exportação (os últimos valores digitados).
    pub brush_author: BrushAuthorPrefs,
    /// Cursor das ferramentas de pintura sempre com a mira fina junto do contorno (Caps Lock liga na hora).
    pub cursor_precise: bool,
    /// Orçamento de memória de vídeo em MB (0 = automático: 60% da placa dividido pelas instâncias abertas; `gpu_mem`).
    pub gpu_budget_mb: u32,
}

/// Autor e licença dos pincéis do usuário (dados do perfil de criador; vão no manifesto do `.aiaibrush`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushAuthorPrefs {
    pub name: String,
    pub email: String,
    pub url: String,
    /// Id SPDX ou texto; vazio = "Proprietário / todos os direitos reservados".
    pub license: String,
}

/// Pressão da caneta pela curva das preferências: `min + (1 − min) · f^gamma`, presa em [0,02; 1].
pub fn pressure_curve(force: f32, min: f32, gamma: f32) -> f32 {
    let f = force.clamp(0.0, 1.0);
    (min + (1.0 - min) * f.powf(gamma)).clamp(0.02, 1.0)
}

/// Preferência de GPU do wgpu: a dedicada (padrão, GTX do notebook) ou a integrada (economiza bateria).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuPref {
    #[default]
    Dedicated,
    Integrated,
}

/// Lê só as preferências que valem antes da janela existir (GPU), com as mesmas regras do app: nada com `--no-session`.
pub fn load_for_startup(no_session: bool, session_file: Option<&str>) -> Prefs {
    if no_session {
        return Prefs::default();
    }
    session_file.map(PathBuf::from).or_else(default_path).map(|p| load(&p)).unwrap_or_default()
}

pub const MAX_RECENT: usize = 10;

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: ThemeKind::Dark, lang: Lang::Pt, workspace: WorkspaceId::default(), ui_zoom: 1.0, recent: Vec::new(), show_start: true, shortcuts: BTreeMap::new(), projects_dir: None, gpu: GpuPref::Dedicated, pen_pressure_min: 0.0, pen_pressure_gamma: 1.0, brush_author: BrushAuthorPrefs::default(), cursor_precise: false, gpu_budget_mb: 0 }
    }
}

impl Prefs {
    /// Zoom dentro dos limites (um JSON editado à mão nunca deixa a UI ilegível).
    pub fn sanitized(mut self) -> Prefs {
        self.ui_zoom = if self.ui_zoom.is_finite() { self.ui_zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 };
        self.recent.truncate(MAX_RECENT);
        self.pen_pressure_min = if self.pen_pressure_min.is_finite() { self.pen_pressure_min.clamp(0.0, 0.6) } else { 0.0 };
        self.pen_pressure_gamma = if self.pen_pressure_gamma.is_finite() { self.pen_pressure_gamma.clamp(0.3, 3.0) } else { 1.0 };
        self
    }

    /// Põe `path` no topo dos recentes, sem duplicar (compara sem caixa, com `/` = `\` e sem barra final).
    pub fn push_recent(&mut self, path: &Path) {
        let key = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
        let k = key(path);
        self.recent.retain(|p| key(p) != k);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
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
        let prefs = Prefs { theme: ThemeKind::Light, lang: Lang::En, workspace: WorkspaceId::Animacao, ui_zoom: 1.25, recent: vec![PathBuf::from(r"C:\x\a.aiai")], show_start: false, shortcuts: [("tool.fill".to_owned(), "K".to_owned())].into(), projects_dir: Some(PathBuf::from(r"D:\anim")), gpu: GpuPref::Integrated, pen_pressure_min: 0.1, pen_pressure_gamma: 1.5, brush_author: BrushAuthorPrefs { name: "Ana".into(), email: "ana@exemplo.com".into(), url: String::new(), license: "CC-BY-4.0".into() }, cursor_precise: true, gpu_budget_mb: 1024 };
        save(&p, &prefs).unwrap();
        assert_eq!(load(&p), prefs);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn recent_moves_to_top_without_duplicates() {
        let mut p = Prefs::default();
        for i in 0..12 {
            p.push_recent(&PathBuf::from(format!(r"C:\p\{i}.aiai")));
        }
        assert_eq!(p.recent.len(), MAX_RECENT);
        p.push_recent(Path::new("c:/P/5.aiai/"));
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0], PathBuf::from("c:/P/5.aiai/"));
        assert_eq!(p.recent.iter().filter(|r| r.to_string_lossy().to_lowercase().contains("5.aiai")).count(), 1);
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

    #[test]
    fn pressure_curve_keeps_a_floor_and_bends_with_gamma() {
        assert!((pressure_curve(1.0, 0.2, 2.0) - 1.0).abs() < 1e-6);
        assert!((pressure_curve(0.0, 0.2, 2.0) - 0.2).abs() < 1e-6, "mínimo vale com caneta encostando de leve");
        assert!(pressure_curve(0.5, 0.0, 2.0) < 0.3 && pressure_curve(0.5, 0.0, 0.5) > 0.7, "gamma > 1 pede mais força");
        assert_eq!(pressure_curve(0.5, 0.0, 1.0), 0.5);
        let p = Prefs { pen_pressure_min: 9.0, pen_pressure_gamma: f32::NAN, ..Prefs::default() }.sanitized();
        assert_eq!((p.pen_pressure_min, p.pen_pressure_gamma), (0.6, 1.0));
    }
}

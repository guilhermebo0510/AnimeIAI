//! Argumentos de linha de comando (sem dependência externa).
//!
//! ```text
//! aiai-studio [--no-session] [--session-file <json>] [--project <pasta>] [--lang pt|en]
//!             [--theme light|dark] [--workspace <chave>] [--size LxA] [--bridge]
//!             [--screenshot <png>] [--frames <n>]
//! ```
//!
//! `--bridge` só é lido e guardado aqui; a bridge local entra na S02.

use crate::i18n::Lang;
use crate::theme::ThemeKind;
use crate::workspace::WorkspaceId;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cli {
    pub workspace: Option<WorkspaceId>,
    pub screenshot: Option<String>,
    /// Quadros a renderizar antes do screenshot (deixa a UI estabilizar).
    pub frames: u32,
    pub size: Option<(f32, f32)>,
    /// Arquivo de preferências alternativo (substitui `%APPDATA%\AnimeIAI\prefs.json`).
    pub session_file: Option<String>,
    /// Nada é lido nem gravado das preferências.
    pub no_session: bool,
    pub project: Option<String>,
    pub lang: Option<Lang>,
    pub theme: Option<ThemeKind>,
    /// Liga a bridge de automação (S02). Por enquanto só é guardado.
    pub bridge: bool,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli { frames: 8, ..Default::default() };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} exige um valor"));
        match arg.as_str() {
            "--workspace" => {
                let v = value("--workspace")?;
                cli.workspace = Some(WorkspaceId::from_key(&v).ok_or_else(|| format!("workspace desconhecido: {v}"))?);
            }
            "--screenshot" => cli.screenshot = Some(value("--screenshot")?),
            "--frames" => cli.frames = value("--frames")?.parse().map_err(|_| "--frames precisa ser inteiro".to_string())?,
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v.split_once(['x', 'X']).ok_or("--size usa o formato LARGURAxALTURA")?;
                let (w, h): (f32, f32) = (w.parse().map_err(|_| "largura inválida".to_string())?, h.parse().map_err(|_| "altura inválida".to_string())?);
                if w < 320.0 || h < 240.0 {
                    return Err("--size mínimo: 320x240".into());
                }
                cli.size = Some((w, h));
            }
            "--session-file" => cli.session_file = Some(value("--session-file")?),
            "--no-session" => cli.no_session = true,
            "--project" => cli.project = Some(value("--project")?),
            "--bridge" => cli.bridge = true,
            "--lang" => {
                let v = value("--lang")?;
                cli.lang = Some(Lang::from_code(&v).ok_or_else(|| format!("idioma desconhecido: {v}"))?);
            }
            "--theme" => {
                let v = value("--theme")?;
                cli.theme = Some(ThemeKind::from_key(&v).ok_or_else(|| format!("tema desconhecido: {v}"))?);
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cli, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_all_flags() {
        let c = p(&[
            "--workspace", "pintura", "--screenshot", "a.png", "--frames", "3", "--size", "1280x720", "--no-session", "--lang", "en", "--theme", "light",
            "--project", "C:/x", "--session-file", "s.json", "--bridge",
        ])
        .unwrap();
        assert_eq!(c.workspace, Some(WorkspaceId::Pintura));
        assert_eq!(c.screenshot.as_deref(), Some("a.png"));
        assert_eq!((c.frames, c.size), (3, Some((1280.0, 720.0))));
        assert!(c.no_session && c.bridge);
        assert_eq!(c.lang, Some(Lang::En));
        assert_eq!(c.theme, Some(ThemeKind::Light));
        assert_eq!(c.project.as_deref(), Some("C:/x"));
        assert_eq!(c.session_file.as_deref(), Some("s.json"));
    }

    #[test]
    fn rejects_bad_input_with_messages() {
        assert!(p(&["--workspace", "x"]).unwrap_err().contains("desconhecido"));
        assert!(p(&["--frames"]).unwrap_err().contains("valor"));
        assert!(p(&["--size", "100"]).is_err());
        assert!(p(&["--lang", "fr"]).is_err());
        assert!(p(&["--theme", "x"]).is_err());
        assert!(p(&["--wat"]).is_err());
    }

    #[test]
    fn defaults_are_inert() {
        let c = p(&[]).unwrap();
        assert_eq!(c.frames, 8);
        assert!(!c.bridge && !c.no_session && c.screenshot.is_none() && c.workspace.is_none());
    }
}

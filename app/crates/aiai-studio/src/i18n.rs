//! Internacionalização: português (padrão) e inglês, troca em tempo real. A tabela é Rust (chave, pt, en);
//! os testes garantem que nenhuma chave falta em um idioma e que toda chave usada no código existe.
//! Japonês entra com a fonte Noto Sans JP já embutida (`theme::install_fonts`), quando houver textos.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Pt,
    En,
}

impl Lang {
    /// `pt`, `pt-BR`, `en` (insensível a maiúsculas).
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.to_ascii_lowercase().as_str() {
            "pt" | "pt-br" | "pt_br" => Some(Lang::Pt),
            "en" | "en-us" | "en_us" => Some(Lang::En),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Pt,
    }
}

/// Traduz `key` no idioma atual. Chave desconhecida devolve a própria chave (visível na UI = fácil de notar;
/// o teste `used_keys_exist` impede isso em produção).
pub fn tr(key: &str) -> &str {
    tr_in(lang(), key)
}

fn index() -> &'static std::collections::HashMap<&'static str, (&'static str, &'static str)> {
    static IDX: std::sync::OnceLock<std::collections::HashMap<&'static str, (&'static str, &'static str)>> = std::sync::OnceLock::new();
    IDX.get_or_init(|| TABLE.iter().map(|(k, pt, en)| (*k, (*pt, *en))).collect())
}

pub fn tr_in(lang: Lang, key: &str) -> &str {
    if let Some((pt, en)) = index().get(key) {
        return match lang {
            Lang::Pt => pt,
            Lang::En => en,
        };
    }
    key
}

/// (chave, pt, en)
pub const TABLE: &[(&str, &str, &str)] = &[
    // Workspaces
    ("workspace.roteiro", "Roteiro", "Script"),
    ("workspace.personagens", "Personagens", "Characters"),
    ("workspace.storyboard", "Storyboard", "Storyboard"),
    ("workspace.layout", "Layout", "Layout"),
    ("workspace.animacao", "Animação", "Animation"),
    ("workspace.pintura", "Pintura", "Painting"),
    ("workspace.cenario", "Cenário", "Backgrounds"),
    ("workspace.composicao", "Composição", "Compositing"),
    ("workspace.cor", "Cor", "Color"),
    ("workspace.audio", "Áudio", "Audio"),
    ("workspace.edicao", "Edição", "Editing"),
    ("workspace.entrega", "Entrega", "Delivery"),
    ("workspace.producao", "Produção", "Production"),
    // Ferramentas
    ("tool.move", "Mover", "Move"),
    ("tool.brush", "Pincel", "Brush"),
    ("tool.pen", "Caneta vetorial", "Vector pen"),
    ("tool.eraser", "Borracha", "Eraser"),
    ("tool.fill", "Balde de tinta", "Fill bucket"),
    ("tool.lasso", "Seleção laço", "Lasso select"),
    ("tool.eyedropper", "Conta-gotas", "Eyedropper"),
    ("tool.hand", "Mão", "Hand"),
    ("tool.zoom", "Zoom", "Zoom"),
    // Painéis: título e estado vazio
    ("panel.canvas.title", "Canvas", "Canvas"),
    ("panel.canvas.empty", "O canvas de desenho entra na S03. O contorno marca o quadro da câmera.", "The drawing canvas arrives in S03. The outline marks the camera frame."),
    ("panel.canvas.camera", "Câmera", "Camera"),
    ("panel.layers.title", "Camadas", "Layers"),
    ("panel.layers.empty", "Nenhuma camada. As camadas do desenho aparecerão aqui.", "No layers. The drawing's layers will appear here."),
    ("panel.properties.title", "Propriedades", "Properties"),
    ("panel.properties.empty", "Nada selecionado.", "Nothing selected."),
    ("panel.palette.title", "Cores", "Colors"),
    ("panel.palette.empty", "A paleta do projeto está vazia.", "The project palette is empty."),
    ("panel.navigator.title", "Navegador", "Navigator"),
    ("panel.navigator.empty", "Sem quadro aberto para navegar.", "No frame open to navigate."),
    ("panel.timeline.title", "Linha do tempo", "Timeline"),
    ("panel.timeline.empty", "A linha do tempo está vazia.", "The timeline is empty."),
    ("panel.script.title", "Roteiro", "Script"),
    ("panel.script.empty", "Escreva o roteiro aqui. O texto ainda não é salvo: o projeto entra na S01.", "Write the script here. Text is not saved yet: projects arrive in S01."),
    ("panel.outline.title", "Estrutura", "Outline"),
    ("panel.outline.empty", "Cenas e sequências do roteiro aparecerão aqui.", "The script's scenes and sequences will appear here."),
    ("panel.characters.title", "Personagens", "Characters"),
    ("panel.characters.empty", "Nenhum personagem criado.", "No characters created."),
    ("panel.sheet.title", "Folha de modelo", "Model sheet"),
    ("panel.sheet.empty", "A folha de modelo do personagem (frente, perfil e costas) entra aqui.", "The character model sheet (front, side and back) goes here."),
    ("panel.shots.title", "Cenas", "Shots"),
    ("panel.shots.empty", "Nenhuma cena criada.", "No shots created."),
    ("panel.board.title", "Prancha", "Board"),
    ("panel.board.empty", "Os quadros do storyboard aparecerão aqui.", "The storyboard panels will appear here."),
    ("panel.preview.title", "Monitor", "Monitor"),
    ("panel.preview.empty", "Sem vídeo para exibir.", "No video to show."),
    ("panel.media.title", "Mídia", "Media"),
    ("panel.media.empty", "Nenhuma mídia importada.", "No media imported."),
    ("panel.mixer.title", "Mixer", "Mixer"),
    ("panel.mixer.empty", "Nenhuma faixa de áudio.", "No audio tracks."),
    ("panel.presets.title", "Predefinições", "Presets"),
    ("panel.presets.empty", "Nenhuma predefinição de exportação.", "No export presets."),
    ("panel.queue.title", "Fila de exportação", "Export queue"),
    ("panel.queue.empty", "Nenhuma exportação na fila.", "No exports in the queue."),
    ("panel.tasks.title", "Tarefas", "Tasks"),
    ("panel.tasks.empty", "Nenhuma tarefa. O quadro de produção entra aqui.", "No tasks. The production board goes here."),
    ("panel.episodes.title", "Episódios", "Episodes"),
    ("panel.episodes.empty", "Nenhum episódio cadastrado.", "No episodes registered."),
    // Menus
    ("menu.soon", "disponível na S01", "available in S01"),
    ("menu.file", "Arquivo", "File"),
    ("menu.file.new", "Novo", "New"),
    ("menu.file.open", "Abrir", "Open"),
    ("menu.file.save", "Salvar", "Save"),
    ("menu.file.save_as", "Salvar como", "Save as"),
    ("menu.file.export", "Exportar", "Export"),
    ("menu.file.quit", "Sair", "Quit"),
    ("menu.edit", "Editar", "Edit"),
    ("menu.edit.undo", "Desfazer", "Undo"),
    ("menu.edit.redo", "Refazer", "Redo"),
    ("menu.edit.prefs", "Preferências", "Preferences"),
    ("menu.view", "Exibir", "View"),
    ("menu.view.theme", "Tema", "Theme"),
    ("menu.view.theme.light", "Claro", "Light"),
    ("menu.view.theme.dark", "Escuro", "Dark"),
    ("menu.view.language", "Idioma", "Language"),
    ("menu.view.zoom", "Zoom da interface", "Interface zoom"),
    ("menu.window", "Janela", "Window"),
    ("menu.window.reset", "Restaurar layout", "Reset layout"),
    ("menu.window.reset_all", "Restaurar todos os layouts", "Reset all layouts"),
    ("menu.help", "Ajuda", "Help"),
    ("menu.help.about", "Sobre o AnimeIAI", "About AnimeIAI"),
    ("lang.pt", "Português (Brasil)", "Português (Brasil)"),
    ("lang.en", "English", "English"),
    // Barra de status
    ("status.project", "Projeto", "Project"),
    ("status.no_project", "sem projeto", "no project"),
    ("status.memory", "Memória", "Memory"),
    // Sobre
    ("about.title", "Sobre", "About"),
    ("about.version", "Versão", "Version"),
    ("about.tagline", "Estúdio de animação 2D com ferramentas próprias.", "2D animation studio with its own tools."),
    ("about.credits", "© 2026 AnimeIAI. Fontes Noto Sans e Noto Sans JP, licença SIL OFL 1.1.", "© 2026 AnimeIAI. Noto Sans and Noto Sans JP fonts, SIL OFL 1.1 license."),
    // Preferências
    ("prefs.title", "Preferências", "Preferences"),
    ("prefs.theme", "Tema", "Theme"),
    ("prefs.language", "Idioma", "Language"),
    ("prefs.zoom", "Zoom da interface", "Interface zoom"),
    ("prefs.file", "Arquivo", "File"),
    ("prefs.no_session", "Sessão desativada: nada é salvo.", "Session disabled: nothing is saved."),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PanelKind;
    use crate::workspace::{ToolId, WorkspaceId};
    use std::collections::HashSet;

    #[test]
    fn no_key_is_missing_or_empty_in_either_language() {
        let mut seen = HashSet::new();
        for (k, pt, en) in TABLE {
            assert!(seen.insert(*k), "chave duplicada: {k}");
            assert!(!k.is_empty() && !pt.trim().is_empty() && !en.trim().is_empty(), "texto vazio em {k}");
            assert!(k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'), "chave fora do padrão: {k}");
        }
    }

    #[test]
    fn lookup_switches_language_and_reports_unknown_keys() {
        assert_eq!(tr_in(Lang::Pt, "menu.file"), "Arquivo");
        assert_eq!(tr_in(Lang::En, "menu.file"), "File");
        assert_eq!(tr_in(Lang::En, "nao.existe"), "nao.existe");
        set_lang(Lang::En);
        assert_eq!(tr("workspace.animacao"), "Animation");
        set_lang(Lang::Pt);
        assert_eq!(tr("workspace.animacao"), "Animação");
    }

    #[test]
    fn language_codes_parse() {
        assert_eq!(Lang::from_code("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_code("EN"), Some(Lang::En));
        assert_eq!(Lang::from_code("jp"), None);
    }

    #[test]
    fn enum_keys_exist() {
        let exists = |k: &str| TABLE.iter().any(|(key, _, _)| *key == k);
        for w in WorkspaceId::ALL {
            assert!(exists(w.title_key()), "{}", w.title_key());
        }
        for t in ToolId::ALL {
            assert!(exists(t.title_key()), "{}", t.title_key());
        }
        for p in PanelKind::ALL {
            assert!(exists(p.title_key()) && exists(p.empty_key()), "{p:?}");
        }
    }

    /// Varre os fontes: toda chave em `tr("...")` e todo literal com prefixo de chave conhecido precisa existir na tabela.
    #[test]
    fn used_keys_exist() {
        const SOURCES: &[(&str, &str)] = &[
            ("app.rs", include_str!("app.rs")),
            ("layout.rs", include_str!("layout.rs")),
            ("workspace.rs", include_str!("workspace.rs")),
            ("brand.rs", include_str!("brand.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("cli.rs", include_str!("cli.rs")),
            ("prefs.rs", include_str!("prefs.rs")),
        ];
        const PREFIXES: &[&str] = &["workspace.", "tool.", "panel.", "menu.", "status.", "about.", "prefs.", "lang."];
        let known: HashSet<&str> = TABLE.iter().map(|(k, _, _)| *k).collect();
        let mut used = 0;
        for (file, src) in SOURCES {
            // só o código de produção (o módulo de testes cita chaves inexistentes de propósito)
            let src = src.split("#[cfg(test)]").next().unwrap_or(src);
            let mut rest = src;
            while let Some(i) = rest.find('"') {
                let after = &rest[i + 1..];
                let Some(j) = after.find('"') else { break };
                let lit = &after[..j];
                rest = &after[j + 1..];
                let is_tr_arg = src[..src.len() - rest.len() - lit.len() - 2].ends_with("tr(");
                let looks_like_key = PREFIXES.iter().any(|p| lit.starts_with(p)) && !lit.contains(' ') && !lit.contains('{') && !lit.ends_with(".json");
                if is_tr_arg || looks_like_key {
                    used += 1;
                    assert!(known.contains(lit), "{file}: chave usada e inexistente: {lit}");
                }
            }
        }
        assert!(used > 60, "varredura achou poucas chaves ({used}); o teste quebrou");
    }
}

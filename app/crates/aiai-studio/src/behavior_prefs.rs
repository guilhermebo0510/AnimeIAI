//! Comportamento do editor que o usuário ajusta nas Preferências (S25, à moda das Preferências do Clip Studio): troca
//! temporária de ferramenta, distância mínima de arrasto, tamanho do pincel arrastando na tela, forma do cursor,
//! nome das cópias, cores da seleção/xadrez/grade, passo de giro, níveis de desfazer e intervalos de cópia automática.
//!
//! Cada campo tem consumidor de verdade (nada aqui é um botão que não faz nada); o que cada um liga está em
//! `sprints/registro/S25-clip-studio/04-preferencias.md`. O arquivo é lido com `serde(default)`: preferências antigas
//! (sem esta seção) abrem com os padrões abaixo.

use serde::{Deserialize, Serialize};

/// Forma do cursor das ferramentas de pintura com ponta (pincel, caneta, borracha).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    /// O contorno da ponta no tamanho de verdade (com a mira quando fica pequeno demais).
    #[default]
    Tip,
    /// Só a mira fina.
    Cross,
    /// Só um ponto.
    Dot,
}

/// O que o cursor mostra quando a ponta fica pequena demais para o contorno aparecer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmallCursor {
    None,
    Dot,
    #[default]
    Cross,
}

/// Cursor das ferramentas sem ponta que o canvas desenha (balde, conta-gotas, seleção).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCursor {
    /// O desenho próprio da ferramenta (balde com a mira, conta-gotas…).
    #[default]
    Tool,
    Cross,
}

/// Como a cópia de uma camada ou material ganha o nome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyName {
    /// `Nome (cópia)`
    #[default]
    Paren,
    /// `Nome cópia`
    Suffix,
    /// `Cópia de Nome`
    Prefix,
    /// `Nome 2`
    Number,
}

/// Densidade da interface: o respiro entre os controles (o tamanho do texto é o zoom da interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiDensity {
    Compact,
    #[default]
    Normal,
    Comfortable,
}

impl UiDensity {
    pub const ALL: [UiDensity; 3] = [UiDensity::Compact, UiDensity::Normal, UiDensity::Comfortable];
    /// Espaço entre controles e enchimento dos botões, em px (o normal é o do `theme::apply`: 6×5 e 8×4).
    pub fn spacing(self) -> ([f32; 2], [f32; 2]) {
        match self {
            UiDensity::Compact => ([4.0, 3.0], [6.0, 2.0]),
            UiDensity::Normal => ([6.0, 5.0], [8.0, 4.0]),
            UiDensity::Comfortable => ([8.0, 7.0], [10.0, 6.0]),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            UiDensity::Compact => "prefs.density.compact",
            UiDensity::Normal => "prefs.density.normal",
            UiDensity::Comfortable => "prefs.density.comfortable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorPrefs {
    // ---- Ferramentas
    /// Segurar a tecla de uma ferramenta a usa só enquanto a tecla estiver apertada (um toque rápido troca de vez).
    pub tool_hold: bool,
    /// Tempo (ms) a partir do qual a tecla segurada vira troca temporária.
    pub tool_hold_ms: u32,
    /// Distância (px da tela) que o ponteiro precisa andar antes de um arrasto de objeto começar.
    pub min_drag_px: f32,
    /// Ctrl+Alt+arrastar na horizontal muda o tamanho da ponta (pincel, caneta, borracha).
    pub size_drag: bool,
    /// Mostra o círculo e o valor enquanto o tamanho muda.
    pub size_drag_preview: bool,
    // ---- Tablet
    /// Liga a pressão da caneta (desligada, a caneta desenha como o mouse: pressão sempre cheia).
    pub pen_pressure: bool,
    /// Usa a inclinação da caneta (pincéis com dinâmica de inclinação ou ponta que segue a inclinação).
    pub pen_tilt: bool,
    /// O botão lateral da caneta, apertado ao encostar, pega a cor (conta-gotas) enquanto estiver no traço.
    pub pen_barrel_picks: bool,
    // ---- Cursor
    pub cursor_brush: CursorShape,
    pub cursor_pen: CursorShape,
    pub cursor_eraser: CursorShape,
    pub cursor_other: ToolCursor,
    pub cursor_small: SmallCursor,
    // ---- Camada e seleção
    pub copy_name: CopyName,
    /// Cor das formigas da seleção (`None` = branco e preto, como o Photoshop).
    pub sel_color: Option<[u8; 3]>,
    // ---- Tela de pintura
    /// Duas cores do xadrez de transparência (`None` = as do tema).
    pub checker: Option<[[u8; 3]; 2]>,
    pub grid_color: [u8; 3],
    /// Opacidade (0–255) das linhas da grade de pixels.
    pub grid_alpha: u8,
    /// Passo dos botões de girar a visão (graus).
    pub rotate_step_deg: f32,
    // ---- Desempenho e interface
    pub undo_levels: u32,
    pub ui_density: UiDensity,
    // ---- Arquivo
    /// Cópia de recuperação do projeto ainda sem pasta (minutos).
    pub recovery_minutes: u32,
    /// Cópia do documento de um projeto com pasta (minutos).
    pub backup_minutes: u32,
}

pub const HOLD_MS_RANGE: (u32, u32) = (100, 2000);
pub const MIN_DRAG_MAX: f32 = 20.0;
pub const UNDO_RANGE: (u32, u32) = (10, 2000);
pub const AUTOSAVE_RANGE: (u32, u32) = (1, 60);

impl Default for BehaviorPrefs {
    fn default() -> Self {
        BehaviorPrefs {
            tool_hold: true,
            tool_hold_ms: 500,
            min_drag_px: 3.0,
            size_drag: true,
            size_drag_preview: true,
            pen_pressure: true,
            pen_tilt: true,
            pen_barrel_picks: true,
            cursor_brush: CursorShape::Tip,
            cursor_pen: CursorShape::Tip,
            cursor_eraser: CursorShape::Tip,
            cursor_other: ToolCursor::Tool,
            cursor_small: SmallCursor::Cross,
            copy_name: CopyName::Paren,
            sel_color: None,
            checker: None,
            grid_color: [255, 255, 255],
            grid_alpha: 38,
            rotate_step_deg: 15.0,
            undo_levels: 500,
            ui_density: UiDensity::Normal,
            recovery_minutes: 2,
            backup_minutes: 5,
        }
    }
}

impl BehaviorPrefs {
    /// Valores dentro dos limites (um JSON editado à mão nunca deixa o editor inutilizável).
    pub fn sanitized(mut self) -> Self {
        let d = BehaviorPrefs::default();
        self.tool_hold_ms = self.tool_hold_ms.clamp(HOLD_MS_RANGE.0, HOLD_MS_RANGE.1);
        self.min_drag_px = if self.min_drag_px.is_finite() { self.min_drag_px.clamp(0.0, MIN_DRAG_MAX) } else { d.min_drag_px };
        self.rotate_step_deg = if self.rotate_step_deg.is_finite() { self.rotate_step_deg.clamp(1.0, 90.0) } else { d.rotate_step_deg };
        self.undo_levels = self.undo_levels.clamp(UNDO_RANGE.0, UNDO_RANGE.1);
        self.recovery_minutes = self.recovery_minutes.clamp(AUTOSAVE_RANGE.0, AUTOSAVE_RANGE.1);
        self.backup_minutes = self.backup_minutes.clamp(AUTOSAVE_RANGE.0, AUTOSAVE_RANGE.1);
        self.grid_alpha = self.grid_alpha.max(4);
        self
    }

    pub fn checker_colors(&self) -> Option<[egui::Color32; 2]> {
        self.checker.map(|[a, b]| [rgb(a), rgb(b)])
    }
}

pub fn rgb(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// Nome da cópia de `base` no estilo escolhido (`word` = "cópia"/"copy", `of` = "Cópia de"/"Copy of").
pub fn copy_name(base: &str, style: CopyName, word: &str, of: &str) -> String {
    match style {
        CopyName::Paren => format!("{base} ({word})"),
        CopyName::Suffix => format!("{base} {word}"),
        CopyName::Prefix => format!("{of} {base}"),
        CopyName::Number => format!("{base} 2"),
    }
}

/// `copy_name` com os textos do idioma atual.
pub fn copy_name_tr(base: &str, style: CopyName) -> String {
    use crate::i18n::tr;
    copy_name(base, style, tr("prefs.copy.word"), tr("prefs.copy.of"))
}

// ---------------------------------------------------------------------------------------------------------------
// entrega das preferências aos desenhistas que só têm o `egui::Context` (formigas, grade, xadrez)

fn ctx_id() -> egui::Id {
    egui::Id::new("aiai.behavior_prefs")
}

/// Publica as preferências na memória do egui (chamado a cada quadro pelo app).
pub fn publish(ctx: &egui::Context, p: &BehaviorPrefs) {
    ctx.data_mut(|d| {
        if d.get_temp::<BehaviorPrefs>(ctx_id()).as_ref() != Some(p) {
            d.insert_temp(ctx_id(), p.clone());
        }
    });
}

/// As preferências publicadas (as padrão antes do primeiro quadro, e nos testes que não passam pelo app).
pub fn current(ctx: &egui::Context) -> BehaviorPrefs {
    ctx.data(|d| d.get_temp::<BehaviorPrefs>(ctx_id())).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------
// troca temporária de ferramenta

/// Troca temporária ativa: a tecla que trocou, a ferramenta de antes e a nova.
#[derive(Debug, Clone, Copy)]
pub struct HoldSwitch {
    pub key: egui::Key,
    pub prev: crate::workspace::ToolId,
    pub tool: crate::workspace::ToolId,
    pub since: std::time::Instant,
    /// A tecla já foi solta depois do tempo e a volta espera o gesto em curso terminar.
    pub released: bool,
}

/// O que fazer com uma troca temporária neste quadro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldStep {
    /// Continua valendo.
    Keep,
    /// A tecla foi solta antes do tempo (toque rápido): a ferramenta nova fica de vez.
    Stay,
    /// A tecla foi solta depois do tempo: volta para a ferramenta de antes.
    Restore,
    /// A ferramenta mudou por outro caminho (ou o gesto ainda corre): nada a restaurar.
    Drop,
}

/// Decide a troca temporária: `key_down` = a tecla segue apertada; `elapsed_ms` desde que trocou; `current` = a ferramenta
/// em uso agora; `busy` = há um gesto no canvas.
pub fn hold_step(h: &HoldSwitch, key_down: bool, elapsed_ms: u128, threshold_ms: u32, current: crate::workspace::ToolId, busy: bool) -> HoldStep {
    if current != h.tool {
        return HoldStep::Drop;
    }
    if key_down {
        return HoldStep::Keep;
    }
    if elapsed_ms < threshold_ms as u128 {
        return HoldStep::Stay;
    }
    if busy {
        return HoldStep::Keep;
    }
    HoldStep::Restore
}

/// Teclas apertadas neste quadro sem Ctrl/Alt/Cmd (o candidato a "a tecla da ferramenta"); lido ANTES de os atalhos
/// consumirem os eventos.
pub fn plain_keys_pressed(ctx: &egui::Context) -> Vec<egui::Key> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } if !(modifiers.ctrl || modifiers.alt || modifiers.command) => Some(*key),
                _ => None,
            })
            .collect()
    })
}

// ---------------------------------------------------------------------------------------------------------------
// arrasto mínimo

/// O arrasto passou da distância mínima (px da tela) desde onde o botão foi apertado?
pub fn drag_armed(origin: egui::Pos2, now: egui::Pos2, min_px: f32) -> bool {
    min_px <= 0.0 || (now - origin).length() >= min_px
}

// ---------------------------------------------------------------------------------------------------------------
// tamanho da ponta arrastando na tela

/// Tamanho novo da ponta: o de partida mais o deslocamento horizontal (px da tela) convertido para px do desenho,
/// preso nos limites da ferramenta.
pub fn size_from_drag(start: f32, dx_screen: f32, scale: f32, (lo, hi): (f32, f32)) -> f32 {
    (start + dx_screen / scale.max(1e-3)).clamp(lo, hi)
}

/// Arrasto de tamanho em curso.
#[derive(Debug, Clone, Copy)]
pub struct SizeDrag {
    pub origin: egui::Pos2,
    pub start: f32,
    pub tool: crate::workspace::ToolId,
}

/// Estado transitório (não vai para o arquivo de preferências).
#[derive(Debug, Default)]
pub struct BehaviorRt {
    /// As preferências deste quadro (copiadas das do app).
    pub prefs: BehaviorPrefs,
    pub hold: Option<HoldSwitch>,
    /// Onde o botão primário foi apertado e se o arrasto já passou da distância mínima.
    pub drag_origin: Option<egui::Pos2>,
    pub drag_armed: bool,
    pub size_drag: Option<SizeDrag>,
    /// Cores do xadrez com que a textura do canvas foi feita (para refazê-la quando a preferência muda).
    pub checker_key: Option<Option<[[u8; 3]; 2]>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::ToolId;

    #[test]
    fn old_prefs_without_the_section_load_with_defaults() {
        let b: BehaviorPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(b, BehaviorPrefs::default());
        // só um campo: o resto continua padrão
        let b: BehaviorPrefs = serde_json::from_str(r#"{"tool_hold_ms":800,"cursor_brush":"dot","copy_name":"prefix"}"#).unwrap();
        assert_eq!((b.tool_hold_ms, b.cursor_brush, b.copy_name, b.min_drag_px), (800, CursorShape::Dot, CopyName::Prefix, 3.0));
    }

    /// Todo texto "prefs.…" citado nas fontes destas Preferências existe na tabela de textos (pt e en).
    #[test]
    fn preference_texts_exist_in_both_languages() {
        use crate::i18n::{tr_in, Lang};
        let mut seen = 0;
        for src in [include_str!("app/prefs_ui.rs"), include_str!("behavior_prefs.rs")] {
            let src = src.split("#[cfg(test)]").next().unwrap_or(src);
            for lit in src.split('"').skip(1).step_by(2) {
                if lit.starts_with("prefs.") && !lit.contains(' ') && !lit.ends_with(':') && !lit.ends_with('.') {
                    seen += 1;
                    for l in [Lang::Pt, Lang::En] {
                        assert_ne!(tr_in(l, lit), lit, "texto sem tradução: {lit}");
                    }
                }
            }
        }
        assert!(seen > 40, "a varredura achou poucas chaves ({seen})");
    }

    /// Um arquivo de preferências antigo (sem a seção) abre com os padrões, e valores fora dos limites são corrigidos.
    #[test]
    fn prefs_file_without_the_section_loads_and_bad_values_are_clamped() {
        let dir = std::env::temp_dir().join(format!("aiai-behavior-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.json");
        std::fs::write(&old, r#"{"lang":"en","ui_zoom":1.25}"#).unwrap();
        assert_eq!(crate::prefs::load(&old).behavior, BehaviorPrefs::default());
        let bad = dir.join("bad.json");
        std::fs::write(&bad, r#"{"behavior":{"tool_hold_ms":1,"undo_levels":999999,"min_drag_px":-4.0,"cursor_brush":"dot"}}"#).unwrap();
        let b = crate::prefs::load(&bad).behavior;
        assert_eq!((b.tool_hold_ms, b.undo_levels, b.min_drag_px, b.cursor_brush), (HOLD_MS_RANGE.0, UNDO_RANGE.1, 0.0, CursorShape::Dot));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_keeps_values_inside_limits() {
        let b = BehaviorPrefs { tool_hold_ms: 5, min_drag_px: f32::NAN, rotate_step_deg: 500.0, undo_levels: 0, recovery_minutes: 0, backup_minutes: 9999, grid_alpha: 0, ..Default::default() }.sanitized();
        assert_eq!((b.tool_hold_ms, b.min_drag_px, b.rotate_step_deg, b.undo_levels), (HOLD_MS_RANGE.0, 3.0, 90.0, UNDO_RANGE.0));
        assert_eq!((b.recovery_minutes, b.backup_minutes, b.grid_alpha), (1, 60, 4));
    }

    #[test]
    fn copy_names_follow_the_chosen_style() {
        assert_eq!(copy_name("Linha", CopyName::Paren, "cópia", "Cópia de"), "Linha (cópia)");
        assert_eq!(copy_name("Linha", CopyName::Suffix, "cópia", "Cópia de"), "Linha cópia");
        assert_eq!(copy_name("Linha", CopyName::Prefix, "copy", "Copy of"), "Copy of Linha");
        assert_eq!(copy_name("Linha", CopyName::Number, "cópia", "Cópia de"), "Linha 2");
    }

    #[test]
    fn tool_hold_returns_only_after_the_threshold() {
        let h = HoldSwitch { key: egui::Key::E, prev: ToolId::Brush, tool: ToolId::Eraser, since: std::time::Instant::now(), released: false };
        // tecla ainda apertada: continua
        assert_eq!(hold_step(&h, true, 900, 500, ToolId::Eraser, false), HoldStep::Keep);
        // toque rápido (solta antes de 500 ms): a borracha fica de vez
        assert_eq!(hold_step(&h, false, 120, 500, ToolId::Eraser, false), HoldStep::Stay);
        // segurou e soltou: volta ao pincel
        assert_eq!(hold_step(&h, false, 700, 500, ToolId::Eraser, false), HoldStep::Restore);
        // soltou depois do tempo mas há um traço em curso: espera
        assert_eq!(hold_step(&h, false, 700, 500, ToolId::Eraser, true), HoldStep::Keep);
        // o usuário trocou de ferramenta por outro caminho: nada a restaurar
        assert_eq!(hold_step(&h, false, 700, 500, ToolId::Fill, false), HoldStep::Drop);
    }

    #[test]
    fn min_drag_waits_for_the_distance() {
        let o = egui::pos2(100.0, 100.0);
        assert!(!drag_armed(o, egui::pos2(102.0, 101.0), 3.0));
        assert!(drag_armed(o, egui::pos2(103.0, 100.0), 3.0));
        assert!(drag_armed(o, o, 0.0), "distância 0 desliga a espera");
    }

    #[test]
    fn size_drag_follows_the_horizontal_motion_in_canvas_pixels() {
        // zoom 2×: 20 px de tela = 10 px do desenho
        assert_eq!(size_from_drag(10.0, 20.0, 2.0, (1.0, 600.0)), 20.0);
        assert_eq!(size_from_drag(10.0, -50.0, 2.0, (1.0, 600.0)), 1.0);
        assert_eq!(size_from_drag(590.0, 500.0, 1.0, (1.0, 600.0)), 600.0);
    }
}

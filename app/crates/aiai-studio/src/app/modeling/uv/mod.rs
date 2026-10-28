//! Editor de UV da Modelagem, vindo do ANIGO (`anigo-studio/src/app/uv_ui.rs` + `uv_ui/*`, commit bc5e83af, mesmo
//! autor): painel 2D com as ilhas (cor por ilha, estiramento ou densidade, xadrez), seleção sincronizada com o
//! viewport, G/R/S nos UVs, e as ações (costuras, desdobrar, empacotar, projeções, métricas) sobre o motor
//! `aiai_3d_edit::uv`.
//!
//! Mudanças em relação ao ANIGO: a sessão dele virou o nosso `ModelingState` através do [`UvHost`] (o mesmo papel do
//! `StudioApp` lá: `modeling` + `uv` + câmera); o snapshot de desfazer é o documento inteiro do nosso `History`;
//! rótulos da interface pelo [`l`] (pt/en); ícones Phosphor trocados por texto; cores do tema fixas no editor (fundo
//! escuro como no ANIGO).

mod actions;
mod cache;
mod canvas;
mod geom;
mod inspector;

pub(crate) use actions::{spanning_tree_seams, UvAction, UvParams};
pub(crate) use cache::UvCache;
pub(crate) use geom::{UvView, Xform};

use super::camera::OrbitCamera;
use super::{EditMode, ModelingState};
use aiai_3d_edit::doc::{ModelDoc, Object3d};
use aiai_3d_edit::Selection;
use glam::Vec2;

/// Rótulo da interface no idioma do app.
pub(crate) fn l(pt: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::lang() == crate::i18n::Lang::En { en } else { pt }
}

/// Como as faces são coloridas no editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Uma cor por ilha.
    #[default]
    Islands,
    /// Mapa de estiramento anisotrópico (`face_stretch`).
    Distortion,
    /// Densidade de texels relativa à média.
    Density,
}

/// Textura de fundo procedural para enxergar a cobertura/estiramento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckerMode {
    #[default]
    Off,
    C8,
    C64,
}

impl CheckerMode {
    pub fn cells(self) -> Option<usize> {
        match self {
            CheckerMode::Off => None,
            CheckerMode::C8 => Some(8),
            CheckerMode::C64 => Some(64),
        }
    }
}

/// Transformação G/R/S (ou arrasto) em andamento no editor de UV.
pub(crate) struct UvModal {
    pub xf: Xform,
    /// `(face, canto, UV original)` de cada canto afetado.
    pub orig: Vec<(u32, usize, Vec2)>,
    pub before: ModelDoc,
    /// Arrasto com o botão pressionado: confirma ao soltar.
    pub drag: bool,
    pub num_buf: String,
    /// Objeto e hash da geometria no início; se mudarem, a operação é descartada.
    pub obj_id: u32,
    pub geo_hash: u64,
}

pub(crate) struct UvState {
    pub view: UvView,
    pub view_inited: bool,
    pub fit_request: bool,
    pub color_mode: ColorMode,
    pub checker: CheckerMode,
    pub show_overlaps: bool,
    pub show_grid: bool,
    pub params: UvParams,
    /// Proporção (largura/altura) do viewport 3D, para "Projetar da vista".
    pub view_aspect: f32,
    pub modal: Option<UvModal>,
    pub box_start: Option<Vec2>,
    pub cache: Option<UvCache>,
    pub tex_checker: [Option<egui::TextureHandle>; 2],
    /// Última mensagem de erro de ação.
    pub last_error: Option<String>,
    /// Face sob o cursor no editor (dica e destaque).
    pub hover_face: Option<u32>,
    /// Mensagem da última ação / da transformação em curso.
    pub status: String,
    /// O editor 2D aparece (no painel Propriedades, modo Edição).
    pub show_editor: bool,
}

impl Default for UvState {
    fn default() -> Self {
        Self {
            view: UvView::default(),
            view_inited: false,
            fit_request: false,
            color_mode: ColorMode::default(),
            checker: CheckerMode::default(),
            show_overlaps: true,
            show_grid: true,
            params: UvParams::default(),
            view_aspect: 1.6,
            modal: None,
            box_start: None,
            cache: None,
            tex_checker: [None, None],
            last_error: None,
            hover_face: None,
            status: String::new(),
            show_editor: true,
        }
    }
}

/// O que o código do editor de UV enxerga (no ANIGO era o `StudioApp`).
pub(crate) struct UvHost<'a> {
    pub modeling: &'a mut ModelingState,
    pub uv: &'a mut UvState,
    pub camera: OrbitCamera,
    pub notes: &'a mut Vec<(String, egui::Rect)>,
}

// ---- o pedaço da sessão do ANIGO que o editor usa, sobre o nosso estado
impl ModelingState {
    pub(crate) fn uv_active_object(&self) -> Option<&Object3d> {
        self.active.and_then(|a| self.doc.objects.get(a))
    }

    pub(crate) fn uv_active_object_mut(&mut self) -> Option<&mut Object3d> {
        self.active.and_then(|a| self.doc.objects.get_mut(a))
    }

    /// Identidade do objeto ativo para o cache e a transformação (o nosso documento não tem id: o índice).
    pub(crate) fn uv_obj_id(&self) -> u32 {
        self.active.map_or(u32::MAX, |a| a as u32)
    }

    pub(crate) fn uv_select_all(&mut self) {
        if let Some(m) = self.uv_active_object().map(|o| o.mesh.clone()) {
            self.set_selection(Selection::select_all(&m));
        }
    }

    pub(crate) fn uv_select_none(&mut self) {
        self.set_selection(Selection::default());
    }

    pub(crate) fn uv_select_invert(&mut self) {
        if let Some(m) = self.uv_active_object().map(|o| o.mesh.clone()) {
            let inv = self.sel.invert(&m, self.sel_mode);
            self.set_selection(inv);
        }
    }

    /// Registra o documento de antes de uma edição feita direto na malha (arrasto de UVs).
    pub(crate) fn uv_commit(&mut self, label: &str, before: ModelDoc) {
        if before != self.doc {
            self.history.push_owned(label, before);
            self.dirty = true;
        }
        self.touch();
    }

    /// Volta ao documento de antes sem mexer no histórico (cancelar o arrasto).
    pub(crate) fn uv_restore(&mut self, before: ModelDoc) {
        self.doc = before;
        self.touch();
    }
}

impl UvHost<'_> {
    /// Executa uma ação de UV com mensagem amigável. Devolve se deu certo.
    pub(crate) fn run_uv_action(&mut self, a: UvAction) -> bool {
        if self.modeling.mode != EditMode::Edit {
            self.uv.status = l("UV: entre no modo Edição (Tab)", "UV: switch to Edit mode (Tab)").into();
            return false;
        }
        let view_proj = self.modeling.uv_active_object().map(|o| {
            let aspect = self.uv.view_aspect.max(0.1);
            self.camera.view_proj(aspect) * o.matrix()
        });
        let params = self.uv.params;
        match actions::run_action(self.modeling, a, &params, view_proj) {
            Ok(msg) => {
                self.uv.status = format!("{}: {msg}", a.label());
                self.uv.last_error = None;
                true
            }
            Err(e) => {
                let text = format!("{}: {e}", a.label());
                self.uv.status = text.clone();
                self.uv.last_error = Some(text);
                false
            }
        }
    }

    /// Seção UV do painel Propriedades: costuras, desdobrar, empacotar, projeções, métricas.
    pub(crate) fn inspector_uv(&mut self, ui: &mut egui::Ui) {
        self.inspector_uv_impl(ui);
    }

    /// O editor 2D de UV (ilhas, estiramento, seleção sincronizada).
    pub(crate) fn uv_editor_panel(&mut self, ui: &mut egui::Ui) {
        self.uv_editor_impl(ui);
    }
}

/// Ação de UV pelo nome (ponte/MCP).
pub(crate) fn action_from_key(k: &str) -> Option<UvAction> {
    UvAction::ALL.into_iter().find(|a| a.key() == k)
}

/// Roda uma ação de UV fora da interface (ponte): a mesma regra do botão.
pub(crate) fn run_headless(ms: &mut ModelingState, uv: &mut UvState, camera: OrbitCamera, a: UvAction) -> Result<String, String> {
    let mut notes = Vec::new();
    let mut host = UvHost { modeling: ms, uv, camera, notes: &mut notes };
    if host.run_uv_action(a) { Ok(host.uv.status.clone()) } else { Err(host.uv.status.clone()) }
}

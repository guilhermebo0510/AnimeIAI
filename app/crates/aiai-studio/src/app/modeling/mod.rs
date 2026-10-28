//! Workspace Modelagem (S24 3D): viewport 3D nativo na GPU portado do ANIGO (Workbench + prévia cel registradas uma vez
//! como texturas do egui e redesenhadas sem leitura de volta; câmera turntable com zoom para o cursor, vistas e
//! ortográfica; gizmo de eixos e de transformação; seleção por clique, caixa e laço), o documento da Modelagem
//! (`aiai_3d_edit::ModelDoc`) e os operadores do modo Edição pelo catálogo do ANIGO (`ops::catalog`), com o painel
//! "Ajustar última operação". Atalhos no contexto Modelagem (`keys`): perfil AnimeIAI (V mover, R girar, T escalar,
//! S seleção) ou Blender/ANIGO (G/R/S modais), em Preferências > Atalhos.
//!
//! Navegação: roda = zoom para o cursor (suave); botão do meio orbita (Shift pan, Ctrl zoom); sem botão do meio,
//! botão direito arrastado orbita (Shift pan, Ctrl zoom), Alt+esquerdo orbita e Espaço+esquerdo faz pan (a mão do resto
//! do app); gizmo de eixos no canto (clicar = vista, arrastar = órbita, botões de pan/zoom/projeção); Ctrl+1/3/7 frente,
//! lado, topo; Ctrl+5 perspectiva/ortográfica (Alt+número troca de workspace no app; Shift+número vira símbolo); F (ou `.` no perfil Blender) enquadra a seleção; Home enquadra tudo.
//! "Salvar na biblioteca" grava um ativo 3D do projeto (GLB + documento editável) e, com sessão, a Biblioteca
//! (`Documentos\AnimeIAI\Modelos 3D`); "Enviar ao Layout" cria a camada 3D no corte.

pub(crate) mod camera;
mod gizmo;
mod gpu;
pub(crate) mod keys;
mod modal;
pub(crate) mod multiview;
mod nav_gizmo;
mod picking;
pub(crate) mod sculpt;
mod workbench;

use super::commands::NodeId;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_3d_edit::doc::{self, History, ModelDoc, Primitive};
use aiai_3d_edit::ops::catalog::{self, OpKind, OpParams};
use aiai_3d_edit::ops::slide::{self, SlideRail};
use aiai_3d_edit::ops::transform::ProportionalFalloff;
use aiai_3d_edit::{EdgeKey, EditMesh, SelectMode, Selection, Topology};
use aiai_core::*;
use camera::{OrbitCamera, ViewPreset};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui};
use glam::{Mat4, Quat, Vec2, Vec3};
use keys::{MAction, ModelKeymap};
use modal::{Axis, Constraint, ModalContext, ModalTransform, TransformKind};
use serde_json::json;
use std::time::Instant;

pub(crate) use gpu::{gpu_bytes, install as install_gpu, ViewShade};

/// Modo do viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditMode {
    #[default]
    Object,
    Edit,
    /// Escultura (pincéis do ANIGO no objeto ativo).
    Sculpt,
}

/// Forma do arrasto de seleção.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum SelectShape {
    #[default]
    Box,
    Lasso,
}

/// Edição proporcional (O): os vértices em volta da seleção acompanham com peso pela distância.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Proportional {
    pub enabled: bool,
    pub radius: f32,
    pub falloff: ProportionalFalloff,
    /// Só vértices ligados (não vaza para outra ilha da malha).
    pub connected: bool,
}

impl Default for Proportional {
    fn default() -> Self {
        Proportional { enabled: false, radius: 1.0, falloff: ProportionalFalloff::Smooth, connected: false }
    }
}

/// Operação modal em curso.
enum ModalOp {
    /// G/R/S (e o "mover" do extrude, preso à normal): a matemática do `ModalTransform` do ANIGO.
    Transform { t: ModalTransform, object: usize, model: Mat4 },
    /// Inset: espessura pela distância do mouse ao pivô.
    Inset { faces: Vec<u32>, pivot_px: Vec2 },
    /// Bevel: largura pela distância do mouse ao pivô; roda = segmentos.
    Bevel { edges: Vec<EdgeKey>, pivot_px: Vec2 },
    /// Loop cut: o anel da aresta, deslizar pelo mouse; roda = número de cortes.
    LoopCut { edge: EdgeKey, screen_dir: Vec2 },
    /// Deslizar aresta/vértice pelos trilhos.
    Slide { rails: Vec<SlideRail>, screen_dir: Vec2, object: usize },
}

struct Modal {
    op: ModalOp,
    /// Operador do catálogo (rótulo do histórico e "Ajustar última operação"); `None` = transformação.
    kind: Option<OpKind>,
    label: &'static str,
    before: ModelDoc,
    before_sel: Selection,
    /// Documento sobre o qual a operação é refeita a cada quadro (o extrude move o resultado dele; o resto, o "antes").
    base: Option<ModelDoc>,
    start: Vec2,
    numeric: String,
    /// Começou num arrasto com a ferramenta da prateleira: soltar confirma.
    from_tool: bool,
    params: OpParams,
}

/// "Ajustar última operação" (o Redo Panel do Blender, como no ANIGO): mudar um parâmetro volta ao estado de antes e
/// refaz a operação.
pub(crate) struct LastOp {
    pub kind: OpKind,
    pub params: OpParams,
    before: ModelDoc,
    before_sel: Selection,
    history_len: usize,
}

/// Estado da Modelagem guardado no editor.
pub(crate) struct ModelingState {
    pub doc: ModelDoc,
    pub history: History,
    /// Ativo 3D do projeto ligado ao documento (depois de salvar ou abrir).
    pub asset: Option<NodeId>,
    pub active: Option<usize>,
    pub mode: EditMode,
    pub sel_mode: SelectMode,
    pub sel: Selection,
    pub cam: OrbitCamera,
    /// Multivista: as câmeras das outras vistas (a da ativa é `cam`).
    pub views: multiview::MultiView,
    /// Modo Escultura: pincéis, simetria, dyntopo e o vínculo com o objeto ativo.
    pub sculpt: sculpt::SculptState,
    pub shade: ViewShade,
    pub snap: bool,
    pub xray: bool,
    pub grid: bool,
    pub proportional: Proportional,
    pub select_shape: SelectShape,
    pub user_files: bool,
    pub dirty: bool,
    pub keys: ModelKeymap,
    /// Parâmetros dos operadores (os últimos usados).
    pub op_params: OpParams,
    pub last_op: Option<LastOp>,
    /// Muda a cada mudança de documento, seleção ou modo (chave das malhas na GPU).
    pub revision: u64,
    /// Medidas do viewport: intervalo entre quadros (média móvel, ms) enquanto a câmera se mexe e tempo de CPU do último
    /// quadro redesenhado.
    pub nav_frame_ms: f32,
    pub gpu_cpu_ms: f32,
    pub frames_drawn: u64,
    modal: Option<Modal>,
    add_menu: Option<Pos2>,
    gpu: gpu::GpuSlot,
    hovered: bool,
    box_start: Option<Vec2>,
    lasso: Vec<Vec2>,
    /// O clique que confirmou uma operação não seleciona nada ao soltar.
    swallow_release: bool,
    /// O viewport pode ler o teclado (nenhum outro widget com o foco).
    kb: bool,
    last_nav: Option<Instant>,
    last_cam: Option<OrbitCamera>,
    gizmo_hover: Option<Axis>,
    topo_cache: Option<(u64, usize, Topology, EditMesh)>,
}

fn default_cam() -> OrbitCamera {
    OrbitCamera { target: Vec3::new(0.0, 0.5, 0.0), yaw: 35f32.to_radians(), pitch: 22f32.to_radians(), distance: 5.0, fov_y: 35f32.to_radians(), ..Default::default() }
}

impl Default for ModelingState {
    fn default() -> Self {
        ModelingState {
            doc: ModelDoc::new("Modelo"),
            history: History::default(),
            asset: None,
            active: None,
            mode: EditMode::Object,
            sel_mode: SelectMode::Face,
            sel: Selection::default(),
            cam: default_cam(),
            views: multiview::MultiView::default(),
            sculpt: sculpt::SculptState::default(),
            shade: ViewShade::Toon,
            snap: false,
            xray: false,
            grid: true,
            proportional: Proportional::default(),
            select_shape: SelectShape::Box,
            user_files: false,
            dirty: false,
            keys: ModelKeymap::default(),
            op_params: OpParams::default(),
            last_op: None,
            revision: 1,
            nav_frame_ms: 0.0,
            gpu_cpu_ms: 0.0,
            frames_drawn: 0,
            modal: None,
            add_menu: None,
            gpu: gpu::GpuSlot::default(),
            hovered: false,
            box_start: None,
            lasso: Vec::new(),
            swallow_release: false,
            kb: true,
            last_nav: None,
            last_cam: None,
            gizmo_hover: None,
            topo_cache: None,
        }
    }
}

impl ModelingState {
    /// Solta as texturas e buffers da GPU (orçamento de VRAM, `gpu_owners`): voltam quando o painel aparecer.
    pub(crate) fn release_gpu(&mut self) -> bool {
        self.gpu.release()
    }

    /// Marca uma mudança de documento/seleção/modo (o viewport sobe de novo o que mudou).
    pub(crate) fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Guarda o "antes" no histórico se o documento mudou.
    fn commit(&mut self, label: &str, before: ModelDoc) {
        if before != self.doc {
            self.history.push_owned(label, before);
            self.dirty = true;
        }
        self.touch();
    }

    /// Muda o documento com um passo de desfazer; erro do operador volta o documento e vira aviso.
    pub(crate) fn edit<R>(&mut self, f: impl FnOnce(&mut ModelDoc) -> Result<R, String>) -> Result<R, String> {
        self.edit_labeled(tr("mdl.hist.edit"), f)
    }

    pub(crate) fn edit_labeled<R>(&mut self, label: &str, f: impl FnOnce(&mut ModelDoc) -> Result<R, String>) -> Result<R, String> {
        let before = self.doc.clone();
        match f(&mut self.doc) {
            Ok(r) => {
                self.commit(label, before);
                self.fix();
                Ok(r)
            }
            Err(e) => {
                self.doc = before;
                Err(e)
            }
        }
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.cancel_modal();
        let ok = self.history.undo(&mut self.doc).is_some();
        if ok {
            self.sel.clear();
            self.fix();
            self.dirty = true;
            self.touch();
        }
        ok
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.cancel_modal();
        let ok = self.history.redo(&mut self.doc).is_some();
        if ok {
            self.sel.clear();
            self.fix();
            self.dirty = true;
            self.touch();
        }
        ok
    }

    fn cancel_modal(&mut self) {
        if let Some(m) = self.modal.take() {
            self.doc = m.before;
            self.sel = m.before_sel;
            self.touch();
        }
    }

    /// Objeto ativo válido; modo Edição só com objeto.
    fn fix(&mut self) {
        if self.active.is_some_and(|a| a >= self.doc.objects.len()) {
            self.active = self.doc.objects.len().checked_sub(1);
            self.sel.clear();
        }
        if self.active.is_none() {
            self.mode = EditMode::Object;
        }
    }

    fn active_mesh(&self) -> Option<&EditMesh> {
        self.active.and_then(|a| self.doc.objects.get(a)).map(|o| &o.mesh)
    }

    /// Começa outro documento (o atual fica no desfazer).
    pub(crate) fn new_doc(&mut self, name: &str) {
        self.cancel_modal();
        let before = std::mem::replace(&mut self.doc, ModelDoc::new(name));
        self.history.push_owned(tr("mdl.hist.new"), before);
        self.asset = None;
        self.active = None;
        self.sel.clear();
        self.mode = EditMode::Object;
        self.dirty = false;
        self.last_op = None;
        self.touch();
    }

    /// Acrescenta uma primitiva apoiada no chão (Y = 0) e a torna ativa.
    pub(crate) fn add_primitive(&mut self, kind: Primitive) -> Result<(), String> {
        let r = self.edit_labeled(tr("mdl.hist.add"), |d| {
            let i = d.add_primitive(kind, 1.0, [0.0; 3], None)?;
            let lo = d.objects[i].mesh.bounds().map(|(lo, _)| lo.y).unwrap_or(0.0);
            d.objects[i].transform.pos = [0.0, -lo, 0.0];
            Ok(i)
        })?;
        self.active = Some(r);
        self.mode = EditMode::Object;
        self.sel.clear();
        self.touch();
        Ok(())
    }

    /// Tira da seleção os índices que não existem mais na malha ativa (a malha mudou por fora: painel, MCP, desfazer de
    /// outro passo). Nenhum caminho da UI lê vértice/aresta/face por índice guardado sem passar por aqui.
    pub(crate) fn sanitize_selection(&mut self) {
        let Some(m) = self.active_mesh() else {
            if !self.sel.is_empty() {
                self.sel.clear();
            }
            return;
        };
        let (nv, nf) = (m.vert_count() as u32, m.face_count() as u32);
        let stale = self.sel.verts.iter().any(|v| *v >= nv) || self.sel.faces.iter().any(|f| *f >= nf) || self.sel.edges.iter().any(|(a, b)| *a >= nv || *b >= nv);
        if stale {
            self.sel.verts.retain(|v| *v < nv);
            self.sel.faces.retain(|f| *f < nf);
            self.sel.edges.retain(|(a, b)| *a < nv && *b < nv);
            self.touch();
        }
    }

    pub(crate) fn set_select_mode(&mut self, m: SelectMode) {
        if let Some(mesh) = self.active_mesh() {
            self.sel = self.sel.flush(mesh, m);
        }
        self.sel_mode = m;
        self.touch();
    }

    pub(crate) fn set_selection(&mut self, s: Selection) {
        self.sel = s;
        self.touch();
    }

    /// Roda um operador do catálogo na malha ativa (modo Edição): um passo de desfazer, seleção resultante e o painel de
    /// ajuste. Erro = malha intacta e a mensagem volta.
    pub(crate) fn run_op(&mut self, kind: OpKind, params: OpParams) -> Result<(), String> {
        if self.mode != EditMode::Edit {
            return Err(tr("mdl.need_edit").to_owned());
        }
        let Some(a) = self.active else { return Err(tr("mdl.need_object").to_owned()) };
        self.cancel_modal();
        let (before, before_sel) = (self.doc.clone(), self.sel.clone());
        let sel = self.sel.clone();
        let mut mesh = self.doc.objects[a].mesh.clone();
        let out = catalog::apply_kind(kind, &params, &mut mesh, &sel).map_err(|e| format!("{}: {e}", tr(&op_label_key(kind))))?;
        mesh.validate().map_err(|e| e.to_string())?;
        self.doc.objects[a].mesh = mesh;
        let flushed = out.selection.flush(&self.doc.objects[a].mesh, self.sel_mode);
        self.sel = if flushed.is_empty() && !out.selection.is_empty() { out.selection.clone() } else { flushed };
        self.commit(tr(&op_label_key(kind)), before.clone());
        self.op_params = params.clone();
        self.last_op = Some(LastOp { kind, params, before, before_sel, history_len: self.history.len() });
        Ok(())
    }

    /// Refaz a última operação com parâmetros novos (painel "Ajustar").
    pub(crate) fn redo_last_with(&mut self, params: OpParams) -> Result<(), String> {
        let Some(last) = self.last_op.take() else { return Ok(()) };
        if last.history_len != self.history.len() {
            return Ok(());
        }
        self.history.pop_undo();
        self.doc = last.before.clone();
        self.sel = last.before_sel.clone();
        let kind = last.kind;
        match self.run_op(kind, params) {
            Ok(()) => Ok(()),
            Err(e) => {
                // volta ao resultado anterior com os parâmetros antigos
                let _ = self.run_op(kind, last.params.clone());
                Err(e)
            }
        }
    }

    /// O painel de ajuste vale enquanto o histórico não andou.
    pub(crate) fn last_op_valid(&self) -> Option<&LastOp> {
        self.last_op.as_ref().filter(|l| l.history_len == self.history.len() && self.mode == EditMode::Edit)
    }
}

/// Chave i18n do rótulo de um operador.
pub(crate) fn op_label_key(k: OpKind) -> String {
    format!("mdl.opk.{}", k.key())
}

fn mat_of(o: &doc::Object3d) -> Mat4 {
    o.matrix()
}

fn local(rect: Rect, p: Pos2) -> Vec2 {
    Vec2::new(p.x - rect.left(), p.y - rect.top())
}

fn to_screen(rect: Rect, p: Vec2) -> Pos2 {
    Pos2::new(rect.left() + p.x, rect.top() + p.y)
}

fn size_of(rect: Rect) -> Vec2 {
    Vec2::new(rect.width().max(1.0), rect.height().max(1.0))
}

/// Cópia da malha com vértices em espaço de mundo (o picking usa a câmera de mundo), como no ANIGO.
fn world_mesh(o: &doc::Object3d) -> EditMesh {
    let model = mat_of(o);
    let mut m = o.mesh.clone();
    m.transform_positions(|p| model.transform_point3(p));
    m
}

/// Malha de mundo e topologia do objeto ativo (cache por revisão).
fn world_topo(ms: &mut ModelingState) -> Option<(&Topology, &EditMesh)> {
    let a = ms.active?;
    let o = ms.doc.objects.get(a)?;
    let fresh = ms.topo_cache.as_ref().is_some_and(|(r, i, _, _)| *r == ms.revision && *i == a);
    if !fresh {
        let w = world_mesh(o);
        let t = Topology::build(&w);
        ms.topo_cache = Some((ms.revision, a, t, w));
    }
    ms.topo_cache.as_ref().map(|(_, _, t, w)| (t, w))
}

// ============================================================================================================ painel

/// Painel Viewport 3D (o central da Modelagem).
pub(crate) fn viewport(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let bar = Rect::from_min_size(body.min, egui::vec2(body.width(), 30.0));
    let area = Rect::from_min_max(egui::pos2(body.left(), bar.bottom()), body.max);
    env.ed.modeling.fix();
    env.ed.modeling.sanitize_selection();
    header(env, ui, bar);
    // multivista: a vista sob o ponteiro vira a ativa (só ela recebe entrada); as outras só desenham
    let rect = {
        let ms = &mut env.ed.modeling;
        let busy = ms.modal.is_some() || ms.box_start.is_some() || !ms.lasso.is_empty();
        ms.views.seed(&ms.cam);
        let panes = ms.views.pane_rects(area);
        for (s, r) in ms.views.visible().into_iter().zip(panes) {
            ms.views.rects[s] = r;
        }
        ms.views.follow_pointer(&mut ms.cam, ui.ctx(), busy);
        ms.views.rects[ms.views.active]
    };
    let others: Vec<usize> = env.ed.modeling.views.visible().into_iter().filter(|&s| s != env.ed.modeling.views.active).collect();
    for s in others {
        let r = env.ed.modeling.views.rects[s];
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, viewport_bg(&t));
        {
            let ms = &mut env.ed.modeling;
            std::mem::swap(&mut ms.cam, &mut ms.views.cams[s]);
        }
        draw_frame(env, ui.ctx(), &painter, r, s);
        {
            let ms = &mut env.ed.modeling;
            std::mem::swap(&mut ms.cam, &mut ms.views.cams[s]);
        }
        view_label(env, ui, r, s);
    }
    env.note("mdl:viewport", rect);
    let resp = ui.interact(rect, ui.id().with("mdl_viewport"), Sense::click_and_drag());
    env.ed.modeling.hovered = resp.hovered() || resp.dragged();
    // o viewport fica com o foco do teclado com o ponteiro em cima (o Tab é dele: Objeto/Edição), menos quando um campo
    // de texto está sendo editado
    let focused = ui.ctx().memory(|m| m.focused());
    if (resp.hovered() && focused.is_none()) || resp.is_pointer_button_down_on() {
        resp.request_focus();
    }
    if resp.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
    }
    env.ed.modeling.kb = focused.is_none() || focused == Some(resp.id) || resp.has_focus();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, viewport_bg(&t));
    // quadro da GPU (nada volta para a CPU: as texturas são nativas do egui)
    let active_slot = env.ed.modeling.views.active;
    draw_frame(env, ui.ctx(), &painter, rect, active_slot);
    // gizmo de eixos (antes da entrada: o ponteiro em cima dele não começa gesto no viewport)
    let nav = {
        let ms = &mut env.ed.modeling;
        let before = ms.cam;
        let r = nav_gizmo::show(ui, rect, &mut ms.cam, "mdl", env.icons, &t, env.notes);
        let _ = before;
        r
    };
    if !nav.hovered {
        input(env, ui, rect, &resp);
    }
    overlay(env, ui, &painter, rect);
    if env.ed.modeling.views.layout != multiview::ViewLayout::Single {
        view_label(env, ui, rect, active_slot);
        ui.painter().rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, t.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    }
    {
        let ms = &mut env.ed.modeling;
        ms.views.dividers_ui(ui, area, &t, env.notes);
    }
    add_menu(env, ui);
    // a câmera mexeu neste quadro: pede o próximo já (zoom e órbita fluidos)
    if env.ed.modeling.last_cam != Some(env.ed.modeling.cam) {
        let now = Instant::now();
        let ms = &mut env.ed.modeling;
        if let Some(prev) = ms.last_nav.filter(|p| now.duration_since(*p).as_millis() < 250) {
            let dt = now.duration_since(prev).as_secs_f32() * 1000.0;
            ms.nav_frame_ms = if ms.nav_frame_ms > 0.0 { ms.nav_frame_ms * 0.85 + dt * 0.15 } else { dt };
        }
        ms.last_nav = Some(now);
        ms.last_cam = Some(ms.cam);
        ui.ctx().request_repaint();
    }
}

fn viewport_bg(t: &crate::theme::Tokens) -> Color32 {
    // um pouco mais escuro que o painel: o modelo claro com contorno escuro aparece bem nos dois temas
    let b = t.bg_frame;
    Color32::from_rgb(b.r().saturating_sub(6), b.g().saturating_sub(6), b.b().saturating_sub(4))
}

/// Impressão digital barata do documento (pega mudanças feitas direto nos campos, fora do `edit`).
fn doc_fingerprint(ms: &ModelingState) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    mix(ms.doc.objects.len() as u64);
    for o in &ms.doc.objects {
        mix(o.mesh.verts.len() as u64);
        mix(o.mesh.faces.len() as u64);
        mix(o.modifiers.len() as u64);
        mix(o.visible as u64);
        mix(o.shading as u64);
        for x in o.transform.pos.iter().chain(&o.transform.rot_deg).chain(&o.transform.scale).chain(&o.color) {
            mix(x.to_bits() as u64);
        }
        if let Ok(s) = serde_json::to_string(&o.modifiers) {
            mix(aiai_3d_render::fnv1a64(s.as_bytes()));
        }
    }
    mix(ms.sel.verts.len() as u64);
    mix(ms.sel.edges.len() as u64);
    mix(ms.sel.faces.len() as u64);
    mix(ms.active.map_or(u64::MAX, |a| a as u64));
    mix(ms.mode as u64);
    mix(ms.sel_mode as u64);
    h
}

/// Nome da vista no canto (clique = trocar: perspectiva, ortográfica, frente, costas, lados, topo, base).
fn view_label(env: &mut Env, ui: &mut Ui, rect: Rect, slot: usize) {
    let t = env.t;
    let kind = {
        let ms = &env.ed.modeling;
        multiview::ViewKind::of(if slot == ms.views.active { &ms.cam } else { &ms.views.cams[slot] })
    };
    let text = tr(kind.label_key());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), t.text);
    let r = Rect::from_min_size(rect.left_top() + egui::vec2(8.0, 6.0), galley.size() + egui::vec2(12.0, 6.0));
    let resp = ui.interact(r, ui.id().with(("mdl_view_kind", slot)), Sense::click());
    env.note(format!("mdl:view{slot}.kind"), r);
    let p = ui.painter();
    p.rect_filled(r, 3.0, if resp.hovered() { t.bg_hover } else { t.bg_header.gamma_multiply(0.85) });
    p.galley(r.min + egui::vec2(6.0, 3.0), galley, t.text);
    let resp = resp.on_hover_text(tr("mview.kind.tip"));
    let mut chosen = None;
    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        for k in multiview::ViewKind::ALL {
            let b = ui.selectable_label(k == kind, tr(k.label_key()));
            env.notes.push((format!("mdl:view{slot}.kind.{}", k.key()), b.rect));
            if b.clicked() {
                chosen = Some(k);
            }
        }
    });
    if let Some(k) = chosen {
        let ms = &mut env.ed.modeling;
        let cam = if slot == ms.views.active { &mut ms.cam } else { &mut ms.views.cams[slot] };
        k.apply(cam);
    }
}

/// Arranjo das vistas (uma, lado a lado, empilhadas, quatro).
pub(crate) fn set_layout(env: &mut Env, l: multiview::ViewLayout) {
    let ms = &mut env.ed.modeling;
    ms.views.set_layout(&mut ms.cam, l);
}

fn draw_frame(env: &mut Env, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, slot: usize) {
    let t = env.t;
    let ppp = ctx.pixels_per_point();
    let ms = &mut env.ed.modeling;
    let fp = doc_fingerprint(ms);
    let bg = viewport_bg(&t);
    let bgf = [bg.r() as f32 / 255.0, bg.g() as f32 / 255.0, bg.b() as f32 / 255.0, 1.0];
    let rev = ms.revision ^ fp.rotate_left(17);
    let f = gpu::FrameIn {
        doc: &ms.doc,
        active: ms.active,
        edit: ms.mode == EditMode::Edit,
        sculpt: ms.mode == EditMode::Sculpt,
        sel_mode: ms.sel_mode,
        sel: &ms.sel,
        cam: &ms.cam,
        size_px: gpu::px_size(glam::Vec2::new(rect.width(), rect.height()), ppp),
        slot,
        ppp,
        shade: ms.shade,
        xray: ms.xray,
        grid: ms.grid,
        bg: bgf,
        revision: rev,
    };
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    match ms.gpu.frame(&f) {
        Ok(out) => {
            if out.redrawn {
                ms.gpu_cpu_ms = out.cpu_ms;
                ms.frames_drawn += 1;
            }
            if let Some(cel) = out.cel {
                painter.image(cel, rect, uv, Color32::WHITE);
            }
            if let Some(ov) = out.overlay {
                painter.image(ov, rect, uv, Color32::WHITE);
            }
            if let Some(e) = ms.gpu.last_error() {
                painter.text(rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, e, egui::FontId::proportional(11.5), t.danger);
            }
        }
        Err(e) => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{}: {e}", tr("mdl.no_gpu")), egui::FontId::proportional(13.0), t.danger);
        }
    }
}

/// Cabeçalho do viewport: modo, seleção, sombreamento, sobreposições, encaixe, proporcional.
fn header(env: &mut Env, ui: &mut Ui, bar: Rect) {
    let t = env.t;
    ui.painter().rect_filled(bar, 0.0, t.bg_header);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    let ms_mode = env.ed.modeling.mode;
    let tab = env.ed.modeling.keys.label(MAction::ToggleEdit);
    for (m, key) in [(EditMode::Object, "mdl.mode.object"), (EditMode::Edit, "mdl.mode.edit"), (EditMode::Sculpt, "mdl.mode.sculpt")] {
        let tip = if m == EditMode::Sculpt { tr("mdl.mode.sculpt.tip").to_owned() } else { format!("{} ({tab})", tr("mdl.mode.tip")) };
        let r = ui.selectable_label(ms_mode == m, tr(key)).on_hover_text(tip);
        env.note(
            format!(
                "mdl:mode.{}",
                match m {
                    EditMode::Object => "object",
                    EditMode::Edit => "edit",
                    EditMode::Sculpt => "sculpt",
                }
            ),
            r.rect,
        );
        if r.clicked() {
            set_mode(env, m);
        }
    }
    ui.separator();
    let edit = env.ed.modeling.mode == EditMode::Edit;
    for (s, key, name, act) in [(SelectMode::Vertex, "mdl.sel.vertex", "vertex", MAction::VertexMode), (SelectMode::Edge, "mdl.sel.edge", "edge", MAction::EdgeMode), (SelectMode::Face, "mdl.sel.face", "face", MAction::FaceMode)] {
        let tip = format!("{} ({})", tr(key), env.ed.modeling.keys.label(act));
        let r = ui.add_enabled(edit, egui::Button::selectable(env.ed.modeling.sel_mode == s, tr(key))).on_hover_text(tip);
        env.note(format!("mdl:sel.{name}"), r.rect);
        if r.clicked() {
            env.ed.modeling.set_select_mode(s);
        }
    }
    let r = ui.add_enabled(edit, egui::Button::selectable(env.ed.modeling.select_shape == SelectShape::Lasso, tr("mdl.sel.lasso"))).on_hover_text(tr("mdl.sel.lasso.tip"));
    env.note("mdl:sel.lasso", r.rect);
    if r.clicked() {
        let ms = &mut env.ed.modeling;
        ms.select_shape = if ms.select_shape == SelectShape::Lasso { SelectShape::Box } else { SelectShape::Lasso };
    }
    ui.separator();
    // faixa estreita (auditoria UX-05): o que não cabe vai para "Mais ▾" em vez de sumir cortado; a largura em que a
    // faixa transbordou fica guardada para voltar ao normal quando a janela alargar
    let ow_id = egui::Id::new("mdl_header_overflow_w");
    let overflow_w: f32 = ui.ctx().data(|d| d.get_temp(ow_id)).unwrap_or(0.0);
    let narrow = bar.width() <= overflow_w;
    if narrow {
        let mut sh = env.ed.modeling.shade;
        let r = egui::ComboBox::from_id_salt("mdl_shade_combo").width(96.0).selected_text(tr(&format!("mdl.shade.{}", sh.key()))).show_ui(ui, |ui| {
            for m in ViewShade::ALL {
                ui.selectable_value(&mut sh, m, tr(&format!("mdl.shade.{}", m.key())));
            }
        });
        env.ed.modeling.shade = sh;
        env.note("mdl:shade_combo", r.response.rect);
        let r = ui.menu_button(tr("mdl.more"), |ui| header_tail(env, ui, edit));
        env.note("mdl:more", r.response.rect);
    } else {
        header_shade(env, ui);
        ui.separator();
        header_tail(env, ui, edit);
    }
    if env.ed.modeling.dirty {
        ui.separator();
        ui.label(egui::RichText::new(tr("mdl.unsaved")).size(11.0).color(t.text_dim));
    }
    if !narrow && ui.min_rect().right() > bar.right() - 2.0 {
        ui.ctx().data_mut(|d| d.insert_temp(ow_id, bar.width()));
        ui.ctx().request_repaint();
    }
}

/// Sombreamento da vista em botões (faixa larga).
fn header_shade(env: &mut Env, ui: &mut Ui) {
    let shade_tip = format!("{} ({})", tr("mdl.shade.tip"), env.ed.modeling.keys.label(MAction::ShadingToggle));
    for m in ViewShade::ALL {
        let r = ui.selectable_label(env.ed.modeling.shade == m, tr(&format!("mdl.shade.{}", m.key()))).on_hover_text(shade_tip.as_str());
        env.note(format!("mdl:shade.{}", m.key()), r.rect);
        if r.clicked() {
            env.ed.modeling.shade = m;
        }
    }
}

/// Raio X, grade, vistas, encaixe e proporcional (na faixa ou no menu "Mais" quando ela não cabe).
fn header_tail(env: &mut Env, ui: &mut Ui, edit: bool) {
    let k = env.ed.modeling.keys.clone();
    let r = ui.selectable_label(env.ed.modeling.xray, tr("mdl.xray")).on_hover_text(k.tip(MAction::Xray));
    env.note("mdl:xray", r.rect);
    if r.clicked() {
        env.ed.modeling.xray = !env.ed.modeling.xray;
    }
    let r = ui.selectable_label(env.ed.modeling.grid, tr("mdl.grid")).on_hover_text(tr("mdl.grid.tip"));
    env.note("mdl:grid", r.rect);
    if r.clicked() {
        env.ed.modeling.grid = !env.ed.modeling.grid;
    }
    // vistas prontas (as mesmas do gizmo de eixos e das teclas)
    let r = ui.menu_button(tr("mdl.view"), |ui| {
        for (key, act) in [("front", Some(MAction::ViewFront)), ("back", None), ("right", Some(MAction::ViewRight)), ("left", None), ("top", Some(MAction::ViewTop)), ("bottom", None)] {
            let label = match act {
                Some(a) => format!("{}  ({})", tr(&format!("mdl.view.{key}")), k.label(a)),
                None => tr(&format!("mdl.view.{key}")).to_owned(),
            };
            if ui.button(label).clicked() {
                set_view(env, key);
                ui.close();
            }
        }
        ui.separator();
        let ortho = env.ed.modeling.cam.orthographic;
        if ui.selectable_label(ortho, format!("{}  ({})", tr("mdl.view.ortho"), k.label(MAction::ViewOrtho))).clicked() {
            set_view(env, if ortho { "persp" } else { "ortho" });
            ui.close();
        }
        if ui.button(format!("{}  ({})", tr("mdl.view.frame"), k.label(MAction::Frame))).clicked() {
            frame_selection(env);
            ui.close();
        }
        if ui.button(format!("{}  ({})", tr("mdl.view.frame_all"), k.label(MAction::FrameAll))).clicked() {
            frame_all(env);
            ui.close();
        }
        // multivista (como o Blender): uma, duas ou quatro vistas, cada uma com a sua câmera
        ui.separator();
        let cur = env.ed.modeling.views.layout;
        for l in multiview::ViewLayout::ALL {
            let label = match l {
                multiview::ViewLayout::Quad => format!("{}  ({})", tr(l.label_key()), k.label(MAction::QuadView)),
                _ => tr(l.label_key()).to_owned(),
            };
            let b = ui.selectable_label(cur == l, label);
            env.notes.push((format!("mdl:layout.{}", l.key()), b.rect));
            if b.clicked() {
                set_layout(env, l);
                ui.close();
            }
        }
        let b = ui.add_enabled(cur != multiview::ViewLayout::Single || env.ed.modeling.views.restore != multiview::ViewLayout::Single, egui::Button::new(format!("{}  ({})", tr("mview.maximize"), k.label(MAction::MaximizeView))));
        if b.clicked() {
            let ms = &mut env.ed.modeling;
            ms.views.toggle_maximize(&mut ms.cam);
            ui.close();
        }
    });
    env.note("mdl:view", r.response.rect);
    let r = ui.selectable_label(env.ed.modeling.snap, tr("mdl.snap")).on_hover_text(tr("mdl.snap.tip"));
    env.note("mdl:snap", r.rect);
    if r.clicked() {
        env.ed.modeling.snap = !env.ed.modeling.snap;
    }
    let prop_on = env.ed.modeling.proportional.enabled;
    let r = ui.add_enabled(edit, egui::Button::selectable(prop_on, tr("mdl.prop"))).on_hover_text(k.tip(MAction::Proportional));
    env.note("mdl:prop", r.rect);
    if r.clicked() {
        env.ed.modeling.proportional.enabled = !prop_on;
    }
    if prop_on && edit {
        let p = &mut env.ed.modeling.proportional;
        ui.add(egui::DragValue::new(&mut p.radius).speed(0.01).range(0.01..=100.0).max_decimals(2)).on_hover_text(tr("mdl.prop.radius"));
        egui::ComboBox::from_id_salt("mdl_prop_falloff").width(80.0).selected_text(tr(falloff_key(p.falloff))).show_ui(ui, |ui| {
            for f in [ProportionalFalloff::Smooth, ProportionalFalloff::Sphere, ProportionalFalloff::Root, ProportionalFalloff::Sharp, ProportionalFalloff::Linear, ProportionalFalloff::Constant] {
                ui.selectable_value(&mut p.falloff, f, tr(falloff_key(f)));
            }
        });
        ui.checkbox(&mut p.connected, tr("mdl.prop.connected"));
    }
}

fn falloff_key(f: ProportionalFalloff) -> &'static str {
    match f {
        ProportionalFalloff::Smooth => "mdl.falloff.smooth",
        ProportionalFalloff::Sphere => "mdl.falloff.sphere",
        ProportionalFalloff::Root => "mdl.falloff.root",
        ProportionalFalloff::Sharp => "mdl.falloff.sharp",
        ProportionalFalloff::Linear => "mdl.falloff.linear",
        ProportionalFalloff::Constant => "mdl.falloff.constant",
        _ => "mdl.falloff.smooth",
    }
}

pub(crate) fn set_mode(env: &mut Env, m: EditMode) {
    let ms = &mut env.ed.modeling;
    ms.cancel_modal();
    if m != EditMode::Object && ms.active.is_none() {
        env.ed.notify(tr("mdl.need_object"));
        return;
    }
    let was = ms.mode;
    if was == EditMode::Sculpt && m != EditMode::Sculpt {
        sculpt::cancel_stroke(ms);
        ms.sculpt.clear();
        if let Some(s) = ms.sculpt.shade_before.take() {
            ms.shade = s;
        }
    }
    if m == EditMode::Sculpt && was != EditMode::Sculpt {
        // a escultura usa o sólido (relevo e máscara à vista); o sombreamento de antes volta ao sair
        if ms.shade.is_cel() {
            ms.sculpt.shade_before = Some(ms.shade);
            ms.shade = ViewShade::Solid;
        }
    }
    ms.mode = m;
    ms.sel.clear();
    ms.touch();
}

// ========================================================================================================= overlay

const ORANGE: Color32 = Color32::from_rgb(255, 154, 46);
const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(0xe8, 0x45, 0x4b), Color32::from_rgb(0x6c, 0xc0, 0x4a), Color32::from_rgb(0x3d, 0x7a, 0xf0)];

fn overlay(env: &mut Env, ui: &mut Ui, painter: &egui::Painter, rect: Rect) {
    let t = env.t;
    if env.ed.modeling.mode == EditMode::Sculpt {
        sculpt_ring(env, painter, rect);
    }
    let size = size_of(rect);
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
    if env.ed.modeling.doc.objects.is_empty() {
        let g = painter.layout(tr("mdl.empty").to_owned(), egui::FontId::proportional(13.0), t.text_dim, rect.width() * 0.6);
        painter.galley(rect.center() - g.size() * 0.5, g, t.text_dim);
    }
    // caixa / laço de seleção
    {
        let ms = &env.ed.modeling;
        if let (Some(start), Some(p)) = (ms.box_start, pointer) {
            if ms.select_shape == SelectShape::Box {
                let r = Rect::from_two_pos(to_screen(rect, start), p);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 154, 46, 28));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
            }
        }
        if ms.lasso.len() > 1 {
            let pts: Vec<Pos2> = ms.lasso.iter().map(|q| to_screen(rect, *q)).collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2, ORANGE)));
        }
    }
    // faces escolhidas na prévia cel (o Workbench só pinta o sólido nos modos Sólido/Arame): preenchimento translúcido
    if env.ed.modeling.mode == EditMode::Edit && env.ed.modeling.shade.is_cel() && !env.ed.modeling.sel.faces.is_empty() && env.ed.modeling.sel.faces.len() <= 4000 {
        let cam = env.ed.modeling.cam;
        let faces: Vec<u32> = env.ed.modeling.sel.faces.iter().copied().collect();
        let xray = env.ed.modeling.xray;
        if let Some((_, w)) = world_topo(&mut env.ed.modeling) {
            let eye = cam.eye();
            let mut mesh = egui::Mesh::default();
            let fill = Color32::from_rgba_unmultiplied(255, 154, 46, 60);
            for f in faces.into_iter().filter(|f| (*f as usize) < w.face_count()) {
                let pts = w.face_positions(f);
                let n = w.face_normal(f);
                let c = w.face_center(f);
                let facing = if cam.orthographic { n.dot(-cam.forward()) } else { n.dot(eye - c) };
                if facing <= 0.0 && !xray {
                    continue;
                }
                let sp: Option<Vec<Pos2>> = pts.iter().map(|p| cam.project(*p, size).map(|(q, _)| to_screen(rect, q))).collect();
                let Some(sp) = sp else { continue };
                let base = mesh.vertices.len() as u32;
                for p in &sp {
                    mesh.colored_vertex(*p, fill);
                }
                for tri in doc::triangulate_polygon(&pts) {
                    mesh.add_triangle(base + tri[0] as u32, base + tri[1] as u32, base + tri[2] as u32);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    // aresta do loop cut sob o cursor
    if *env.tool == ToolId::MLoopCut && env.ed.modeling.mode == EditMode::Edit && env.ed.modeling.modal.is_none() {
        if let Some(p) = pointer.filter(|p| rect.contains(*p)) {
            let cam = env.ed.modeling.cam;
            if let Some((topo, w)) = world_topo(&mut env.ed.modeling) {
                let ctx = picking::PickContext { mesh: w, camera: &cam, size, xray: false };
                if let Some((a, b)) = picking::pick_edge(&ctx, topo, local(rect, p), 12.0) {
                    if let (Some((pa, _)), Some((pb, _))) = (cam.project(w.co(a), size), cam.project(w.co(b), size)) {
                        painter.line_segment([to_screen(rect, pa), to_screen(rect, pb)], Stroke::new(3.0, Color32::from_rgb(255, 220, 90)));
                    }
                }
            }
        }
    }
    // gizmo de transformação (ferramentas mover/girar/escalar)
    let ms = &env.ed.modeling;
    if let (Some(kind), Some(pivot)) = (tool_transform(*env.tool), gizmo_pivot(ms)) {
        if ms.modal.is_none() || ms.modal.as_ref().is_some_and(|m| m.from_tool) {
            draw_gizmo(painter, rect, &ms.cam, pivot, kind, ms.gizmo_hover);
        }
    }
    // operação em curso: eixo, raio proporcional e valor
    if let Some(md) = &ms.modal {
        if let ModalOp::Transform { t: tr_, .. } = &md.op {
            let seg = |a: Vec3, b: Vec3, s: Stroke| {
                if let (Some((pa, _)), Some((pb, _))) = (ms.cam.project(a, size), ms.cam.project(b, size)) {
                    painter.line_segment([to_screen(rect, pa), to_screen(rect, pb)], s);
                }
            };
            match tr_.constraint {
                Constraint::Axis(a) => seg(tr_.pivot - a.vector() * 50.0, tr_.pivot + a.vector() * 50.0, Stroke::new(1.4, AXIS_COLORS[a as usize])),
                Constraint::Direction(d) => seg(tr_.pivot - d * 50.0, tr_.pivot + d * 50.0, Stroke::new(1.4, ORANGE)),
                Constraint::Plane(a) => {
                    for b in [Axis::X, Axis::Y, Axis::Z].into_iter().filter(|b| *b != a) {
                        seg(tr_.pivot - b.vector() * 50.0, tr_.pivot + b.vector() * 50.0, Stroke::new(1.0, AXIS_COLORS[b as usize].gamma_multiply(0.7)));
                    }
                }
                Constraint::None => {}
            }
            if tr_.weights.is_some() {
                // raio proporcional na tela
                if let (Some((c, _)), Some((e, _))) = (ms.cam.project(tr_.pivot, size), ms.cam.project(tr_.pivot + ms.cam.right() * ms.proportional.radius, size)) {
                    painter.circle_stroke(to_screen(rect, c), c.distance(e), Stroke::new(1.0, Color32::from_white_alpha(140)));
                }
            }
        }
        let txt = modal_status(ms, md, pointer.map(|p| local(rect, p)).unwrap_or(md.start), size);
        hint(painter, rect, &txt, t.text, true);
    } else {
        hint(painter, rect, &hint_text(ms), t.text, false);
    }
}

/// Dica do rodapé com as teclas do perfil em uso.
/// Entrada do modo Escultura: arrastar = traço (Ctrl inverte, Shift alisa), Esc cancela, [ e ] mudam o raio, Tab volta
/// ao modo Objeto; sem botão, o círculo do pincel segue a superfície.
fn sculpt_input(env: &mut Env, ctx: &egui::Context, resp: &egui::Response, mouse: Vec2, size: Vec2, mods: (bool, bool, bool, bool)) {
    let (alt, shift, ctrl, space) = mods;
    let pressure = ctx.input(|i| i.events.iter().rev().find_map(|e| match e {
        egui::Event::Touch { force: Some(f), phase, .. } if *phase != egui::TouchPhase::End => Some(*f),
        _ => None,
    }));
    let time = ctx.input(|i| i.time);
    let keys_here = env.ed.modeling.hovered && env.ed.modeling.kb;
    if keys_here {
        let (esc, smaller, bigger, tab) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::OpenBracket), i.key_pressed(egui::Key::CloseBracket), i.key_pressed(egui::Key::Tab)));
        if esc {
            sculpt::cancel_stroke(&mut env.ed.modeling);
        }
        if smaller || bigger {
            let s = env.ed.modeling.sculpt.active_slot_mut();
            s.radius_px = (s.radius_px * if bigger { 1.15 } else { 1.0 / 1.15 }).clamp(sculpt::RADIUS_MIN_PX, sculpt::RADIUS_MAX_PX);
        }
        if tab && !env.ed.modeling.sculpt.is_stroking() {
            set_mode(env, EditMode::Object);
            return;
        }
    }
    let ms = &mut env.ed.modeling;
    let view = sculpt::ViewInfo { camera: ms.cam, size };
    let stroking = ms.sculpt.is_stroking();
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt && !space && !stroking {
        let press = ctx.input(|i| i.pointer.press_origin()).map(|p| local(resp.rect, p)).unwrap_or(mouse);
        let r = sculpt::begin_stroke(ms, &view, view.ray(press), pressure.unwrap_or(1.0), time, sculpt::StrokeMods { invert: ctrl, smooth: shift });
        if let Err(e) = r {
            ms.sculpt.error = Some(e);
        }
        if ms.sculpt.is_stroking() && press != mouse {
            sculpt::continue_stroke(ms, view.ray(mouse), pressure.unwrap_or(1.0), time);
        }
    } else if stroking && ctx.input(|i| i.pointer.primary_down()) {
        sculpt::continue_stroke(ms, view.ray(mouse), pressure.unwrap_or(1.0), time);
        ctx.request_repaint();
    } else if stroking {
        sculpt::end_stroke(ms);
    }
    if resp.hovered() && !ms.sculpt.is_stroking() {
        sculpt::update_hover(ms, view.ray(mouse));
    } else if !resp.hovered() {
        ms.sculpt.hover = None;
    }
}

/// Círculo do pincel de escultura na superfície (raio em px de tela levado ao ponto sob o cursor).
fn sculpt_ring(env: &Env, painter: &egui::Painter, rect: Rect) {
    let ms = &env.ed.modeling;
    let Some(h) = ms.sculpt.hover.or(None) else { return };
    let Some(obj) = ms.active.and_then(|a| ms.doc.objects.get(a)) else { return };
    let size = size_of(rect);
    let model = obj.matrix();
    let slot = ms.sculpt.slot(ms.sculpt.active);
    let r_local = sculpt::px_to_local_radius(&ms.cam, size.y, h.world_point, slot.radius_px, model);
    let pts: Vec<Pos2> = sculpt::ring_points_world(model, h.local_point, h.local_normal, r_local, 48).into_iter().filter_map(|w| ms.cam.project(w, size)).map(|(p, _)| to_screen(rect, p)).collect();
    let ink = if ms.sculpt.active.is_mask() { Color32::from_rgb(120, 170, 255) } else { Color32::WHITE };
    if pts.len() > 2 {
        painter.add(egui::Shape::line(pts, Stroke::new(1.4, ink)));
    }
    let _ = sculpt::normal_to_world;
}

fn hint_text(ms: &ModelingState) -> String {
    if ms.mode == EditMode::Sculpt {
        return tr("mdl.hint.sculpt").to_owned();
    }
    let k = &ms.keys;
    let acts: &[MAction] = if ms.mode == EditMode::Edit {
        &[MAction::ToggleEdit, MAction::Select, MAction::Move, MAction::Rotate, MAction::Scale, MAction::Extrude, MAction::Inset, MAction::Bevel, MAction::LoopCut, MAction::Slide, MAction::Delete]
    } else {
        &[MAction::ToggleEdit, MAction::Move, MAction::Rotate, MAction::Scale, MAction::Duplicate, MAction::Delete, MAction::AddMenu, MAction::Frame]
    };
    let mut parts: Vec<String> = acts.iter().map(|a| k.tip(*a)).collect();
    parts.push(tr("mdl.hint.nav").to_owned());
    parts.join(" · ")
}

fn hint(painter: &egui::Painter, rect: Rect, text: &str, ink: Color32, strong: bool) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(if strong { 13.0 } else { 11.5 }), if strong { Color32::WHITE } else { ink });
    let pos = if strong { Pos2::new(rect.center().x - galley.size().x * 0.5, rect.bottom() - 34.0) } else { rect.left_bottom() + egui::vec2(10.0, -24.0) };
    painter.rect_filled(Rect::from_min_size(pos - egui::vec2(6.0, 3.0), galley.size() + egui::vec2(12.0, 6.0)), 5.0, Color32::from_rgba_unmultiplied(20, 20, 26, if strong { 220 } else { 150 }));
    painter.galley(pos, galley, ink);
}

fn modal_status(ms: &ModelingState, md: &Modal, mouse: Vec2, size: Vec2) -> String {
    let ctx = ModalContext { camera: ms.cam, viewport: size };
    let num = if md.numeric.is_empty() { String::new() } else { format!("  [{}]", md.numeric) };
    match &md.op {
        ModalOp::Transform { t, .. } => {
            let base = t.status_text(mouse, &ctx);
            let label = tr(md.label);
            let rest = base.split_once("  ").map(|(_, r)| r).unwrap_or("");
            format!("{label}  {rest}")
        }
        ModalOp::Inset { .. } => format!("{}  {:.3}{num}", tr(md.label), md.params.thickness),
        ModalOp::Bevel { .. } => format!("{}  {:.3}  ·  {} {}{num}", tr(md.label), md.params.width, md.params.segments, tr("mdl.segments")),
        ModalOp::LoopCut { .. } => format!("{}  {} {}  ·  {:.2}{num}", tr(md.label), md.params.cuts, tr("mdl.cuts"), md.params.slide),
        ModalOp::Slide { .. } => format!("{}  {:.2}{num}", tr(md.label), md.params.slide),
    }
}

fn draw_gizmo(painter: &egui::Painter, rect: Rect, cam: &OrbitCamera, pivot: Vec3, kind: TransformKind, hot: Option<Axis>) {
    let size = size_of(rect);
    let s = gizmo::world_size(cam, pivot, size.y);
    let to_s = |p: Vec3| cam.project(p, size).map(|(px, _)| to_screen(rect, px));
    let Some(origin) = to_s(pivot) else { return };
    for (axis, color) in [(Axis::X, AXIS_COLORS[0]), (Axis::Y, AXIS_COLORS[1]), (Axis::Z, AXIS_COLORS[2])] {
        let is_hot = hot == Some(axis);
        let c = if is_hot { Color32::from_rgb(0xff, 0xe0, 0x60) } else { color };
        let w = if is_hot { 3.5 } else { 2.5 };
        match kind {
            TransformKind::Grab | TransformKind::Scale => {
                if let Some(end) = to_s(pivot + axis.vector() * s) {
                    painter.line_segment([origin, end], Stroke::new(w, c));
                    if kind == TransformKind::Grab {
                        let dir = (end - origin).normalized();
                        let n = egui::vec2(-dir.y, dir.x);
                        painter.add(egui::Shape::convex_polygon(vec![end + dir * 12.0, end + n * 6.0, end - n * 6.0], c, Stroke::NONE));
                    } else {
                        painter.rect_filled(Rect::from_center_size(end, egui::vec2(10.0, 10.0)), 1.0, c);
                    }
                }
            }
            TransformKind::Rotate => {
                let pts: Vec<Pos2> = gizmo::ring_points(pivot, axis, s * 0.9).into_iter().filter_map(to_s).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(w, c)));
            }
        }
    }
    painter.circle_filled(origin, 4.0, Color32::WHITE);
}

fn tool_transform(t: ToolId) -> Option<TransformKind> {
    match t {
        ToolId::MMove => Some(TransformKind::Grab),
        ToolId::MRotate => Some(TransformKind::Rotate),
        ToolId::MScale => Some(TransformKind::Scale),
        _ => None,
    }
}

fn gizmo_pivot(ms: &ModelingState) -> Option<Vec3> {
    let a = ms.active?;
    let o = ms.doc.objects.get(a)?;
    match ms.mode {
        EditMode::Object | EditMode::Sculpt => Some(Vec3::from(o.transform.pos)),
        EditMode::Edit => {
            if ms.sel.verts.is_empty() {
                return None;
            }
            let m = mat_of(o);
            let n = o.mesh.vert_count() as u32;
            let pts: Vec<Vec3> = ms.sel.verts.iter().filter(|v| **v < n).map(|v| m.transform_point3(o.mesh.co(*v))).collect();
            (!pts.is_empty()).then(|| pts.iter().sum::<Vec3>() / pts.len() as f32)
        }
    }
}

// =========================================================================================================== entrada

fn input(env: &mut Env, ui: &mut Ui, rect: Rect, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    let size = size_of(rect);
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let mouse = pointer.map(|p| local(rect, p)).unwrap_or(size * 0.5);
    let (alt, shift, ctrl, delta, scroll, zoom_delta, space) = ctx.input(|i| (i.modifiers.alt, i.modifiers.shift, i.modifiers.command, i.pointer.delta(), i.smooth_scroll_delta.y, i.zoom_delta(), i.key_down(egui::Key::Space)));
    let modal_on = env.ed.modeling.modal.is_some();
    // ---- câmera (ANIGO: roda = zoom para o cursor; meio orbita; Shift pan; Ctrl zoom)
    {
        let ms = &mut env.ed.modeling;
        let wheel_for_modal = modal_on && ms.modal.as_ref().is_some_and(|m| matches!(m.op, ModalOp::Bevel { .. } | ModalOp::LoopCut { .. }) || (matches!(m.op, ModalOp::Transform { .. }) && ms.proportional.enabled && ms.mode == EditMode::Edit));
        if resp.hovered() && !wheel_for_modal {
            let aspect = size.x / size.y;
            let ndc = Vec2::new(mouse.x / size.x * 2.0 - 1.0, 1.0 - mouse.y / size.y * 2.0);
            if scroll.abs() > 0.0 {
                let factor = (1.0 - scroll * 0.0025).clamp(0.5, 1.5);
                ms.cam.zoom_to_cursor(factor, ndc, aspect);
            }
            if (zoom_delta - 1.0).abs() > 1e-4 {
                ms.cam.zoom_to_cursor((1.0 / zoom_delta).clamp(0.5, 1.5), ndc, aspect);
            }
        }
        let d = delta;
        let mid = resp.dragged_by(egui::PointerButton::Middle);
        let right = !modal_on && resp.dragged_by(egui::PointerButton::Secondary);
        let alt_orbit = alt && !modal_on && resp.dragged_by(egui::PointerButton::Primary);
        let space_pan = space && !modal_on && resp.dragged_by(egui::PointerButton::Primary);
        if space_pan {
            ms.cam.pan(d.x, d.y, size.y);
        } else if mid || right || alt_orbit {
            if shift {
                ms.cam.pan(d.x, d.y, size.y);
            } else if ctrl {
                ms.cam.zoom((1.0 + d.y * 0.01).clamp(0.5, 1.5));
            } else {
                ms.cam.orbit(d.x, d.y);
            }
        }
        if space && resp.hovered() {
            crate::icons::set_hand_cursor(&ctx, resp.dragged());
        }
        if mid || right || alt_orbit || space_pan {
            ms.box_start = None;
            ms.lasso.clear();
            return;
        }
    }
    // ---- escultura: o arrasto esculpe (a câmera acima continua valendo)
    if env.ed.modeling.mode == EditMode::Sculpt {
        sculpt_input(env, &ctx, resp, mouse, size, (alt, shift, ctrl, space));
        return;
    }
    // ---- teclas (com o ponteiro no viewport ou com uma operação em curso)
    let keys_here = (env.ed.modeling.hovered || modal_on) && env.ed.modeling.kb;
    if keys_here {
        if modal_on {
            modal_keys(env, &ctx);
        } else {
            keys(env, &ctx, rect, mouse);
        }
    }
    // ---- operação em curso: segue o mouse; clique confirma, direito cancela
    if env.ed.modeling.modal.is_some() {
        // roda: segmentos do bevel, cortes do loop cut, raio proporcional
        if scroll.abs() > 0.5 {
            let step = if scroll > 0.0 { 1i32 } else { -1 };
            let ms = &mut env.ed.modeling;
            if let Some(md) = ms.modal.as_mut() {
                match md.op {
                    ModalOp::Bevel { .. } => md.params.segments = (md.params.segments as i32 + step).clamp(1, 32) as u32,
                    ModalOp::LoopCut { .. } => md.params.cuts = (md.params.cuts as i32 + step).clamp(1, 64) as u32,
                    ModalOp::Transform { .. } if ms.proportional.enabled => ms.proportional.radius = (ms.proportional.radius * if step > 0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.01, 100.0),
                    _ => {}
                }
            }
            ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
        }
        update_modal(env, mouse, size, ctrl);
        let (lclick, rclick, released) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Primary), i.pointer.button_pressed(egui::PointerButton::Secondary), i.pointer.button_released(egui::PointerButton::Primary)));
        let from_tool = env.ed.modeling.modal.as_ref().is_some_and(|m| m.from_tool);
        if rclick {
            env.ed.modeling.cancel_modal();
        } else if (lclick && !from_tool) || (released && from_tool) {
            confirm_modal(env);
            env.ed.modeling.swallow_release = lclick;
        }
        ctx.request_repaint();
        return;
    }
    if env.ed.modeling.swallow_release {
        if !ctx.input(|i| i.pointer.primary_down()) {
            env.ed.modeling.swallow_release = false;
        }
        return;
    }
    let tool = *env.tool;
    // ---- gizmo: alça sob o cursor
    let pivot = gizmo_pivot(&env.ed.modeling);
    let tk = tool_transform(tool);
    env.ed.modeling.gizmo_hover = match (pivot, tk) {
        (Some(p), Some(k)) if resp.hovered() => gizmo::hit_test(k, &env.ed.modeling.cam, size, p, mouse),
        _ => None,
    };
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt && !space {
        let press = ctx.input(|i| i.pointer.press_origin()).map(|p| local(rect, p)).unwrap_or(mouse);
        match tool {
            ToolId::MMove | ToolId::MRotate | ToolId::MScale => {
                let k = tk.expect("ferramenta de transformação");
                let axis = pivot.and_then(|p| gizmo::hit_test(k, &env.ed.modeling.cam, size, p, press));
                let has_target = match env.ed.modeling.mode {
                    EditMode::Object | EditMode::Sculpt => env.ed.modeling.active.is_some(),
                    EditMode::Edit => !env.ed.modeling.sel.verts.is_empty(),
                };
                if has_target || axis.is_some() {
                    if start_transform(env, k, press, true) {
                        if let (Some(a), Some(md)) = (axis, env.ed.modeling.modal.as_mut()) {
                            if let ModalOp::Transform { t, .. } = &mut md.op {
                                t.toggle_axis(a, false);
                            }
                        }
                    }
                    return;
                }
                // nada escolhido no modo Edição: o arrasto vira seleção
                env.ed.modeling.box_start = Some(press);
            }
            ToolId::MExtrude => {
                start_extrude(env, press, true);
                return;
            }
            ToolId::MInset => {
                start_inset(env, size, press, true);
                return;
            }
            ToolId::MBevel => {
                start_bevel(env, size, press, true);
                return;
            }
            ToolId::MLoopCut => {
                start_loop_cut(env, size, press, true);
                return;
            }
            _ => {
                env.ed.modeling.box_start = Some(press);
                env.ed.modeling.lasso = vec![press];
            }
        }
    }
    // ---- caixa / laço
    if env.ed.modeling.box_start.is_some() && env.ed.modeling.select_shape == SelectShape::Lasso && ctx.input(|i| i.pointer.primary_down()) {
        let ms = &mut env.ed.modeling;
        if ms.lasso.last().is_none_or(|l| l.distance(mouse) > 3.0) {
            ms.lasso.push(mouse);
        }
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) {
        let ms = &mut env.ed.modeling;
        if let Some(start) = ms.box_start.take() {
            let lasso = std::mem::take(&mut ms.lasso);
            let sub = ctrl;
            if ms.select_shape == SelectShape::Lasso && lasso.len() > 2 {
                apply_region_select(env, Region::Lasso(lasso), size, shift, sub);
            } else if (mouse - start).length() > 4.0 {
                apply_region_select(env, Region::Box(start.min(mouse), start.max(mouse)), size, shift, sub);
            } else {
                click_select(env, size, mouse, shift);
            }
        }
        return;
    }
    if resp.clicked_by(egui::PointerButton::Primary) && !alt && !space {
        env.ed.modeling.box_start = None;
        env.ed.modeling.lasso.clear();
        if tool == ToolId::MLoopCut && env.ed.modeling.mode == EditMode::Edit {
            if start_loop_cut(env, size, mouse, false) {
                confirm_modal(env);
            }
        } else {
            click_select(env, size, mouse, shift);
        }
    }
}

/// Teclas dentro de uma operação: eixo (Shift = plano), número, confirmar/cancelar.
fn modal_keys(env: &mut Env, ctx: &egui::Context) {
    use egui::Key;
    let ev: Vec<(Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    let typed: String = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
    for (k, m) in &ev {
        let Some(md) = env.ed.modeling.modal.as_mut() else { return };
        match k {
            Key::X | Key::Y | Key::Z if !m.command && !m.alt => {
                if let ModalOp::Transform { t, .. } = &mut md.op {
                    let a = match k {
                        Key::X => Axis::X,
                        Key::Y => Axis::Y,
                        _ => Axis::Z,
                    };
                    t.toggle_axis(a, m.shift);
                }
            }
            Key::Backspace => {
                if let ModalOp::Transform { t, .. } = &mut md.op {
                    t.pop_numeric();
                } else {
                    md.numeric.pop();
                }
            }
            Key::Enter => {
                confirm_modal(env);
                return;
            }
            Key::Escape => {
                env.ed.modeling.cancel_modal();
                return;
            }
            _ => {}
        }
    }
    if let Some(md) = env.ed.modeling.modal.as_mut() {
        for c in typed.chars() {
            if let ModalOp::Transform { t, .. } = &mut md.op {
                if c.is_ascii_digit() || c == '.' || c == ',' || c == '-' {
                    t.push_numeric(c);
                }
            } else if c.is_ascii_digit() || c == '.' || c == ',' || (c == '-' && md.numeric.is_empty()) {
                md.numeric.push(if c == ',' { '.' } else { c });
            }
        }
    }
}

/// Teclas do contexto Modelagem (fora de operação).
fn keys(env: &mut Env, ctx: &egui::Context, rect: Rect, mouse: Vec2) {
    let size = size_of(rect);
    // desfazer/refazer do app (o viewport tem o foco do teclado: o atalho global não chega até o documento aqui)
    let app_keys = env.ed.keys.clone();
    let (undo, redo) = ctx.input_mut(|i| {
        let redo = app_keys.get(crate::keymap::Action::EditRedo).is_some_and(|c| c.take(i)) || crate::keymap::Chord::ctrl(egui::Key::Y).take(i);
        let undo = app_keys.get(crate::keymap::Action::EditUndo).is_some_and(|c| c.take(i));
        (undo, redo)
    });
    if redo || undo {
        let ok = if redo { env.ed.modeling.redo() } else { env.ed.modeling.undo() };
        let key = match (redo, ok) {
            (true, true) => "mdl.redone",
            (true, false) => "mdl.nothing_redo",
            (false, true) => "mdl.undone",
            (false, false) => "mdl.nothing_undo",
        };
        env.ed.notify(tr(key));
        return;
    }
    let km = env.ed.modeling.keys.clone();
    let mut fired: Vec<MAction> = Vec::new();
    ctx.input_mut(|i| {
        for (a, ..) in keys::ACTIONS {
            if km.take(i, a) {
                fired.push(a);
            }
        }
    });
    for a in fired {
        let edit = env.ed.modeling.mode == EditMode::Edit;
        // ferramentas: o perfil AnimeIAI escolhe a ferramenta; o Blender (ou "teclas modais") começa a operação
        if let Some(tool) = a.tool() {
            if !km.modal_keys || a == MAction::Select {
                *env.tool = tool;
                continue;
            }
        }
        let ms = &mut env.ed.modeling;
        match a {
            MAction::Move => {
                start_transform(env, TransformKind::Grab, mouse, false);
            }
            MAction::Rotate => {
                start_transform(env, TransformKind::Rotate, mouse, false);
            }
            MAction::Scale => {
                start_transform(env, TransformKind::Scale, mouse, false);
            }
            MAction::Extrude if edit => start_extrude(env, mouse, false),
            MAction::Inset if edit => {
                start_inset(env, size, mouse, false);
            }
            MAction::Bevel if edit => {
                start_bevel(env, size, mouse, false);
            }
            MAction::LoopCut if edit => {
                start_loop_cut(env, size, mouse, false);
            }
            MAction::Slide if edit => start_slide(env, size, mouse),
            MAction::ToggleEdit => {
                let next = if edit { EditMode::Object } else { EditMode::Edit };
                set_mode(env, next);
            }
            MAction::VertexMode if edit => ms.set_select_mode(SelectMode::Vertex),
            MAction::EdgeMode if edit => ms.set_select_mode(SelectMode::Edge),
            MAction::FaceMode if edit => ms.set_select_mode(SelectMode::Face),
            MAction::SelectAll if edit => {
                if let Some(m) = ms.active_mesh() {
                    let s = Selection::select_all(m).flush(m, ms.sel_mode);
                    ms.set_selection(s);
                }
            }
            MAction::SelectNone if edit => ms.set_selection(Selection::default()),
            MAction::SelectInvert if edit => {
                if let Some(m) = ms.active_mesh() {
                    let s = ms.sel.invert(m, ms.sel_mode).flush(m, ms.sel_mode);
                    ms.set_selection(s);
                }
            }
            MAction::SelectLinked if edit => {
                if let Some(m) = ms.active_mesh() {
                    let s = aiai_3d_edit::select::select_linked(m, &ms.sel).flush(m, ms.sel_mode);
                    ms.set_selection(s);
                }
            }
            MAction::SelectMore if edit => {
                if let Some(m) = ms.active_mesh() {
                    let s = aiai_3d_edit::select::grow_faces(m, &ms.sel.flush(m, SelectMode::Face)).flush(m, ms.sel_mode);
                    ms.set_selection(s);
                }
            }
            MAction::SelectLess if edit => {
                if let Some(m) = ms.active_mesh() {
                    let s = aiai_3d_edit::select::shrink_faces(m, &ms.sel.flush(m, SelectMode::Face)).flush(m, ms.sel_mode);
                    ms.set_selection(s);
                }
            }
            MAction::Delete => delete_selection(env, false),
            MAction::Dissolve if edit => delete_selection(env, true),
            MAction::Merge if edit => run(env, OpKind::MergeCenter),
            MAction::MergeNear => merge_near(env),
            MAction::FillHole if edit => run(env, OpKind::FillHole),
            MAction::Bisect if edit => run(env, OpKind::Bisect),
            MAction::Triangulate if edit => run(env, OpKind::Triangulate),
            MAction::RecalcNormals if edit => run(env, OpKind::RecalcOutside),
            MAction::Hide if edit => run(env, OpKind::HideSelected),
            MAction::Reveal if edit => run(env, OpKind::RevealAll),
            MAction::Duplicate if !edit => duplicate_active(env, mouse),
            MAction::AddMenu => env.ed.modeling.add_menu = Some(to_screen(rect, mouse)),
            MAction::Frame => frame_selection(env),
            MAction::FrameAll => frame_all(env),
            MAction::ViewFront => ms.cam.set_preset(ViewPreset::Front),
            MAction::ViewRight => ms.cam.set_preset(ViewPreset::Right),
            MAction::ViewTop => ms.cam.set_preset(ViewPreset::Top),
            MAction::ViewOrtho => ms.cam.orthographic = !ms.cam.orthographic,
            MAction::MaximizeView => ms.views.toggle_maximize(&mut ms.cam),
            MAction::QuadView => {
                let l = if ms.views.layout == multiview::ViewLayout::Quad { multiview::ViewLayout::Single } else { multiview::ViewLayout::Quad };
                ms.views.set_layout(&mut ms.cam, l);
            }
            MAction::Proportional if edit => ms.proportional.enabled = !ms.proportional.enabled,
            MAction::Xray => ms.xray = !ms.xray,
            MAction::ShadingToggle => {
                ms.shade = match ms.shade {
                    ViewShade::Toon => ViewShade::Solid,
                    ViewShade::Solid => ViewShade::Wire,
                    _ => ViewShade::Toon,
                }
            }
            _ => {}
        }
    }
}

/// Roda um operador e avisa o erro.
pub(crate) fn run(env: &mut Env, kind: OpKind) {
    let p = env.ed.modeling.op_params.clone();
    if let Err(e) = env.ed.modeling.run_op(kind, p) {
        env.ed.notify(e);
    }
}

// ========================================================================================================= seleção

fn click_select(env: &mut Env, size: Vec2, mouse: Vec2, extend: bool) {
    let ms = &mut env.ed.modeling;
    let cam = ms.cam;
    match ms.mode {
        EditMode::Object | EditMode::Sculpt => {
            let (o, d) = cam.ray_from_pixel(mouse, size);
            let mut best: Option<(usize, f32)> = None;
            for (i, obj) in ms.doc.objects.iter().enumerate().filter(|(_, o)| o.visible) {
                let inv = mat_of(obj).inverse();
                let lo = inv.transform_point3(o);
                let ld = inv.transform_vector3(d);
                let eval = obj.evaluated().unwrap_or_else(|_| obj.mesh.clone());
                if let Some((_, t)) = picking::raycast_mesh(&eval, lo, ld) {
                    if best.is_none_or(|(_, bt)| t < bt) {
                        best = Some((i, t));
                    }
                }
            }
            let next = best.map(|(i, _)| i).or(if extend { ms.active } else { None });
            if next != ms.active {
                ms.active = next;
                ms.sel.clear();
                ms.touch();
            }
        }
        EditMode::Edit => {
            let (xray, sel_mode, cur) = (ms.xray, ms.sel_mode, ms.sel.clone());
            let Some((topo, w)) = world_topo(ms) else { return };
            let ctx = picking::PickContext { mesh: w, camera: &cam, size, xray };
            let hit = picking::pick(&ctx, topo, sel_mode, mouse);
            let sel = picking::apply_click(w, &cur, sel_mode, hit, extend);
            ms.set_selection(sel);
        }
    }
}

enum Region {
    Box(Vec2, Vec2),
    Lasso(Vec<Vec2>),
}

fn apply_region_select(env: &mut Env, region: Region, size: Vec2, extend: bool, subtract: bool) {
    let ms = &mut env.ed.modeling;
    if ms.mode != EditMode::Edit {
        return;
    }
    let cam = ms.cam;
    let (xray, sel_mode, cur) = (ms.xray, ms.sel_mode, ms.sel.clone());
    let Some((topo, w)) = world_topo(ms) else { return };
    let ctx = picking::PickContext { mesh: w, camera: &cam, size, xray };
    let boxed = match region {
        Region::Box(lo, hi) => picking::box_select(&ctx, topo, lo, hi, sel_mode),
        Region::Lasso(poly) => picking::lasso_select(&ctx, topo, &poly, sel_mode),
    };
    let sel = if subtract {
        let mut s = cur;
        for v in &boxed.verts {
            s.verts.remove(v);
        }
        for e in &boxed.edges {
            s.edges.remove(e);
        }
        for f in &boxed.faces {
            s.faces.remove(f);
        }
        s.flush(w, sel_mode)
    } else if extend {
        let mut s = cur;
        s.verts.extend(boxed.verts.iter().copied());
        s.edges.extend(boxed.edges.iter().copied());
        s.faces.extend(boxed.faces.iter().copied());
        s
    } else {
        boxed
    };
    ms.set_selection(sel);
}

/// Vértices da seleção (modo Edição).
fn selected_verts(ms: &ModelingState) -> Vec<u32> {
    match ms.active_mesh() {
        Some(m) => doc::selection_verts(m, &ms.sel, ms.sel_mode),
        None => Vec::new(),
    }
}

fn selected_edges(ms: &ModelingState) -> Vec<EdgeKey> {
    let Some(m) = ms.active_mesh() else { return Vec::new() };
    let nv = m.vert_count() as u32;
    if ms.sel.verts.iter().any(|v| *v >= nv) || ms.sel.edges.iter().any(|(a, b)| *a >= nv || *b >= nv) || ms.sel.faces.iter().any(|f| *f as usize >= m.face_count()) {
        return Vec::new();
    }
    let s = ms.sel.flush(m, ms.sel_mode);
    if !s.edges.is_empty() {
        return s.edges.into_iter().collect();
    }
    doc::all_edges(m).into_iter().filter(|(a, b)| ms.sel.verts.contains(a) && ms.sel.verts.contains(b)).collect()
}

// ======================================================================================================== operações

/// Começa G/R/S (com edição proporcional no modo Edição). `false` = nada para transformar.
fn start_transform(env: &mut Env, kind: TransformKind, mouse: Vec2, from_tool: bool) -> bool {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else {
        env.ed.notify(tr("mdl.need_object"));
        return false;
    };
    let o = &ms.doc.objects[a];
    let model = mat_of(o);
    let mut weights: Option<Vec<f32>> = None;
    let (pivot, originals) = match ms.mode {
        EditMode::Edit => {
            let verts = selected_verts(ms);
            if verts.is_empty() {
                env.ed.notify(tr("mdl.need_selection"));
                return false;
            }
            let mut originals: Vec<(u32, Vec3)> = verts.iter().map(|v| (*v, model.transform_point3(o.mesh.co(*v)))).collect();
            let pivot = originals.iter().map(|(_, p)| *p).sum::<Vec3>() / originals.len() as f32;
            if ms.proportional.enabled {
                let s = o.transform.scale;
                let avg = ((s[0].abs() + s[1].abs() + s[2].abs()) / 3.0).max(1e-4);
                let w = aiai_3d_edit::ops::transform::proportional_weights(&o.mesh, &verts, ms.proportional.radius / avg, ms.proportional.falloff, ms.proportional.connected);
                originals = (0..o.mesh.vert_count() as u32).filter(|v| w.get(*v as usize).copied().unwrap_or(0.0) > 1e-4).map(|v| (v, model.transform_point3(o.mesh.co(v)))).collect();
                weights = Some(originals.iter().map(|(v, _)| w[*v as usize]).collect());
            }
            (pivot, originals)
        }
        EditMode::Object | EditMode::Sculpt => {
            let p = Vec3::from(o.transform.pos);
            (p, vec![(0u32, p)])
        }
    };
    let mut t = ModalTransform::new(kind, mouse, pivot, originals);
    t.weights = weights;
    let label = match kind {
        TransformKind::Grab => "mdl.op.grab",
        TransformKind::Rotate => "mdl.op.rotate",
        TransformKind::Scale => "mdl.op.scale",
    };
    ms.modal = Some(Modal { op: ModalOp::Transform { t, object: a, model }, kind: None, label, before: ms.doc.clone(), before_sel: ms.sel.clone(), base: None, start: mouse, numeric: String::new(), from_tool, params: ms.op_params.clone() });
    true
}

/// Extrude (E): extruda a região (faces; ou arestas/vértices) e já move ao longo da normal.
fn start_extrude(env: &mut Env, mouse: Vec2, from_tool: bool) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else {
        env.ed.notify(tr("mdl.need_edit"));
        return;
    };
    let (before, before_sel) = (ms.doc.clone(), ms.sel.clone());
    let sel = ms.sel.flush(&ms.doc.objects[a].mesh, ms.sel_mode);
    if sel.is_empty() {
        env.ed.notify(tr("mdl.need_selection"));
        return;
    }
    let mut mesh = ms.doc.objects[a].mesh.clone();
    let p = OpParams { distance: 0.0, ..ms.op_params.clone() };
    let out = match catalog::apply_kind(OpKind::Extrude, &p, &mut mesh, &sel) {
        Ok(o) => o,
        Err(e) => {
            env.ed.notify(format!("{}: {e}", tr("mdl.opk.extrude")));
            return;
        }
    };
    let o = &ms.doc.objects[a];
    let model = mat_of(o);
    let rot = Mat4::from_quat(doc::object_rotation(o));
    let n: Vec3 = out.selection.faces.iter().filter(|f| (**f as usize) < mesh.faces.len()).map(|f| mesh.face_normal(*f)).sum();
    ms.doc.objects[a].mesh = mesh;
    let new_sel = out.selection.flush(&ms.doc.objects[a].mesh, ms.sel_mode);
    ms.sel = if new_sel.is_empty() { out.selection.clone() } else { new_sel };
    ms.touch();
    let verts: Vec<u32> = doc::selection_verts(&ms.doc.objects[a].mesh, &ms.sel, ms.sel_mode);
    if verts.is_empty() {
        ms.doc = before;
        ms.sel = before_sel;
        return;
    }
    let mo = &ms.doc.objects[a].mesh;
    let originals: Vec<(u32, Vec3)> = verts.iter().map(|v| (*v, model.transform_point3(mo.co(*v)))).collect();
    let pivot = originals.iter().map(|(_, p)| *p).sum::<Vec3>() / originals.len() as f32;
    let mut t = ModalTransform::new(TransformKind::Grab, mouse, pivot, originals);
    if n.length_squared() > 1e-8 {
        t.constraint = Constraint::Direction(rot.transform_vector3(n).normalize());
    }
    ms.modal = Some(Modal { op: ModalOp::Transform { t, object: a, model }, kind: Some(OpKind::Extrude), label: "mdl.opk.extrude", before, before_sel, base: Some(ms.doc.clone()), start: mouse, numeric: String::new(), from_tool, params: p });
}

fn pivot_px_of(ms: &ModelingState, size: Vec2, verts: &[u32], a: usize) -> Option<Vec2> {
    let o = ms.doc.objects.get(a)?;
    let m = mat_of(o);
    let n = o.mesh.vert_count() as u32;
    let pts: Vec<Vec3> = verts.iter().filter(|v| **v < n).map(|v| m.transform_point3(o.mesh.co(*v))).collect();
    if pts.is_empty() {
        return None;
    }
    let c: Vec3 = pts.iter().sum::<Vec3>() / pts.len() as f32;
    ms.cam.project(c, size).map(|(p, _)| p)
}

fn start_inset(env: &mut Env, size: Vec2, mouse: Vec2, from_tool: bool) -> bool {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else {
        env.ed.notify(tr("mdl.need_edit"));
        return false;
    };
    let mesh = &ms.doc.objects[a].mesh;
    let faces: Vec<u32> = ms.sel.flush(mesh, ms.sel_mode).faces.into_iter().collect();
    if faces.is_empty() {
        env.ed.notify(tr("mdl.need_faces"));
        return false;
    }
    let verts = doc::selection_verts(mesh, &Selection::from_faces(mesh, faces.iter().copied()), SelectMode::Face);
    let pivot_px = pivot_px_of(ms, size, &verts, a).unwrap_or(size * 0.5);
    ms.modal = Some(Modal { op: ModalOp::Inset { faces, pivot_px }, kind: Some(OpKind::Inset), label: "mdl.opk.inset", before: ms.doc.clone(), before_sel: ms.sel.clone(), base: None, start: mouse, numeric: String::new(), from_tool, params: ms.op_params.clone() });
    true
}

fn start_bevel(env: &mut Env, size: Vec2, mouse: Vec2, from_tool: bool) -> bool {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else {
        env.ed.notify(tr("mdl.need_edit"));
        return false;
    };
    let edges = selected_edges(ms);
    if edges.is_empty() {
        env.ed.notify(tr("mdl.need_edges"));
        return false;
    }
    let verts: Vec<u32> = edges.iter().flat_map(|(x, y)| [*x, *y]).collect();
    let pivot_px = pivot_px_of(ms, size, &verts, a).unwrap_or(size * 0.5);
    ms.modal = Some(Modal { op: ModalOp::Bevel { edges, pivot_px }, kind: Some(OpKind::Bevel), label: "mdl.opk.bevel", before: ms.doc.clone(), before_sel: ms.sel.clone(), base: None, start: mouse, numeric: String::new(), from_tool, params: ms.op_params.clone() });
    true
}

/// Loop cut na aresta sob o cursor (ou na escolhida): o anel aparece já, o mouse desliza, a roda muda os cortes.
fn start_loop_cut(env: &mut Env, size: Vec2, mouse: Vec2, from_tool: bool) -> bool {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else {
        env.ed.notify(tr("mdl.need_edit"));
        return false;
    };
    let cam = ms.cam;
    let picked = world_topo(ms).and_then(|(topo, w)| {
        let ctx = picking::PickContext { mesh: w, camera: &cam, size, xray: true };
        picking::pick_edge(&ctx, topo, mouse, 14.0)
    });
    let Some(edge) = picked.or_else(|| selected_edges(ms).first().copied()) else {
        env.ed.notify(tr("mdl.need_edge_hover"));
        return false;
    };
    // direção do deslizar na tela: perpendicular à aresta cortada (o anel anda ao longo dela)
    let o = &ms.doc.objects[a];
    let m = mat_of(o);
    let nv = o.mesh.vert_count() as u32;
    if edge.0 >= nv || edge.1 >= nv {
        env.ed.notify(tr("mdl.need_edge_hover"));
        return false;
    }
    let (pa, pb) = (cam.project(m.transform_point3(o.mesh.co(edge.0)), size), cam.project(m.transform_point3(o.mesh.co(edge.1)), size));
    let screen_dir = match (pa, pb) {
        (Some((pa, _)), Some((pb, _))) => (pb - pa) * 0.5,
        _ => Vec2::new(100.0, 0.0),
    };
    let before = ms.doc.clone();
    let before_sel = ms.sel.clone();
    let mut params = ms.op_params.clone();
    params.slide = 0.0;
    ms.modal = Some(Modal { op: ModalOp::LoopCut { edge, screen_dir }, kind: Some(OpKind::LoopCut), label: "mdl.opk.loop_cut", before, before_sel, base: None, start: mouse, numeric: String::new(), from_tool, params });
    update_modal(env, mouse, size, false);
    if env.ed.modeling.modal.is_none() {
        return false;
    }
    true
}

/// Deslizar aresta/vértice (Shift+V; Shift+G no perfil Blender).
fn start_slide(env: &mut Env, size: Vec2, mouse: Vec2) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else { return };
    let verts = selected_verts(ms);
    let o = &ms.doc.objects[a];
    let m = mat_of(o);
    let inv = m.inverse();
    let dir_local = inv.transform_vector3(ms.cam.right());
    let rails = match slide::slide_rails(&o.mesh, &verts, dir_local) {
        Ok(r) => r,
        Err(e) => {
            env.ed.notify(e.to_string());
            return;
        }
    };
    let screen_dir = rails
        .iter()
        .find_map(|r| {
            let p = r.plus?;
            let (Some((a, _)), Some((b, _))) = (ms.cam.project(m.transform_point3(r.origin), size), ms.cam.project(m.transform_point3(p), size)) else { return None };
            Some(b - a)
        })
        .unwrap_or(Vec2::new(100.0, 0.0));
    let mut params = ms.op_params.clone();
    params.slide = 0.0;
    ms.modal = Some(Modal { op: ModalOp::Slide { rails, screen_dir, object: a }, kind: None, label: "mdl.op.slide", before: ms.doc.clone(), before_sel: ms.sel.clone(), base: None, start: mouse, numeric: String::new(), from_tool: false, params });
}

fn snap(v: f32, step: f32, on: bool) -> f32 {
    if on && step > 0.0 { (v / step).round() * step } else { v }
}

fn update_modal(env: &mut Env, mouse: Vec2, size: Vec2, ctrl_down: bool) {
    // encaixe: o botão do cabeçalho liga; Ctrl inverte durante a operação
    let snap_on = env.ed.modeling.snap ^ ctrl_down;
    let ms = &mut env.ed.modeling;
    let cam = ms.cam;
    let Some(md) = ms.modal.as_mut() else { return };
    let num: Option<f32> = md.numeric.parse::<f32>().ok();
    let ctx = ModalContext { camera: cam, viewport: size };
    // metros por pixel na profundidade do alvo da câmera
    let wpp = |_p_px: Vec2| -> f32 { 2.0 * cam.distance * (cam.fov_y * 0.5).tan() / size.y.max(1.0) };
    let mut err: Option<String> = None;
    match &mut md.op {
        ModalOp::Transform { t, object, model } => {
            t.snap_step = if snap_on { if t.kind == TransformKind::Grab { 0.1 } else { 1.0 } } else { 0.0 };
            let positions = t.apply(mouse, &ctx);
            let (idx, model) = (*object, *model);
            let mut doc = md.base.clone().unwrap_or_else(|| md.before.clone());
            match ms.mode {
                EditMode::Edit => {
                    let inv = model.inverse();
                    if let Some(o) = doc.objects.get_mut(idx) {
                        for (v, world) in positions {
                            if let Some(vert) = o.mesh.verts.get_mut(v as usize) {
                                vert.co = inv.transform_point3(world);
                            }
                        }
                    }
                }
                EditMode::Object | EditMode::Sculpt => {
                    if let (Some(o), Some(b)) = (doc.objects.get_mut(idx), md.before.objects.get(idx)) {
                        match t.kind {
                            TransformKind::Grab => {
                                if let Some((_, p)) = positions.first() {
                                    o.transform.pos = p.to_array();
                                }
                            }
                            TransformKind::Rotate => {
                                let angle = t.rotation_angle(mouse, &ctx);
                                let axis = match t.constraint {
                                    Constraint::Axis(a) | Constraint::Plane(a) => a.vector(),
                                    Constraint::Direction(d) => d.normalize_or_zero(),
                                    Constraint::None => -cam.forward(),
                                };
                                let q = Quat::from_axis_angle(axis.normalize_or(Vec3::Y), angle) * doc::object_rotation(b);
                                o.transform.rot_deg = aiai_3d_model::camera::rotation_deg(q);
                            }
                            TransformKind::Scale => {
                                let f = t.scale_factor(mouse, &ctx);
                                let s = match t.constraint {
                                    Constraint::Axis(a) => {
                                        let mut v = Vec3::ONE;
                                        v[a as usize] = f;
                                        v
                                    }
                                    Constraint::Plane(a) => {
                                        let mut v = Vec3::splat(f);
                                        v[a as usize] = 1.0;
                                        v
                                    }
                                    _ => Vec3::splat(f),
                                };
                                let ns = Vec3::from(b.transform.scale) * s;
                                if ns.abs().min_element() >= 1e-4 {
                                    o.transform.scale = ns.to_array();
                                }
                            }
                        }
                    }
                }
            }
            if md.kind == Some(OpKind::Extrude) {
                // distância ao longo da normal, no espaço do objeto (o painel de ajuste refaz com ela)
                let tr_ = t.translation(mouse, &ctx);
                let d = match t.constraint {
                    Constraint::Direction(n) => tr_.dot(n),
                    _ => tr_.length(),
                };
                let sc = md.before.objects.get(idx).map(|o| (o.transform.scale[0].abs() + o.transform.scale[1].abs() + o.transform.scale[2].abs()) / 3.0).unwrap_or(1.0);
                md.params.distance = d / sc.max(1e-4);
            }
            ms.doc = doc;
        }
        ModalOp::Inset { faces, pivot_px } => {
            let r0 = (md.start - *pivot_px).length();
            let r1 = (mouse - *pivot_px).length();
            let mut v = num.unwrap_or(((r1 - r0) * wpp(*pivot_px)).abs());
            v = snap(v, 0.05, snap_on && num.is_none()).max(1e-4);
            let a = ms.active.unwrap_or(0);
            let k = mat_of(&md.before.objects[a]).inverse();
            let kk = glam::Mat3::from_mat4(k).col(0).length().max(1e-6);
            md.params.thickness = v * kk;
            let mut doc = md.before.clone();
            let sel = Selection::from_faces(&doc.objects[a].mesh, faces.iter().copied());
            match catalog::apply_kind(OpKind::Inset, &md.params, &mut doc.objects[a].mesh, &sel) {
                Ok(out) => {
                    ms.sel = out.selection.flush(&doc.objects[a].mesh, ms.sel_mode);
                    ms.doc = doc;
                }
                Err(e) => err = Some(e.to_string()),
            }
        }
        ModalOp::Bevel { edges, pivot_px } => {
            let r0 = (md.start - *pivot_px).length();
            let r1 = (mouse - *pivot_px).length();
            let mut v = num.unwrap_or(((r1 - r0) * wpp(*pivot_px)).abs());
            v = snap(v, 0.05, snap_on && num.is_none()).max(1e-4);
            let a = ms.active.unwrap_or(0);
            let kk = glam::Mat3::from_mat4(mat_of(&md.before.objects[a]).inverse()).col(0).length().max(1e-6);
            md.params.width = v * kk;
            let mut doc = md.before.clone();
            let sel = Selection::from_edges(edges.iter().copied());
            match catalog::apply_kind(OpKind::Bevel, &md.params, &mut doc.objects[a].mesh, &sel) {
                Ok(_) => {
                    ms.sel = Selection::default();
                    ms.doc = doc;
                }
                Err(e) => err = Some(e.to_string()),
            }
        }
        ModalOp::LoopCut { edge, screen_dir } => {
            let l2 = screen_dir.length_squared().max(1.0);
            let f = num.unwrap_or(((mouse - md.start).dot(*screen_dir) / l2).clamp(-0.95, 0.95));
            md.params.slide = snap(f, 0.1, snap_on && num.is_none()).clamp(-0.95, 0.95);
            let a = ms.active.unwrap_or(0);
            let mut doc = md.before.clone();
            let sel = Selection::from_edges([*edge]);
            match catalog::apply_kind(OpKind::LoopCut, &md.params, &mut doc.objects[a].mesh, &sel) {
                Ok(out) => {
                    ms.sel_mode = SelectMode::Edge;
                    ms.sel = Selection::from_edges(out.selection.edges.iter().copied());
                    ms.doc = doc;
                }
                Err(e) => err = Some(e.to_string()),
            }
        }
        ModalOp::Slide { rails, screen_dir, object } => {
            let l2 = screen_dir.length_squared().max(1.0);
            let f = num.unwrap_or(((mouse - md.start).dot(*screen_dir) / l2).clamp(-1.0, 1.0));
            md.params.slide = snap(f, 0.1, snap_on && num.is_none());
            let mut doc = md.before.clone();
            if let Err(e) = slide::apply_slide(&mut doc.objects[*object].mesh, rails, md.params.slide) {
                err = Some(e.to_string());
            } else {
                ms.doc = doc;
            }
        }
    }
    ms.touch();
    if let Some(e) = err {
        // operador recusou: a prévia volta ao estado de antes (a mensagem aparece se a operação não começar)
        if matches!(ms.modal.as_ref().map(|m| &m.op), Some(ModalOp::LoopCut { .. })) && ms.doc == ms.modal.as_ref().map(|m| m.before.clone()).unwrap_or_default() {
            ms.cancel_modal();
            env.ed.notify(e);
        }
    }
}

fn confirm_modal(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    let Some(md) = ms.modal.take() else { return };
    let label = tr(md.label);
    ms.commit(label, md.before.clone());
    if let Some(kind) = md.kind {
        // o painel "Ajustar última operação" refaz com os parâmetros desta operação
        ms.op_params = OpParams { slide: 0.0, distance: 0.0, ..md.params.clone() };
        let params = if kind == OpKind::Extrude { OpParams { distance: md.params.distance, ..md.params.clone() } } else { md.params.clone() };
        ms.last_op = Some(LastOp { kind, params, before: md.before, before_sel: md.before_sel, history_len: ms.history.len() });
    } else {
        ms.last_op = None;
    }
}

/// Funde os vértices que ficaram no mesmo lugar (Shift+M).
pub(crate) fn merge_near(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else {
        env.ed.notify(tr("mdl.need_object"));
        return;
    };
    let before = ms.doc.objects[a].mesh.vert_count();
    match ms.edit_labeled(tr("mdl.merged"), |d| doc::merge_by_distance(&mut d.objects[a].mesh, None, 1e-4)) {
        Ok(_) => {
            let n = before - ms.doc.objects[a].mesh.vert_count();
            ms.sel.clear();
            ms.touch();
            env.ed.notify(format!("{}: {n}", tr("mdl.merged")));
        }
        Err(e) => env.ed.notify(e),
    }
}

fn delete_selection(env: &mut Env, dissolve: bool) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else { return };
    match ms.mode {
        EditMode::Object | EditMode::Sculpt => {
            let r = ms.edit_labeled(tr("mdl.delete_object"), |d| {
                d.objects.remove(a);
                Ok(())
            });
            if r.is_ok() {
                ms.active = None;
                ms.fix();
                ms.touch();
            }
        }
        EditMode::Edit => {
            if ms.sel.is_empty() {
                env.ed.notify(tr("mdl.need_selection"));
                return;
            }
            let kind = match (dissolve, ms.sel_mode) {
                (false, SelectMode::Vertex) => OpKind::DeleteVerts,
                (false, SelectMode::Edge) => OpKind::DeleteEdges,
                (false, SelectMode::Face) => OpKind::DeleteFaces,
                (true, SelectMode::Vertex) => OpKind::DissolveVerts,
                (true, SelectMode::Edge) => OpKind::DissolveEdges,
                (true, SelectMode::Face) => OpKind::DissolveFaces,
            };
            run(env, kind);
            env.ed.modeling.sel.clear();
        }
    }
}

fn duplicate_active(env: &mut Env, mouse: Vec2) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else { return };
    let r = ms.edit_labeled(tr("mdl.duplicate"), |d| {
        if d.objects.len() >= doc::MAX_OBJECTS {
            return Err(format!("limite de {} objetos", doc::MAX_OBJECTS));
        }
        let mut o = d.objects[a].clone();
        o.name = d.unique_name(&o.name);
        d.objects.push(o);
        Ok(d.objects.len() - 1)
    });
    match r {
        Ok(i) => {
            ms.active = Some(i);
            ms.touch();
            start_transform(env, TransformKind::Grab, mouse, false);
        }
        Err(e) => env.ed.notify(e),
    }
}

fn bounds_of(ms: &ModelingState, only_active: bool) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let mut any = false;
    for (i, o) in ms.doc.objects.iter().enumerate() {
        if only_active && Some(i) != ms.active {
            continue;
        }
        let m = mat_of(o);
        let verts: Vec<u32> = if only_active && ms.mode == EditMode::Edit && !ms.sel.is_empty() { selected_verts(ms) } else { (0..o.mesh.vert_count() as u32).collect() };
        for v in verts {
            let p = m.transform_point3(o.mesh.co(v));
            lo = lo.min(p);
            hi = hi.max(p);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

pub(crate) fn frame_selection(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    if let Some((lo, hi)) = bounds_of(ms, ms.active.is_some()) {
        ms.cam.frame_bounds(lo, hi);
    }
}

pub(crate) fn frame_all(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    match bounds_of(ms, false) {
        Some((lo, hi)) => ms.cam.frame_bounds(lo, hi),
        None => ms.cam = default_cam(),
    }
}

/// Vista pronta (botões do painel/bridge): `front`, `right`, `top`… e `ortho`/`persp`.
pub(crate) fn set_view(env: &mut Env, key: &str) -> bool {
    let cam = &mut env.ed.modeling.cam;
    match key {
        "front" => cam.set_preset(ViewPreset::Front),
        "back" => cam.set_preset(ViewPreset::Back),
        "right" => cam.set_preset(ViewPreset::Right),
        "left" => cam.set_preset(ViewPreset::Left),
        "top" => cam.set_preset(ViewPreset::Top),
        "bottom" => cam.set_preset(ViewPreset::Bottom),
        "ortho" => cam.orthographic = true,
        "persp" => cam.orthographic = false,
        _ => return false,
    }
    true
}

/// Estado da Modelagem para o `GET_UI_STATE` (bridge e harness): modo, seleção, câmera, sombreamento, atalhos e medidas
/// do viewport.
pub(crate) fn state_json(ms: &ModelingState) -> serde_json::Value {
    let r3 = |v: f32| (v * 1000.0).round() / 1000.0;
    json!({
        "objects": ms.doc.objects.len(),
        "active": ms.active,
        "mode": match ms.mode {
            EditMode::Object => "object",
            EditMode::Edit => "edit",
            EditMode::Sculpt => "sculpt",
        },
        "select_mode": format!("{:?}", ms.sel_mode).to_lowercase(),
        "selection": { "verts": ms.sel.verts.len(), "edges": ms.sel.edges.len(), "faces": ms.sel.faces.len() },
        "mesh": ms.active.and_then(|a| ms.doc.objects.get(a)).map(|o| json!({ "verts": o.mesh.vert_count(), "faces": o.mesh.face_count(), "modifiers": o.modifiers.iter().map(|m| m.key()).collect::<Vec<_>>() })),
        "camera": { "target": [r3(ms.cam.target.x), r3(ms.cam.target.y), r3(ms.cam.target.z)], "yaw_deg": r3(ms.cam.yaw.to_degrees()), "pitch_deg": r3(ms.cam.pitch.to_degrees()), "distance": r3(ms.cam.distance), "ortho": ms.cam.orthographic },
        "shade": ms.shade.key(),
        "sculpt": {
            "brush": sculpt::brush_key(ms.sculpt.active),
            "radius_px": ms.sculpt.slot(ms.sculpt.active).radius_px,
            "symmetry": ms.sculpt.symmetry.mirror,
            "dyntopo": ms.sculpt.dyntopo,
            "hover": ms.sculpt.hover.is_some(),
            "last_stroke": ms.sculpt.last_stroke.map(|l| json!({ "dabs": l.dabs, "topology_changed": l.topology_changed, "ms": r3(l.millis) })),
        },
        "views": {
            "layout": ms.views.layout.key(),
            "active": ms.views.active,
            "kinds": ms.views.visible().into_iter().map(|s| multiview::ViewKind::of(if s == ms.views.active { &ms.cam } else { &ms.views.cams[s] }).key()).collect::<Vec<_>>(),
        },
        "xray": ms.xray,
        "proportional": ms.proportional.enabled,
        "modal": ms.modal.as_ref().map(|m| m.label),
        "last_op": ms.last_op_valid().map(|l| l.kind.key()),
        "history": ms.history.labels(),
        "keys_preset": ms.keys.preset.key(),
        "modal_keys": ms.keys.modal_keys,
        "dirty": ms.dirty,
        "nav_frame_ms": r3(ms.nav_frame_ms),
        "gpu_cpu_ms": r3(ms.gpu_cpu_ms),
        "frames_drawn": ms.frames_drawn,
        "gpu_bytes": gpu::gpu_bytes(),
    })
}

// ===================================================================================================== adicionar

fn add_menu(env: &mut Env, ui: &mut Ui) {
    let Some(at) = env.ed.modeling.add_menu else { return };
    let mut close = false;
    let area = egui::Area::new(ui.id().with("mdl_add_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new(tr("mdl.add")).strong());
            for k in Primitive::ALL {
                let r = ui.button(tr(&format!("mdl.prim.{}", k.key())));
                if r.clicked() {
                    if let Err(e) = env.ed.modeling.add_primitive(k) {
                        env.ed.notify(e);
                    }
                    close = true;
                }
            }
        });
    });
    let outside = ui.ctx().input(|i| i.pointer.any_pressed()) && !area.response.rect.contains(ui.ctx().input(|i| i.pointer.interact_pos()).unwrap_or_default());
    if close || outside || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        env.ed.modeling.add_menu = None;
    }
}

// ================================================================================================= salvar e enviar

/// Grava o documento no ativo 3D do projeto (cria ou atualiza; um passo de desfazer do projeto). `None` = falhou.
/// A Modelagem tem mudanças que ainda não estão no projeto (auditoria V2-02)?
pub(crate) fn pending(ed: &crate::app::editor::Editor) -> bool {
    ed.modeling.dirty && !ed.modeling.doc.objects.is_empty()
}

/// Grava a modelagem pendente no projeto (o mesmo ativo de "Salvar no projeto"), sem pedir nada: o Salvar geral passa
/// por aqui antes de gravar o arquivo, para "Projeto salvo" valer também para o modelo.
pub(crate) fn flush_into_project(core: &mut aiai_core::CoreSession, ed: &mut crate::app::editor::Editor) -> Result<(), String> {
    if !pending(ed) {
        return Ok(());
    }
    let existing = ed.modeling.asset.clone().filter(|a| core.project.node(a).is_some());
    let doc = ed.modeling.doc.clone();
    let (cmd, id, _) = aiai_tools::modeling::save_command(&core.project, &mut core.ids, &mut core.blobs, existing.as_ref(), &doc).map_err(|e| e.to_string())?;
    let cmd = Command::Batch { commands: vec![cmd], label: Some(format!("Modelagem: salvar \"{}\"", doc.name)) };
    if ed.apply(core, cmd) {
        ed.modeling.asset = Some(id);
        ed.modeling.dirty = false;
        Ok(())
    } else {
        Err(tr("mdl.save_failed").to_owned())
    }
}

pub(crate) fn save_to_project(env: &mut Env) -> Option<NodeId> {
    let ms = &env.ed.modeling;
    if ms.doc.objects.is_empty() {
        env.ed.notify(tr("mdl.nothing_to_save"));
        return None;
    }
    let existing = ms.asset.clone().filter(|a| env.core.project.node(a).is_some());
    let doc = ms.doc.clone();
    let core = &mut *env.core;
    match aiai_tools::modeling::save_command(&core.project, &mut core.ids, &mut core.blobs, existing.as_ref(), &doc) {
        Ok((cmd, id, _info)) => {
            let cmd = Command::Batch { commands: vec![cmd], label: Some(format!("Modelagem: salvar \"{}\"", doc.name)) };
            if env.ed.apply(env.core, cmd) {
                env.ed.modeling.asset = Some(id.clone());
                env.ed.modeling.dirty = false;
                Some(id)
            } else {
                None
            }
        }
        Err(e) => {
            env.ed.notify(e.to_string());
            None
        }
    }
}

/// "Salvar na biblioteca": o ativo do projeto e, com sessão, a cópia em `Documentos\AnimeIAI\Modelos 3D` (ação LIBRARY).
pub(crate) fn save_to_library(env: &mut Env) {
    let Some(id) = save_to_project(env) else { return };
    if !env.ed.modeling.user_files {
        env.ed.notify(tr("mdl.saved_project_only"));
        return;
    }
    let name = env.ed.modeling.doc.name.clone();
    match super::actions::run(env.core, "LIBRARY", json!({ "op": "save", "from": "model", "asset": id, "name": name })) {
        Ok(v) => {
            let path = v.pointer("/item/path").and_then(|p| p.as_str()).unwrap_or("Documentos\\AnimeIAI\\Modelos 3D").to_owned();
            env.ed.notify(format!("{}: {path}", tr("mdl.saved_library")));
        }
        Err(e) => env.ed.notify(format!("{} ({e})", tr("mdl.saved_project_only"))),
    }
}

/// "Enviar ao Layout": salva no projeto se preciso e cria a camada 3D do modelo no corte ativo; abre o Layout.
pub(crate) fn send_to_layout(env: &mut Env) {
    let Some(cut) = env.ed.active_cut(env.core) else {
        env.ed.notify(tr("mdl.need_cut"));
        return;
    };
    let id = match env.ed.modeling.asset.clone().filter(|a| env.core.project.node(a).is_some() && !env.ed.modeling.dirty) {
        Some(a) => a,
        None => match save_to_project(env) {
            Some(a) => a,
            None => return,
        },
    };
    match super::actions::run(env.core, "MODEL3D", json!({ "op": "import", "asset": id, "cut": cut })) {
        Ok(v) => {
            if let Some(l) = v.get("layer").and_then(|x| x.as_str()) {
                env.ed.layer = Some(Id(l.to_string()));
                env.ed.cel_number = None;
            }
            env.ed.show_reference = true;
            env.ed.want_workspace = Some(WorkspaceId::Layout);
            env.ed.notify(tr("mdl.sent"));
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Abre um modelo editável do projeto (o documento atual fica no desfazer).
pub(crate) fn open_asset(env: &mut Env, asset: &NodeId) {
    match aiai_tools::modeling::asset_doc(&env.core.project, asset) {
        Ok(d) => {
            let ms = &mut env.ed.modeling;
            ms.cancel_modal();
            let before = std::mem::replace(&mut ms.doc, d);
            ms.history.push_owned(tr("mdl.open"), before);
            ms.asset = Some(asset.clone());
            ms.active = if ms.doc.objects.is_empty() { None } else { Some(0) };
            ms.mode = EditMode::Object;
            ms.sel.clear();
            ms.dirty = false;
            ms.last_op = None;
            ms.touch();
            frame_all(env);
        }
        Err(e) => env.ed.notify(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_op_records_history_and_redo_last_replays_with_new_params() {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Cube).unwrap();
        ms.mode = EditMode::Edit;
        let m = &ms.doc.objects[0].mesh;
        ms.sel = Selection::from_faces(m, [0u32]);
        let depth = ms.history.len();
        ms.run_op(OpKind::Inset, OpParams { thickness: 0.05, ..OpParams::default() }).unwrap();
        assert_eq!(ms.history.len(), depth + 1);
        let a = ms.doc.objects[0].mesh.content_hash();
        ms.redo_last_with(OpParams { thickness: 0.2, ..OpParams::default() }).unwrap();
        assert_eq!(ms.history.len(), depth + 1, "refazer com parâmetros novos não empilha passo");
        assert_ne!(a, ms.doc.objects[0].mesh.content_hash());
        assert!(ms.undo());
        assert_eq!(ms.doc.objects[0].mesh.face_count(), 6);
    }

    /// Regressão (pânico em `EditMesh::co`, "len 8, index 8"): a seleção apontava para vértices do extrude depois que a
    /// malha voltou a 8 vértices. Índices velhos saem da seleção e nenhum caminho da UI lê por eles.
    #[test]
    fn stale_selection_indices_never_reach_the_mesh() {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Cube).unwrap();
        ms.mode = EditMode::Edit;
        ms.sel = Selection::from_faces(&ms.doc.objects[0].mesh, [0u32]);
        ms.run_op(OpKind::Extrude, OpParams { distance: 0.3, ..OpParams::default() }).unwrap();
        assert!(ms.sel.verts.iter().any(|v| *v >= 8), "a seleção é a tampa nova (vértices 8..)");
        // a malha volta a 8 vértices por fora do viewport (painel/MCP), com a seleção velha ainda guardada
        let stale = ms.sel.clone();
        ms.doc.objects[0].mesh = aiai_3d_edit::primitives::cube(1.0);
        ms.sel = stale;
        assert!(gizmo_pivot(&ms).is_none(), "só índices velhos: sem pivô (e sem pânico)");
        ms.sel.verts.insert(0);
        assert!(gizmo_pivot(&ms).is_some(), "o pivô ignora os índices que não existem");
        assert!(selected_edges(&ms).is_empty());
        assert!(pivot_px_of(&ms, Vec2::new(800.0, 600.0), &[0, 8, 11], 0).is_some());
        let _ = bounds_of(&ms, true);
        ms.sanitize_selection();
        let n = ms.doc.objects[0].mesh.vert_count() as u32;
        assert!(ms.sel.verts.iter().all(|v| *v < n) && ms.sel.faces.iter().all(|f| *f < 6));
        // desfazer o extrude também limpa a seleção
        ms.sel = Selection::from_verts([9u32, 10]);
        assert!(ms.undo());
        assert!(ms.sel.is_empty());
    }

    #[test]
    fn failing_op_leaves_doc_and_history_alone() {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Cube).unwrap();
        ms.mode = EditMode::Edit;
        let before = ms.doc.clone();
        let depth = ms.history.len();
        assert!(ms.run_op(OpKind::Bridge, OpParams::default()).is_err());
        assert_eq!(ms.doc, before);
        assert_eq!(ms.history.len(), depth);
    }

    #[test]
    fn every_operator_has_a_label() {
        for k in OpKind::ALL {
            let key = op_label_key(k);
            assert_ne!(tr(&key), key, "{key}");
        }
        for k in aiai_3d_edit::doc::ModifierSpec::KEYS {
            let key = format!("mdl.mod.{k}");
            assert_ne!(tr(&key), key, "{key}");
        }
        for s in ViewShade::ALL {
            let key = format!("mdl.shade.{}", s.key());
            assert_ne!(tr(&key), key, "{key}");
        }
        for (_, _, label, _, _) in keys::ACTIONS {
            assert_ne!(tr(label), label, "{label}");
        }
    }
}

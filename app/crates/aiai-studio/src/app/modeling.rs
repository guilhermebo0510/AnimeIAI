//! Workspace Modelagem (S24 3D, fase 12): viewport 3D próprio (sem canvas 2D) com o documento da Modelagem
//! (`aiai_3d_edit::ModelDoc`), renderizado pelo mesmo serviço 3D da camada (renderer cel do ANIGO na GPU, com contorno:
//! o que se vê é o que vai ser a referência) e as sobreposições do modo Edição desenhadas por cima (grade, arestas,
//! vértices, faces, seleção). Modos Objeto/Edição (Tab), seleção de vértice/aresta/face (1/2/3), G/R/S com eixo (X/Y/Z),
//! encaixe (Ctrl) e valor digitado, extrude (E), inset (I), loop cut (Ctrl+R), bevel (Ctrl+B), merge (M), apagar (X) e
//! dissolver (Ctrl+X), duplicar (Shift+D), adicionar (Shift+A); órbita no botão do meio (Shift: pan, Ctrl: zoom) ou
//! Alt+esquerdo, roda = zoom; desfazer/refazer próprios (Ctrl+Z/Ctrl+Shift+Z enquanto a Modelagem está aberta).
//! "Salvar na biblioteca" grava o modelo num ativo 3D do projeto (GLB + documento editável) e, com sessão, na
//! Biblioteca (`Documentos\AnimeIAI\Modelos 3D`, pela ação `LIBRARY`); "Enviar ao Layout" cria a camada 3D no corte.

use super::commands::NodeId;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_3d_edit::doc::{self, History, ModelDoc, Primitive};
use aiai_3d_edit::{EdgeKey, EditMesh, SelectMode, Selection, edge_key};
use aiai_3d_model::camera::{CamView, OrbitCamera, layer_from_orbit, rotation_deg, rotation_quat};
use aiai_3d_model::{Model, raster};
use aiai_core::model3d::{Model3dState, RenderMode3d, Transform3d};
use aiai_core::*;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, Ui};
use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Modo do viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditMode {
    #[default]
    Object,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ModalKind {
    Grab,
    Rotate,
    Scale,
    Extrude,
    Inset,
    Bevel,
}

impl ModalKind {
    fn key(self) -> &'static str {
        match self {
            ModalKind::Grab => "mdl.op.grab",
            ModalKind::Rotate => "mdl.op.rotate",
            ModalKind::Scale => "mdl.op.scale",
            ModalKind::Extrude => "mdl.op.extrude",
            ModalKind::Inset => "mdl.op.inset",
            ModalKind::Bevel => "mdl.op.bevel",
        }
    }
}

/// Operação modal em curso (G/R/S/E/I/Ctrl+B): refeita a cada quadro a partir do documento de antes.
struct Modal {
    kind: ModalKind,
    before: ModelDoc,
    before_sel: Selection,
    axis: Option<usize>,
    start: Pos2,
    /// Pivô no mundo e na tela (pontos do viewport).
    pivot: Vec3,
    pivot_px: Vec2,
    verts: Vec<u32>,
    faces: Vec<u32>,
    edges: Vec<EdgeKey>,
    /// Extrude: normal média (mundo).
    normal: Vec3,
    numeric: String,
    /// Começou num arrasto com a ferramenta da prateleira: soltar confirma.
    from_tool: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Nav {
    Orbit,
    Pan,
    Zoom,
}

type FrameRx = crossbeam_channel::Receiver<Result<Vec<u8>, String>>;

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
    pub shade: RenderMode3d,
    pub snap: bool,
    pub user_files: bool,
    pub dirty: bool,
    modal: Option<Modal>,
    nav: Option<Nav>,
    add_menu: Option<Pos2>,
    tex: Option<TextureHandle>,
    shown_key: Option<String>,
    pending: Option<(String, (u32, u32), FrameRx)>,
    model_cache: Option<(BlobHash, Option<Arc<Model>>)>,
    hovered: bool,
    drag_from: Option<Pos2>,
    /// O clique que confirmou uma operação não seleciona nada ao soltar.
    swallow_release: bool,
    /// O viewport pode ler o teclado (nenhum outro widget com o foco).
    kb: bool,
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
            cam: OrbitCamera { target: Vec3::new(0.0, 0.5, 0.0), yaw: 35f32.to_radians(), pitch: 22f32.to_radians(), distance: 5.0, fov_y: 35f32.to_radians(), ..Default::default() },
            shade: RenderMode3d::Toon,
            snap: false,
            user_files: false,
            dirty: false,
            modal: None,
            nav: None,
            add_menu: None,
            tex: None,
            shown_key: None,
            pending: None,
            model_cache: None,
            hovered: false,
            drag_from: None,
            swallow_release: false,
            kb: true,
        }
    }
}

impl ModelingState {
    /// Guarda o "antes" no histórico se o documento mudou.
    fn commit(&mut self, before: ModelDoc) {
        if before != self.doc {
            self.history.push(before);
            self.dirty = true;
        }
    }

    /// Muda o documento com um passo de desfazer; erro do operador volta o documento e vira aviso.
    pub(crate) fn edit<R>(&mut self, f: impl FnOnce(&mut ModelDoc) -> Result<R, String>) -> Result<R, String> {
        let before = self.doc.clone();
        match f(&mut self.doc) {
            Ok(r) => {
                self.commit(before);
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
        let ok = self.history.undo(&mut self.doc);
        if ok {
            self.sel.clear();
            self.fix();
            self.dirty = true;
        }
        ok
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.cancel_modal();
        let ok = self.history.redo(&mut self.doc);
        if ok {
            self.sel.clear();
            self.fix();
            self.dirty = true;
        }
        ok
    }

    fn cancel_modal(&mut self) {
        if let Some(m) = self.modal.take() {
            self.doc = m.before;
            self.sel = m.before_sel;
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
        self.history.push(before);
        self.asset = None;
        self.active = None;
        self.sel.clear();
        self.mode = EditMode::Object;
        self.dirty = false;
    }

    /// Acrescenta uma primitiva apoiada no chão (Y = 0) e a torna ativa.
    pub(crate) fn add_primitive(&mut self, kind: Primitive) -> Result<(), String> {
        let r = self.edit(|d| {
            let i = d.add_primitive(kind, 1.0, [0.0; 3], None)?;
            let lo = d.objects[i].mesh.bounds().map(|(lo, _)| lo.y).unwrap_or(0.0);
            d.objects[i].transform.pos = [0.0, -lo, 0.0];
            Ok(i)
        })?;
        self.active = Some(r);
        self.mode = EditMode::Object;
        self.sel.clear();
        Ok(())
    }
}

fn mat_of(o: &doc::Object3d) -> Mat4 {
    o.matrix()
}

fn view_of(ms: &ModelingState, size: egui::Vec2) -> CamView {
    CamView::from_orbit(&ms.cam, Vec2::new(size.x.max(1.0), size.y.max(1.0)))
}

fn to_px(rect: Rect, p: Pos2) -> Vec2 {
    Vec2::new(p.x - rect.left(), p.y - rect.top())
}

fn to_screen(rect: Rect, p: Vec2) -> Pos2 {
    Pos2::new(rect.left() + p.x, rect.top() + p.y)
}

// ============================================================================================================ painel

/// Painel Viewport 3D (o central da Modelagem).
pub(crate) fn viewport(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let bar = Rect::from_min_size(body.min, egui::vec2(body.width(), 30.0));
    let rect = Rect::from_min_max(egui::pos2(body.left(), bar.bottom()), body.max);
    header(env, ui, bar);
    env.note("mdl:viewport", rect);
    let resp = ui.interact(rect, ui.id().with("mdl_viewport"), Sense::click_and_drag());
    env.ed.modeling.hovered = resp.hovered() || resp.dragged();
    // o viewport fica com o foco do teclado com o ponteiro em cima (o Tab é dele: Objeto/Edição, não a navegação dos
    // botões), menos quando um campo de texto está sendo editado
    let focused = ui.ctx().memory(|m| m.focused());
    if (resp.hovered() && focused.is_none()) || resp.is_pointer_button_down_on() {
        resp.request_focus();
    }
    if resp.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
    }
    // teclas do viewport só sem outro widget com o foco (um campo de texto sendo editado fica com as teclas)
    env.ed.modeling.kb = focused.is_none() || focused == Some(resp.id) || resp.has_focus();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, viewport_bg(&t));
    request_frame(env, ui.ctx(), rect);
    let ms = &env.ed.modeling;
    if let Some(tex) = &ms.tex {
        if ms.shown_key.is_some() {
            painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
    }
    input(env, ui, rect, &resp);
    overlay(env, &painter, rect);
    add_menu(env, ui);
}

fn viewport_bg(t: &crate::theme::Tokens) -> Color32 {
    // um pouco mais escuro que o painel: o modelo claro com contorno escuro aparece bem nos dois temas
    let b = t.bg_frame;
    Color32::from_rgb(b.r().saturating_sub(6), b.g().saturating_sub(6), b.b().saturating_sub(4))
}

/// Cabeçalho do viewport: modo, seleção, sombreamento, encaixe, salvar e enviar.
fn header(env: &mut Env, ui: &mut Ui, bar: Rect) {
    let t = env.t;
    ui.painter().rect_filled(bar, 0.0, t.bg_header);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    let ms_mode = env.ed.modeling.mode;
    for (m, key) in [(EditMode::Object, "mdl.mode.object"), (EditMode::Edit, "mdl.mode.edit")] {
        let r = ui.selectable_label(ms_mode == m, tr(key)).on_hover_text(tr("mdl.mode.tip"));
        env.note(format!("mdl:mode.{}", if m == EditMode::Object { "object" } else { "edit" }), r.rect);
        if r.clicked() {
            set_mode(env, m);
        }
    }
    ui.separator();
    let edit = env.ed.modeling.mode == EditMode::Edit;
    for (s, key, name) in [(SelectMode::Vertex, "mdl.sel.vertex", "vertex"), (SelectMode::Edge, "mdl.sel.edge", "edge"), (SelectMode::Face, "mdl.sel.face", "face")] {
        let r = ui.add_enabled(edit, egui::Button::selectable(env.ed.modeling.sel_mode == s, tr(key)));
        env.note(format!("mdl:sel.{name}"), r.rect);
        if r.clicked() {
            env.ed.modeling.sel_mode = s;
            env.ed.modeling.sel.clear();
        }
    }
    ui.separator();
    for (m, key) in [(RenderMode3d::Toon, "mdl.shade.toon"), (RenderMode3d::Flat, "mdl.shade.flat"), (RenderMode3d::Wire, "mdl.shade.wire")] {
        let r = ui.selectable_label(env.ed.modeling.shade == m, tr(key));
        env.note(format!("mdl:shade.{}", m.key()), r.rect);
        if r.clicked() {
            env.ed.modeling.shade = m;
        }
    }
    ui.separator();
    let r = ui.selectable_label(env.ed.modeling.snap, tr("mdl.snap")).on_hover_text(tr("mdl.snap.tip"));
    env.note("mdl:snap", r.rect);
    if r.clicked() {
        env.ed.modeling.snap = !env.ed.modeling.snap;
    }
    // salvar e enviar ficam no painel Objetos; aqui só o aviso de não salvo
    if env.ed.modeling.dirty {
        ui.separator();
        ui.label(egui::RichText::new(tr("mdl.unsaved")).size(11.0).color(t.text_dim));
    }
}

fn set_mode(env: &mut Env, m: EditMode) {
    let ms = &mut env.ed.modeling;
    ms.cancel_modal();
    if m == EditMode::Edit && ms.active.is_none() {
        env.ed.notify(tr("mdl.need_object"));
        return;
    }
    ms.mode = m;
    ms.sel.clear();
}

// ========================================================================================================== render

/// Pede o quadro do modelo ao serviço 3D (GPU no device do egui; sem GPU, o rasterizador CPU numa thread) quando o
/// documento, a câmera, o tamanho ou o sombreamento mudam; recolhe o que ficou pronto.
fn request_frame(env: &mut Env, ctx: &egui::Context, rect: Rect) {
    let ms = &mut env.ed.modeling;
    if let Some((key, size, rx)) = &ms.pending {
        if let Ok(r) = rx.try_recv() {
            let (key, size) = (key.clone(), *size);
            ms.pending = None;
            match r {
                Ok(rgba) if rgba.len() == (size.0 * size.1 * 4) as usize => {
                    let img = egui::ColorImage { size: [size.0 as usize, size.1 as usize], source_size: egui::vec2(size.0 as f32, size.1 as f32), pixels: rgba.chunks_exact(4).map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])).collect() };
                    match &mut ms.tex {
                        Some(t) => t.set(img, TextureOptions::LINEAR),
                        None => ms.tex = Some(ctx.load_texture("mdl-viewport", img, TextureOptions::LINEAR)),
                    }
                    ms.shown_key = Some(key);
                }
                Ok(_) => {}
                Err(e) => env.ed.notify(e),
            }
        }
    }
    let ms = &mut env.ed.modeling;
    let hash = ms.doc.content_hash();
    if ms.model_cache.as_ref().is_none_or(|(h, _)| *h != hash) {
        ms.model_cache = Some((hash.clone(), ms.doc.to_model().ok().map(Arc::new)));
    }
    let Some(model) = ms.model_cache.as_ref().and_then(|(_, m)| m.clone()) else {
        ms.shown_key = None;
        return;
    };
    let ppp = ctx.pixels_per_point();
    let (mut w, mut h) = ((rect.width() * ppp).round().max(16.0) as u32, (rect.height() * ppp).round().max(16.0) as u32);
    // teto de 2 MP (o viewport cheio em 4K não precisa de mais para referência)
    let k = ((w as f32 * h as f32) / 2_000_000.0).sqrt();
    if k > 1.0 {
        w = (w as f32 / k) as u32;
        h = (h as f32 / k) as u32;
    }
    let state = Model3dState {
        model_hash: hash.clone(),
        transform: Transform3d::default(),
        camera: layer_from_orbit(&ms.cam),
        mode: ms.shade,
        outline_px: 1.6 * ppp,
        outline_color: [0.11, 0.12, 0.15],
        color: None,
        pose: None,
    };
    let key = format!("{}|{}|{w}x{h}", hash.0, serde_json::to_string(&state).unwrap_or_default());
    if ms.shown_key.as_ref() == Some(&key) || ms.pending.as_ref().is_some_and(|(k, _, _)| *k == key) {
        return;
    }
    if ms.pending.is_some() {
        // um pedido por vez; o mais novo sai quando este voltar
        ctx.request_repaint_after(Duration::from_millis(16));
        return;
    }
    let rx = match super::model3d::preview_request(hash, Arc::clone(&model), state.clone(), w, h) {
        Some(rx) => rx,
        None => {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let c2 = ctx.clone();
            let _ = std::thread::Builder::new().name("aiai-mdl-cpu".into()).spawn(move || {
                let px = raster::render_cpu(&model, &state, w, h);
                let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let _ = tx.send(Ok(px.iter().flat_map(|p| [b(p[0]), b(p[1]), b(p[2]), b(p[3])]).collect()));
                c2.request_repaint();
            });
            rx
        }
    };
    ms.pending = Some((key, (w, h), rx));
    ctx.request_repaint_after(Duration::from_millis(16));
}

// ========================================================================================================= overlay

const ORANGE: Color32 = Color32::from_rgb(255, 160, 40);
const AXIS: [Color32; 3] = [Color32::from_rgb(220, 70, 70), Color32::from_rgb(90, 190, 80), Color32::from_rgb(70, 120, 230)];

fn overlay(env: &mut Env, painter: &egui::Painter, rect: Rect) {
    let t = env.t;
    let ms = &env.ed.modeling;
    let view = view_of(ms, rect.size());
    let scr = |p: Vec3| view.project(p).map(|(q, _)| to_screen(rect, q));
    let seg = |a: Vec3, b: Vec3, s: Stroke| {
        if let (Some(pa), Some(pb)) = (scr(a), scr(b)) {
            painter.line_segment([pa, pb], s);
        }
    };
    // grade do chão (XZ) e eixos
    let dark = t.bg_panel.r() < 128;
    let grid = if dark { Color32::from_white_alpha(22) } else { Color32::from_black_alpha(26) };
    for i in -10..=10 {
        let v = i as f32 * 0.5;
        if i != 0 {
            seg(Vec3::new(v, 0.0, -5.0), Vec3::new(v, 0.0, 5.0), Stroke::new(1.0, grid));
            seg(Vec3::new(-5.0, 0.0, v), Vec3::new(5.0, 0.0, v), Stroke::new(1.0, grid));
        }
    }
    seg(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), Stroke::new(1.4, AXIS[0].gamma_multiply(0.8)));
    seg(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0), Stroke::new(1.4, AXIS[2].gamma_multiply(0.8)));
    if ms.doc.objects.is_empty() {
        let g = painter.layout(tr("mdl.empty").to_owned(), egui::FontId::proportional(13.0), t.text_dim, rect.width() * 0.6);
        painter.galley(rect.center() - g.size() * 0.5, g, t.text_dim);
    }
    if let Some(a) = ms.active.and_then(|a| ms.doc.objects.get(a).map(|o| (a, o))) {
        let (_, o) = a;
        let m = mat_of(o);
        let mesh = &o.mesh;
        let topo = aiai_3d_edit::Topology::build(mesh);
        let world: Vec<Vec3> = (0..mesh.vert_count() as u32).map(|v| m.transform_point3(mesh.co(v))).collect();
        let nm = Mat3::from_mat4(m).inverse().transpose();
        let front: Vec<bool> = (0..mesh.face_count() as u32)
            .map(|f| {
                let n = (nm * mesh.face_normal(f)).normalize_or_zero();
                let c = m.transform_point3(mesh.face_center(f));
                n.dot(view.eye - c) > 0.0
            })
            .collect();
        match ms.mode {
            EditMode::Object => {
                for e in &topo.edges {
                    seg(world[e.verts.0 as usize], world[e.verts.1 as usize], Stroke::new(1.4, ORANGE.gamma_multiply(0.85)));
                }
            }
            EditMode::Edit => {
                let ink = if dark { Color32::from_rgb(170, 190, 220) } else { Color32::from_rgb(40, 50, 70) };
                // faces escolhidas: preenchimento translúcido
                if ms.sel_mode == SelectMode::Face {
                    for &f in &ms.sel.faces {
                        let Some(face) = mesh.faces.get(f as usize) else { continue };
                        let pts: Vec<Vec3> = face.verts().map(|v| world[v as usize]).collect();
                        let sp: Vec<Option<Pos2>> = pts.iter().map(|p| scr(*p)).collect();
                        if sp.iter().all(Option::is_some) {
                            let sp: Vec<Pos2> = sp.into_iter().flatten().collect();
                            let mut mshape = egui::Mesh::default();
                            for p in &sp {
                                mshape.colored_vertex(*p, Color32::from_rgba_unmultiplied(255, 160, 40, 70));
                            }
                            for tri in doc::triangulate_polygon(&pts) {
                                mshape.add_triangle(tri[0] as u32, tri[1] as u32, tri[2] as u32);
                            }
                            painter.add(Shape::mesh(mshape));
                        }
                    }
                }
                for e in &topo.edges {
                    let (a, b) = e.verts;
                    let vis = e.faces.is_empty() || e.faces.iter().any(|f| front.get(*f as usize).copied().unwrap_or(true));
                    let sel = match ms.sel_mode {
                        SelectMode::Vertex => ms.sel.verts.contains(&a) && ms.sel.verts.contains(&b),
                        SelectMode::Edge => ms.sel.edges.contains(&edge_key(a, b)),
                        SelectMode::Face => e.faces.iter().any(|f| ms.sel.faces.contains(f)),
                    };
                    let s = if sel { Stroke::new(2.0, ORANGE) } else if vis { Stroke::new(1.2, ink) } else { Stroke::new(1.0, ink.gamma_multiply(0.25)) };
                    seg(world[a as usize], world[b as usize], s);
                }
                match ms.sel_mode {
                    SelectMode::Vertex => {
                        let used = mesh.used_verts();
                        for (v, p) in world.iter().enumerate() {
                            if !used.get(v).copied().unwrap_or(false) {
                                continue;
                            }
                            if let Some(q) = scr(*p) {
                                let sel = ms.sel.verts.contains(&(v as u32));
                                painter.circle_filled(q, if sel { 3.6 } else { 2.6 }, if sel { ORANGE } else { ink });
                            }
                        }
                    }
                    SelectMode::Face => {
                        for f in 0..mesh.face_count() as u32 {
                            if !front[f as usize] && !ms.sel.faces.contains(&f) {
                                continue;
                            }
                            if let Some(q) = scr(m.transform_point3(mesh.face_center(f))) {
                                let sel = ms.sel.faces.contains(&f);
                                painter.rect_filled(Rect::from_center_size(q, egui::vec2(4.5, 4.5)), 0.5, if sel { ORANGE } else { ink.gamma_multiply(0.8) });
                            }
                        }
                    }
                    SelectMode::Edge => {}
                }
            }
        }
    }
    // operação em curso: eixo e valor
    if let Some(md) = &ms.modal {
        if let Some(a) = md.axis {
            let dir = Vec3::from([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]][a]);
            seg(md.pivot - dir * 50.0, md.pivot + dir * 50.0, Stroke::new(1.4, AXIS[a]));
        }
        let ax = md.axis.map(|a| ["X", "Y", "Z"][a]).unwrap_or("");
        let txt = format!("{}  {ax}  {}", tr(md.kind.key()), modal_value_text(ms, md));
        hint(painter, rect, &txt, t.text);
    } else {
        let key = if ms.mode == EditMode::Edit { "mdl.hint.edit" } else { "mdl.hint.object" };
        hint(painter, rect, tr(key), t.text);
    }
}

fn hint(painter: &egui::Painter, rect: Rect, text: &str, ink: Color32) {
    let pos = rect.left_bottom() + egui::vec2(10.0, -24.0);
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), ink);
    painter.rect_filled(Rect::from_min_size(pos - egui::vec2(4.0, 2.0), galley.size() + egui::vec2(8.0, 4.0)), 3.0, Color32::from_black_alpha(140));
    painter.galley(pos, galley, ink);
}

fn modal_value_text(ms: &ModelingState, md: &Modal) -> String {
    if !md.numeric.is_empty() {
        return format!("[{}]", md.numeric);
    }
    let o = ms.active.and_then(|a| ms.doc.objects.get(a));
    match (md.kind, ms.mode, o) {
        (ModalKind::Grab | ModalKind::Rotate | ModalKind::Scale, EditMode::Object, Some(o)) => {
            let t = &o.transform;
            match md.kind {
                ModalKind::Grab => format!("{:.3} {:.3} {:.3} m", t.pos[0], t.pos[1], t.pos[2]),
                ModalKind::Rotate => format!("{:.1}° {:.1}° {:.1}°", t.rot_deg[0], t.rot_deg[1], t.rot_deg[2]),
                _ => format!("{:.3} {:.3} {:.3}", t.scale[0], t.scale[1], t.scale[2]),
            }
        }
        _ => String::new(),
    }
}

// =========================================================================================================== entrada

fn input(env: &mut Env, ui: &mut Ui, rect: Rect, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let view = view_of(&env.ed.modeling, rect.size());
    // navegação: meio (Shift pan, Ctrl zoom) ou Alt+esquerdo; roda = zoom
    let (mid_down, alt, shift, ctrl, delta, scroll) = ctx.input(|i| (i.pointer.middle_down(), i.modifiers.alt, i.modifiers.shift, i.modifiers.command, i.pointer.delta(), i.smooth_scroll_delta.y));
    let primary_down = ctx.input(|i| i.pointer.primary_down());
    {
        let ms = &mut env.ed.modeling;
        if ms.nav.is_none() && resp.hovered() && (ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle)) || (alt && ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)))) {
            ms.nav = Some(if shift { Nav::Pan } else if ctrl { Nav::Zoom } else { Nav::Orbit });
        }
        if let Some(n) = ms.nav {
            if !(mid_down || (primary_down && alt)) {
                ms.nav = None;
            } else {
                match n {
                    Nav::Orbit => ms.cam.orbit(delta.x, delta.y),
                    Nav::Pan => ms.cam.pan(delta.x, delta.y, rect.height()),
                    Nav::Zoom => ms.cam.zoom((delta.y * 0.01).exp()),
                }
            }
        }
        if resp.hovered() && scroll.abs() > 0.0 && ms.modal.is_none() {
            ms.cam.zoom((-scroll * 0.0015).exp());
        }
    }
    if env.ed.modeling.nav.is_some() {
        return;
    }
    // teclas (com o ponteiro no viewport ou com uma operação em curso)
    let keys_here = (env.ed.modeling.hovered || env.ed.modeling.modal.is_some()) && env.ed.modeling.kb;
    if keys_here {
        keys(env, &ctx, rect, &view, pointer);
    }
    // operação em curso: segue o mouse; clique confirma, direito cancela
    if env.ed.modeling.modal.is_some() {
        if let Some(p) = pointer {
            update_modal(env, &view, rect, p, ctrl);
        }
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
    let tool = *env.tool;
    // arrasto com a ferramenta da prateleira: mover/girar/escalar/extrude/inset/bevel
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt {
        env.ed.modeling.drag_from = pointer;
        let kind = match tool {
            ToolId::MMove => Some(ModalKind::Grab),
            ToolId::MRotate => Some(ModalKind::Rotate),
            ToolId::MScale => Some(ModalKind::Scale),
            ToolId::MExtrude => Some(ModalKind::Extrude),
            ToolId::MInset => Some(ModalKind::Inset),
            ToolId::MBevel => Some(ModalKind::Bevel),
            _ => None,
        };
        if let (Some(k), Some(p)) = (kind, pointer) {
            start_modal(env, k, &view, rect, p, true);
            return;
        }
    }
    if env.ed.modeling.swallow_release {
        // ainda segurando o botão do clique que confirmou: espera soltar; ao soltar, ignora esse clique
        if !ctx.input(|i| i.pointer.primary_down()) {
            env.ed.modeling.swallow_release = false;
        }
        return;
    }
    if resp.clicked_by(egui::PointerButton::Primary) && !alt {
        if let Some(p) = pointer {
            if tool == ToolId::MLoopCut {
                loop_cut_at(env, &view, rect, p);
            } else {
                click_select(env, &view, rect, p, shift);
            }
        }
    }
}

fn keys(env: &mut Env, ctx: &egui::Context, rect: Rect, view: &CamView, pointer: Option<Pos2>) {
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
    let typed: Vec<String> = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
    let p = pointer.unwrap_or(rect.center());
    // dentro de uma operação: eixo, número, confirmar/cancelar
    if env.ed.modeling.modal.is_some() {
        for (k, m) in &ev {
            let md = env.ed.modeling.modal.as_mut().expect("modal");
            match k {
                Key::X | Key::Y | Key::Z if m.is_none() => {
                    let a = match k {
                        Key::X => 0,
                        Key::Y => 1,
                        _ => 2,
                    };
                    md.axis = if md.axis == Some(a) { None } else { Some(a) };
                }
                Key::Backspace => {
                    md.numeric.pop();
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
        for t in typed {
            if let Some(md) = env.ed.modeling.modal.as_mut() {
                for c in t.chars() {
                    if c.is_ascii_digit() || c == '.' || c == ',' || (c == '-' && md.numeric.is_empty()) {
                        md.numeric.push(if c == ',' { '.' } else { c });
                    }
                }
            }
        }
        return;
    }
    for (k, m) in ev {
        let edit = env.ed.modeling.mode == EditMode::Edit;
        match (k, m.command, m.shift) {
            (Key::Tab, false, false) => {
                let next = if edit { EditMode::Object } else { EditMode::Edit };
                set_mode(env, next);
            }
            (Key::Num1, false, false) if edit => {
                env.ed.modeling.sel_mode = SelectMode::Vertex;
                env.ed.modeling.sel.clear();
            }
            (Key::Num2, false, false) if edit => {
                env.ed.modeling.sel_mode = SelectMode::Edge;
                env.ed.modeling.sel.clear();
            }
            (Key::Num3, false, false) if edit => {
                env.ed.modeling.sel_mode = SelectMode::Face;
                env.ed.modeling.sel.clear();
            }
            (Key::A, false, false) => select_all_toggle(env),
            (Key::A, false, true) => env.ed.modeling.add_menu = Some(p),
            (Key::G, false, false) => start_modal(env, ModalKind::Grab, view, rect, p, false),
            (Key::R, false, false) => start_modal(env, ModalKind::Rotate, view, rect, p, false),
            (Key::S, false, false) => start_modal(env, ModalKind::Scale, view, rect, p, false),
            (Key::E, false, false) if edit => start_modal(env, ModalKind::Extrude, view, rect, p, false),
            (Key::I, false, false) if edit => start_modal(env, ModalKind::Inset, view, rect, p, false),
            (Key::B, true, false) if edit => start_modal(env, ModalKind::Bevel, view, rect, p, false),
            (Key::R, true, false) if edit => loop_cut_at(env, view, rect, p),
            (Key::M, false, false) if edit => merge_selection(env),
            (Key::M, false, true) if edit => merge_near(env),
            (Key::X | Key::Delete, false, false) => delete_selection(env, false),
            (Key::X, true, false) if edit => delete_selection(env, true),
            (Key::D, false, true) if !edit => duplicate_active(env, view, rect, p),
            (Key::F, false, false) => frame_selection(env),
            (Key::Home, false, false) => frame_all(env),
            _ => {}
        }
    }
}

// ========================================================================================================= seleção

fn ray_tri(ro: Vec3, rd: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = rd.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-10 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ro - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = rd.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-5).then_some(t)
}

/// Face (e distância) que o raio acerta primeiro numa malha com matriz `m`.
fn ray_mesh(mesh: &EditMesh, m: Mat4, ro: Vec3, rd: Vec3) -> Option<(f32, u32)> {
    let mut best: Option<(f32, u32)> = None;
    for (fi, f) in mesh.faces.iter().enumerate() {
        let pts: Vec<Vec3> = f.verts().map(|v| m.transform_point3(mesh.co(v))).collect();
        for tri in doc::triangulate_polygon(&pts) {
            if let Some(t) = ray_tri(ro, rd, pts[tri[0]], pts[tri[1]], pts[tri[2]]) {
                if best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, fi as u32));
                }
            }
        }
    }
    best
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-9 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    (a + ab * t).distance(p)
}

fn click_select(env: &mut Env, view: &CamView, rect: Rect, p: Pos2, add: bool) {
    let px = to_px(rect, p);
    let (ro, rd) = view.ray(px);
    let ms = &mut env.ed.modeling;
    match ms.mode {
        EditMode::Object => {
            let mut best: Option<(f32, usize)> = None;
            for (i, o) in ms.doc.objects.iter().enumerate().filter(|(_, o)| o.visible) {
                let Ok(e) = o.evaluated() else { continue };
                if let Some((t, _)) = ray_mesh(&e, mat_of(o), ro, rd) {
                    if best.is_none_or(|(bt, _)| t < bt) {
                        best = Some((t, i));
                    }
                }
            }
            ms.active = best.map(|(_, i)| i).or(if add { ms.active } else { None });
        }
        EditMode::Edit => {
            let Some(a) = ms.active else { return };
            let o = &ms.doc.objects[a];
            let m = mat_of(o);
            let mesh = &o.mesh;
            if !add {
                ms.sel.clear();
            }
            match ms.sel_mode {
                SelectMode::Vertex => {
                    let used = mesh.used_verts();
                    let mut best: Option<(f32, f32, u32)> = None;
                    for v in 0..mesh.vert_count() as u32 {
                        if !used.get(v as usize).copied().unwrap_or(false) {
                            continue;
                        }
                        if let Some((q, z)) = view.project(m.transform_point3(mesh.co(v))) {
                            let d = q.distance(px);
                            if d <= 10.0 && best.is_none_or(|(bd, bz, _)| d < bd - 2.0 || (d < bd + 2.0 && z < bz)) {
                                best = Some((d, z, v));
                            }
                        }
                    }
                    if let Some((_, _, v)) = best {
                        if !ms.sel.verts.remove(&v) {
                            ms.sel.verts.insert(v);
                        }
                    }
                }
                SelectMode::Edge => {
                    if let Some(e) = pick_edge(mesh, m, view, px) {
                        if !ms.sel.edges.remove(&e) {
                            ms.sel.edges.insert(e);
                        }
                    }
                }
                SelectMode::Face => {
                    if let Some((_, f)) = ray_mesh(mesh, m, ro, rd) {
                        if !ms.sel.faces.remove(&f) {
                            ms.sel.faces.insert(f);
                        }
                    }
                }
            }
        }
    }
}

fn pick_edge(mesh: &EditMesh, m: Mat4, view: &CamView, px: Vec2) -> Option<EdgeKey> {
    let topo = aiai_3d_edit::Topology::build(mesh);
    let mut best: Option<(f32, EdgeKey)> = None;
    for e in &topo.edges {
        let (a, b) = e.verts;
        let (Some((pa, _)), Some((pb, _))) = (view.project(m.transform_point3(mesh.co(a))), view.project(m.transform_point3(mesh.co(b)))) else { continue };
        let d = seg_dist(px, pa, pb);
        if d <= 8.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e.verts));
        }
    }
    best.map(|(_, e)| e)
}

fn select_all_toggle(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    if ms.mode != EditMode::Edit {
        return;
    }
    let Some(mesh) = ms.active_mesh() else { return };
    let all = Selection::select_all(mesh);
    ms.sel = if ms.sel.is_empty() { all } else { Selection::default() };
}

/// Vértices da seleção (modo Edição) ou `None` sem seleção.
fn selected_verts(ms: &ModelingState) -> Vec<u32> {
    match ms.active_mesh() {
        Some(m) => doc::selection_verts(m, &ms.sel, ms.sel_mode),
        None => Vec::new(),
    }
}

fn selected_faces(ms: &ModelingState) -> Vec<u32> {
    let Some(m) = ms.active_mesh() else { return Vec::new() };
    match ms.sel_mode {
        SelectMode::Face => ms.sel.faces.iter().copied().collect(),
        // em vértice/aresta: as faces com todos os vértices escolhidos
        _ => {
            let vs: std::collections::BTreeSet<u32> = doc::selection_verts(m, &ms.sel, ms.sel_mode).into_iter().collect();
            (0..m.face_count() as u32).filter(|f| m.faces[*f as usize].verts().all(|v| vs.contains(&v))).collect()
        }
    }
}

fn selected_edges(ms: &ModelingState) -> Vec<EdgeKey> {
    let Some(m) = ms.active_mesh() else { return Vec::new() };
    match ms.sel_mode {
        SelectMode::Edge => ms.sel.edges.iter().copied().collect(),
        SelectMode::Face => {
            let mut s = std::collections::BTreeSet::new();
            for f in &ms.sel.faces {
                if let Some(face) = m.faces.get(*f as usize) {
                    for (a, b) in face.edges() {
                        s.insert(edge_key(a, b));
                    }
                }
            }
            s.into_iter().collect()
        }
        SelectMode::Vertex => doc::all_edges(m).into_iter().filter(|(a, b)| ms.sel.verts.contains(a) && ms.sel.verts.contains(b)).collect(),
    }
}

// ======================================================================================================== operações

fn start_modal(env: &mut Env, kind: ModalKind, view: &CamView, rect: Rect, p: Pos2, from_tool: bool) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else {
        env.ed.notify(tr("mdl.need_object"));
        return;
    };
    let o = &ms.doc.objects[a];
    let m = mat_of(o);
    let (mut verts, mut faces, mut edges) = (Vec::new(), Vec::new(), Vec::new());
    let mut normal = Vec3::Y;
    let pivot = match ms.mode {
        EditMode::Object => {
            if matches!(kind, ModalKind::Extrude | ModalKind::Inset | ModalKind::Bevel) {
                env.ed.notify(tr("mdl.need_edit"));
                return;
            }
            Vec3::from(o.transform.pos)
        }
        EditMode::Edit => {
            verts = selected_verts(ms);
            match kind {
                ModalKind::Extrude | ModalKind::Inset => {
                    faces = selected_faces(ms);
                    if faces.is_empty() {
                        env.ed.notify(tr("mdl.need_faces"));
                        return;
                    }
                    let nm = Mat3::from_mat4(m).inverse().transpose();
                    let n: Vec3 = faces.iter().map(|f| o.mesh.face_normal(*f) * o.mesh.face_area(*f).max(1e-6)).sum();
                    normal = (nm * n).normalize_or(Vec3::Y);
                    verts = doc::selection_verts(&o.mesh, &Selection::from_faces(&o.mesh, faces.iter().copied()), SelectMode::Face);
                }
                ModalKind::Bevel => {
                    edges = selected_edges(ms);
                    if edges.is_empty() {
                        env.ed.notify(tr("mdl.need_edges"));
                        return;
                    }
                }
                _ => {}
            }
            if verts.is_empty() {
                env.ed.notify(tr("mdl.need_selection"));
                return;
            }
            let c: Vec3 = verts.iter().map(|v| o.mesh.co(*v)).sum::<Vec3>() / verts.len() as f32;
            m.transform_point3(c)
        }
    };
    let pivot_px = view.project(pivot).map(|(q, _)| q).unwrap_or(to_px(rect, rect.center()));
    ms.modal = Some(Modal { kind, before: ms.doc.clone(), before_sel: ms.sel.clone(), axis: None, start: p, pivot, pivot_px, verts, faces, edges, normal, numeric: String::new(), from_tool });
}

fn axis_vec(a: usize) -> Vec3 {
    [Vec3::X, Vec3::Y, Vec3::Z][a]
}

/// Desloca `d` px da tela ao longo do eixo de mundo `axis` (passando pelo pivô): metros.
fn screen_to_axis(view: &CamView, pivot: Vec3, axis: Vec3, d: Vec2) -> f32 {
    let (Some((a, _)), Some((b, _))) = (view.project(pivot), view.project(pivot + axis)) else { return 0.0 };
    let s = b - a;
    let l2 = s.length_squared();
    if l2 < 1e-6 { 0.0 } else { d.dot(s) / l2 }
}

fn snap(v: f32, step: f32, on: bool) -> f32 {
    if on && step > 0.0 { (v / step).round() * step } else { v }
}

fn update_modal(env: &mut Env, view: &CamView, rect: Rect, p: Pos2, ctrl_down: bool) {
    // encaixe: o botão do cabeçalho liga; Ctrl inverte durante a operação
    let ctrl = env.ed.modeling.snap ^ ctrl_down;
    let ms = &mut env.ed.modeling;
    let Some(md) = ms.modal.as_ref() else { return };
    let Some(a) = ms.active else { return };
    let num: Option<f32> = md.numeric.parse::<f32>().ok();
    let d = to_px(rect, p) - to_px(rect, md.start);
    let cur_px = to_px(rect, p);
    let mut doc = md.before.clone();
    let mut sel = md.before_sel.clone();
    let wpp = view.world_per_pixel(md.pivot);
    let o_before = &md.before.objects[a];
    let m = mat_of(o_before);
    let inv = m.inverse();
    let mut err = None;
    match md.kind {
        ModalKind::Grab => {
            let mut delta = match md.axis {
                Some(ax) => axis_vec(ax) * num.unwrap_or_else(|| screen_to_axis(view, md.pivot, axis_vec(ax), d)),
                None => match num {
                    Some(v) => Vec3::X * v,
                    None => view.right * d.x * wpp - view.up * d.y * wpp,
                },
            };
            if ctrl && num.is_none() {
                delta = Vec3::new(snap(delta.x, 0.1, true), snap(delta.y, 0.1, true), snap(delta.z, 0.1, true));
            }
            let o = &mut doc.objects[a];
            match ms.mode {
                EditMode::Object => o.transform.pos = (Vec3::from(o_before.transform.pos) + delta).to_array(),
                EditMode::Edit => {
                    let local = inv.transform_point3(md.pivot + delta) - inv.transform_point3(md.pivot);
                    if let Err(e) = doc::translate(&mut o.mesh, &md.verts, local) {
                        err = Some(e);
                    }
                }
            }
        }
        ModalKind::Rotate => {
            let v0 = to_px(rect, md.start) - md.pivot_px;
            let v1 = cur_px - md.pivot_px;
            let (a0, a1) = (v0.y.atan2(v0.x), v1.y.atan2(v1.x));
            // ângulo na tela (y para baixo) → sentido anti-horário visto pela câmera
            let mut ang = -(a1 - a0);
            let toward = -view.forward;
            let axis = match md.axis {
                Some(ax) => {
                    let v = axis_vec(ax);
                    if v.dot(toward) < 0.0 {
                        ang = -ang;
                    }
                    v
                }
                None => toward,
            };
            if let Some(v) = num {
                ang = v.to_radians();
            } else if ctrl {
                ang = snap(ang, 15f32.to_radians(), true);
            }
            let q = Quat::from_axis_angle(axis.normalize_or(Vec3::Y), ang);
            let o = &mut doc.objects[a];
            match ms.mode {
                EditMode::Object => o.transform.rot_deg = rotation_deg(q * rotation_quat(o_before.transform.rot_deg)),
                EditMode::Edit => {
                    let local_axis = (Mat3::from_mat4(inv) * axis).normalize_or(Vec3::Y);
                    if let Err(e) = doc::rotate(&mut o.mesh, &md.verts, local_axis, ang) {
                        err = Some(e);
                    }
                }
            }
        }
        ModalKind::Scale => {
            let r0 = (to_px(rect, md.start) - md.pivot_px).length().max(4.0);
            let r1 = (cur_px - md.pivot_px).length();
            let mut f = num.unwrap_or(r1 / r0);
            if ctrl && num.is_none() {
                f = snap(f, 0.1, true);
            }
            let s = match md.axis {
                Some(ax) => {
                    let mut v = Vec3::ONE;
                    v[ax] = f;
                    v
                }
                None => Vec3::splat(f),
            };
            let o = &mut doc.objects[a];
            match ms.mode {
                EditMode::Object => {
                    let ns = Vec3::from(o_before.transform.scale) * s;
                    if ns.abs().min_element() >= 1e-4 {
                        o.transform.scale = ns.to_array();
                    }
                }
                EditMode::Edit => {
                    if let Err(e) = doc::scale(&mut o.mesh, &md.verts, s) {
                        err = Some(e);
                    }
                }
            }
        }
        ModalKind::Extrude => {
            let mut dist = num.unwrap_or_else(|| screen_to_axis(view, md.pivot, md.normal, d));
            if ctrl && num.is_none() {
                dist = snap(dist, 0.1, true);
            }
            // metros no mundo → unidades do objeto ao longo da normal
            let k = (Mat3::from_mat4(inv) * md.normal).length().max(1e-6);
            let o = &mut doc.objects[a];
            match doc::extrude(&mut o.mesh, &md.faces, dist * k) {
                Ok(out) => {
                    sel = out.selection;
                    if ms.sel_mode != SelectMode::Face {
                        sel = Selection::from_faces(&o.mesh, sel.faces.iter().copied());
                    }
                }
                Err(e) => err = Some(e),
            }
        }
        ModalKind::Inset | ModalKind::Bevel => {
            let mut v = num.unwrap_or_else(|| (cur_px - md.pivot_px).length().max(0.0) * wpp - (to_px(rect, md.start) - md.pivot_px).length() * wpp);
            v = v.abs();
            if ctrl && num.is_none() {
                v = snap(v, 0.05, true);
            }
            let k = Mat3::from_mat4(inv).col(0).length().max(1e-6);
            let o = &mut doc.objects[a];
            let r = if md.kind == ModalKind::Inset { doc::inset(&mut o.mesh, &md.faces, (v * k).max(1e-4), 0.0) } else { doc::bevel(&mut o.mesh, &md.edges, (v * k).max(1e-4), 1) };
            match r {
                Ok(out) => {
                    if md.kind == ModalKind::Inset {
                        sel = out.selection;
                    } else {
                        sel = Selection::default();
                    }
                }
                Err(e) => err = Some(e),
            }
        }
    }
    if err.is_none() {
        ms.doc = doc;
        ms.sel = sel;
    }
}

fn confirm_modal(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    if let Some(md) = ms.modal.take() {
        ms.commit(md.before);
    }
}

fn loop_cut_at(env: &mut Env, view: &CamView, rect: Rect, p: Pos2) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active.filter(|_| ms.mode == EditMode::Edit) else {
        env.ed.notify(tr("mdl.need_edit"));
        return;
    };
    let o = &ms.doc.objects[a];
    let e = pick_edge(&o.mesh, mat_of(o), view, to_px(rect, p)).or_else(|| selected_edges(ms).first().copied());
    let Some(e) = e else {
        env.ed.notify(tr("mdl.need_edge_hover"));
        return;
    };
    match ms.edit(|d| doc::loop_cut(&mut d.objects[a].mesh, e, 1)) {
        Ok(out) => {
            ms.sel_mode = SelectMode::Edge;
            ms.sel = Selection::from_edges(out.selection.edges.iter().copied());
        }
        Err(e) => env.ed.notify(e),
    }
}

fn merge_selection(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else { return };
    let verts = selected_verts(ms);
    if verts.len() < 2 {
        env.ed.notify(tr("mdl.need_two_verts"));
        return;
    }
    match ms.edit(|d| doc::merge_center(&mut d.objects[a].mesh, &verts)) {
        Ok(_) => ms.sel.clear(),
        Err(e) => env.ed.notify(e),
    }
}

/// Funde os vértices que ficaram no mesmo lugar (Shift+M; depois de achatar faces, por exemplo).
pub(crate) fn merge_near(env: &mut Env) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else {
        env.ed.notify(tr("mdl.need_object"));
        return;
    };
    let before = ms.doc.objects[a].mesh.vert_count();
    match ms.edit(|d| doc::merge_by_distance(&mut d.objects[a].mesh, None, 1e-4)) {
        Ok(_) => {
            let n = before - ms.doc.objects[a].mesh.vert_count();
            ms.sel.clear();
            env.ed.notify(format!("{}: {n}", tr("mdl.merged")));
        }
        Err(e) => env.ed.notify(e),
    }
}

fn delete_selection(env: &mut Env, dissolve: bool) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else { return };
    match ms.mode {
        EditMode::Object => {
            let r = ms.edit(|d| {
                d.objects.remove(a);
                Ok(())
            });
            if r.is_ok() {
                ms.active = None;
                ms.fix();
            }
        }
        EditMode::Edit => {
            if ms.sel.is_empty() {
                env.ed.notify(tr("mdl.need_selection"));
                return;
            }
            let (sel, mode) = (ms.sel.clone(), ms.sel_mode);
            let r = ms.edit(|d| if dissolve { doc::dissolve(&mut d.objects[a].mesh, &sel, mode) } else { doc::delete(&mut d.objects[a].mesh, &sel, mode) });
            match r {
                Ok(_) => ms.sel.clear(),
                Err(e) => env.ed.notify(e),
            }
        }
    }
}

fn duplicate_active(env: &mut Env, view: &CamView, rect: Rect, p: Pos2) {
    let ms = &mut env.ed.modeling;
    let Some(a) = ms.active else { return };
    let r = ms.edit(|d| {
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
            start_modal(env, ModalKind::Grab, view, rect, p, false);
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
        None => ms.cam = ModelingState::default().cam,
    }
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
            ms.history.push(before);
            ms.asset = Some(asset.clone());
            ms.active = if ms.doc.objects.is_empty() { None } else { Some(0) };
            ms.mode = EditMode::Object;
            ms.sel.clear();
            ms.dirty = false;
            frame_all(env);
        }
        Err(e) => env.ed.notify(e.to_string()),
    }
}

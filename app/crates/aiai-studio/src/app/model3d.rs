//! Camada 3D no app (S24 3D): o serviço da GPU que desenha as camadas `model3d` para o compositor (renderer cel
//! vendorizado do ANIGO, numa thread própria, no device do egui), a ferramenta 3D no canvas (gizmo mover/girar/escalar,
//! órbita/pan/dolly da câmera da camada, um passo de undo por gesto, prévia reduzida ao vivo durante o arrasto), a seção
//! 3D das Propriedades e a importação (menu "+" das Camadas e arrastar arquivo para o canvas).
//!
//! Pose (S24 3D, fases 8–9): no modo Pose da ferramenta 3D (tecla 4) as juntas do esqueleto (Manequim ou modelo com pele)
//! aparecem no canvas; clicar escolhe o osso, os anéis giram o osso em volta da junta (Shift: 15°) e arrastar mão/pé faz
//! IK de dois ossos. Cada gesto é um passo de undo (chave `m3d.pose` onde a pose já é animada). Poses prontas, espelhar,
//! repouso e as poses do usuário (`Documentos\AnimeIAI\Poses\*.aiaipose`, nada com `--no-session`) ficam nas Propriedades.

use super::commands::{self as cmds, NodeId};
use super::ui_kit::{Env, dim_label, section};
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_3d_model::camera::{CamView, layer_from_orbit, orbit_from_layer};
use aiai_3d_model::gizmo::{Axis3, DragDelta, GizmoDrag, GizmoFrame, GizmoMode, Handle, hit_test};
use aiai_3d_model::pose as mpose;
use aiai_3d_model::{Model, gizmo_apply, raster};
use aiai_core::model3d::{Model3dParams, Model3dState, POSE_KEY, Pose3d, RenderMode3d, hex_rgb, parse_hex_rgb};
use aiai_core::*;
use aiai_render::raster3d::{Layer3dJob, Layer3dRaster};
use egui::{Color32, Pos2, Rect, Shape, Stroke, TextureHandle, TextureOptions, Ui, Vec2};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

// ======================================================================================================= serviço GPU

struct Job {
    hash: BlobHash,
    model: Arc<Model>,
    state: Model3dState,
    w: u32,
    h: u32,
    reply: crossbeam_channel::Sender<Result<Vec<u8>, String>>,
}

/// Renderizador das camadas 3D na GPU: uma thread com o `HeadlessRenderer` (renderer cel do ANIGO) no device do egui e
/// a cena de cada modelo montada uma vez. O modo `wire` vai para o rasterizador CPU (o renderer cel não desenha arestas).
pub(crate) struct GpuRaster {
    tx: crossbeam_channel::Sender<Job>,
    pub adapter: String,
}

static GPU: OnceLock<Arc<GpuRaster>> = OnceLock::new();

/// Liga o serviço GPU no device do egui e o registra no compositor (o canvas, a bridge e o MCP com o app aberto passam a
/// usá-lo). Sem sucesso, o compositor continua com o rasterizador CPU e o motivo vai para o log.
pub(crate) fn install_gpu(rs: &egui_wgpu::RenderState) {
    // viewport nativo da Modelagem (texturas registradas no egui, redesenhadas na GPU)
    super::modeling::install_gpu(rs);
    if GPU.get().is_some() {
        return;
    }
    let info = rs.adapter.get_info();
    let renderer = match aiai_3d_render::HeadlessRenderer::from_device(Arc::new(rs.device.clone()), Arc::new(rs.queue.clone()), info.clone()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("camada 3D: renderer GPU indisponível ({e}); usando o rasterizador CPU");
            return;
        }
    };
    let (tx, rx) = crossbeam_channel::unbounded::<Job>();
    let spawned = std::thread::Builder::new().name("aiai-3d-gpu".into()).spawn(move || gpu_worker(renderer, rx));
    if let Err(e) = spawned {
        tracing::warn!("camada 3D: thread do renderer não abriu ({e}); usando o rasterizador CPU");
        return;
    }
    let g = Arc::new(GpuRaster { tx, adapter: format!("{} ({:?})", info.name, info.backend) });
    let _ = GPU.set(Arc::clone(&g));
    aiai_render::raster3d::set_layer3d_raster(Some(g as Arc<dyn Layer3dRaster>));
    tracing::info!("camada 3D: renderer cel na GPU ({} {:?})", info.name, info.backend);
}

/// Bytes de GPU que o serviço 3D guarda entre quadros (alvos e mapa de sombra; ver `gpu_mem`).
static GPU_CACHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Pedido do app (pressão de VRAM) para o serviço 3D soltar o que guarda entre quadros.
static GPU_RELEASE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Sem quadro 3D por este tempo, o serviço solta os alvos guardados (voltam no próximo quadro).
const GPU_IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(20);

pub(crate) fn gpu_cached_bytes() -> u64 {
    GPU_CACHED.load(std::sync::atomic::Ordering::Relaxed) + super::modeling::gpu_bytes()
}

pub(crate) fn gpu_release_cached() {
    GPU_RELEASE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn gpu_worker(renderer: aiai_3d_render::HeadlessRenderer, rx: crossbeam_channel::Receiver<Job>) {
    use std::sync::atomic::Ordering;
    let mut scenes: Vec<(BlobHash, aiai_3d_core::Scene)> = Vec::new();
    let mut last_job = std::time::Instant::now();
    loop {
        let job = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(job) => job,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                let idle = last_job.elapsed() >= GPU_IDLE_RELEASE && GPU_CACHED.load(Ordering::Relaxed) > 0;
                if GPU_RELEASE.swap(false, Ordering::Relaxed) || idle {
                    renderer.release_cached_targets();
                    GPU_CACHED.store(renderer.cached_gpu_bytes(), Ordering::Relaxed);
                }
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        last_job = std::time::Instant::now();
        if GPU_RELEASE.swap(false, Ordering::Relaxed) {
            renderer.release_cached_targets();
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
            if job.state.mode == RenderMode3d::Wire {
                return Ok(to_u8(&raster::render_cpu(&job.model, &job.state, job.w, job.h)));
            }
            let i = match scenes.iter().position(|(h, _)| *h == job.hash) {
                Some(i) => i,
                None => {
                    scenes.push((job.hash.clone(), aiai_3d_model::scene::base_scene(&job.model)));
                    if scenes.len() > 4 {
                        scenes.remove(0);
                    }
                    scenes.len() - 1
                }
            };
            let scene = &mut scenes[i].1;
            aiai_3d_model::scene::update_scene(scene, &job.model, &job.state, job.w, job.h);
            let (img, _m) = pollster::block_on(renderer.render_scene(scene, job.w, job.h)).map_err(|e| format!("render 3D na GPU: {e}"))?;
            Ok(img.into_raw())
        }));
        GPU_CACHED.store(renderer.cached_gpu_bytes(), Ordering::Relaxed);
        let _ = job.reply.send(r.unwrap_or_else(|_| Err("o renderer 3D entrou em pânico".into())));
    }
}

fn to_u8(px: &[[f32; 4]]) -> Vec<u8> {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    px.iter().flat_map(|p| [b(p[0]), b(p[1]), b(p[2]), b(p[3])]).collect()
}

impl GpuRaster {
    /// Pede um quadro sem esperar (prévia do arrasto); a resposta chega no canal.
    fn request(&self, hash: BlobHash, model: Arc<Model>, state: Model3dState, w: u32, h: u32) -> Option<crossbeam_channel::Receiver<Result<Vec<u8>, String>>> {
        let (reply, rx) = crossbeam_channel::bounded(1);
        self.tx.send(Job { hash, model, state, w, h, reply }).ok()?;
        Some(rx)
    }
}

impl Layer3dRaster for GpuRaster {
    fn backend(&self) -> &str {
        "gpu"
    }

    fn render(&self, job: &Layer3dJob) -> Result<Vec<[f32; 4]>, String> {
        // pose: malha posada na CPU (skinning) e cena própria por pose (a chave inclui a pose)
        let model = aiai_3d_model::load_posed(job.blobs, job.model_hash, job.state.pose.as_ref())?;
        let key = mpose::posed_key(job.model_hash, job.state.pose.as_ref());
        let rx = self.request(key, model, job.state.clone(), job.width, job.height).ok_or("serviço 3D da GPU parado")?;
        let rgba = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "o render 3D na GPU não respondeu em 30 s".to_string())??;
        if rgba.len() != (job.width * job.height * 4) as usize {
            return Err(format!("render 3D na GPU devolveu {} bytes para {}x{}", rgba.len(), job.width, job.height));
        }
        Ok(rgba.chunks_exact(4).map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]).collect())
    }
}

// ======================================================================================================= estado de UI

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DragKind {
    Gizmo,
    Orbit,
    Pan,
    Dolly,
    /// Pose: anel de rotação de um osso.
    PoseRing,
    /// Pose: IK de mão/pé (a junta segue o cursor num plano de frente para a câmera).
    PoseIk,
}

struct Drag {
    layer: NodeId,
    cut: NodeId,
    frame: u32,
    kind: DragKind,
    start: Model3dState,
    cur: Model3dState,
    pivot: glam::Vec3,
    view: CamView,
    scale: f32,
    gizmo: Option<GizmoDrag>,
    last_screen: Pos2,
    /// Pose: osso arrastado, pose do começo e plano do IK (ponto, normal) no mundo.
    bone: Option<usize>,
    start_pose: Pose3d,
    ik_plane: Option<(glam::Vec3, glam::Vec3)>,
}

/// Imagens do corte sem a camada 3D (o que fica abaixo e acima), para a prévia do arrasto não mostrar o modelo duas vezes.
struct Split {
    key: (u64, NodeId, u32, NodeId, bool, bool),
    below: TextureHandle,
    above: TextureHandle,
}

type SplitRx = crossbeam_channel::Receiver<Result<(egui::ColorImage, egui::ColorImage), String>>;
type LiveRx = crossbeam_channel::Receiver<Result<Vec<u8>, String>>;

/// Estado da ferramenta 3D guardado no editor.
#[derive(Default)]
pub(crate) struct M3dState {
    pub mode: GizmoModeUi,
    drag: Option<Drag>,
    hover: Option<Handle>,
    split: Option<Split>,
    split_pending: Option<((u64, NodeId, u32, NodeId, bool, bool), SplitRx)>,
    live_tex: Option<TextureHandle>,
    live_state: Option<Model3dState>,
    live_pending: Option<(Model3dState, (u32, u32), LiveRx)>,
    live_size: (u32, u32),
    /// Depois de soltar: a prévia fica até o render do documento novo chegar.
    hold: Option<(u64, Instant)>,
    /// Pose: osso escolhido (nome) e IK desligado (padrão: ligado nas mãos e pés).
    pub pose_bone: Option<String>,
    pub no_ik: bool,
    hover_joint: Option<usize>,
    /// Poses do usuário em `Documentos\AnimeIAI\Poses` (lidas ao abrir a seção) e o nome da próxima a salvar.
    pub user_files: bool,
    user_poses: Option<Vec<UserPose>>,
    pose_name: String,
}

impl M3dState {
    /// Fora de um arrasto: solta as imagens da prévia (abaixo/acima da camada e o quadro ao vivo; orçamento de VRAM,
    /// `gpu_owners`). O próximo arrasto pede de novo.
    pub(crate) fn release_gpu(&mut self) -> bool {
        if self.drag.is_some() || self.hold.is_some() {
            return false;
        }
        self.live_state = None;
        self.split.take().is_some() | self.live_tex.take().is_some()
    }
}

/// Pose guardada pelo usuário (arquivo `.aiaipose`, o envelope de recurso da Biblioteca com a pose no `payload`).
#[derive(Clone)]
struct UserPose {
    name: String,
    path: std::path::PathBuf,
    pose: Pose3d,
}

/// Modo do gizmo (mover/girar/escalar).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum GizmoModeUi {
    #[default]
    Translate,
    Rotate,
    Scale,
    /// Posar o esqueleto (juntas, anéis por osso, IK de mãos e pés).
    Pose,
}

impl GizmoModeUi {
    fn gizmo(self) -> GizmoMode {
        match self {
            GizmoModeUi::Translate => GizmoMode::Translate,
            GizmoModeUi::Rotate | GizmoModeUi::Pose => GizmoMode::Rotate,
            GizmoModeUi::Scale => GizmoMode::Scale,
        }
    }
}

// ======================================================================================================= contexto

struct Ctx3 {
    layer: NodeId,
    cut: NodeId,
    frame: u32,
    state: Model3dState,
    pivot: glam::Vec3,
    view: CamView,
    /// Camada → canvas (movimento 2D da camada); `None` = identidade.
    place: Option<aiai_paint::Affine>,
}

/// Camada ativa, se for 3D: id, dados e parâmetros.
pub(crate) fn active(env: &mut Env) -> Option<(NodeId, LayerData, Model3dParams)> {
    let layer = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &layer)?.clone();
    if l.kind != LayerType::Model3d {
        return None;
    }
    let p = Model3dParams::from_layer_params(&l.params)?.ok()?;
    Some((layer, l, p))
}

/// Visão do gizmo: o canvas da camada em pixels de TELA (canvas × `scale`), para o gizmo e a tolerância do clique terem
/// tamanho constante na tela em qualquer zoom. Pontos da camada entram multiplicados por `scale`.
fn view_for(model: &Model, s: &Model3dState, canvas: (u32, u32), scale: f32) -> CamView {
    let (c, r) = raster::world_bounds(model, s);
    let cam = orbit_from_layer(&s.camera, c, r);
    CamView::from_orbit(&cam, glam::Vec2::new(canvas.0 as f32 * scale, canvas.1 as f32 * scale))
}

fn ctx(env: &mut Env, scale: f32) -> Option<Ctx3> {
    let (layer, l, p) = active(env)?;
    let cut = env.ed.active_cut(env.core)?;
    let frame = env.ed.frame;
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let model = aiai_3d_model::load_model(&env.core.blobs, &p.model_hash).ok()?;
    let state = match &env.ed.m3d.drag {
        Some(d) if d.layer == layer => d.cur.clone(),
        _ => p.eval(&l.keys, frame as f32),
    };
    let view = view_for(&model, &state, canvas, scale);
    let place = cmds::layer_placement(&env.core.project, &cut, &layer, frame);
    Some(Ctx3 { layer, cut, frame, pivot: glam::Vec3::from(model.center()), state, view, place })
}

fn to_layer(place: Option<aiai_paint::Affine>, c: Vec2) -> glam::Vec2 {
    match place.and_then(|a| a.inverse()) {
        Some(inv) => {
            let (x, y) = inv.apply(c.x, c.y);
            glam::Vec2::new(x, y)
        }
        None => glam::Vec2::new(c.x, c.y),
    }
}

fn to_canvas(place: Option<aiai_paint::Affine>, p: glam::Vec2) -> Vec2 {
    match place {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => Vec2::new(p.x, p.y),
    }
}

// ======================================================================================================= gestos

/// Começo de gesto no canvas. A ferramenta 3D age na camada 3D ativa; com o Mover, Alt+arrastar orbita a câmera dela.
/// Devolve `true` se o gesto é daqui.
pub(crate) fn press(env: &mut Env, xf: &super::editor::Xf, pos: Pos2, mods: egui::Modifiers) -> bool {
    let tool = *env.tool;
    if !(tool == ToolId::Model3d || (tool == ToolId::Move && mods.alt)) {
        return false;
    }
    let scale = xf.scale.max(1e-3);
    let Some(c) = ctx(env, scale) else {
        if tool == ToolId::Model3d {
            env.ed.notify(tr("m3d.need_layer"));
            return true;
        }
        return false;
    };
    if cmds::layer_data(&env.core.project, &c.layer).is_some_and(|l| l.locked) {
        env.ed.notify(tr("m3d.locked"));
        return true;
    }
    let lp = to_layer(c.place, xf.to_canvas(pos)) * scale;
    // pose: anel do osso escolhido, clique numa junta (escolhe; mão/pé com IK começa o arrasto) ou cai na órbita
    if tool == ToolId::Model3d && env.ed.m3d.mode == GizmoModeUi::Pose && !mods.alt && !mods.shift && !mods.command {
        if let Some(pv) = pose_view(env, &c) {
            let sel = env.ed.m3d.pose_bone.as_deref().and_then(|n| pv.skin().index(n));
            let base = Drag { layer: c.layer.clone(), cut: c.cut.clone(), frame: c.frame, kind: DragKind::PoseRing, start: c.state.clone(), cur: c.state.clone(), pivot: c.pivot, view: c.view, scale, gizmo: None, last_screen: pos, bone: None, start_pose: pv.pose.clone(), ik_plane: None };
            if let Some(b) = sel {
                let f = GizmoFrame::new(pv.world[b], glam::Quat::IDENTITY, &c.view);
                if let Some(h) = hit_test(GizmoMode::Rotate, &f, &c.view, lp) {
                    if let Some(g) = GizmoDrag::begin(GizmoMode::Rotate, h, f, &c.view, lp) {
                        env.ed.m3d.drag = Some(Drag { gizmo: Some(g), bone: Some(b), ..base });
                        env.ed.m3d.hold = None;
                        return true;
                    }
                }
            }
            if let Some(b) = pv.pick(&c.view, lp) {
                let name = pv.skin().bones[b].name.clone();
                let ik = !env.ed.m3d.no_ik && mpose::IK_ENDS.contains(&name.as_str());
                env.ed.m3d.pose_bone = Some(name);
                if ik {
                    env.ed.m3d.drag = Some(Drag { kind: DragKind::PoseIk, bone: Some(b), ik_plane: Some((pv.world[b], c.view.forward)), ..base });
                    env.ed.m3d.hold = None;
                }
                return true;
            }
        }
    }
    let mode = env.ed.m3d.mode.gizmo();
    let mut kind = if mods.alt {
        DragKind::Orbit
    } else if mods.shift {
        DragKind::Pan
    } else if mods.command {
        DragKind::Dolly
    } else {
        DragKind::Orbit
    };
    let mut gizmo = None;
    if tool == ToolId::Model3d && !mods.alt && !mods.shift && !mods.command {
        let f = gizmo_apply::frame_for(mode, &c.state.transform, c.pivot, &c.view);
        if let Some(h) = hit_test(mode, &f, &c.view, lp) {
            gizmo = GizmoDrag::begin(mode, h, f, &c.view, lp);
            if gizmo.is_some() {
                kind = DragKind::Gizmo;
            }
        }
    }
    env.ed.m3d.drag = Some(Drag { layer: c.layer, cut: c.cut, frame: c.frame, kind, start: c.state.clone(), cur: c.state, pivot: c.pivot, view: c.view, scale, gizmo, last_screen: pos, bone: None, start_pose: Pose3d::default(), ik_plane: None });
    env.ed.m3d.hold = None;
    true
}

pub(crate) fn dragging(env: &Env) -> bool {
    env.ed.m3d.drag.is_some()
}

/// Arrasto: atualiza o estado em edição (sem tocar no documento) e pede a prévia.
pub(crate) fn drag(env: &mut Env, xf: &super::editor::Xf, pos: Pos2) {
    let shift = env.ed.shift;
    let place = env.ed.m3d.drag.as_ref().and_then(|d| cmds::layer_placement(&env.core.project, &d.cut, &d.layer, d.frame));
    let Some(d) = env.ed.m3d.drag.as_mut() else { return };
    let Ok(model) = aiai_3d_model::load_model(&env.core.blobs, &d.cur.model_hash) else { return };
    let canvas = cmds::canvas_size(&env.core.project, &d.cut);
    let ds = pos - d.last_screen;
    d.last_screen = pos;
    match d.kind {
        DragKind::Gizmo => {
            let lp = to_layer(place, xf.to_canvas(pos)) * d.scale;
            if let Some(g) = d.gizmo.as_mut() {
                if let Some(delta) = g.update(&d.view, lp) {
                    d.cur.transform = gizmo_apply::apply(&d.start.transform, d.pivot, delta, shift);
                }
            }
        }
        DragKind::Orbit | DragKind::Pan | DragKind::Dolly => {
            let (c, r) = raster::world_bounds(&model, &d.cur);
            let mut cam = orbit_from_layer(&d.cur.camera, c, r);
            match d.kind {
                DragKind::Orbit => cam.orbit(ds.x, ds.y),
                DragKind::Pan => {
                    // o alvo anda com o cursor (deslocamento em px do canvas, no plano da tela)
                    let dc = xf.delta_to_canvas(ds);
                    cam.pan(dc.x, dc.y, canvas.1 as f32);
                }
                _ => cam.zoom((ds.y * 0.01).exp()),
            }
            d.cur.camera = layer_from_orbit(&cam);
        }
        DragKind::PoseRing | DragKind::PoseIk => {
            let (Some(skin), Some(b)) = (model.skin.as_ref(), d.bone) else { return };
            let lp = to_layer(place, xf.to_canvas(pos)) * d.scale;
            if d.kind == DragKind::PoseRing {
                if let Some(DragDelta::Rotate { axis, angle }) = d.gizmo.as_mut().and_then(|g| g.update(&d.view, lp)) {
                    let a = if shift {
                        let step = gizmo_apply::ROTATE_SNAP_DEG.to_radians();
                        (angle / step).round() * step
                    } else {
                        angle
                    };
                    // eixo do mundo → espaço do modelo (a camada pode estar girada)
                    let local = aiai_3d_model::camera::rotation_quat(d.cur.transform.rot_deg).inverse() * axis;
                    let w = glam::Quat::from_axis_angle(local.normalize_or(glam::Vec3::Y), a);
                    d.cur.pose = Some(mpose::rotate_bone_world(skin, &d.start_pose, b, w));
                }
            } else if let Some((o, n)) = d.ik_plane {
                let (ro, rd) = d.view.ray(lp);
                if let Some(p) = aiai_3d_model::gizmo::ray_plane(ro, rd, o, n) {
                    let t = aiai_3d_model::camera::model_matrix(&d.cur.transform).inverse().transform_point3(p);
                    if let Some(np) = mpose::solve_ik(skin, &d.start_pose, b, t) {
                        d.cur.pose = Some(np);
                    }
                }
            }
        }
    }
}

/// Soltar: um comando (um passo de undo) com o que mudou; chave onde a propriedade já é animada.
pub(crate) fn release(env: &mut Env) {
    let Some(d) = env.ed.m3d.drag.take() else { return };
    if d.cur == d.start {
        return;
    }
    let posing = matches!(d.kind, DragKind::PoseRing | DragKind::PoseIk);
    let cmd = if posing {
        aiai_tools::model3d::pose_command(&env.core.project, &d.layer, d.frame, d.cur.pose.as_ref().unwrap_or(&Pose3d::default()), false)
    } else {
        aiai_tools::model3d::state_command(&env.core.project, &d.layer, d.frame, &d.cur, false)
    };
    match cmd {
        Ok(Some(cmd)) => {
            let label = match d.kind {
                DragKind::Gizmo => "Camada 3D: transformar",
                DragKind::PoseRing | DragKind::PoseIk => "Camada 3D: pose",
                _ => "Camada 3D: câmera",
            };
            let cmd = Command::Batch { commands: vec![cmd], label: Some(label.into()) };
            if env.ed.apply(env.core, cmd) {
                env.ed.m3d.hold = Some((env.core.change_counter, Instant::now()));
            }
        }
        Ok(None) => {}
        Err(e) => env.ed.notify(e.to_string()),
    }
}

/// Esc durante o arrasto: volta ao estado de antes, sem comando.
pub(crate) fn cancel(env: &mut Env) {
    env.ed.m3d.drag = None;
    env.ed.m3d.live_state = None;
}

// ======================================================================================================= prévia ao vivo

fn split_key(env: &Env, d: &Drag) -> (u64, NodeId, u32, NodeId, bool, bool) {
    (env.core.change_counter, d.cut.clone(), d.frame, d.layer.clone(), env.ed.show_guides, env.ed.show_reference)
}

fn to_color_image(img: &aiai_paint::Image) -> egui::ColorImage {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    egui::ColorImage { size: [img.width as usize, img.height as usize], source_size: egui::vec2(img.width as f32, img.height as f32), pixels: img.data.iter().map(|p| Color32::from_rgba_premultiplied(b(p[0]), b(p[1]), b(p[2]), b(p[3]))).collect() }
}

/// Pede as imagens de baixo/cima (thread) e a prévia do modelo (GPU, ou CPU sem GPU), uma de cada vez; recolhe o que
/// ficou pronto. Chamado a cada quadro pelo `paint_live`.
fn pump(env: &mut Env, ctx: &egui::Context) {
    // imagens sem a camada 3D
    if let Some(d) = env.ed.m3d.drag.as_ref() {
        let key = split_key(env, d);
        let have = env.ed.m3d.split.as_ref().is_some_and(|s| s.key == key) || env.ed.m3d.split_pending.as_ref().is_some_and(|(k, _)| *k == key);
        if !have {
            let project = env.core.project.clone();
            let blobs = env.core.blobs.clone();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let k2 = key.clone();
            let c2 = ctx.clone();
            let ok = std::thread::Builder::new()
                .name("aiai-3d-split".into())
                .spawn(move || {
                    let (_, cut, frame, layer, guides, reference) = k2;
                    let mut all = Vec::new();
                    super::live::ordered_leaves(&project, &cut, &mut all);
                    let r = match all.iter().position(|(id, _)| *id == layer) {
                        None => Err("a camada 3D não está no corte".to_string()),
                        Some(pos) => {
                            let ids = |r: std::ops::Range<usize>| -> Vec<NodeId> { all[r].iter().filter(|(_, l)| l.visible).map(|(id, _)| id.clone()).collect() };
                            let render = |ids: Vec<NodeId>| -> Result<egui::ColorImage, String> {
                                if ids.is_empty() {
                                    let (w, h) = cmds::canvas_size(&project, &cut);
                                    return Ok(egui::ColorImage::new([w as usize, h as usize], vec![Color32::TRANSPARENT; (w * h) as usize]));
                                }
                                let opts = aiai_render::RenderOptions { include_guides: guides, include_reference: reference, only_layers: Some(ids), background: None, camera: false, layer_motion: true };
                                aiai_render::render_frame(&project, &blobs, &cut, frame, &opts).map(|o| to_color_image(&o.image)).map_err(|e| e.to_string())
                            };
                            render(ids(0..pos)).and_then(|b| render(ids(pos + 1..all.len())).map(|a| (b, a)))
                        }
                    };
                    let _ = tx.send(r);
                    c2.request_repaint();
                })
                .is_ok();
            if ok {
                env.ed.m3d.split_pending = Some((key, rx));
            }
        }
    }
    if let Some((key, rx)) = &env.ed.m3d.split_pending {
        if let Ok(r) = rx.try_recv() {
            let key = key.clone();
            env.ed.m3d.split_pending = None;
            match r {
                Ok((b, a)) => {
                    env.ed.m3d.split = Some(Split { key, below: ctx.load_texture("m3d-below", b, TextureOptions::LINEAR), above: ctx.load_texture("m3d-above", a, TextureOptions::LINEAR) });
                }
                Err(e) => env.ed.notify(e),
            }
        }
    }
    // prévia do modelo em resolução reduzida
    if let Some((st, size, rx)) = &env.ed.m3d.live_pending {
        if let Ok(r) = rx.try_recv() {
            let (st, size) = (st.clone(), *size);
            env.ed.m3d.live_pending = None;
            if let Ok(rgba) = r {
                let img = egui::ColorImage { size: [size.0 as usize, size.1 as usize], source_size: egui::vec2(size.0 as f32, size.1 as f32), pixels: rgba.chunks_exact(4).map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])).collect() };
                match &mut env.ed.m3d.live_tex {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => env.ed.m3d.live_tex = Some(ctx.load_texture("m3d-live", img, TextureOptions::LINEAR)),
                }
                env.ed.m3d.live_state = Some(st);
                env.ed.m3d.live_size = size;
            }
        }
    }
    let Some(d) = env.ed.m3d.drag.as_ref() else { return };
    if env.ed.m3d.live_pending.is_some() || env.ed.m3d.live_state.as_ref() == Some(&d.cur) {
        return;
    }
    let Ok(model) = aiai_3d_model::load_posed(&env.core.blobs, &d.cur.model_hash, d.cur.pose.as_ref()) else { return };
    let (cw, ch) = cmds::canvas_size(&env.core.project, &d.cut);
    let r = ((cw as f32 * ch as f32 / 550_000.0).sqrt().ceil() as u32).max(1);
    let size = (cw.div_ceil(r).max(1), ch.div_ceil(r).max(1));
    let st = d.cur.clone();
    let rx = match GPU.get() {
        Some(g) => g.request(mpose::posed_key(&st.model_hash, st.pose.as_ref()), model, st.clone(), size.0, size.1),
        None => {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let (m2, s2, c2) = (model, st.clone(), ctx.clone());
            let _ = std::thread::Builder::new().name("aiai-3d-cpu".into()).spawn(move || {
                let _ = tx.send(Ok(to_u8(&raster::render_cpu(&m2, &s2, size.0, size.1))));
                c2.request_repaint();
            });
            Some(rx)
        }
    };
    if let Some(rx) = rx {
        env.ed.m3d.live_pending = Some((st, size, rx));
    }
    ctx.request_repaint_after(Duration::from_millis(16));
}

/// Desenha a prévia no lugar do quadro durante o arrasto (e até o render novo chegar depois de soltar): abaixo, o modelo
/// na posição em edição (com a opacidade da camada) e acima. `true` = desenhou (o canvas não desenha o quadro normal).
pub(crate) fn paint_live(env: &mut Env, painter: &egui::Painter, xf: &super::editor::Xf, cut: &NodeId, shown_counter: Option<u64>) -> bool {
    let ctx = painter.ctx().clone();
    pump(env, &ctx);
    if let Some((c, t)) = env.ed.m3d.hold {
        if shown_counter.is_some_and(|s| s >= c) || t.elapsed() > Duration::from_secs(5) {
            env.ed.m3d.hold = None;
            env.ed.m3d.live_state = None;
        }
    }
    let active = env.ed.m3d.drag.is_some() || env.ed.m3d.hold.is_some();
    let (Some(live), true) = (env.ed.m3d.live_tex.as_ref(), active) else { return false };
    if env.ed.m3d.live_state.is_none() {
        return false;
    }
    let (layer, frame) = match (&env.ed.m3d.drag, env.ed.layer.clone()) {
        (Some(d), _) => (d.layer.clone(), d.frame),
        (None, Some(l)) => (l, env.ed.frame),
        _ => return false,
    };
    if env.core.project.cut_of(&layer).as_ref() != Some(cut) {
        return false;
    }
    let (cw, ch) = cmds::canvas_size(&env.core.project, cut);
    let full = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
    let cs = [Vec2::new(0.0, 0.0), Vec2::new(cw as f32, 0.0), Vec2::new(cw as f32, ch as f32), Vec2::new(0.0, ch as f32)];
    let sp: [Pos2; 4] = std::array::from_fn(|i| xf.to_screen(cs[i]));
    let split = env.ed.m3d.split.as_ref().filter(|s| s.key.1 == *cut && s.key.3 == layer);
    let quad = |tex: egui::TextureId, pts: [Pos2; 4], tint: Color32| {
        let mut m = egui::Mesh::with_texture(tex);
        for (p, u) in pts.iter().zip(full.iter()) {
            m.vertices.push(egui::epaint::Vertex { pos: *p, uv: *u, color: tint });
        }
        m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        Shape::mesh(m)
    };
    match split {
        Some(s) => painter.add(quad(s.below.id(), sp, Color32::WHITE)),
        None => match &env.ed.shown {
            Some(sh) if sh.rendered.key.cut == *cut => {
                let uvm = sh.rendered.uv_max;
                let mut m = egui::Mesh::with_texture(sh.tex.id());
                let uv = [Pos2::new(0.0, 0.0), Pos2::new(uvm[0], 0.0), Pos2::new(uvm[0], uvm[1]), Pos2::new(0.0, uvm[1])];
                for (p, u) in sp.iter().zip(uv.iter()) {
                    m.vertices.push(egui::epaint::Vertex { pos: *p, uv: *u, color: Color32::WHITE });
                }
                m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                painter.add(Shape::mesh(m))
            }
            _ => painter.add(Shape::Noop),
        },
    };
    // o modelo: imagem do canvas (reduzida) levada pelo movimento 2D da camada
    let place = cmds::layer_placement(&env.core.project, cut, &layer, frame);
    let lp: [Pos2; 4] = std::array::from_fn(|i| xf.to_screen(to_canvas(place, glam::Vec2::new(cs[i].x, cs[i].y))));
    let o = cmds::layer_data(&env.core.project, &layer).map(|l| l.opacity).unwrap_or(1.0).clamp(0.0, 1.0);
    let a = (o * 255.0).round() as u8;
    painter.add(quad(live.id(), lp, Color32::from_rgba_premultiplied(a, a, a, a)));
    if let Some(s) = split {
        painter.add(quad(s.above.id(), sp, Color32::WHITE));
    }
    true
}

// ======================================================================================================= overlay

const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(232, 72, 72), Color32::from_rgb(96, 200, 88), Color32::from_rgb(72, 128, 240)];

/// Gizmo (com a ferramenta 3D e uma camada 3D ativa) e a dica de navegação.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &super::editor::Xf, cut: &NodeId) {
    if *env.tool != ToolId::Model3d {
        return;
    }
    let t = env.t;
    let scale = xf.scale.max(1e-3);
    let Some(c) = ctx(env, scale) else {
        painter.text(rect.left_top() + egui::vec2(10.0, 30.0), egui::Align2::LEFT_TOP, tr("m3d.need_layer"), egui::FontId::proportional(12.0), t.text_dim);
        return;
    };
    if c.cut != *cut {
        return;
    }
    if env.ed.m3d.mode == GizmoModeUi::Pose {
        paint_pose(env, painter, rect, xf, &c, scale);
        return;
    }
    let mode = env.ed.m3d.mode.gizmo();
    let f = gizmo_apply::frame_for(mode, &c.state.transform, c.pivot, &c.view);
    let scr = |w: glam::Vec3| -> Option<Pos2> { c.view.project(w).map(|(p, _)| xf.to_screen(to_canvas(c.place, p / scale))) };
    // alça sob o cursor (sem arrasto)
    if env.ed.m3d.drag.is_none() {
        env.ed.m3d.hover = env.ed.cursor.filter(|p| rect.contains(*p)).and_then(|p| hit_test(mode, &f, &c.view, to_layer(c.place, xf.to_canvas(p)) * scale));
    }
    let active_handle = env.ed.m3d.drag.as_ref().and_then(|d| d.gizmo.map(|g| g.handle)).or(env.ed.m3d.hover);
    let col = |a: Axis3, h: Handle| -> Color32 { if active_handle == Some(h) { Color32::from_rgb(255, 214, 64) } else { AXIS_COLORS[a.index()] } };
    let line = |pts: &[Pos2], c32: Color32, w: f32| {
        painter.add(Shape::line(pts.to_vec(), Stroke::new(w + 2.0, Color32::from_black_alpha(110))));
        painter.add(Shape::line(pts.to_vec(), Stroke::new(w, c32)));
    };
    match mode {
        GizmoMode::Rotate => {
            for a in Axis3::ALL {
                let pts: Vec<Pos2> = f.ring_points(a, 72).into_iter().filter_map(scr).collect();
                if pts.len() > 2 {
                    line(&pts, col(a, Handle::Ring(a)), 2.0);
                }
            }
        }
        _ => {
            for a in Axis3::ALL {
                let (s, e) = f.axis_segment(a);
                if let (Some(ps), Some(pe)) = (scr(s), scr(e)) {
                    line(&[ps, pe], col(a, Handle::Axis(a)), 2.5);
                    if mode == GizmoMode::Scale {
                        painter.rect_filled(Rect::from_center_size(pe, egui::vec2(9.0, 9.0)), 1.0, col(a, Handle::Axis(a)));
                    } else {
                        painter.circle_filled(pe, 4.5, col(a, Handle::Axis(a)));
                    }
                }
                let q = f.plane_quad(a);
                let pq: Vec<Pos2> = q.iter().filter_map(|w| scr(*w)).collect();
                if pq.len() == 4 {
                    let hot = active_handle == Some(Handle::Plane(a));
                    let c32 = AXIS_COLORS[a.index()];
                    let fill = if hot { Color32::from_rgba_unmultiplied(255, 214, 64, 120) } else { Color32::from_rgba_unmultiplied(c32.r(), c32.g(), c32.b(), 60) };
                    painter.add(Shape::convex_polygon(pq, fill, Stroke::new(1.0, c32)));
                }
            }
            if let Some(o) = scr(f.origin) {
                let hot = active_handle == Some(Handle::Center);
                painter.circle(o, 6.0, if hot { Color32::from_rgb(255, 214, 64) } else { Color32::from_white_alpha(220) }, Stroke::new(1.5, Color32::from_black_alpha(160)));
            }
        }
    }
    // dica
    let hint = format!("{} — {}", tr(match env.ed.m3d.mode {
        GizmoModeUi::Translate => "m3d.mode.translate",
        GizmoModeUi::Rotate => "m3d.mode.rotate",
        GizmoModeUi::Scale => "m3d.mode.scale",
        GizmoModeUi::Pose => "m3d.mode.pose",
    }), tr("m3d.hint"));
    hint_box(painter, rect, &hint, t.text);
}

fn hint_box(painter: &egui::Painter, rect: Rect, hint: &str, ink: Color32) {
    let pos = rect.left_bottom() + egui::vec2(10.0, -26.0);
    let galley = painter.layout_no_wrap(hint.to_owned(), egui::FontId::proportional(11.5), ink);
    painter.rect_filled(Rect::from_min_size(pos - egui::vec2(4.0, 2.0), galley.size() + egui::vec2(8.0, 4.0)), 3.0, Color32::from_black_alpha(140));
    painter.galley(pos, galley, ink);
}

// ======================================================================================================= pose

/// Esqueleto da camada ativa no quadro: modelo (com pele), pose avaliada, juntas posáveis no mundo e projetadas.
struct PoseView {
    model: Arc<Model>,
    pose: Pose3d,
    /// Posição de mundo de cada osso (todos), já com a transformação da camada.
    world: Vec<glam::Vec3>,
    bones: Vec<usize>,
}

impl PoseView {
    fn skin(&self) -> &aiai_3d_model::Skin {
        self.model.skin.as_ref().expect("PoseView só existe com pele")
    }

    /// Junta posável mais perto do cursor (px da visão), dentro de 10 px.
    fn pick(&self, view: &CamView, lp: glam::Vec2) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for &b in &self.bones {
            if let Some((p, _)) = view.project(self.world[b]) {
                let d = p.distance(lp);
                if d <= 10.0 && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, b));
                }
            }
        }
        best.map(|(_, b)| b)
    }

    /// Pai posável mais próximo (sobe a cadeia até achar um osso da lista).
    fn shown_parent(&self, b: usize) -> Option<usize> {
        let skin = self.skin();
        let mut cur = skin.bones[b].parent;
        while let Some(p) = cur {
            if self.bones.contains(&p) {
                return Some(p);
            }
            cur = skin.bones[p].parent;
        }
        None
    }
}

fn pose_view(env: &Env, c: &Ctx3) -> Option<PoseView> {
    let model = aiai_3d_model::load_model(&env.core.blobs, &c.state.model_hash).ok()?;
    let skin = model.skin.as_ref()?;
    let pose = c.state.pose.clone().unwrap_or_default();
    let g = mpose::globals(skin, &pose);
    let mm = aiai_3d_model::camera::model_matrix(&c.state.transform);
    let world = g.iter().map(|x| mm.transform_point3(x.pos)).collect();
    let bones = mpose::pose_bones(skin);
    Some(PoseView { model, pose, world, bones })
}

/// Juntas, ossos e os anéis do osso escolhido (modo Pose).
fn paint_pose(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &super::editor::Xf, c: &Ctx3, scale: f32) {
    let t = env.t;
    let Some(pv) = pose_view(env, c) else {
        hint_box(painter, rect, tr("m3d.pose.no_skin"), t.text);
        return;
    };
    let scr = |w: glam::Vec3| -> Option<Pos2> { c.view.project(w).map(|(p, _)| xf.to_screen(to_canvas(c.place, p / scale))) };
    let cursor_lp = env.ed.cursor.filter(|p| rect.contains(*p)).map(|p| to_layer(c.place, xf.to_canvas(p)) * scale);
    let dragging = env.ed.m3d.drag.is_some();
    if !dragging {
        env.ed.m3d.hover_joint = cursor_lp.and_then(|lp| pv.pick(&c.view, lp));
    }
    let sel = env.ed.m3d.pose_bone.as_deref().and_then(|n| pv.skin().index(n));
    // ossos (linhas da junta ao pai posável)
    for &b in &pv.bones {
        let Some(parent) = pv.shown_parent(b) else { continue };
        if let (Some(a), Some(bp)) = (scr(pv.world[b]), scr(pv.world[parent])) {
            painter.line_segment([bp, a], Stroke::new(3.0, Color32::from_black_alpha(120)));
            painter.line_segment([bp, a], Stroke::new(1.5, Color32::from_rgb(140, 220, 255)));
        }
    }
    for &b in &pv.bones {
        let Some(p) = scr(pv.world[b]) else { continue };
        let ik_end = !env.ed.m3d.no_ik && mpose::IK_ENDS.contains(&pv.skin().bones[b].name.as_str());
        let (r, fill) = if Some(b) == sel {
            (6.5, Color32::from_rgb(255, 214, 64))
        } else if Some(b) == env.ed.m3d.hover_joint {
            (5.5, Color32::WHITE)
        } else if ik_end {
            (5.0, Color32::from_rgb(255, 150, 90))
        } else {
            (4.0, Color32::from_rgb(140, 220, 255))
        };
        painter.circle(p, r, fill, Stroke::new(1.3, Color32::from_black_alpha(170)));
    }
    // anéis do osso escolhido
    if let Some(b) = sel {
        let f = GizmoFrame::new(pv.world[b], glam::Quat::IDENTITY, &c.view);
        if !dragging {
            env.ed.m3d.hover = cursor_lp.and_then(|lp| hit_test(GizmoMode::Rotate, &f, &c.view, lp));
        }
        let active = env.ed.m3d.drag.as_ref().and_then(|d| d.gizmo.map(|g| g.handle)).or(env.ed.m3d.hover);
        for a in Axis3::ALL {
            let pts: Vec<Pos2> = f.ring_points(a, 72).into_iter().filter_map(scr).collect();
            if pts.len() > 2 {
                let col = if active == Some(Handle::Ring(a)) { Color32::from_rgb(255, 214, 64) } else { AXIS_COLORS[a.index()] };
                painter.add(Shape::line(pts.clone(), Stroke::new(3.5, Color32::from_black_alpha(110))));
                painter.add(Shape::line(pts, Stroke::new(2.0, col)));
            }
        }
        if let Some(p) = scr(pv.world[b]) {
            let name = &pv.skin().bones[b].name;
            let galley = painter.layout_no_wrap(name.clone(), egui::FontId::proportional(11.0), t.text);
            let at = p + egui::vec2(10.0, -18.0);
            painter.rect_filled(Rect::from_min_size(at - egui::vec2(3.0, 1.0), galley.size() + egui::vec2(6.0, 2.0)), 3.0, Color32::from_black_alpha(150));
            painter.galley(at, galley, t.text);
        }
    }
    hint_box(painter, rect, &format!("{} — {}", tr("m3d.mode.pose"), tr("m3d.pose.hint")), t.text);
}

/// Atalhos da ferramenta 3D: Esc cancela o arrasto; com a ferramenta ativa, 1/2/3 = mover/girar/escalar.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    if env.ed.m3d.drag.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel(env);
    }
    if *env.tool == ToolId::Model3d {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Num1) && i.modifiers.is_none() {
                env.ed.m3d.mode = GizmoModeUi::Translate;
            }
            if i.key_pressed(egui::Key::Num2) && i.modifiers.is_none() {
                env.ed.m3d.mode = GizmoModeUi::Rotate;
            }
            if i.key_pressed(egui::Key::Num3) && i.modifiers.is_none() {
                env.ed.m3d.mode = GizmoModeUi::Scale;
            }
            if i.key_pressed(egui::Key::Num4) && i.modifiers.is_none() {
                env.ed.m3d.mode = GizmoModeUi::Pose;
            }
        });
    }
}

// ======================================================================================================= interface

/// Opções da ferramenta (barra do canvas e Propriedades): modo do gizmo e enquadrar.
pub(crate) fn tool_options(env: &mut Env, ui: &mut Ui, compact: bool) {
    let t = env.t;
    ui.horizontal_wrapped(|ui| {
        for (m, key) in [(GizmoModeUi::Translate, "m3d.mode.translate"), (GizmoModeUi::Rotate, "m3d.mode.rotate"), (GizmoModeUi::Scale, "m3d.mode.scale"), (GizmoModeUi::Pose, "m3d.mode.pose")] {
            let r = ui.selectable_label(env.ed.m3d.mode == m, tr(key));
            env.note(format!("m3d:mode.{}", key.rsplit('.').next().unwrap_or("")), r.rect);
            if r.clicked() {
                env.ed.m3d.mode = m;
            }
        }
        let fr = ui.button(tr("m3d.frame_model"));
        env.note("m3d:frame_model", fr.rect);
        if fr.clicked() {
            reset_camera(env);
        }
    });
    if !compact {
        dim_label(ui, &t, tr("m3d.hint"));
    }
}

fn run_model3d(env: &mut Env, params: serde_json::Value) {
    let r = super::actions::run(env.core, "MODEL3D", params);
    if let Err(e) = r {
        env.ed.notify(e);
    }
}

/// Reenquadra o modelo na câmera da camada (mantém o ângulo).
pub(crate) fn reset_camera(env: &mut Env) {
    let Some((layer, ..)) = active(env) else {
        env.ed.notify(tr("m3d.need_layer"));
        return;
    };
    let frame = env.ed.frame;
    run_model3d(env, json!({ "op": "camera", "layer": layer, "frame": frame, "frame_model": true }));
}

/// Seção 3D das Propriedades (camada 3D ativa): modelo, licença, aparência, referência, transformação e câmera.
pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let Some((layer, l, p)) = active(env) else { return };
    let t = env.t;
    let frame = env.ed.frame;
    let st = p.eval(&l.keys, frame as f32);
    section(ui, &t, "m3d.section");
    let info = env.core.project.node(&p.asset).and_then(|n| match &n.data {
        NodeKind::Asset(a) => Some((n.name.clone(), a.params.get("model3d").and_then(|v| serde_json::from_value::<aiai_3d_model::ModelInfo>(v.clone()).ok()), a.source.clone())),
        _ => None,
    });
    egui::Grid::new(("m3d_info", &layer)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        if let Some((name, info, source)) = &info {
            dim_label(ui, &t, tr("m3d.model"));
            ui.label(name);
            ui.end_row();
            if let Some(i) = info {
                dim_label(ui, &t, tr("m3d.file"));
                ui.label(format!("{} ({})", i.file_name, i.format));
                ui.end_row();
                dim_label(ui, &t, tr("m3d.geometry"));
                ui.label(format!("{} {} · {} {} · {} {}", i.triangles, tr("m3d.tris"), i.parts, tr("m3d.parts"), i.materials, tr("m3d.materials")));
                ui.end_row();
                dim_label(ui, &t, tr("m3d.license"));
                ui.label(egui::RichText::new(i.license.summary()).size(11.5)).on_hover_text(source.as_str());
                ui.end_row();
            }
        }
    });
    // pose logo no alto (Manequim e modelos com pele): é o que se mexe a todo momento
    pose_properties(env, ui, &layer, &l, &p, frame);
    // aparência
    let mut mode = p.render.mode;
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("m3d.render_mode"));
        let r = egui::ComboBox::from_id_salt(("m3d_mode", &layer)).selected_text(tr(mode_key(mode))).show_ui(ui, |ui| {
            for m in RenderMode3d::ALL {
                ui.selectable_value(&mut mode, m, tr(mode_key(m)));
            }
        });
        env.note("m3d:render_mode", r.response.rect);
    });
    if mode != p.render.mode {
        run_model3d(env, json!({ "op": "mode", "layer": layer, "mode": mode }));
    }
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("m3d.outline"));
        if let Some(v) = super::panels::commit_on_release(ui, egui::Id::new(("m3d_outline", &layer)), p.render.outline_px, |ui, v| ui.add(egui::DragValue::new(v).range(0.0..=64.0).speed(0.1).max_decimals(1).suffix(" px"))) {
            run_model3d(env, json!({ "op": "mode", "layer": layer, "outline_px": v }));
        }
        let oc = parse_hex_rgb(&p.render.outline_color).unwrap_or([0.1; 3]);
        let mut c32 = Color32::from_rgb((oc[0] * 255.0) as u8, (oc[1] * 255.0) as u8, (oc[2] * 255.0) as u8);
        let r = ui.color_edit_button_srgba(&mut c32);
        if r.changed() {
            run_model3d(env, json!({ "op": "mode", "layer": layer, "outline_color": hex_rgb([c32.r() as f32 / 255.0, c32.g() as f32 / 255.0, c32.b() as f32 / 255.0]) }));
        }
    });
    ui.horizontal(|ui| {
        let mut one = p.render.color.is_some();
        let r = ui.checkbox(&mut one, tr("m3d.single_color"));
        env.note("m3d:single_color", r.rect);
        if r.changed() {
            run_model3d(env, json!({ "op": "mode", "layer": layer, "color": if one { "#8ab4ff" } else { "" } }));
        }
        if let Some(c) = p.render.color.as_deref().and_then(|c| parse_hex_rgb(c).ok()) {
            let mut c32 = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                run_model3d(env, json!({ "op": "mode", "layer": layer, "color": hex_rgb([c32.r() as f32 / 255.0, c32.g() as f32 / 255.0, c32.b() as f32 / 255.0]) }));
            }
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("m3d.opacity"));
        if let Some(v) = super::panels::commit_on_release(ui, egui::Id::new(("m3d_opacity", &layer)), l.opacity, |ui, v| ui.add(egui::Slider::new(v, 0.0..=1.0).max_decimals(2))) {
            run_model3d(env, json!({ "op": "mode", "layer": layer, "opacity": v }));
        }
    });
    let mut reference = p.render.reference;
    let r = ui.checkbox(&mut reference, tr("m3d.reference")).on_hover_text(tr("m3d.reference.tip"));
    env.note("m3d:reference", r.rect);
    if reference != p.render.reference {
        run_model3d(env, json!({ "op": "mode", "layer": layer, "reference": reference }));
    }
    // transformação e câmera (valores no quadro atual; chave onde já é animado)
    ui.add_space(2.0);
    egui::Grid::new(("m3d_xf", &layer)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let vec3_row = |ui: &mut Ui, env: &mut Env, key: &str, field: &str, op: &str, v: [f32; 3], speed: f64| {
            dim_label(ui, &t, tr(key));
            ui.horizontal(|ui| {
                for axis in 0..3 {
                    if let Some(x) = super::panels::commit_on_release(ui, egui::Id::new((field, &layer, axis)), v[axis], |ui, x| ui.add(egui::DragValue::new(x).speed(speed).max_decimals(3))) {
                        let mut nv = v;
                        nv[axis] = x;
                        run_model3d(env, json!({ "op": op, "layer": layer, "frame": frame, field: nv }));
                    }
                }
            });
            ui.end_row();
        };
        vec3_row(ui, env, "m3d.pos", "pos", "transform", st.transform.pos, 0.01);
        vec3_row(ui, env, "m3d.rot", "rot_deg", "transform", st.transform.rot_deg, 0.5);
        vec3_row(ui, env, "m3d.scale", "scale", "transform", st.transform.scale, 0.01);
        let num = |ui: &mut Ui, env: &mut Env, key: &str, field: &str, v: f32, speed: f64, range: std::ops::RangeInclusive<f32>| {
            dim_label(ui, &t, tr(key));
            if let Some(x) = super::panels::commit_on_release(ui, egui::Id::new((field, &layer)), v, |ui, x| ui.add(egui::DragValue::new(x).speed(speed).range(range).max_decimals(2))) {
                run_model3d(env, json!({ "op": "camera", "layer": layer, "frame": frame, field: x }));
            }
            ui.end_row();
        };
        num(ui, env, "m3d.yaw", "yaw_deg", st.camera.yaw_deg, 0.5, -36000.0..=36000.0);
        num(ui, env, "m3d.pitch", "pitch_deg", st.camera.pitch_deg, 0.5, -89.9..=89.9);
        num(ui, env, "m3d.distance", "distance", st.camera.distance, 0.02, 0.001..=100000.0);
        num(ui, env, "m3d.fov", "fov_deg", st.camera.fov_deg, 0.2, 1.0..=150.0);
    });
    ui.horizontal(|ui| {
        let r = ui.button(tr("m3d.reset_camera"));
        env.note("m3d:reset_camera", r.rect);
        if r.clicked() {
            reset_camera(env);
        }
        let k = ui.button(tr("m3d.key_here")).on_hover_text(tr("m3d.key_here.tip"));
        env.note("m3d:key_here", k.rect);
        if k.clicked() {
            match force_key_all(env, &layer, frame, &st) {
                Ok(Some(cmd)) => {
                    let _ = env.ed.apply(env.core, cmd);
                }
                Ok(None) => {}
                Err(e) => env.ed.notify(e),
            }
        }
    });
}

/// Seção Pose das Propriedades (camada com esqueleto): poses prontas, espelhar, repouso, chave, IK, o osso escolhido e as
/// poses do usuário.
fn pose_properties(env: &mut Env, ui: &mut Ui, layer: &NodeId, l: &LayerData, p: &Model3dParams, frame: u32) {
    let Ok(model) = aiai_3d_model::load_model(&env.core.blobs, &p.model_hash) else { return };
    let Some(skin) = model.skin.as_ref() else { return };
    let t = env.t;
    let cur = p.pose.as_ref().map(|pp| pp.eval(&l.keys, frame as f32)).unwrap_or_default();
    section(ui, &t, "m3d.pose.section");
    let pose = |env: &mut Env, extra: serde_json::Value| {
        let mut req = json!({ "op": "pose", "layer": layer, "frame": frame });
        if let (Some(o), Some(e)) = (req.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        run_model3d(env, req);
    };
    ui.horizontal_wrapped(|ui| {
        for k in mpose::PRESETS {
            let r = ui.button(tr(&format!("m3d.pose.preset.{k}")));
            env.note(format!("m3d:pose.{k}"), r.rect);
            if r.clicked() {
                pose(env, json!({ "preset": k }));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        let r = ui.button(tr("m3d.pose.mirror"));
        env.note("m3d:pose.mirror", r.rect);
        if r.clicked() {
            pose(env, json!({ "mirror": true }));
        }
        let r = ui.button(tr("m3d.pose.key")).on_hover_text(tr("m3d.pose.key.tip"));
        env.note("m3d:pose.key", r.rect);
        if r.clicked() {
            pose(env, json!({ "key": true }));
        }
        let mut ik = !env.ed.m3d.no_ik;
        let r = ui.checkbox(&mut ik, tr("m3d.pose.ik")).on_hover_text(tr("m3d.pose.ik.tip"));
        env.note("m3d:pose.ik", r.rect);
        env.ed.m3d.no_ik = !ik;
    });
    let keyed = l.keys.frames(POSE_KEY);
    if !keyed.is_empty() {
        dim_label(ui, &t, &format!("{}: {}", tr("m3d.pose.keys"), keyed.iter().map(|f| (f + 1).to_string()).collect::<Vec<_>>().join(", ")));
    }
    // osso escolhido: rotação (delta sobre o repouso, graus nos eixos do mundo)
    match env.ed.m3d.pose_bone.clone().filter(|n| skin.index(n).is_some()) {
        Some(name) => {
            let e = aiai_3d_model::camera::rotation_deg(glam::Quat::from_array(cur.rotation(&name)));
            ui.horizontal(|ui| {
                dim_label(ui, &t, &format!("{} {name}", tr("m3d.pose.bone")));
                for axis in 0..3 {
                    if let Some(x) = super::panels::commit_on_release(ui, egui::Id::new(("m3d_bone", layer, &name, axis)), e[axis], |ui, x| ui.add(egui::DragValue::new(x).speed(0.5).max_decimals(1).suffix("°"))) {
                        let mut ne = e;
                        ne[axis] = x;
                        pose(env, json!({ "bones": { name.clone(): ne } }));
                    }
                }
                let r = ui.small_button(tr("m3d.pose.bone_reset"));
                if r.clicked() {
                    pose(env, json!({ "bones": { name.clone(): [0.0, 0.0, 0.0] } }));
                }
            });
        }
        None => dim_label(ui, &t, tr("m3d.pose.pick_bone")),
    }
    // poses do usuário
    if !env.ed.m3d.user_files {
        dim_label(ui, &t, tr("m3d.pose.no_session"));
        return;
    }
    if env.ed.m3d.user_poses.is_none() {
        env.ed.m3d.user_poses = Some(list_user_poses());
    }
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut env.ed.m3d.pose_name).hint_text(tr("m3d.pose.name")).desired_width(140.0));
        env.note("m3d:pose.name", r.rect);
        let s = ui.button(tr("m3d.pose.save"));
        env.note("m3d:pose.save", s.rect);
        if s.clicked() {
            let name = env.ed.m3d.pose_name.trim().to_owned();
            let name = if name.is_empty() { format!("{} {}", tr("m3d.pose.section"), frame + 1) } else { name };
            match save_user_pose(&name, &cur) {
                Ok(path) => {
                    env.ed.notify(format!("{}: {}", tr("m3d.pose.saved"), path.display()));
                    env.ed.m3d.user_poses = None;
                    env.ed.m3d.pose_name.clear();
                }
                Err(e) => env.ed.notify(e),
            }
        }
    });
    let list = env.ed.m3d.user_poses.clone().unwrap_or_default();
    if list.is_empty() {
        dim_label(ui, &t, tr("m3d.pose.none_saved"));
    }
    for (i, up) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(&up.name);
            let a = ui.small_button(tr("m3d.pose.apply"));
            env.note(format!("m3d:pose.user.{i}"), a.rect);
            if a.clicked() {
                pose(env, json!({ "pose": up.pose }));
            }
            if ui.small_button(tr("m3d.pose.trash")).on_hover_text(tr("m3d.pose.trash.tip")).clicked() {
                match trash_user_pose(&up.path) {
                    Ok(()) => env.ed.m3d.user_poses = None,
                    Err(e) => env.ed.notify(e),
                }
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------- poses do usuário

fn poses_dir() -> Option<std::path::PathBuf> {
    aiai_paint::userdirs::animeiai_dir().map(|d| d.join("Poses"))
}

/// Pose de um arquivo: envelope de recurso (`payload.pose` ou o próprio `payload`) ou a pose crua.
fn pose_of_json(v: &serde_json::Value) -> Option<(Option<String>, Pose3d)> {
    let name = v.get("name").and_then(|n| n.as_str()).map(str::to_owned);
    let body = v.get("payload").map(|p| p.get("pose").unwrap_or(p)).unwrap_or(v);
    let pose: Pose3d = serde_json::from_value(body.clone()).ok()?;
    pose.validate().ok()?;
    Some((name, pose))
}

fn list_user_poses() -> Vec<UserPose> {
    let Some(dir) = poses_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<UserPose> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("aiaipose") || x.eq_ignore_ascii_case("json")))
        .filter(|p| std::fs::metadata(p).map(|m| m.len() < 1 << 20).unwrap_or(false))
        .filter_map(|path| {
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            let (name, pose) = pose_of_json(&v)?;
            let name = name.unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            Some(UserPose { name, path, pose })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Grava a pose como `.aiaipose` (o envelope de recurso que a Biblioteca indexa; a pose vai em `payload.pose`).
fn save_user_pose(name: &str, pose: &Pose3d) -> Result<std::path::PathBuf, String> {
    let dir = poses_dir().ok_or_else(|| tr("m3d.pose.no_docs").to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect::<String>().trim().to_owned();
    let stem = if stem.is_empty() { "pose".to_owned() } else { stem };
    let path = dir.join(format!("{stem}.aiaipose"));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let id = format!("pose-{}", &BlobHash::of(format!("{stem}{now}").as_bytes()).0[..16]);
    let doc = json!({
        "format": "animeiai-asset",
        "format_version": 1,
        "min_reader": 1,
        "kind": "pose",
        "id": id,
        "name": name,
        "description": "pose do Manequim (camada 3D do AnimeIAI)",
        "app_version": format!("AnimeIAI {}", env!("CARGO_PKG_VERSION")),
        "tags": ["pose", "manequim"],
        "payload": { "pose": pose, "rig": "humanoide VRM" },
    });
    let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("aiaipose.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Manda a pose para `Poses\_Lixeira` (a mesma lixeira da Biblioteca; nada é apagado de vez).
fn trash_user_pose(path: &std::path::Path) -> Result<(), String> {
    let dir = path.parent().ok_or("pose sem pasta")?.join("_Lixeira");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().ok_or("pose sem nome")?;
    let mut dest = dir.join(name);
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("{} ({n}).aiaipose", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        n += 1;
    }
    std::fs::rename(path, &dest).map_err(|e| format!("{}: {e}", path.display()))
}

/// Cria o Manequim posável no corte (camada 3D em pé, no topo) e escolhe a camada, já no modo Pose.
pub(crate) fn add_mannequin(env: &mut Env, cut: &NodeId) {
    match super::actions::run(env.core, "MODEL3D", json!({ "op": "mannequin", "cut": cut })) {
        Ok(v) => {
            if let Some(l) = v.get("layer").and_then(|x| x.as_str()) {
                env.ed.layer = Some(Id(l.to_string()));
                env.ed.cel_number = None;
            }
            env.ed.show_reference = true;
            env.ed.m3d.mode = GizmoModeUi::Pose;
            *env.tool = ToolId::Model3d;
            env.ed.notify(tr("m3d.mannequin.added"));
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Chave em todas as propriedades (transformação e câmera) no quadro, com os valores atuais.
fn force_key_all(env: &Env, layer: &NodeId, frame: u32, st: &Model3dState) -> Result<Option<Command>, String> {
    let (l, _) = aiai_tools::model3d::layer_params(&env.core.project, layer).map_err(|e| e.to_string())?;
    let mut keys = l.keys.clone();
    let mut changed = serde_json::Map::new();
    for prop in aiai_core::model3d::KEY_PROPS {
        let Some(v) = aiai_core::model3d::state_prop(st, prop) else { continue };
        if keys.0.get(prop).is_some_and(|k| k.iter().any(|x| x.frame == frame && x.value == v)) {
            continue;
        }
        keys.set_key(prop, frame, v);
        changed.insert(prop.to_string(), json!(keys.0.get(prop)));
    }
    Ok((!changed.is_empty()).then(|| Command::SetProps { id: layer.clone(), name: None, data: json!({ "keys": changed }) }))
}

pub(crate) fn mode_key(m: RenderMode3d) -> &'static str {
    match m {
        RenderMode3d::Toon => "m3d.render.toon",
        RenderMode3d::Flat => "m3d.render.flat",
        RenderMode3d::Wire => "m3d.render.wire",
        RenderMode3d::Silhouette => "m3d.render.silhouette",
    }
}

/// Extensões que o canvas aceita soltar como modelo 3D (o .vrm chega ao importador e volta "em breve").
pub(crate) const DROP_EXTS: [&str; 4] = ["glb", "gltf", "obj", "vrm"];

/// Importa um arquivo de modelo para o corte (nova camada 3D de referência no topo) e seleciona a camada.
pub(crate) fn import_into(core: &mut CoreSession, ed: &mut super::editor::Editor, cut: &NodeId, path: &std::path::Path) -> Result<String, String> {
    let v = super::actions::run(core, "MODEL3D", json!({ "op": "import", "path": path.display().to_string(), "cut": cut }))?;
    if let Some(l) = v.get("layer").and_then(|x| x.as_str()) {
        ed.layer = Some(Id(l.to_string()));
        ed.cel_number = None;
    }
    ed.show_reference = true;
    let tris = v.pointer("/info/triangles").and_then(|x| x.as_u64()).unwrap_or(0);
    let lic = v.pointer("/info/license").and_then(|x| serde_json::from_value::<aiai_3d_model::LicenseInfo>(x.clone()).ok()).map(|l| l.summary()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(format!("{}: {stem} ({tris} {}; {lic})", tr("msg.imported"), tr("m3d.tris")))
}

/// Diálogo "Modelo 3D…" do menu "+" das Camadas.
pub(crate) fn import_dialog(env: &mut Env, cut: &NodeId) {
    let Some(path) = rfd::FileDialog::new().set_title(tr("m3d.import_title")).add_filter("glTF / OBJ", &["glb", "gltf", "obj"]).pick_file() else { return };
    match import_into(env.core, env.ed, cut, &path) {
        Ok(m) => env.ed.notify(m),
        Err(e) => env.ed.notify(e),
    }
}

/// Pede um quadro do modelo à GPU sem esperar (prévia da Biblioteca, fora do documento); `None` = sem serviço GPU (a
/// prévia cai para o rasterizador CPU). A resposta é RGBA 8 bits pré-multiplicado, `w`×`h`.
pub(crate) fn preview_request(hash: BlobHash, model: Arc<Model>, state: Model3dState, w: u32, h: u32) -> Option<crossbeam_channel::Receiver<Result<Vec<u8>, String>>> {
    GPU.get()?.request(hash, model, state, w, h)
}

/// Adaptador do serviço GPU da camada 3D (para a barra de status/telemetria); `None` = rasterizador CPU.
#[allow(dead_code)]
pub(crate) fn gpu_name() -> Option<String> {
    GPU.get().map(|g| g.adapter.clone())
}

//! Modo Escultura da Modelagem, vindo do ANIGO (`anigo-studio/src/modeling/sculpt.rs`, commit bc5e83af, mesmo autor):
//! 20 pincéis (desenhar, argila, alisar, achatar, inflar, beliscar, agarrar, máscara…), simetria, dyntopo, voxel remesh,
//! simetrizar e suavizar tudo, sobre o motor `aiai_3d_edit::sculpt` (copiado do `anigo-modeling`).
//!
//! Mudanças em relação ao ANIGO: a sessão dele (`ModelingSession` com checkpoint) virou o nosso `ModelingState`
//! (documento + `History` por snapshot: o traço guarda o documento de antes e só entra no desfazer se mudou algo);
//! sem o espelho incremental na GPU (`sculpt_mesh.rs`): o objeto sobe de novo a cada quadro do traço; textos pelo i18n.

use super::camera::OrbitCamera;
use super::ModelingState;
use crate::i18n::tr;
use aiai_3d_edit::bvh::TriBvh;
use aiai_3d_edit::doc::ModelDoc;
use aiai_3d_edit::sculpt::automask::AutoMask;
use aiai_3d_edit::sculpt::brush::{apply_dab, mask_clear_all, mask_invert_all, Brush, BrushKind, Dab, SculptContext};
use aiai_3d_edit::sculpt::dyntopo::{dyntopo_step, DyntopoParams};
use aiai_3d_edit::sculpt::stroke::{StrokeSample, StrokeState, StrokeUpdate};
use aiai_3d_edit::sculpt::symmetry::{symmetrize_mesh, Symmetry, SymmetrizeDirection};
use aiai_3d_edit::sculpt::voxel_remesh::{voxel_remesh_report, VoxelParams};
use aiai_3d_edit::{EditMesh, ModelError};
use glam::{Mat3, Mat4, Vec2, Vec3};

/// Os 20 pincéis, na ordem do painel.
pub(crate) const SCULPT_BRUSHES: [BrushKind; 20] = [
    BrushKind::Draw,
    BrushKind::Clay,
    BrushKind::ClayStrips,
    BrushKind::Smooth,
    BrushKind::Flatten,
    BrushKind::Inflate,
    BrushKind::Pinch,
    BrushKind::Grab,
    BrushKind::SnakeHook,
    BrushKind::Crease,
    BrushKind::MaskPaint,
    BrushKind::Fill,
    BrushKind::Scrape,
    BrushKind::Nudge,
    BrushKind::Layer,
    BrushKind::Elastic,
    BrushKind::MaskSmooth,
    BrushKind::MaskSharpen,
    BrushKind::MaskInvert,
    BrushKind::MaskClear,
];

/// Chave do pincel (i18n `sculpt.<chave>`, ponte).
pub(crate) fn brush_key(kind: BrushKind) -> &'static str {
    match kind {
        BrushKind::Draw => "draw",
        BrushKind::Clay => "clay",
        BrushKind::ClayStrips => "clay_strips",
        BrushKind::Smooth => "smooth",
        BrushKind::Flatten => "flatten",
        BrushKind::Fill => "fill",
        BrushKind::Scrape => "scrape",
        BrushKind::Pinch => "pinch",
        BrushKind::Inflate => "inflate",
        BrushKind::Grab => "grab",
        BrushKind::SnakeHook => "snake_hook",
        BrushKind::Nudge => "nudge",
        BrushKind::Crease => "crease",
        BrushKind::Layer => "layer",
        BrushKind::Elastic => "elastic",
        BrushKind::MaskPaint => "mask",
        BrushKind::MaskSmooth => "mask_smooth",
        BrushKind::MaskSharpen => "mask_sharpen",
        BrushKind::MaskInvert => "mask_invert",
        BrushKind::MaskClear => "mask_clear",
    }
}

pub(crate) fn brush_from_key(k: &str) -> Option<BrushKind> {
    SCULPT_BRUSHES.into_iter().find(|&b| brush_key(b) == k)
}

pub(crate) fn brush_label(kind: BrushKind) -> &'static str {
    tr(match kind {
        BrushKind::Draw => "sculpt.draw",
        BrushKind::Clay => "sculpt.clay",
        BrushKind::ClayStrips => "sculpt.clay_strips",
        BrushKind::Smooth => "sculpt.smooth",
        BrushKind::Flatten => "sculpt.flatten",
        BrushKind::Fill => "sculpt.fill",
        BrushKind::Scrape => "sculpt.scrape",
        BrushKind::Pinch => "sculpt.pinch",
        BrushKind::Inflate => "sculpt.inflate",
        BrushKind::Grab => "sculpt.grab",
        BrushKind::SnakeHook => "sculpt.snake_hook",
        BrushKind::Nudge => "sculpt.nudge",
        BrushKind::Crease => "sculpt.crease",
        BrushKind::Layer => "sculpt.layer",
        BrushKind::Elastic => "sculpt.elastic",
        BrushKind::MaskPaint => "sculpt.mask",
        BrushKind::MaskSmooth => "sculpt.mask_smooth",
        BrushKind::MaskSharpen => "sculpt.mask_sharpen",
        BrushKind::MaskInvert => "sculpt.mask_invert",
        BrushKind::MaskClear => "sculpt.mask_clear",
    })
}

pub(crate) const RADIUS_MIN_PX: f32 = 6.0;
pub(crate) const RADIUS_MAX_PX: f32 = 600.0;
pub(crate) const STRENGTH_MAX: f32 = 2.0;

/// Pincel com o raio em px de tela (o raio no objeto sai da profundidade).
#[derive(Debug, Clone)]
pub(crate) struct BrushSlot {
    pub brush: Brush,
    pub radius_px: f32,
}

impl BrushSlot {
    fn new(kind: BrushKind) -> Self {
        BrushSlot { brush: Brush::new(kind), radius_px: 56.0 }
    }
}

// ------------------------------------------------------------------ geometria

/// Escala média do objeto (raio de mundo → local).
pub(crate) fn mean_scale(model: Mat4) -> f32 {
    let (x, y, z) = (model.x_axis.truncate().length(), model.y_axis.truncate().length(), model.z_axis.truncate().length());
    let s = (x + y + z) / 3.0;
    if s.is_finite() && s > 1e-9 { s } else { 1.0 }
}

/// Tamanho de 1 px de tela em unidades de mundo à profundidade de `world_point`.
pub(crate) fn world_per_pixel(camera: &OrbitCamera, viewport_h_px: f32, world_point: Vec3) -> f32 {
    let h = viewport_h_px.max(1.0);
    let depth = if camera.orthographic { camera.distance } else { (world_point - camera.eye()).dot(camera.forward()).max(camera.near) };
    2.0 * depth * (camera.fov_y * 0.5).tan() / h
}

pub(crate) fn px_to_local_radius(camera: &OrbitCamera, viewport_h_px: f32, world_point: Vec3, px: f32, model: Mat4) -> f32 {
    (px * world_per_pixel(camera, viewport_h_px, world_point) / mean_scale(model)).max(1e-6)
}

pub(crate) fn ray_to_local(model: Mat4, origin: Vec3, dir: Vec3) -> (Vec3, Vec3) {
    let inv = model.inverse();
    (inv.transform_point3(origin), inv.transform_vector3(dir).normalize_or_zero())
}

pub(crate) fn normal_to_world(model: Mat4, n: Vec3) -> Vec3 {
    let m3 = Mat3::from_mat4(model);
    (m3.inverse().transpose() * n).normalize_or_zero()
}

/// Anel do pincel no plano tangente (local), levado ao mundo; `segments + 1` pontos.
pub(crate) fn ring_points_world(model: Mat4, center_local: Vec3, normal_local: Vec3, radius_local: f32, segments: usize) -> Vec<Vec3> {
    let n = normal_local.try_normalize().unwrap_or(Vec3::Y);
    let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = n.cross(helper).normalize_or_zero();
    let v = n.cross(u).normalize_or_zero();
    let segs = segments.max(8);
    (0..=segs)
        .map(|i| {
            let a = i as f32 / segs as f32 * std::f32::consts::TAU;
            model.transform_point3(center_local + (u * a.cos() + v * a.sin()) * radius_local)
        })
        .collect()
}

/// Voxel sugerido: ~1/64 da maior dimensão da malha.
pub(crate) fn suggest_voxel_size(mesh: &EditMesh) -> f32 {
    mesh.bounds().map(|(lo, hi)| ((hi - lo).max_element() / 64.0).max(1e-4)).unwrap_or(0.05)
}

/// Câmera + tamanho do viewport (px lógicos).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewInfo {
    pub camera: OrbitCamera,
    pub size: Vec2,
}

impl ViewInfo {
    pub(crate) fn ray(&self, px: Vec2) -> (Vec3, Vec3) {
        self.camera.ray_from_pixel(px, self.size)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StrokeMods {
    /// Ctrl: inverte o efeito.
    pub invert: bool,
    /// Shift: alisar temporário.
    pub smooth: bool,
}

/// Ponto sob o cursor (círculo do pincel).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hover {
    pub local_point: Vec3,
    pub local_normal: Vec3,
    pub world_point: Vec3,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StrokeSummary {
    pub dabs: usize,
    pub topology_changed: bool,
    pub millis: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct Bound {
    obj: usize,
    revision: u64,
    verts: usize,
    faces: usize,
}

struct Run {
    obj: usize,
    kind: BrushKind,
    dabs: usize,
    topology_changed: bool,
    dyntopo_edge: Option<f32>,
    started: std::time::Instant,
    /// Documento de antes do traço (vira o passo de desfazer se algo mudou).
    before: ModelDoc,
}

pub(crate) struct SculptState {
    pub slots: Vec<BrushSlot>,
    pub active: BrushKind,
    pub symmetry: Symmetry,
    pub dyntopo: bool,
    pub dyntopo_detail_px: f32,
    /// 0 = automático.
    pub voxel_size: f32,
    pub voxel_smooth: u32,
    pub voxel_shade_smooth: bool,
    pub smooth_all_iters: u32,
    pub symmetrize_axis: usize,
    pub hover: Option<Hover>,
    pub error: Option<String>,
    pub status: Option<String>,
    pub last_stroke: Option<StrokeSummary>,
    /// Sombreamento de antes de entrar na escultura (a escultura usa o sólido: mostra a máscara e o relevo).
    pub shade_before: Option<super::ViewShade>,
    ctx: Option<SculptContext>,
    bvh: Option<TriBvh>,
    bound: Option<Bound>,
    stroke: StrokeState,
    run: Option<Run>,
}

impl Default for SculptState {
    fn default() -> Self {
        let mut symmetry = Symmetry::none();
        // rosto e corpo: espelho em X ligado de partida (como o Blender)
        symmetry.mirror[0] = true;
        SculptState {
            slots: SCULPT_BRUSHES.iter().map(|&k| BrushSlot::new(k)).collect(),
            active: BrushKind::Draw,
            symmetry,
            dyntopo: false,
            dyntopo_detail_px: 14.0,
            voxel_size: 0.0,
            voxel_smooth: 0,
            voxel_shade_smooth: true,
            smooth_all_iters: 3,
            symmetrize_axis: 0,
            hover: None,
            error: None,
            status: None,
            last_stroke: None,
            shade_before: None,
            ctx: None,
            bvh: None,
            bound: None,
            stroke: StrokeState::new(),
            run: None,
        }
    }
}

fn slot_index(kind: BrushKind) -> usize {
    SCULPT_BRUSHES.iter().position(|&k| k == kind).unwrap_or(0)
}

impl SculptState {
    pub(crate) fn slot(&self, kind: BrushKind) -> &BrushSlot {
        &self.slots[slot_index(kind)]
    }

    pub(crate) fn active_slot_mut(&mut self) -> &mut BrushSlot {
        let i = slot_index(self.active);
        &mut self.slots[i]
    }

    pub(crate) fn is_stroking(&self) -> bool {
        self.run.is_some()
    }

    /// Esquece o objeto (ao sair do modo).
    pub(crate) fn clear(&mut self) {
        self.ctx = None;
        self.bvh = None;
        self.bound = None;
        self.hover = None;
        self.run = None;
        self.stroke = StrokeState::new();
    }

    fn rebuild(&mut self, mesh: &EditMesh) {
        self.hover = None;
        match SculptContext::new(mesh) {
            Ok(ctx) => {
                self.bvh = Some(TriBvh::build(mesh));
                self.ctx = Some(ctx);
                self.error = None;
            }
            Err(e) => {
                self.ctx = None;
                self.bvh = None;
                self.error = Some(format!("{}: {e}", tr("sculpt.err.mesh")));
            }
        }
    }
}

/// Contexto e BVH do objeto ativo na revisão atual (refeitos quando o objeto, a topologia ou a revisão mudam: desfazer,
/// remesh…). Nada durante um traço.
pub(crate) fn sync(ms: &mut ModelingState) {
    if ms.sculpt.run.is_some() {
        return;
    }
    let Some(a) = ms.active.filter(|&a| a < ms.doc.objects.len()) else {
        if ms.sculpt.bound.is_some() {
            ms.sculpt.clear();
        }
        return;
    };
    let mesh = &ms.doc.objects[a].mesh;
    let want = Bound { obj: a, revision: ms.revision, verts: mesh.verts.len(), faces: mesh.faces.len() };
    if ms.sculpt.bound.as_ref() == Some(&want) {
        return;
    }
    let mesh = mesh.clone();
    ms.sculpt.rebuild(&mesh);
    ms.sculpt.bound = Some(want);
}

/// Depois de mexer na malha sem refazer o contexto: a revisão anda (a GPU sobe de novo) e o vínculo continua valendo.
fn bump(ms: &mut ModelingState) {
    ms.touch();
    let rev = ms.revision;
    let a = ms.active;
    if let (Some(b), Some(a)) = (ms.sculpt.bound.as_mut(), a) {
        b.revision = rev;
        if let Some(o) = ms.doc.objects.get(a) {
            b.verts = o.mesh.verts.len();
            b.faces = o.mesh.faces.len();
        }
    }
}

/// Raio do cursor (MUNDO) na malha ativa: atualiza o ponto sob o pincel.
pub(crate) fn update_hover(ms: &mut ModelingState, ray_world: (Vec3, Vec3)) {
    sync(ms);
    let sc = &mut ms.sculpt;
    sc.hover = None;
    let Some(obj) = ms.active.and_then(|a| ms.doc.objects.get(a)) else { return };
    let (Some(bvh), Some(ctx)) = (sc.bvh.as_ref(), sc.ctx.as_ref()) else { return };
    let model = obj.matrix();
    let (o, d) = ray_to_local(model, ray_world.0, ray_world.1);
    if d == Vec3::ZERO {
        return;
    }
    let Some(hit) = bvh.raycast(o, d, f32::MAX) else { return };
    let mut n = Vec3::ZERO;
    if let Some(tr) = bvh.tri_ref(hit.tri_index) {
        for k in 0..3 {
            if let Some(vn) = ctx.vert_normals.get(tr.verts[k] as usize) {
                n += *vn * hit.bary[k];
            }
        }
    }
    let n = n.try_normalize().unwrap_or(hit.normal);
    sc.hover = Some(Hover { local_point: hit.point, local_normal: n, world_point: model.transform_point3(hit.point) });
}

/// Começa um traço no raio de MUNDO do cursor. `Ok(false)` = o raio não acerta o objeto.
pub(crate) fn begin_stroke(ms: &mut ModelingState, view: &ViewInfo, ray_world: (Vec3, Vec3), pressure: f32, time: f64, mods: StrokeMods) -> Result<bool, String> {
    if ms.sculpt.run.is_some() {
        return Err(tr("sculpt.err.busy").into());
    }
    sync(ms);
    let Some(idx) = ms.active.filter(|&a| a < ms.doc.objects.len()) else { return Err(tr("sculpt.err.no_object").into()) };
    let model = ms.doc.objects[idx].matrix();
    let before = ms.doc.clone();
    let sc = &mut ms.sculpt;
    if sc.ctx.is_none() || sc.bvh.is_none() {
        return Err(sc.error.clone().unwrap_or_else(|| tr("sculpt.err.unavailable").into()));
    }
    let (lo, ld) = ray_to_local(model, ray_world.0, ray_world.1);
    if ld == Vec3::ZERO {
        return Ok(false);
    }
    let Some(hit) = sc.bvh.as_ref().and_then(|b| b.raycast(lo, ld, f32::MAX)) else { return Ok(false) };
    let world_hit = model.transform_point3(hit.point);
    let kind = if mods.smooth && !sc.active.is_mask() { BrushKind::Smooth } else { sc.active };
    let slot = sc.slots[slot_index(kind)].clone();
    let mut brush = slot.brush.clone();
    if mods.invert && !mods.smooth {
        brush.direction_invert = !brush.direction_invert;
    }
    brush.radius = px_to_local_radius(&view.camera, view.size.y, world_hit, slot.radius_px, model);
    let dyn_edge = (sc.dyntopo && !kind.is_mask() && !kind.uses_frozen_set()).then(|| px_to_local_radius(&view.camera, view.size.y, world_hit, sc.dyntopo_detail_px, model));
    let sample = StrokeSample { ray_origin: lo, ray_dir: ld, pressure: pressure.clamp(0.0, 1.0), time };
    let sym = sc.symmetry.clone();
    let (Some(ctx), Some(bvh)) = (sc.ctx.as_mut(), sc.bvh.as_mut()) else { return Ok(false) };
    let upd = match sc.stroke.begin(&mut ms.doc.objects[idx].mesh, bvh, ctx, &brush, &sym, &sample) {
        Ok(u) => u,
        Err(e) => {
            ms.doc = before;
            let msg = format!("{}: {e}", tr("sculpt.err.refused"));
            ms.sculpt.error = Some(msg.clone());
            return Err(msg);
        }
    };
    if !upd.started {
        return Ok(false);
    }
    ms.sculpt.run = Some(Run { obj: idx, kind, dabs: upd.dabs, topology_changed: false, dyntopo_edge: dyn_edge, started: std::time::Instant::now(), before });
    bump(ms);
    Ok(true)
}

/// Nova amostra do traço (raio de MUNDO).
pub(crate) fn continue_stroke(ms: &mut ModelingState, ray_world: (Vec3, Vec3), pressure: f32, time: f64) {
    let Some(idx) = ms.sculpt.run.as_ref().map(|r| r.obj) else { return };
    let Some(model) = ms.doc.objects.get(idx).map(|o| o.matrix()) else { return };
    let (lo, ld) = ray_to_local(model, ray_world.0, ray_world.1);
    if ld == Vec3::ZERO {
        return;
    }
    let sc = &mut ms.sculpt;
    let (Some(ctx), Some(bvh)) = (sc.ctx.as_mut(), sc.bvh.as_mut()) else { return };
    let sample = StrokeSample { ray_origin: lo, ray_dir: ld, pressure: pressure.clamp(0.0, 1.0), time };
    match sc.stroke.continue_stroke(&mut ms.doc.objects[idx].mesh, bvh, ctx, &sample) {
        Ok(upd) => {
            if let Some(r) = sc.run.as_mut() {
                r.dabs += upd.dabs;
            }
            apply_dyntopo(ms, idx, &upd);
            if upd.dabs > 0 {
                bump(ms);
            }
        }
        Err(e) => sc.error = Some(format!("{}: {e}", tr("sculpt.err.stroke"))),
    }
}

fn apply_dyntopo(ms: &mut ModelingState, idx: usize, upd: &StrokeUpdate) {
    let Some(edge) = ms.sculpt.run.as_ref().and_then(|r| r.dyntopo_edge) else { return };
    if upd.result.affected_verts.is_empty() {
        return;
    }
    let region = upd.result.affected_verts.clone();
    match dyntopo_step(&mut ms.doc.objects[idx].mesh, &region, edge, &DyntopoParams::default()) {
        Ok(rep) if rep.changed() => {
            let mesh = ms.doc.objects[idx].mesh.clone();
            let sc = &mut ms.sculpt;
            match SculptContext::new(&mesh) {
                Ok(ctx) => {
                    sc.bvh = Some(TriBvh::build(&mesh));
                    sc.ctx = Some(ctx);
                }
                Err(e) => sc.error = Some(format!("dyntopo: {e}")),
            }
            if let Some(r) = sc.run.as_mut() {
                r.topology_changed = true;
            }
        }
        Ok(_) => {}
        Err(e) => ms.sculpt.error = Some(format!("dyntopo: {e}")),
    }
}

/// Fecha o traço: entra no desfazer se mudou algo.
pub(crate) fn end_stroke(ms: &mut ModelingState) -> Option<StrokeSummary> {
    let run = ms.sculpt.run.take()?;
    let changed = {
        let sc = &mut ms.sculpt;
        let (Some(ctx), Some(obj)) = (sc.ctx.as_mut(), ms.doc.objects.get(run.obj)) else { return None };
        let outcome = sc.stroke.end(&obj.mesh, ctx);
        !outcome.deltas.is_empty() || run.topology_changed
    };
    let summary = StrokeSummary { dabs: run.dabs, topology_changed: run.topology_changed, millis: run.started.elapsed().as_secs_f32() * 1000.0 };
    if changed {
        ms.history.push_owned(&format!("{}: {}", tr("sculpt.hist"), brush_label(run.kind)), run.before);
        ms.dirty = true;
        bump(ms);
        ms.sculpt.status = Some(format!("{}: {} {}", brush_label(run.kind), run.dabs, tr("sculpt.dabs")));
    }
    ms.sculpt.last_stroke = Some(summary);
    Some(summary)
}

/// Cancela o traço (Esc): o documento volta ao de antes.
pub(crate) fn cancel_stroke(ms: &mut ModelingState) {
    if let Some(run) = ms.sculpt.run.take() {
        ms.doc = run.before;
        ms.sculpt.stroke = StrokeState::new();
        ms.sculpt.bound = None;
        ms.touch();
    }
}

/// Traço roteirizado (ponte, testes): caminhos em coordenadas normalizadas do viewport (0..1).
pub(crate) fn run_script(ms: &mut ModelingState, view: &ViewInfo, brush: BrushKind, invert: bool, paths: &[Vec<Vec2>]) -> usize {
    ms.sculpt.active = brush;
    let mods = StrokeMods { invert, smooth: false };
    let mut total = 0;
    let mut t = 0.0f64;
    for path in paths {
        let pts: Vec<Vec2> = path.iter().map(|p| *p * view.size).collect();
        let Some(&first) = pts.first() else { continue };
        match begin_stroke(ms, view, view.ray(first), 1.0, t, mods) {
            Ok(true) => {}
            _ => continue,
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let steps = (((b - a).length() / 4.0).ceil() as usize).max(1);
            for s in 1..=steps {
                t += 0.004;
                continue_stroke(ms, view.ray(a.lerp(b, s as f32 / steps as f32)), 1.0, t);
            }
        }
        if let Some(s) = end_stroke(ms) {
            total += s.dabs;
        }
    }
    total
}

// ------------------------------------------------------------------ malha inteira

fn active_index(ms: &ModelingState) -> Result<usize, String> {
    ms.active.filter(|&a| a < ms.doc.objects.len()).ok_or_else(|| tr("sculpt.err.no_object").to_owned())
}

/// Inverte (ou limpa) a máscara de todos os vértices.
pub(crate) fn mask_all(ms: &mut ModelingState, invert: bool) -> Result<usize, String> {
    let idx = active_index(ms)?;
    let label = if invert { tr("sculpt.mask_invert") } else { tr("sculpt.mask_clear") };
    let before = ms.doc.clone();
    let delta = if invert { mask_invert_all(&mut ms.doc.objects[idx].mesh) } else { mask_clear_all(&mut ms.doc.objects[idx].mesh) };
    if delta.is_empty() {
        ms.sculpt.status = Some(tr("sculpt.mask_nothing").into());
        return Ok(0);
    }
    let n = delta.entries.len();
    ms.history.push_owned(label, before);
    ms.dirty = true;
    bump(ms);
    ms.sculpt.status = Some(format!("{label}: {n}"));
    Ok(n)
}

/// Alisa a malha toda (pincel de alisar do tamanho do objeto; respeita a máscara).
pub(crate) fn smooth_all(ms: &mut ModelingState, iterations: u32, strength: f32) -> Result<(), String> {
    sync(ms);
    let idx = active_index(ms)?;
    let Some((lo, hi)) = ms.doc.objects[idx].mesh.bounds() else { return Err(tr("sculpt.err.empty").into()) };
    let before = ms.doc.clone();
    let sc = &mut ms.sculpt;
    let (Some(ctx), Some(bvh)) = (sc.ctx.as_mut(), sc.bvh.as_mut()) else { return Err(sc.error.clone().unwrap_or_else(|| tr("sculpt.err.unavailable").into())) };
    let mut brush = Brush::new(BrushKind::Smooth);
    brush.falloff = aiai_3d_edit::sculpt::falloff::Falloff::Constant;
    brush.radius = ((hi - lo).length() * 2.0).max(1e-3);
    brush.automask = AutoMask::default();
    let dab = Dab::new((lo + hi) * 0.5, Vec3::Y, brush.radius, strength.clamp(0.01, 1.0));
    let mesh = &mut ms.doc.objects[idx].mesh;
    ctx.begin_stroke(mesh, &brush, None, Vec3::Y, None).map_err(|e| format!("{}: {e}", tr("sculpt.smooth_all")))?;
    let mut res = Ok(());
    for _ in 0..iterations.clamp(1, 32) {
        if let Err(e) = apply_dab(mesh, bvh, &dab, &brush, ctx) {
            res = Err(format!("{}: {e}", tr("sculpt.smooth_all")));
            break;
        }
    }
    let deltas = ctx.end_stroke(mesh);
    if let Err(msg) = res {
        ms.doc = before;
        ms.sculpt.bound = None;
        ms.sculpt.error = Some(msg.clone());
        return Err(msg);
    }
    if deltas.is_empty() {
        ms.sculpt.status = Some(tr("sculpt.nothing").into());
        return Ok(());
    }
    ms.history.push_owned(tr("sculpt.smooth_all"), before);
    ms.dirty = true;
    bump(ms);
    ms.sculpt.status = Some(format!("{}: {iterations}", tr("sculpt.smooth_all")));
    Ok(())
}

/// Voxel remesh do objeto ativo (malha nova, uniforme, de quadrados).
pub(crate) fn voxel_remesh(ms: &mut ModelingState) -> Result<String, String> {
    let idx = active_index(ms)?;
    let sc = &ms.sculpt;
    let size = if sc.voxel_size > 0.0 { sc.voxel_size } else { suggest_voxel_size(&ms.doc.objects[idx].mesh) };
    let params = VoxelParams { smooth_iterations: sc.voxel_smooth, ..VoxelParams::default() };
    let shade_smooth = sc.voxel_shade_smooth;
    let mesh = &ms.doc.objects[idx].mesh;
    let (mut new, rep) = voxel_remesh_report(mesh, size, &params).map_err(|e| voxel_error(&e))?;
    new.name = mesh.name.clone();
    if shade_smooth {
        for f in &mut new.faces {
            f.smooth = true;
        }
    }
    let text = format!("{}: {} {} · voxel {:.4}", tr("sculpt.voxel"), rep.quads, tr("sculpt.faces"), size);
    let label = tr("sculpt.voxel");
    ms.edit_labeled(label, |d| {
        d.objects[idx].mesh = new;
        Ok(())
    })?;
    ms.sculpt.bound = None;
    ms.sculpt.status = Some(text.clone());
    Ok(text)
}

fn voxel_error(e: &ModelError) -> String {
    format!("{}: {e}", tr("sculpt.voxel"))
}

/// Simetriza a malha no eixo (0 X, 1 Y, 2 Z): o lado positivo copia para o negativo (ou o contrário).
pub(crate) fn symmetrize(ms: &mut ModelingState, axis: usize, positive_to_negative: bool) -> Result<usize, String> {
    let idx = active_index(ms)?;
    let dir = if positive_to_negative { SymmetrizeDirection::PositiveToNegative } else { SymmetrizeDirection::NegativeToPositive };
    let mut changed = 0usize;
    ms.edit_labeled(tr("sculpt.symmetrize"), |d| {
        changed = symmetrize_mesh(&mut d.objects[idx].mesh, axis.min(2), dir, 1e-3).map_err(|e| format!("{}: {e} ({})", tr("sculpt.symmetrize"), tr("sculpt.symmetrize.need")))?;
        Ok(())
    })?;
    ms.sculpt.bound = None;
    ms.sculpt.status = Some(format!("{}: {changed}", tr("sculpt.symmetrize")));
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_3d_edit::doc::Primitive;

    fn state_with_sphere() -> ModelingState {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Sphere).unwrap();
        ms
    }

    /// Vista de frente centrada no objeto.
    fn front_view(ms: &ModelingState) -> ViewInfo {
        let o = &ms.doc.objects[0];
        let (lo, hi) = o.mesh.bounds().unwrap();
        let m = o.matrix();
        let mut cam = OrbitCamera::default();
        cam.set_preset(super::super::camera::ViewPreset::Front);
        cam.frame_bounds(m.transform_point3(lo), m.transform_point3(hi));
        ViewInfo { camera: cam, size: Vec2::new(800.0, 600.0) }
    }

    #[test]
    fn a_draw_stroke_raises_the_surface_and_is_one_undo_step() {
        let mut ms = state_with_sphere();
        let before = ms.doc.clone();
        let steps = ms.history.labels().len();
        let view = front_view(&ms);
        let dabs = run_script(&mut ms, &view, BrushKind::Draw, false, &[vec![Vec2::new(0.45, 0.5), Vec2::new(0.55, 0.5)]]);
        assert!(dabs > 0, "o traço acertou a esfera");
        assert_ne!(ms.doc, before, "a malha mudou");
        assert_eq!(ms.history.labels().len(), steps + 1, "um passo de desfazer");
        assert!(ms.undo());
        assert_eq!(ms.doc, before);
        // fora do objeto: nada muda, nada entra no desfazer
        let n = ms.history.labels().len();
        run_script(&mut ms, &view, BrushKind::Draw, false, &[vec![Vec2::new(0.02, 0.02), Vec2::new(0.05, 0.03)]]);
        assert_eq!(ms.history.labels().len(), n);
    }

    #[test]
    fn mask_protects_and_whole_mesh_tools_work() {
        let mut ms = state_with_sphere();
        assert!(mask_all(&mut ms, true).unwrap() > 0, "inverter a máscara vazia mascara tudo");
        let masked = ms.doc.clone();
        let v = front_view(&ms);
        let dabs = run_script(&mut ms, &v, BrushKind::Draw, false, &[vec![Vec2::new(0.45, 0.5), Vec2::new(0.55, 0.5)]]);
        assert!(dabs > 0, "o traço acertou a esfera");
        let a = &masked.objects[0].mesh;
        let b = &ms.doc.objects[0].mesh;
        assert!(a.verts.iter().zip(&b.verts).all(|(x, y)| (x.co - y.co).length() < 1e-6), "máscara cheia: nada se mexe");
        mask_all(&mut ms, false).unwrap();
        smooth_all(&mut ms, 2, 0.5).unwrap();
        let r = voxel_remesh(&mut ms).unwrap();
        assert!(r.contains("voxel") || r.contains("Voxel"), "{r}");
        assert!(ms.doc.objects[0].mesh.faces.len() > 10);
    }

    #[test]
    fn brush_keys_round_trip() {
        for k in SCULPT_BRUSHES {
            assert_eq!(brush_from_key(brush_key(k)), Some(k));
        }
    }
}

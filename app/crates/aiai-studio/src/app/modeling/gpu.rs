//! GPU do viewport da Modelagem, do jeito do ANIGO (`viewport_ui.rs::render_into` + `celview.rs`): o Workbench desenha
//! grade, sólido/arame e as sobreposições de edição; a prévia cel (o mesmo renderer da camada 3D, `aiai-3d-render`, com
//! contorno) renderiza com `render_to_texture`. As duas texturas são registradas UMA vez no egui
//! (`register_native_texture`, re-registradas só quando o tamanho muda) e redesenhadas direto na GPU: nada é lido de
//! volta para a CPU. Malha só sobe quando o objeto muda (chave por conteúdo); mexer na câmera só troca uniformes.

use super::camera::OrbitCamera;
use super::workbench::{self, build_draw_data, build_draw_data_eval, BuildOptions, DrawItem, GpuMesh, ShadingMode, ViewSettings, Workbench};
use aiai_3d_core::math::OrthographicBounds;
use aiai_3d_core::mesh::{Mesh, Vertex};
use aiai_3d_core::scene::{Scene, SceneNode, StylizedMaterial};
use aiai_3d_edit::doc::{self, ModelDoc};
use aiai_3d_edit::{SelectMode, Selection};
use aiai_3d_model::raster::SHADE_MUL;
use aiai_3d_model::scene::precompensate;
use glam::Vec3;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Estado da GPU do egui, guardado quando o app liga o serviço 3D (`model3d::install_gpu`).
static RENDER_STATE: OnceLock<egui_wgpu::RenderState> = OnceLock::new();
/// Bytes de VRAM das texturas do viewport (entra na contabilidade do `gpu_mem` pelo `model3d::gpu_cached_bytes`).
static GPU_BYTES: AtomicU64 = AtomicU64::new(0);

pub(crate) fn install(rs: &egui_wgpu::RenderState) {
    let _ = RENDER_STATE.set(rs.clone());
}

pub(crate) fn gpu_bytes() -> u64 {
    GPU_BYTES.load(Ordering::Relaxed)
}

/// Sombreamento do viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum ViewShade {
    /// Cel com contorno (o renderer da camada 3D): o que vai ser a referência. Padrão.
    #[default]
    Toon,
    /// Cel chapado (sem banda de sombra), com contorno.
    Flat,
    /// Sólido com luz de estúdio (Workbench), como o modo Sólido do Blender.
    Solid,
    /// Arame (Workbench).
    Wire,
}

impl ViewShade {
    pub const ALL: [ViewShade; 4] = [ViewShade::Toon, ViewShade::Flat, ViewShade::Solid, ViewShade::Wire];

    pub fn key(self) -> &'static str {
        match self {
            ViewShade::Toon => "toon",
            ViewShade::Flat => "flat",
            ViewShade::Solid => "solid",
            ViewShade::Wire => "wire",
        }
    }

    pub fn is_cel(self) -> bool {
        matches!(self, ViewShade::Toon | ViewShade::Flat)
    }
}

/// O que o viewport precisa para um quadro.
pub(crate) struct FrameIn<'a> {
    pub doc: &'a ModelDoc,
    pub active: Option<usize>,
    pub edit: bool,
    pub sel_mode: SelectMode,
    pub sel: &'a Selection,
    pub cam: &'a OrbitCamera,
    pub size_px: (u32, u32),
    pub ppp: f32,
    pub shade: ViewShade,
    pub xray: bool,
    pub grid: bool,
    /// Fundo do tema (gradiente do Workbench no Sólido/Arame).
    pub bg: [f32; 4],
    /// Muda a cada edição do documento (chave da malha do objeto em edição).
    pub revision: u64,
}

/// Texturas prontas para o painel.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameOut {
    pub cel: Option<egui::TextureId>,
    pub overlay: Option<egui::TextureId>,
    /// Tempo de CPU para montar e submeter o quadro (ms); 0 quando nada mudou e o quadro anterior foi reaproveitado.
    pub cpu_ms: f32,
    pub redrawn: bool,
}

struct Cached {
    key: u64,
    mesh: GpuMesh,
}

/// Chaves dos objetos (conteúdo) recalculadas só quando a revisão muda.
#[derive(Default)]
struct Keys {
    rev: u64,
    objects: Vec<u64>,
}

struct Cel {
    renderer: aiai_3d_render::HeadlessRenderer,
    texture: Option<(wgpu::Texture, (u32, u32))>,
    tex_id: Option<egui::TextureId>,
    scene: Scene,
    node_keys: Vec<u64>,
    last_error: Option<String>,
}

pub(crate) struct ModelingGpu {
    wb: Workbench,
    wb_tex: Option<egui::TextureId>,
    cache: Vec<Cached>,
    cel: Option<Cel>,
    /// Assinatura do último quadro desenhado (nada mudou = reaproveita as texturas).
    last_sig: u64,
    keys: Keys,
    /// Revisão (e sombreamento) com que os buffers do Workbench foram montados: só a câmera mudou = só uniformes.
    synced: Option<(u64, ViewShade)>,
    pub last_error: Option<String>,
}

fn fnv(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

fn hash_str(s: &str) -> u64 {
    fnv(&[aiai_3d_render::fnv1a64(s.as_bytes())])
}

fn f32s(v: &[f32]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits() as u64).collect()
}

/// Câmera do renderer cel a partir da câmera orbital (inclusive a ortográfica, com a mesma altura de volume do
/// Workbench: `2·distância·tan(fov/2)`), como o `core_camera` do ANIGO.
pub(crate) fn core_camera(cam: &OrbitCamera, aspect: f32) -> aiai_3d_core::math::Camera {
    aiai_3d_core::math::Camera {
        eye: cam.eye(),
        target: cam.target,
        up: cam.up(),
        fov_y: cam.fov_y,
        aspect,
        z_near: cam.near,
        z_far: cam.far,
        orthographic: cam.orthographic.then(|| OrthographicBounds::from_height(2.0 * cam.distance * (cam.fov_y * 0.5).tan(), aspect)),
    }
}

/// Chave do conteúdo de um objeto (malha, modificadores, sombreamento, cor).
fn object_key(o: &doc::Object3d) -> u64 {
    let mods = serde_json::to_string(&o.modifiers).unwrap_or_default();
    let mut parts = vec![o.mesh.content_hash(), hash_str(&mods), o.shading as u64];
    parts.extend(f32s(&o.color));
    fnv(&parts)
}

impl ModelingGpu {
    fn new(rs: &egui_wgpu::RenderState) -> Result<ModelingGpu, String> {
        let wb = Workbench::new(rs.device.clone(), rs.queue.clone())?;
        let info = rs.adapter.get_info();
        let cel = match aiai_3d_render::HeadlessRenderer::from_device(std::sync::Arc::new(rs.device.clone()), std::sync::Arc::new(rs.queue.clone()), info) {
            Ok(renderer) => {
                let mut scene = Scene::new_empty();
                scene.background_color = [0.0; 4];
                Some(Cel { renderer, texture: None, tex_id: None, scene, node_keys: Vec::new(), last_error: None })
            }
            Err(e) => {
                tracing::warn!("Modelagem: prévia cel indisponível ({e}); o viewport usa o sólido do Workbench");
                None
            }
        };
        Ok(ModelingGpu { wb, wb_tex: None, cache: Vec::new(), cel, last_sig: 0, keys: Keys::default(), synced: None, last_error: None })
    }

    /// Solta as texturas e os buffers (orçamento de VRAM); voltam no próximo quadro.
    fn release(&mut self, rs: &egui_wgpu::RenderState) {
        let mut r = rs.renderer.write();
        if let Some(t) = self.wb_tex.take() {
            r.free_texture(&t);
        }
        if let Some(c) = self.cel.as_mut() {
            if let Some(t) = c.tex_id.take() {
                r.free_texture(&t);
            }
            c.texture = None;
            c.renderer.release_cached_targets();
        }
        drop(r);
        self.wb.release_targets();
        self.cache.clear();
        self.last_sig = 0;
        self.synced = None;
        GPU_BYTES.store(0, Ordering::Relaxed);
    }

    fn refresh_keys(&mut self, f: &FrameIn) {
        if self.keys.rev != f.revision || self.keys.objects.len() != f.doc.objects.len() {
            self.keys.objects = f.doc.objects.iter().map(object_key).collect();
            self.keys.rev = f.revision;
        }
    }

    fn sync_wb(&mut self, f: &FrameIn) {
        let mut next: Vec<Cached> = Vec::with_capacity(f.doc.objects.len());
        for (i, o) in f.doc.objects.iter().enumerate() {
            if !o.visible {
                continue;
            }
            let is_edit = f.edit && f.active == Some(i);
            let is_active = f.active == Some(i);
            let sel_hash = if is_edit { hash_str(&format!("{:?}{:?}{:?}", f.sel.verts, f.sel.edges, f.sel.faces)) } else { 0 };
            let key = fnv(&[self.keys.objects[i], is_edit as u64, (is_active && !is_edit) as u64, f.sel_mode as u64, sel_hash, f.shade as u64]);
            let model = o.matrix();
            let tint = [1.0, 1.0, 1.0, 1.0];
            if let Some(pos) = self.cache.iter().position(|c| c.key == key) {
                let c = self.cache.swap_remove(pos);
                self.wb.set_object_transform(&c.mesh, model, tint);
                next.push(c);
                continue;
            }
            let base = [o.color[0], o.color[1], o.color[2], 1.0];
            let opts = BuildOptions {
                edit_mode: is_edit,
                select_mode: f.sel_mode,
                selection: is_edit.then_some(f.sel),
                active_vert: None,
                object_wire: f.shade == ViewShade::Wire,
                highlight_object: is_active && !is_edit,
                base_color: base,
                normals: doc::normal_params(o.shading),
            };
            let data = match o.evaluated() {
                Ok(eval) if !o.modifiers.is_empty() => build_draw_data_eval(&o.mesh, &eval, &opts),
                Ok(_) => build_draw_data(&o.mesh, &opts),
                Err(e) => {
                    self.last_error = Some(format!("{}: {e}", o.name));
                    build_draw_data(&o.mesh, &opts)
                }
            };
            let mesh = self.wb.upload(&data, model, tint);
            next.push(Cached { key, mesh });
        }
        self.cache = next;
    }

    fn sync_cel(cel: &mut Cel, f: &FrameIn, obj_keys: &[u64], aspect: f32, h: u32) {
        let visible: Vec<&doc::Object3d> = f.doc.objects.iter().filter(|o| o.visible).collect();
        let keys: Vec<u64> = f.doc.objects.iter().zip(obj_keys).filter(|(o, _)| o.visible).map(|(o, k)| k ^ (o.matrix().determinant() < 0.0) as u64).collect();
        if keys != cel.node_keys {
            cel.scene.nodes.clear();
            cel.last_error = None;
            for (i, o) in visible.iter().enumerate() {
                let eval = match o.evaluated() {
                    Ok(m) => m,
                    Err(e) => {
                        cel.last_error = Some(format!("{}: {e}", o.name));
                        o.mesh.clone()
                    }
                };
                let p = doc::object_part_local(o, &eval);
                let vertices: Vec<Vertex> = p.positions.iter().zip(&p.normals).map(|(q, n)| Vertex::new(*q, *n, [0.0, 0.0])).collect();
                let mut node = SceneNode::new(format!("obj{i}"), o.name.clone());
                if !p.indices.is_empty() {
                    node = node.with_mesh(Mesh::new(o.name.clone(), vertices, p.indices));
                }
                node.material = Some(StylizedMaterial::default());
                cel.scene.add_node(node);
            }
            cel.scene.rebuild_children();
            cel.node_keys = keys;
        }
        let toon = f.shade == ViewShade::Toon;
        cel.scene.camera = core_camera(f.cam, aspect);
        let back = -f.cam.forward();
        let light = (-0.45 * f.cam.right() + 0.75 * f.cam.up() + 0.5 * back).normalize_or(Vec3::Y);
        let l = &mut cel.scene.light;
        l.direction = if toon { light.to_array() } else { back.to_array() };
        l.color = [1.0; 3];
        l.intensity = 1.0;
        l.ambient_intensity = 1.0;
        l.ambient_sky = [1.0; 3];
        l.ambient_ground = [1.0; 3];
        l.shadow_color = [1.0; 3];
        cel.scene.background_color = [0.0; 4];
        // contorno de 1,6 px (o mesmo da prévia anterior e da camada 3D), em NDC
        let outline_ndc = 2.0 * 1.6 * f.ppp / h.max(1) as f32;
        for (node, o) in cel.scene.nodes.iter_mut().zip(visible.iter()) {
            if o.matrix().determinant() < 0.0 {
                // espelhado por escala negativa: a malha já veio no mundo (ver `object_part_local`)
                node.transform = Default::default();
            } else {
                node.transform.translation = Vec3::from(o.transform.pos);
                node.transform.rotation = doc::object_rotation(o);
                node.transform.scale = Vec3::from(o.transform.scale);
            }
            let base = o.color;
            let shade = [base[0] * SHADE_MUL[0], base[1] * SHADE_MUL[1], base[2] * SHADE_MUL[2]];
            let lit = precompensate(base);
            let sh = if toon { precompensate(shade) } else { lit };
            let m = node.material.get_or_insert_with(StylizedMaterial::default);
            m.base_color = [lit[0], lit[1], lit[2], 1.0];
            m.shade_color = [sh[0], sh[1], sh[2], 1.0];
            m.spec_intensity = 0.0;
            m.rim_intensity = 0.0;
            m.outline_color = [0.11, 0.12, 0.15, 1.0];
            m.outline_width = outline_ndc;
            m.outline_opacity = 1.0;
        }
    }

    fn frame(&mut self, rs: &egui_wgpu::RenderState, f: &FrameIn) -> FrameOut {
        let t0 = std::time::Instant::now();
        let (w, h) = (f.size_px.0.max(1), f.size_px.1.max(1));
        let c = f.cam;
        let mut sig = vec![w as u64, h as u64, f.shade as u64, f.xray as u64, f.grid as u64, f.edit as u64, f.sel_mode as u64, f.revision, f.active.map_or(u64::MAX, |a| a as u64), c.orthographic as u64];
        sig.extend(f32s(&[c.target.x, c.target.y, c.target.z, c.yaw, c.pitch, c.distance, c.fov_y, f.ppp]));
        sig.extend(f32s(&f.bg));
        let sig = fnv(&sig);
        let mut out = FrameOut { cel: self.cel.as_ref().and_then(|c| c.tex_id), overlay: self.wb_tex, cpu_ms: 0.0, redrawn: false };
        if sig == self.last_sig && out.overlay.is_some() && (!f.shade.is_cel() || out.cel.is_some()) {
            return out;
        }
        self.last_sig = sig;
        self.last_error = None;
        self.refresh_keys(f);
        // Workbench: Sólido/Arame com fundo; Cel = só sobreposições por cima da prévia cel
        if self.synced != Some((f.revision, f.shade)) {
            self.sync_wb(f);
            self.synced = Some((f.revision, f.shade));
        }
        let use_cel = f.shade.is_cel() && self.cel.is_some();
        let settings = ViewSettings {
            shading: match f.shade {
                ViewShade::Wire => ShadingMode::Wireframe,
                ViewShade::Solid => ShadingMode::Solid,
                _ if use_cel => ShadingMode::OverlayOnly,
                _ => ShadingMode::Solid,
            },
            show_grid: f.grid,
            xray: f.xray,
            grid_scale: 1.0,
        };
        self.wb.background_top = f.bg;
        self.wb.background_bottom = [f.bg[0] * 0.8, f.bg[1] * 0.8, f.bg[2] * 0.82, 1.0];
        let items: Vec<DrawItem> = self.cache.iter().map(|c| DrawItem { mesh: &c.mesh }).collect();
        let recreated = self.wb.render((w, h), f.cam, &items, &settings);
        drop(items);
        if recreated || self.wb_tex.is_none() {
            let mut r = rs.renderer.write();
            if let Some(old) = self.wb_tex.take() {
                r.free_texture(&old);
            }
            if let Some(view) = self.wb.output_view() {
                self.wb_tex = Some(r.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear));
            }
        }
        out.overlay = self.wb_tex;
        // prévia cel
        if use_cel {
            if let Some(cel) = self.cel.as_mut() {
                let aspect = w as f32 / h as f32;
                Self::sync_cel(cel, f, &self.keys.objects, aspect, h);
                let fresh = cel.texture.as_ref().is_none_or(|t| t.1 != (w, h));
                if fresh {
                    let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("mdl cel"),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: aiai_3d_render::offscreen_color_format(),
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    cel.texture = Some((tex, (w, h)));
                }
                let has_mesh = cel.scene.nodes.iter().any(|n| n.mesh.is_some());
                if let Some((tex, _)) = &cel.texture {
                    let r = if has_mesh {
                        cel.renderer.render_to_texture(&cel.scene, tex).map(|_| ())
                    } else {
                        // cena vazia: limpa a textura (o renderer recusa cena sem malha)
                        let view = tex.create_view(&Default::default());
                        let mut enc = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mdl cel clear") });
                        drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("mdl cel clear"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        }));
                        rs.queue.submit(Some(enc.finish()));
                        Ok(())
                    };
                    if let Err(e) = r {
                        cel.last_error = Some(format!("{e:#}"));
                    }
                    if fresh || cel.tex_id.is_none() {
                        let view = tex.create_view(&Default::default());
                        let mut rr = rs.renderer.write();
                        if let Some(old) = cel.tex_id.take() {
                            rr.free_texture(&old);
                        }
                        cel.tex_id = Some(rr.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear));
                    }
                }
                if let Some(e) = &cel.last_error {
                    self.last_error = Some(format!("cel: {e}"));
                }
                out.cel = cel.tex_id;
            }
        } else {
            out.cel = None;
        }
        // VRAM: alvo MSAA 4× + profundidade 4× + resolve do Workbench, mais a textura cel e os alvos do renderer
        let px = w as u64 * h as u64;
        let cel_bytes = self.cel.as_ref().map_or(0, |c| c.renderer.cached_gpu_bytes() + if c.texture.is_some() { px * 4 } else { 0 });
        GPU_BYTES.store(px * 4 * (4 + 4 + 1) + cel_bytes, Ordering::Relaxed);
        out.cpu_ms = t0.elapsed().as_secs_f32() * 1000.0;
        out.redrawn = true;
        out
    }
}

/// O GPU do viewport (um por app; vive na thread da UI).
pub(crate) struct GpuSlot {
    gpu: Option<ModelingGpu>,
    failed: Option<String>,
}

impl Default for GpuSlot {
    fn default() -> Self {
        GpuSlot { gpu: None, failed: None }
    }
}

impl GpuSlot {
    /// Desenha o quadro (ou reaproveita o anterior). `None` = sem GPU do egui (o viewport mostra o aviso).
    pub(crate) fn frame(&mut self, f: &FrameIn) -> Result<FrameOut, String> {
        let Some(rs) = RENDER_STATE.get() else {
            return Err("sem GPU do egui (wgpu)".into());
        };
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.gpu.is_none() {
            match ModelingGpu::new(rs) {
                Ok(g) => self.gpu = Some(g),
                Err(e) => {
                    self.failed = Some(e.clone());
                    return Err(e);
                }
            }
        }
        let g = self.gpu.as_mut().expect("criado acima");
        Ok(g.frame(rs, f))
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.gpu.as_ref().and_then(|g| g.last_error.as_deref())
    }

    /// Solta texturas e buffers; `true` se havia algo.
    pub(crate) fn release(&mut self) -> bool {
        match (self.gpu.as_mut(), RENDER_STATE.get()) {
            (Some(g), Some(rs)) => {
                let had = g.wb_tex.is_some() || g.cel.as_ref().is_some_and(|c| c.tex_id.is_some());
                g.release(rs);
                had
            }
            _ => false,
        }
    }
}

pub(crate) use workbench::px_size;

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_3d_edit::doc::Primitive;
    use std::time::Instant;

    fn device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).ok()?;
        let info = adapter.get_info();
        let (d, q) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some((d, q, info))
    }

    fn doc() -> ModelDoc {
        let mut d = ModelDoc::new("bench");
        let i = d.add_primitive(Primitive::Sphere, 1.0, [0.0, 1.0, 0.0], None).unwrap();
        d.objects[i].modifiers.push(doc::ModifierSpec::Subsurf { levels: 2, enabled: true });
        d.add_primitive(Primitive::Cube, 1.0, [1.8, 0.5, 0.0], None).unwrap();
        d
    }

    /// Mede o caminho antigo (quadro na GPU lido de volta, convertido e re-subido como textura) contra o nativo
    /// (`render_to_texture` + Workbench, nada volta para a CPU), no mesmo tamanho de viewport, com a câmera mudando a
    /// cada quadro (como no zoom). Sem adaptador wgpu o teste só avisa.
    #[test]
    fn native_viewport_is_faster_than_readback_per_camera_frame() {
        let Some((device, queue, info)) = device() else {
            eprintln!("sem adaptador wgpu: teste pulado");
            return;
        };
        let (w, h) = (1280u32, 720u32);
        let d = doc();
        let model = std::sync::Arc::new(d.to_model().unwrap());
        // ---- antes: HeadlessRenderer::render_scene (leitura de volta) + ColorImage + upload, como o viewport antigo
        let old = aiai_3d_render::HeadlessRenderer::from_device(std::sync::Arc::new(device.clone()), std::sync::Arc::new(queue.clone()), info.clone()).unwrap();
        let mut scene = aiai_3d_model::scene::base_scene(&model);
        let upload = device.create_texture(&wgpu::TextureDescriptor { label: Some("bench upload"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let mut cam = OrbitCamera::default();
        let n = 20;
        let mut old_ms = Vec::new();
        for k in 0..n {
            cam.zoom(if k % 2 == 0 { 0.97 } else { 1.03 });
            let state = aiai_core::model3d::Model3dState { model_hash: d.content_hash(), transform: Default::default(), camera: aiai_3d_model::camera::layer_from_orbit(&aiai_3d_model::camera::OrbitCamera { target: cam.target, yaw: cam.yaw, pitch: cam.pitch, distance: cam.distance, fov_y: cam.fov_y, ..Default::default() }), mode: aiai_core::model3d::RenderMode3d::Toon, outline_px: 1.6, outline_color: [0.11, 0.12, 0.15], color: None, pose: None };
            let t = Instant::now();
            aiai_3d_model::scene::update_scene(&mut scene, &model, &state, w, h);
            let (img, _) = pollster::block_on(old.render_scene(&scene, w, h)).unwrap();
            let px: Vec<egui::Color32> = img.as_raw().chunks_exact(4).map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])).collect();
            queue.write_texture(upload.as_image_copy(), bytemuck::cast_slice(&px), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
            queue.submit(None);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if k > 2 {
                old_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // ---- depois: o caminho do viewport nativo (cel + Workbench), esperando a GPU terminar para medir de verdade
        let mut wb = Workbench::new(device.clone(), queue.clone()).unwrap();
        let renderer = aiai_3d_render::HeadlessRenderer::from_device(std::sync::Arc::new(device.clone()), std::sync::Arc::new(queue.clone()), info).unwrap();
        let mut scene = Scene::new_empty();
        scene.background_color = [0.0; 4];
        let mut cel = Cel { renderer, texture: None, tex_id: None, scene, node_keys: Vec::new(), last_error: None };
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some("bench cel"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: aiai_3d_render::offscreen_color_format(), usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
        let keys: Vec<u64> = d.objects.iter().map(object_key).collect();
        let items_data: Vec<GpuMesh> = d.objects.iter().map(|o| wb.upload(&build_draw_data_eval(&o.mesh, &o.evaluated().unwrap(), &BuildOptions::default()), o.matrix(), [1.0; 4])).collect();
        let sel = Selection::default();
        let mut new_ms = Vec::new();
        for k in 0..n {
            cam.zoom(if k % 2 == 0 { 0.97 } else { 1.03 });
            let t = Instant::now();
            let f = FrameIn { doc: &d, active: Some(0), edit: false, sel_mode: SelectMode::Face, sel: &sel, cam: &cam, size_px: (w, h), ppp: 1.0, shade: ViewShade::Toon, xray: false, grid: true, bg: [0.1, 0.1, 0.12, 1.0], revision: 1 };
            ModelingGpu::sync_cel(&mut cel, &f, &keys, w as f32 / h as f32, h);
            cel.renderer.render_to_texture(&cel.scene, &tex).unwrap();
            let items: Vec<DrawItem> = items_data.iter().map(|m| DrawItem { mesh: m }).collect();
            wb.render((w, h), &cam, &items, &ViewSettings { shading: ShadingMode::OverlayOnly, ..Default::default() });
            let cpu = t.elapsed().as_secs_f64() * 1000.0;
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if k > 2 {
                new_ms.push((cpu, t.elapsed().as_secs_f64() * 1000.0));
            }
        }
        let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let old_avg = avg(&old_ms);
        let new_cpu = avg(&new_ms.iter().map(|x| x.0).collect::<Vec<_>>());
        let new_total = avg(&new_ms.iter().map(|x| x.1).collect::<Vec<_>>());
        eprintln!("viewport {w}x{h} por quadro de câmera: antes (leitura de volta + re-upload) {old_avg:.2} ms; depois (nativo) CPU {new_cpu:.2} ms, até a GPU terminar {new_total:.2} ms");
        if let Ok(dir) = std::env::var("AIAI_TEST_OUT") {
            let _ = std::fs::write(format!("{dir}/mdl_viewport_bench.txt"), format!("w={w} h={h} old_ms={old_avg:.3} new_cpu_ms={new_cpu:.3} new_total_ms={new_total:.3}\n"));
        }
        assert!(cel.last_error.is_none());
        assert!(new_total < old_avg, "o caminho nativo tem de ser mais rápido que a leitura de volta");
    }
}

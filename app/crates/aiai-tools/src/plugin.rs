//! Plugins `.aiaiplugin` (auditoria 4.9g): filtros escritos em WGSL que rodam na GPU (wgpu, o mesmo do app), sem
//! acesso a arquivo, rede ou ao resto do programa (o shader só vê a imagem e os parâmetros).
//!
//! Um plugin é uma pasta `<nome>.aiaiplugin` (ou um `.aiaiplugin` em zip com o mesmo conteúdo) com `manifest.json`:
//!
//! ```json
//! {"id": "exemplo.inverter", "name": "Inverter cores", "version": "1.0", "kind": "filter", "shader": "filter.wgsl",
//!  "workgroup": [8, 8], "params": [{"key": "amount", "label": "Quantidade", "min": 0, "max": 1, "default": 1}]}
//! ```
//!
//! e o shader de computação, com estas ligações (a imagem chega sem pré-multiplicar, sRGB 0..1):
//!
//! ```wgsl
//! struct Params { size: vec2<u32>, frame: u32, count: u32, p: array<vec4<f32>, 4> }  // até 16 parâmetros, na ordem do manifesto
//! @group(0) @binding(0) var src: texture_2d<f32>;
//! @group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
//! @group(0) @binding(2) var<uniform> u: Params;
//! @compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3<u32>) { ... }
//! ```
//!
//! Aplicar um filtro troca os tiles da camada raster (um passo de desfazer).

use crate::context::*;
use aiai_core::*;
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginParam {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one")]
    pub max: f32,
    #[serde(default)]
    pub default: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// "filter" (o único tipo por enquanto).
    pub kind: String,
    pub shader: String,
    #[serde(default = "wg")]
    pub workgroup: [u32; 2],
    #[serde(default)]
    pub params: Vec<PluginParam>,
}

fn wg() -> [u32; 2] {
    [8, 8]
}

/// Plugin lido: o manifesto, o código do shader e de onde veio.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub shader: String,
    pub path: PathBuf,
}

/// Pasta padrão dos plugins do usuário: `%APPDATA%\AnimeIAI\plugins` (ou `AIAI_PLUGINS_DIR`).
pub fn plugins_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("AIAI_PLUGINS_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map(|a| PathBuf::from(a).join("AnimeIAI").join("plugins"))
}

/// Arquivos do zip (só "guardado" e "deflate", o que os compactadores comuns gravam), pelo nome.
fn read_zip(bytes: &[u8]) -> ToolResult<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut i = 0usize;
    let u16_at = |b: &[u8], k: usize| -> ToolResult<usize> { b.get(k..k + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or_else(|| ToolError::Invalid("zip truncado".into())) };
    let u32_at = |b: &[u8], k: usize| -> ToolResult<usize> { b.get(k..k + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize).ok_or_else(|| ToolError::Invalid("zip truncado".into())) };
    while i + 30 <= bytes.len() && bytes[i..i + 4] == [0x50, 0x4b, 0x03, 0x04] {
        let flags = u16_at(bytes, i + 6)?;
        let method = u16_at(bytes, i + 8)?;
        let csize = u32_at(bytes, i + 18)?;
        let nlen = u16_at(bytes, i + 26)?;
        let xlen = u16_at(bytes, i + 28)?;
        if flags & 0x08 != 0 {
            return Err(ToolError::Invalid("zip com descritor de dados não é suportado (grave o plugin sem streaming)".into()));
        }
        let name = String::from_utf8_lossy(bytes.get(i + 30..i + 30 + nlen).ok_or_else(|| ToolError::Invalid("zip truncado".into()))?).replace('\\', "/");
        let start = i + 30 + nlen + xlen;
        let data = bytes.get(start..start + csize).ok_or_else(|| ToolError::Invalid("zip truncado".into()))?;
        let content = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data).map_err(|e| ToolError::Invalid(format!("zip: {e:?}")))?,
            m => return Err(ToolError::Invalid(format!("zip: compressão {m} não suportada"))),
        };
        // a pasta de dentro do zip (plugin.aiaiplugin/manifest.json) não importa: vale o nome do arquivo
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        if !base.is_empty() {
            out.insert(base, content);
        }
        i = start + csize;
    }
    Ok(out)
}

/// Lê um plugin (pasta ou zip).
pub fn load_plugin(path: &Path) -> ToolResult<Plugin> {
    let files: BTreeMap<String, Vec<u8>> = if path.is_dir() {
        let mut m = BTreeMap::new();
        for e in std::fs::read_dir(path).map_err(|e| ToolError::Invalid(format!("{}: {e}", path.display())))?.flatten() {
            if e.path().is_file() {
                if let Ok(b) = std::fs::read(e.path()) {
                    m.insert(e.file_name().to_string_lossy().into_owned(), b);
                }
            }
        }
        m
    } else {
        read_zip(&std::fs::read(path).map_err(|e| ToolError::Invalid(format!("{}: {e}", path.display())))?)?
    };
    let man = files.get("manifest.json").ok_or_else(|| ToolError::Invalid(format!("{}: sem manifest.json", path.display())))?;
    let manifest: PluginManifest = serde_json::from_slice(man).map_err(|e| ToolError::Invalid(format!("manifest.json: {e}")))?;
    if manifest.kind != "filter" {
        return Err(ToolError::Invalid(format!("tipo de plugin '{}' não suportado (só 'filter')", manifest.kind)));
    }
    if manifest.params.len() > 16 {
        return Err(ToolError::Invalid("no máximo 16 parâmetros".into()));
    }
    if manifest.workgroup.iter().any(|v| *v == 0 || *v > 32) {
        return Err(ToolError::Invalid("workgroup fora de 1..32".into()));
    }
    let shader = files.get(&manifest.shader).ok_or_else(|| ToolError::Invalid(format!("shader '{}' não está no plugin", manifest.shader)))?;
    let shader = String::from_utf8(shader.clone()).map_err(|_| ToolError::Invalid("shader não é texto UTF-8".into()))?;
    Ok(Plugin { manifest, shader, path: path.to_path_buf() })
}

/// Plugins de uma pasta (os que não abrem vão para a lista de problemas).
pub fn list_plugins(dir: &Path) -> (Vec<Plugin>, Vec<String>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    let Ok(rd) = std::fs::read_dir(dir) else { return (ok, bad) };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("aiaiplugin"))).collect();
    paths.sort();
    for p in paths {
        match load_plugin(&p) {
            Ok(pl) => ok.push(pl),
            Err(e) => bad.push(format!("{}: {e}", p.display())),
        }
    }
    (ok, bad)
}

/// O plugin pelo caminho (pasta/zip) ou pelo `id` na pasta padrão.
pub fn find_plugin(spec: &str) -> ToolResult<Plugin> {
    let p = Path::new(spec);
    if p.exists() {
        return load_plugin(p);
    }
    let dir = plugins_dir().ok_or_else(|| ToolError::Invalid("pasta de plugins desconhecida".into()))?;
    let (all, _) = list_plugins(&dir);
    all.into_iter().find(|pl| pl.manifest.id == spec || pl.manifest.name == spec).ok_or_else(|| ToolError::Invalid(format!("plugin '{spec}' não encontrado em {}", dir.display())))
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();

fn gpu() -> ToolResult<&'static Gpu> {
    GPU.get_or_init(|| {
        let made = std::panic::catch_unwind(|| {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::all(), ..wgpu::InstanceDescriptor::new_without_display_handle() });
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .ok()?;
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("AnimeIAI plugins"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))
            .ok()?;
            Some(Gpu { device, queue })
        });
        made.ok().flatten()
    })
    .as_ref()
    .ok_or_else(|| ToolError::Invalid("sem GPU (wgpu) para rodar o plugin".into()))
}

/// Roda o filtro na imagem (pré-multiplicada, como as camadas) e devolve a nova (pré-multiplicada).
pub fn run_filter(pl: &Plugin, img: &Image, params: &BTreeMap<String, f32>, frame: u32) -> ToolResult<Image> {
    let g = gpu()?;
    let (w, h) = (img.width.max(1), img.height.max(1));
    let lim = g.device.limits().max_texture_dimension_2d;
    if w > lim || h > lim {
        return Err(ToolError::Invalid(format!("imagem {w}x{h} maior que o limite da GPU ({lim})")));
    }
    // parâmetros na ordem do manifesto (padrão, preso ao intervalo)
    let mut pv = [0f32; 16];
    for (i, p) in pl.manifest.params.iter().enumerate() {
        let v = params.get(&p.key).copied().unwrap_or(p.default);
        let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
        pv[i] = if v.is_finite() { v.clamp(lo, hi) } else { p.default };
    }
    let dev = &g.device;
    let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&pl.manifest.id), source: wgpu::ShaderSource::Wgsl(pl.shader.as_str().into()) });
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let src = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("plugin src"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let dst = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("plugin dst"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    // a imagem sai do pré-multiplicado para o shader
    let mut straight: Vec<f32> = Vec::with_capacity((w * h * 4) as usize);
    for p in &img.data {
        let a = p[3];
        if a > 0.0 {
            straight.extend_from_slice(&[p[0] / a, p[1] / a, p[2] / a, a]);
        } else {
            straight.extend_from_slice(&[0.0, 0.0, 0.0, 0.0]);
        }
    }
    g.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &src, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytemuck_f32(&straight),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 16), rows_per_image: Some(h) },
        size,
    );
    let mut ubytes = Vec::with_capacity(80);
    for v in [w, h, frame, pl.manifest.params.len() as u32] {
        ubytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in pv {
        ubytes.extend_from_slice(&v.to_le_bytes());
    }
    let ubuf = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("plugin params"), size: ubytes.len() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    g.queue.write_buffer(&ubuf, 0, &ubytes);
    let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("plugin"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });
    let sv = src.create_view(&Default::default());
    let dv = dst.create_view(&Default::default());
    let bind = dev.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("plugin"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sv) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) },
            wgpu::BindGroupEntry { binding: 2, resource: ubuf.as_entire_binding() },
        ],
    });
    let pll = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("plugin"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let pipe = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("plugin"),
        layout: Some(&pll),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    // leitura de volta: linhas alinhadas a 256 bytes
    let row = (w * 16).div_ceil(256) * 256;
    let rb = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("plugin out"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = dev.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipe);
        pass.set_bind_group(0, &bind, &[]);
        let [gx, gy] = pl.manifest.workgroup;
        pass.dispatch_workgroups(w.div_ceil(gx), h.div_ceil(gy), 1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &dst, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        size,
    );
    g.queue.submit([enc.finish()]);
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(ToolError::Invalid(format!("plugin {}: {e}", pl.manifest.id)));
    }
    let slice = rb.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = dev.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().map_err(|_| ToolError::Invalid("GPU não devolveu a imagem".into()))?.map_err(|e| ToolError::Invalid(format!("GPU: {e}")))?;
    let data = slice.get_mapped_range().map_err(|e| ToolError::Invalid(format!("GPU: {e:?}")))?;
    let mut out = Image::new(w, h);
    for y in 0..h as usize {
        let rowb = &data[y * row as usize..y * row as usize + (w as usize * 16)];
        for x in 0..w as usize {
            let f = |k: usize| f32::from_le_bytes([rowb[x * 16 + k * 4], rowb[x * 16 + k * 4 + 1], rowb[x * 16 + k * 4 + 2], rowb[x * 16 + k * 4 + 3]]);
            let a = f(3).clamp(0.0, 1.0);
            let c = |v: f32| if v.is_finite() { v.max(0.0) * a } else { 0.0 };
            out.data[y * w as usize + x] = [c(f(0)), c(f(1)), c(f(2)), if a.is_finite() { a } else { 0.0 }];
        }
    }
    drop(data);
    rb.unmap();
    Ok(out)
}

fn bytemuck_f32(v: &[f32]) -> &[u8] {
    // f32 é 4 bytes sem padding: a fatia de bytes cobre exatamente a de floats
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PluginFilterRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada raster.
    pub layer: Id,
    /// Caminho do `.aiaiplugin` (pasta ou zip) ou o `id` dele na pasta de plugins do usuário.
    pub plugin: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
}

/// O filtro aplicado no cel exposto da camada raster: os tiles novos num passo de desfazer.
pub fn plugin_filter_command(p: &Project, blobs: &mut BlobStore, req: &PluginFilterRequest) -> ToolResult<(Command, Value)> {
    let pl = find_plugin(&req.plugin)?;
    let cut = cut_data(p, &req.cut)?;
    let l = layer_data(p, &req.layer)?;
    if l.kind != LayerType::Raster {
        return Err(ToolError::Invalid("o filtro de plugin pinta numa camada raster".into()));
    }
    let cel = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe nenhum cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cd) = &cel.data else { unreachable!() };
    let (w, h) = aiai_render::compose::cut_canvas(p, cut);
    let img = crate::paint::cel_raster(blobs, cd, w, h)?;
    let out = run_filter(&pl, &img, &req.params, req.frame)?;
    // tiles que ficaram vazios saem; os de antes que sumiram também
    let mut tiles = crate::paint::tiles_for_region(blobs, &out, [0, 0, w as i32, h as i32]);
    let new: std::collections::BTreeSet<(i32, i32)> = tiles.iter().map(|t| (t.x, t.y)).collect();
    for t in &cd.tiles {
        if !new.contains(&(t.x, t.y)) {
            tiles.push(command::TileEdit { x: t.x, y: t.y, blob: None });
        }
    }
    Ok((Command::SetTiles { cel: cel.id.clone(), tiles }, json!({ "plugin": pl.manifest.id, "name": pl.manifest.name })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVERT: &str = r#"
struct Params { size: vec2<u32>, frame: u32, count: u32, p: array<vec4<f32>, 4> }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> u: Params;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.size.x || id.y >= u.size.y) { return; }
    let c = textureLoad(src, vec2<i32>(id.xy), 0);
    let k = u.p[0].x;
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(mix(c.rgb, vec3<f32>(1.0) - c.rgb, k), c.a));
}
"#;

    fn write_plugin(dir: &Path, shader: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{"id":"teste.inverter","name":"Inverter","kind":"filter","shader":"f.wgsl","params":[{"key":"amount","min":0,"max":1,"default":1}]}"#).unwrap();
        std::fs::write(dir.join("f.wgsl"), shader).unwrap();
    }

    #[test]
    fn a_wgsl_plugin_inverts_on_the_gpu_and_a_broken_one_reports_the_error() {
        let base = std::env::temp_dir().join(format!("aiai-plugin-{}", std::process::id()));
        let dir = base.join("inverter.aiaiplugin");
        write_plugin(&dir, INVERT);
        let pl = load_plugin(&dir).unwrap();
        let mut img = Image::new(20, 10);
        img.data[0] = [0.2 * 0.5, 0.4 * 0.5, 0.6 * 0.5, 0.5];
        match run_filter(&pl, &img, &BTreeMap::new(), 0) {
            Ok(out) => {
                let p = out.data[0];
                assert!((p[3] - 0.5).abs() < 1e-6 && (p[0] / p[3] - 0.8).abs() < 1e-4 && (p[2] / p[3] - 0.4).abs() < 1e-4, "{p:?}");
                assert_eq!(out.data[5], [0.0; 4], "transparente continua transparente");
                // meio a meio
                let half = run_filter(&pl, &img, &BTreeMap::from([("amount".into(), 0.5)]), 0).unwrap();
                assert!((half.data[0][0] / half.data[0][3] - 0.5).abs() < 1e-4);
                // shader quebrado: erro com o que o validador disse, sem derrubar nada
                let bad = base.join("quebrado.aiaiplugin");
                write_plugin(&bad, "fn main( { nada }");
                let e = run_filter(&load_plugin(&bad).unwrap(), &img, &BTreeMap::new(), 0).unwrap_err();
                assert!(e.to_string().contains("teste.inverter"), "{e}");
                // a lista da pasta acha os dois
                let (ok, _) = list_plugins(&base);
                assert_eq!(ok.len(), 2);
            }
            // máquina sem GPU (CI): o erro é claro (o teste avisa que pulou a parte da GPU)
            Err(e) => {
                eprintln!("plugin: sem GPU, parte da GPU pulada: {e}");
                assert!(e.to_string().contains("GPU"), "{e}")
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plugins_also_load_from_zip_files() {
        let base = std::env::temp_dir().join(format!("aiai-plugin-zip-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let files: Vec<(&str, Vec<u8>)> = vec![
            ("inverter.aiaiplugin/manifest.json", br#"{"id":"z.inv","name":"Z","kind":"filter","shader":"f.wgsl"}"#.to_vec()),
            ("inverter.aiaiplugin/f.wgsl", INVERT.as_bytes().to_vec()),
        ];
        let zip = crate::export::zip_stored(&files);
        let p = base.join("z.aiaiplugin");
        std::fs::write(&p, zip).unwrap();
        let pl = load_plugin(&p).unwrap();
        assert_eq!(pl.manifest.id, "z.inv");
        assert!(pl.shader.contains("textureStore"));
        let _ = std::fs::remove_dir_all(&base);
    }
}

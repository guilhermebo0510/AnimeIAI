//! Camadas 3D no compositor (S24 3D): o compositor continua CPU e sem wgpu; quem desenha o modelo é um
//! [`Layer3dRaster`]. O app registra o serviço da GPU ([`set_layer3d_raster`]); sem registro (headless, MCP, testes) vale
//! o rasterizador CPU do `aiai-3d-model`. Em todos os casos o resultado passa pelo mesmo cache por (modelo, estado
//! avaliado no quadro, tamanho, versão do caminho de render), guardado em RGBA8 pré-multiplicado.

use aiai_core::model3d::Model3dState;
use aiai_core::{BlobHash, BlobStore};
use aiai_paint::Image;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// Pedido de um quadro de camada 3D: estado já avaliado (valores-base + chaves) e o tamanho do canvas do corte.
pub struct Layer3dJob<'a> {
    pub model_hash: &'a BlobHash,
    pub state: &'a Model3dState,
    pub blobs: &'a BlobStore,
    pub width: u32,
    pub height: u32,
}

/// Quem desenha camadas 3D. Saída: RGBA sRGB pré-multiplicado 0..1, `width`×`height`.
pub trait Layer3dRaster: Send + Sync {
    /// Nome curto (entra na chave do cache: GPU e CPU podem diferir no pixel).
    fn backend(&self) -> &str;
    fn render(&self, job: &Layer3dJob) -> Result<Vec<[f32; 4]>, String>;
}

/// Rasterizador CPU (reserva e caminho de referência).
pub struct CpuRaster;

impl Layer3dRaster for CpuRaster {
    fn backend(&self) -> &str {
        "cpu"
    }

    fn render(&self, job: &Layer3dJob) -> Result<Vec<[f32; 4]>, String> {
        let model = aiai_3d_model::load_posed(job.blobs, job.model_hash, job.state.pose.as_ref())?;
        Ok(aiai_3d_model::raster::render_cpu(&model, job.state, job.width, job.height))
    }
}

static PROVIDER: RwLock<Option<Arc<dyn Layer3dRaster>>> = RwLock::new(None);

/// Registra (ou remove, com `None`) o renderizador das camadas 3D deste processo. Limpa o cache de rasters.
pub fn set_layer3d_raster(p: Option<Arc<dyn Layer3dRaster>>) {
    if let Ok(mut g) = PROVIDER.write() {
        *g = p;
    }
    if let Some(c) = CACHE.get() {
        if let Ok(mut c) = c.lock() {
            c.clear();
        }
    }
}

/// Renderizador em uso (o registrado, ou a CPU).
pub fn layer3d_raster() -> Arc<dyn Layer3dRaster> {
    PROVIDER.read().ok().and_then(|g| g.clone()).unwrap_or_else(|| Arc::new(CpuRaster))
}

// ------------------------------------------------------------------------------------------------------------- cache

/// Teto do cache de rasters (bytes RGBA8).
pub const CACHE_CAP_BYTES: usize = 256 << 20;

struct Entry {
    key: [u8; 32],
    w: u32,
    h: u32,
    rgba: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct RasterCache {
    entries: VecDeque<Entry>,
    bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

impl RasterCache {
    fn get(&mut self, key: &[u8; 32]) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        let i = self.entries.iter().position(|e| &e.key == key)?;
        let e = self.entries.remove(i)?;
        let out = (e.w, e.h, Arc::clone(&e.rgba));
        self.entries.push_back(e);
        Some(out)
    }

    fn put(&mut self, key: [u8; 32], w: u32, h: u32, rgba: Vec<u8>) {
        self.bytes += rgba.len();
        self.entries.push_back(Entry { key, w, h, rgba: Arc::new(rgba) });
        while self.bytes > CACHE_CAP_BYTES && self.entries.len() > 1 {
            if let Some(e) = self.entries.pop_front() {
                self.bytes -= e.rgba.len();
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

static CACHE: OnceLock<Mutex<RasterCache>> = OnceLock::new();

fn cache() -> &'static Mutex<RasterCache> {
    CACHE.get_or_init(|| Mutex::new(RasterCache::default()))
}

/// Chave determinística do quadro: modelo, estado avaliado, tamanho, renderizador e versão do caminho de render. O
/// id da camada NÃO entra: duas camadas iguais no mesmo quadro dividem o raster.
pub fn cache_key(backend: &str, job: &Layer3dJob) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"aiai-model3d-raster");
    h.update(&aiai_3d_model::RENDER_VERSION.to_le_bytes());
    h.update(backend.as_bytes());
    h.update(job.model_hash.0.as_bytes());
    h.update(serde_json::to_string(job.state).unwrap_or_default().as_bytes());
    h.update(&job.width.to_le_bytes());
    h.update(&job.height.to_le_bytes());
    *h.finalize().as_bytes()
}

/// Estatística do cache (acertos, faltas, entradas), para testes e para a telemetria.
pub fn cache_stats() -> (u64, u64, usize) {
    cache().lock().map(|c| (c.hits, c.misses, c.len())).unwrap_or((0, 0, 0))
}

fn to_rgba8(px: &[[f32; 4]]) -> Vec<u8> {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    px.iter().flat_map(|p| [b(p[0]), b(p[1]), b(p[2]), b(p[3])]).collect()
}

fn to_image(w: u32, h: u32, rgba: &[u8]) -> Image {
    let mut img = Image::new(w, h);
    for (d, s) in img.data.iter_mut().zip(rgba.chunks_exact(4)) {
        *d = [s[0] as f32 / 255.0, s[1] as f32 / 255.0, s[2] as f32 / 255.0, s[3] as f32 / 255.0];
    }
    img
}

/// Quadro da camada 3D pelo renderizador em uso, passando pelo cache.
pub fn render_layer3d(job: &Layer3dJob) -> Result<Image, String> {
    let provider = layer3d_raster();
    let key = cache_key(provider.backend(), job);
    if let Ok(mut c) = cache().lock() {
        if let Some((w, h, rgba)) = c.get(&key) {
            c.hits += 1;
            return Ok(to_image(w, h, &rgba));
        }
        c.misses += 1;
    }
    let px = provider.render(job)?;
    if px.len() != (job.width as usize) * (job.height as usize) {
        return Err(format!("renderizador 3D '{}' devolveu {} pixels para {}x{}", provider.backend(), px.len(), job.width, job.height));
    }
    let rgba = to_rgba8(&px);
    let img = to_image(job.width, job.height, &rgba);
    if let Ok(mut c) = cache().lock() {
        c.put(key, job.width, job.height, rgba);
    }
    Ok(img)
}

//! Serviço de render do canvas: uma thread de fundo que roda o MESMO `aiai_render::render_frame` do `RENDER` da bridge
//! (sem câmera: o canvas inteiro do corte, em coordenadas do desenho), com fila de "o mais novo vence" (pedidos velhos
//! saem da fila antes de começar), cache por (change_counter, corte, quadro, guias, referência, redução) e imagens já
//! prontas para virar textura do egui. Durante a reprodução o mesmo serviço renderiza os quadros à frente em escala reduzida.

use super::commands::NodeId;
use aiai_core::{BlobStore, Project};
use aiai_render::{FrameCache, RenderOptions, render_frame_cached};
use egui::{Color32, ColorImage};
use rayon::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Cópia do documento que as threads de render leem (barata: blobs são `Arc`).
pub(crate) struct Snapshot {
    pub project: Project,
    pub blobs: BlobStore,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct FrameKey {
    pub counter: u64,
    pub cut: NodeId,
    pub frame: u32,
    pub guides: bool,
    pub reference: bool,
    /// Divisor do tamanho da imagem (1 = resolução cheia).
    pub reduce: u32,
    /// Solo da Composição (só de visão): só estas camadas entram no quadro; vazio = todas.
    pub solo: Vec<NodeId>,
}

/// Quadro pronto: pixels pré-multiplicados (o que o egui espera) e o tamanho do canvas que eles cobrem.
pub(crate) struct Rendered {
    pub key: FrameKey,
    pub image: Arc<ColorImage>,
    /// Tamanho do canvas do corte em pixels do documento.
    pub canvas: [u32; 2],
    /// Fração da textura que cobre o canvas (a redução arredonda para cima; 1.0 quando divide exato).
    pub uv_max: [f32; 2],
    pub render_ms: f32,
    pub warnings: Vec<String>,
}

impl Rendered {
    fn bytes(&self) -> usize {
        self.image.pixels.len() * 4
    }

    /// Cor (sRGB, não pré-multiplicada, 0..1) no pixel do canvas (x, y); `None` fora ou transparente.
    pub(crate) fn sample(&self, x: i32, y: i32) -> Option<[f32; 4]> {
        if x < 0 || y < 0 || x >= self.canvas[0] as i32 || y >= self.canvas[1] as i32 {
            return None;
        }
        let r = self.key.reduce as i32;
        let [w, h] = self.image.size;
        let (tx, ty) = (((x / r) as usize).min(w - 1), ((y / r) as usize).min(h - 1));
        let c = self.image.pixels[ty * w + tx];
        let [r8, g8, b8, a8] = c.to_srgba_unmultiplied();
        if a8 == 0 {
            return None;
        }
        Some([r8 as f32 / 255.0, g8 as f32 / 255.0, b8 as f32 / 255.0, a8 as f32 / 255.0])
    }
}

struct Job {
    key: FrameKey,
    snap: Arc<Snapshot>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    running: Option<FrameKey>,
    shutdown: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

pub(crate) type Done = Result<Rendered, (FrameKey, String)>;

struct Entry {
    r: Arc<Rendered>,
    used: u64,
}

pub(crate) struct RenderService {
    shared: Arc<Shared>,
    rx: crossbeam_channel::Receiver<Done>,
    cache: Vec<Entry>,
    tick: u64,
    snapshot: Option<(u64, Arc<Snapshot>)>,
    cap_bytes: usize,
    /// Último erro de render (corte/quadro inválido...), para mostrar no canvas.
    pub last_error: Option<(FrameKey, String)>,
    /// Média móvel do tempo de render (ms), para antecipar quadros na reprodução.
    pub avg_ms: f32,
    pub renders_done: u64,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl RenderService {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        let shared = Arc::new(Shared { q: Mutex::new(Queue::default()), cv: Condvar::new() });
        let (tx, rx) = crossbeam_channel::unbounded::<Done>();
        let worker_shared = Arc::clone(&shared);
        let ctx = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("aiai-canvas-render".into())
            .spawn(move || worker(worker_shared, tx, ctx))
            .ok();
        RenderService { shared, rx, cache: Vec::new(), tick: 0, snapshot: None, cap_bytes: 384 << 20, last_error: None, avg_ms: 0.0, renders_done: 0, handle }
    }

    /// Recolhe o que as threads terminaram (chamar uma vez por quadro). Devolve as chaves novas.
    pub(crate) fn poll(&mut self) -> Vec<FrameKey> {
        let mut fresh = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            match done {
                Ok(r) => {
                    self.avg_ms = if self.avg_ms == 0.0 { r.render_ms } else { self.avg_ms * 0.7 + r.render_ms * 0.3 };
                    self.renders_done += 1;
                    fresh.push(r.key.clone());
                    self.tick += 1;
                    let used = self.tick;
                    self.cache.retain(|e| e.r.key != r.key);
                    self.cache.push(Entry { r: Arc::new(r), used });
                }
                Err((key, msg)) => {
                    tracing::warn!("render do canvas falhou ({:?}): {msg}", key);
                    self.last_error = Some((key, msg));
                }
            }
        }
        self.evict();
        fresh
    }

    fn evict(&mut self) {
        let mut total: usize = self.cache.iter().map(|e| e.r.bytes()).sum();
        while total > self.cap_bytes && self.cache.len() > 1 {
            let (i, _) = self.cache.iter().enumerate().min_by_key(|(_, e)| e.used).expect("cache não vazio");
            total -= self.cache[i].r.bytes();
            self.cache.remove(i);
        }
    }

    pub(crate) fn get(&mut self, key: &FrameKey) -> Option<Arc<Rendered>> {
        self.tick += 1;
        let t = self.tick;
        let e = self.cache.iter_mut().find(|e| &e.r.key == key)?;
        e.used = t;
        Some(Arc::clone(&e.r))
    }

    /// Quadros prontos (sem tocar o LRU) que combinam com o resto da chave, para escolher o mais fresco na reprodução.
    pub(crate) fn cached_frames(&self, like: &FrameKey) -> Vec<u32> {
        self.cache.iter().filter(|e| e.r.key.counter == like.counter && e.r.key.cut == like.cut && e.r.key.guides == like.guides && e.r.key.reference == like.reference && e.r.key.reduce == like.reduce).map(|e| e.r.key.frame).collect()
    }

    pub(crate) fn cached_bytes(&self) -> usize {
        self.cache.iter().map(|e| e.r.bytes()).sum()
    }

    pub(crate) fn busy(&self) -> bool {
        let q = self.shared.q.lock().expect("fila");
        q.running.is_some() || !q.jobs.is_empty()
    }

    /// Cópia do documento na revisão atual, a mesma que os renders do canvas usam (outras threads, como a da mesa de
    /// luz, aproveitam a cópia em vez de clonar o projeto de novo).
    pub(crate) fn snapshot(&mut self, core: &aiai_core::CoreSession) -> Arc<Snapshot> {
        match &self.snapshot {
            Some((c, s)) if *c == core.change_counter => Arc::clone(s),
            _ => {
                let s = Arc::new(Snapshot { project: core.project.clone(), blobs: core.blobs.clone() });
                self.snapshot = Some((core.change_counter, Arc::clone(&s)));
                s
            }
        }
    }

    /// Substitui a fila de pedidos (ordem = prioridade). Chaves já no cache ou já em execução não entram; o que estava na
    /// fila e não foi pedido de novo é descartado (pedido velho).
    pub(crate) fn request(&mut self, core: &aiai_core::CoreSession, keys: &[FrameKey]) {
        let missing: Vec<&FrameKey> = keys.iter().filter(|k| !self.cache.iter().any(|e| &e.r.key == *k)).collect();
        let mut q = self.shared.q.lock().expect("fila");
        let running = q.running.clone();
        let wanted: Vec<&FrameKey> = missing.into_iter().filter(|k| running.as_ref() != Some(*k)).collect();
        if wanted.is_empty() {
            q.jobs.clear();
            return;
        }
        let counter = wanted[0].counter;
        let snap = match &self.snapshot {
            Some((c, s)) if *c == counter && counter == core.change_counter => Arc::clone(s),
            _ => {
                let s = Arc::new(Snapshot { project: core.project.clone(), blobs: core.blobs.clone() });
                self.snapshot = Some((core.change_counter, Arc::clone(&s)));
                s
            }
        };
        // se a chave pede um documento que já não é o da sessão (não deveria), não renderiza
        q.jobs = wanted.into_iter().filter(|k| k.counter == core.change_counter).map(|k| Job { key: k.clone(), snap: Arc::clone(&snap) }).collect();
        drop(q);
        self.shared.cv.notify_one();
    }
}

impl Drop for RenderService {
    fn drop(&mut self) {
        if let Ok(mut q) = self.shared.q.lock() {
            q.shutdown = true;
            q.jobs.clear();
        }
        self.shared.cv.notify_all();
        // a thread pode estar no meio de um render: não esperar (o processo está saindo)
        let _ = self.handle.take();
    }
}

fn worker(shared: Arc<Shared>, tx: crossbeam_channel::Sender<Done>, ctx: egui::Context) {
    // foto do composto abaixo da camada em edição: as camadas de baixo não são recompostas a cada traço
    let mut cache = FrameCache::default();
    loop {
        let job = {
            let mut q = shared.q.lock().expect("fila");
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(j) = q.jobs.pop_front() {
                    q.running = Some(j.key.clone());
                    break j;
                }
                q = shared.cv.wait(q).expect("fila");
            }
        };
        let t0 = Instant::now();
        let key = job.key.clone();
        // pool próprio (auditoria 4.4d): o render do canvas não enfileira atrás das ferramentas (pincel, balde) no pool
        // global, nem elas atrás dele
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_pool().install(|| render_job(&job, t0, &mut cache))));
        let done: Done = match result {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err((key, e)),
            Err(_) => {
                cache = FrameCache::default(); // a foto pode ter ficado pela metade
                Err((key, "o render do canvas entrou em pânico".to_owned()))
            }
        };
        if let Ok(mut q) = shared.q.lock() {
            q.running = None;
        }
        if tx.send(done).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// Threads do render do canvas: metade dos núcleos (pelo menos 2); a outra metade fica com o pool global das ferramentas.
fn render_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        rayon::ThreadPoolBuilder::new()
            .num_threads((n / 2).max(2))
            .thread_name(|i| format!("aiai-render-{i}"))
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(2).build().expect("pool de render"))
    })
}

fn render_job(job: &Job, t0: Instant, cache: &mut FrameCache) -> Result<Rendered, String> {
    let k = &job.key;
    let only_layers = (!k.solo.is_empty()).then(|| k.solo.clone());
    let opts = RenderOptions { include_guides: k.guides, include_reference: k.reference, only_layers, background: None, camera: false, layer_motion: true };
    let out = render_frame_cached(&job.snap.project, &job.snap.blobs, &k.cut, k.frame, &opts, cache).map_err(|e| e.to_string())?;
    let (w, h) = (out.image.width as usize, out.image.height as usize);
    let r = k.reduce.max(1) as usize;
    let (nw, nh) = (w.div_ceil(r), h.div_ceil(r));
    let px = |p: [f32; 4]| Color32::from_rgba_premultiplied((p[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (p[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (p[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
    // conversão em paralelo: no quadro 1920×1080 ela custava, sozinha, quase o mesmo que o render depois de uma pincelada
    let mut pixels = vec![Color32::TRANSPARENT; nw * nh];
    if r == 1 {
        pixels.par_iter_mut().zip(out.image.data.par_iter()).for_each(|(d, p)| *d = px(*p));
    } else {
        pixels.par_chunks_mut(nw).enumerate().for_each(|(ty, row)| {
            for (tx, d) in row.iter_mut().enumerate() {
                let (x0, y0) = (tx * r, ty * r);
                let (x1, y1) = ((x0 + r).min(w), (y0 + r).min(h));
                let mut acc = [0.0f32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = out.image.data[y * w + x];
                        for c in 0..4 {
                            acc[c] += p[c];
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)) as f32;
                *d = px([acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n]);
            }
        });
    }
    let uv_max = [w as f32 / r as f32 / nw as f32, h as f32 / r as f32 / nh as f32];
    Ok(Rendered {
        key: k.clone(),
        image: Arc::new(ColorImage { size: [nw, nh], source_size: egui::vec2(nw as f32, nh as f32), pixels }),
        canvas: [w as u32, h as u32],
        uv_max,
        render_ms: t0.elapsed().as_secs_f32() * 1000.0,
        warnings: out.warnings,
    })
}

/// Menor divisor ≥ `want` que divide o canvas por inteiro (evita desalinhar a textura reduzida); senão o próprio `want`.
pub(crate) fn pick_reduce(w: u32, h: u32, want: u32, max_side: u32) -> u32 {
    let need = w.max(h).div_ceil(max_side.max(1)).max(1);
    let want = want.max(need).max(1);
    (want..=want + 6).find(|r| w % r == 0 && h % r == 0).unwrap_or(want)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::*;

    #[test]
    fn reduce_picks_exact_divisors_and_respects_the_texture_limit() {
        assert_eq!(pick_reduce(1920, 1080, 1, 8192), 1);
        assert_eq!(pick_reduce(1920, 1080, 2, 8192), 2);
        assert_eq!(pick_reduce(1920, 1080, 7, 8192), 8);
        assert_eq!(pick_reduce(16384, 9000, 1, 8192), 2);
    }

    #[test]
    fn the_service_renders_in_the_background_and_caches() {
        let ctx = egui::Context::default();
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 32;
        core.project.settings.height = 18;
        let cut = super::super::commands::quick_start(&mut core, 6, None).unwrap();
        let layer = super::super::commands::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let mut svc = RenderService::new(&ctx);
        let key = FrameKey { counter: core.change_counter, cut: cut.clone(), frame: 0, guides: false, reference: false, reduce: 1, solo: Vec::new() };
        svc.request(&core, std::slice::from_ref(&key));
        let t0 = Instant::now();
        while svc.get(&key).is_none() {
            svc.poll();
            assert!(t0.elapsed().as_secs() < 20, "o render não terminou");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let r = svc.get(&key).unwrap();
        assert_eq!((r.canvas, r.image.size), ([32, 18], [32, 18]));
        assert!(r.sample(5, 5).is_none(), "camada vazia = transparente");
        // reduzido pela metade
        let key2 = FrameKey { reduce: 2, ..key.clone() };
        svc.request(&core, std::slice::from_ref(&key2));
        while svc.get(&key2).is_none() {
            svc.poll();
            assert!(t0.elapsed().as_secs() < 20);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(svc.get(&key2).unwrap().image.size, [16, 9]);
        // quadro fora da duração vira erro, não pânico
        let bad = FrameKey { frame: 99, ..key.clone() };
        svc.request(&core, std::slice::from_ref(&bad));
        while svc.last_error.is_none() {
            svc.poll();
            assert!(t0.elapsed().as_secs() < 20);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = layer;
    }
}

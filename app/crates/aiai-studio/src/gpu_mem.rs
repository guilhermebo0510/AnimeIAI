//! Memória de vídeo (VRAM): contabilidade, orçamento com pressão e resiliência a GPU sem memória / dispositivo perdido.
//!
//! **Contabilidade.** Quatro fontes, da mais fina à mais ampla:
//! - texturas do egui, agrupadas por subsistema pelo nome dado em `load_texture` (`TextureManager::allocated`; o
//!   egui-wgpu cria cada uma com 1 nível de mip, então o tamanho é `largura × altura × bytes por pixel`);
//! - o que o serviço 3D guarda entre quadros (alvos MSAA/profundidade/DoF e mapa de sombra, `model3d::gpu_cached_bytes`);
//! - o relatório do alocador do wgpu (`Device::generate_allocator_report`: alocado e reservado em blocos);
//! - no Windows, o uso e o orçamento do processo segundo o sistema (DXGI `QueryVideoMemoryInfo`). O uso conta toda
//!   alocação do processo naquele adaptador (Vulkan inclusive: tudo passa pelo WDDM) e o orçamento encolhe quando
//!   outros programas, outras instâncias do AnimeIAI inclusive, ocupam a placa.
//!
//! **Orçamento.** Automático = 60% da memória dedicada do adaptador (1,5 GB se desconhecida), dividido pelo número de
//! instâncias do AnimeIAI abertas (registro em `%LOCALAPPDATA%\AnimeIAI\instances`), nunca abaixo de 512 MB; ou o valor
//! escolhido nas Preferências > Desempenho. O limite efetivo é o menor entre o orçamento e 90% do orçamento do sistema.
//! A pressão (normal / alta / crítica) sai do uso do processo contra o limite e de erros de falta de memória vistos
//! pelo wgpu; o app usa a pressão para soltar texturas (`app::gpu_evict`) e as prévias para baixar a resolução.
//!
//! **Resiliência.** O wgpu trata falta de memória de dois jeitos: criar textura ou buffer sem memória é um erro comum
//! (vai ao tratador de erros sem captura, que por padrão encerra o processo), mas faltar memória para um buffer de
//! passagem (`write_buffer`/`write_texture`/`write_buffer_with`) ou o driver perder a placa derruba o dispositivo
//! inteiro. Com o dispositivo perdido, a próxima pintura do egui-wgpu entra em pânico ("Failed to create staging
//! buffer"), sem jeito de recriar o dispositivo pela API do eframe 0.36. Por isso aqui:
//! - o tratador de erros sem captura registra (log, arquivo `gpu-errors.log`, contadores) e, em falta de memória,
//!   pede despejo imediato em vez de encerrar;
//! - o aviso de dispositivo perdido marca o estado; o app, no quadro seguinte, grava o que falta, reabre a si mesmo no
//!   mesmo projeto e sai antes que o egui pinte (ver `app::gpu_lost_exit`);
//! - se a perda acontecer dentro da própria pintura, o gancho de pânico (`main.rs`) chama [`panic_note`], que explica a
//!   causa no relatório de queda, deixa um marcador para a próxima abertura mostrar e reabre o app.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MB: u64 = 1024 * 1024;
/// Orçamento quando o adaptador não informa a memória dedicada.
pub const FALLBACK_BUDGET: u64 = 1536 * MB;
/// Piso do orçamento automático (abaixo disso a interface sozinha já não cabe com folga).
pub const MIN_AUTO_BUDGET: u64 = 512 * MB;
/// Fração da memória dedicada usada pelo orçamento automático.
pub const AUTO_FRACTION: f64 = 0.60;
/// Escolhas do orçamento nas Preferências (MB; 0 = automático).
pub const BUDGET_CHOICES_MB: [u32; 7] = [0, 512, 768, 1024, 1536, 2048, 3072];

// ------------------------------------------------------------------------------------------------ subsistemas

/// (chave, prefixos do nome da textura). A ordem é a das tabelas (Preferências e bridge).
pub const SUBSYSTEMS: &[(&str, &[&str])] = &[
    ("canvas", &["canvas-image", "canvas-checker"]),
    ("overlays", &["live-stroke", "move-"]),
    ("onion", &["onion-skin"]),
    ("view3d", &["m3d-", "mdl-viewport", "lib-turn-"]),
    ("previews", &["edit-preview", "cut-preview", "comp-alpha", "subview", "deliver-poster", "retake-compare-", "prod-big-", "lib-image-"]),
    ("thumbs_library", &["lib-thumb-"]),
    ("thumbs_production", &["cut-thumb-", "storyboard-panel-"]),
    ("thumbs_brushes", &["brush_thumb_", "tool-thumb-", "brush-maker-preview", "import-tip"]),
    ("thumbs_effects", &["fx_thumb_"]),
    ("thumbs_deliver", &["deliver-thumb-"]),
    ("fonts", &["fontprev-"]),
    ("audio", &["audio_spec"]),
    ("ui", &["icon-", "brand-", "egui_font_texture"]),
];

/// Subsistema de uma textura pelo nome (`"other"` quando nenhum prefixo casa).
pub fn classify(name: &str) -> &'static str {
    SUBSYSTEMS.iter().find(|(_, pre)| pre.iter().any(|p| name.starts_with(p))).map(|(k, _)| *k).unwrap_or("other")
}

/// Teto de bytes de um cache de miniaturas sob o orçamento `budget` e a pressão `p` (a fração some com a pressão).
pub fn cache_cap(budget: u64, share_pct: u64, p: Pressure) -> u64 {
    let base = budget / 100 * share_pct;
    match p {
        Pressure::Normal => base,
        Pressure::High => base / 2,
        Pressure::Critical => base / 8,
    }
}

// ------------------------------------------------------------------------------------------------ pressão

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pressure {
    #[default]
    Normal,
    High,
    Critical,
}

impl Pressure {
    pub fn key(self) -> &'static str {
        match self {
            Pressure::Normal => "normal",
            Pressure::High => "high",
            Pressure::Critical => "critical",
        }
    }

    /// Escala de resolução das prévias (1 = cheia): sob pressão as prévias vêm menores em vez de falhar.
    pub fn preview_scale(self) -> f32 {
        match self {
            Pressure::Normal => 1.0,
            Pressure::High => 0.75,
            Pressure::Critical => 0.5,
        }
    }

    /// Pressão para um uso `used` contra o limite `limit`, com histerese a partir da anterior (`prev`): sobe em 80% e
    /// 95%, desce só abaixo de 70% e 85%.
    pub fn from_ratio(used: u64, limit: u64, prev: Pressure) -> Pressure {
        if limit == 0 {
            return prev;
        }
        let r = used as f64 / limit as f64;
        let up = if r >= 0.95 {
            Pressure::Critical
        } else if r >= 0.80 {
            Pressure::High
        } else {
            Pressure::Normal
        };
        let down = if r < 0.70 {
            Pressure::Normal
        } else if r < 0.85 {
            Pressure::High
        } else {
            Pressure::Critical
        };
        if up > prev {
            up
        } else if down < prev {
            down
        } else {
            prev
        }
    }
}

/// Escala de prévia atual (para quem não tem acesso ao `GpuMem`, como threads de prévia): ver [`Pressure::preview_scale`].
pub fn preview_scale() -> f32 {
    match CURRENT_PRESSURE.load(Ordering::Relaxed) {
        2 => Pressure::Critical.preview_scale(),
        1 => Pressure::High.preview_scale(),
        _ => 1.0,
    }
}

static CURRENT_PRESSURE: AtomicU32 = AtomicU32::new(0);

/// Reduz `img` pela escala de prévia atual (média de caixas). Imagem pequena ou pressão normal: devolve a mesma.
pub fn scale_for_pressure(img: egui::ColorImage) -> egui::ColorImage {
    let s = preview_scale();
    if s >= 1.0 || img.size[0] < 64 || img.size[1] < 64 {
        return img;
    }
    downscale(&img, s)
}

/// Média de caixas para `scale` (0 < scale < 1).
pub fn downscale(img: &egui::ColorImage, scale: f32) -> egui::ColorImage {
    let [w, h] = img.size;
    let (nw, nh) = (((w as f32 * scale).round() as usize).max(1), ((h as f32 * scale).round() as usize).max(1));
    let mut out = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for yy in y0..y1.min(h) {
                for xx in x0..x1.min(w) {
                    let c = img.pixels[yy * w + xx];
                    acc[0] += c.r() as u32;
                    acc[1] += c.g() as u32;
                    acc[2] += c.b() as u32;
                    acc[3] += c.a() as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            out.push(egui::Color32::from_rgba_premultiplied((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8));
        }
    }
    egui::ColorImage::new([nw, nh], out)
}

// ------------------------------------------------------------------------------------------------ erros e perda

static LOST: AtomicBool = AtomicBool::new(false);
static LOST_REASON: Mutex<String> = Mutex::new(String::new());
static OOM_EVENTS: AtomicU32 = AtomicU32::new(0);
static OOM_PENDING: AtomicBool = AtomicBool::new(false);
static OTHER_ERRORS: AtomicU32 = AtomicU32::new(0);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());
static ERROR_LINES: AtomicU32 = AtomicU32::new(0);
/// Simulação (bridge `gpu_simulate`): a próxima amostra trata o uso como este valor.
static SIM_USED: AtomicU64 = AtomicU64::new(0);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn logs_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("AnimeIAI").join("logs")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Uma linha em `logs\gpu-errors.log` (no máximo 200 por execução: um erro repetido a cada quadro não enche o disco).
fn log_line(text: &str) {
    if ERROR_LINES.fetch_add(1, Ordering::Relaxed) >= 200 {
        return;
    }
    let dir = logs_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("gpu-errors.log")) {
            let _ = writeln!(f, "{} pid {} {}", now_ms(), std::process::id(), text.replace('\n', " | "));
        }
    }
}

/// Camada do `tracing` que copia para `gpu-errors.log` os erros do wgpu e do gpu-allocator (inclusive os que chegam
/// pelo `log`). As mensagens do carregador Vulkan (`wgpu_hal::vulkan::instance`, ruído de toda abertura) ficam de fora.
pub struct WgpuLogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for WgpuLogLayer {
    fn on_event(&self, ev: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let m = ev.metadata();
        let t = m.target();
        if !(t.starts_with("wgpu") || t.starts_with("gpu_allocator")) || t.starts_with("wgpu_hal::vulkan::instance") {
            return;
        }
        struct V(String);
        impl tracing::field::Visit for V {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        let mut v = V(String::new());
        ev.record(&mut v);
        log_line(&format!("{} {t}: {}", m.level(), v.0));
    }
}

/// Instala no dispositivo do egui o aviso de perda e o tratador de erros sem captura (ver o topo do módulo).
pub fn install_device_hooks(device: &wgpu::Device) {
    device.set_device_lost_callback(|reason, message| {
        // `Destroyed` = o próprio app soltou o dispositivo (saída normal): não é perda
        if reason == wgpu::DeviceLostReason::Destroyed {
            return;
        }
        let text = format!("{reason:?}: {message}");
        *lock(&LOST_REASON) = text.clone();
        LOST.store(true, Ordering::SeqCst);
        log_line(&format!("DEVICE LOST {text}"));
        tracing::error!("GPU: dispositivo perdido ({text})");
    });
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
        let text = e.to_string();
        let text: String = text.chars().take(2000).collect();
        match e {
            wgpu::Error::OutOfMemory { .. } => {
                OOM_EVENTS.fetch_add(1, Ordering::SeqCst);
                OOM_PENDING.store(true, Ordering::SeqCst);
                tracing::error!("GPU sem memória: {text}");
                log_line(&format!("OUT OF MEMORY {text}"));
            }
            _ => {
                OTHER_ERRORS.fetch_add(1, Ordering::SeqCst);
                tracing::error!("erro do wgpu: {text}");
                log_line(&format!("ERROR {text}"));
            }
        }
        *lock(&LAST_ERROR) = text;
    }));
}

/// Motivo da perda do dispositivo, se aconteceu.
pub fn device_lost() -> Option<String> {
    LOST.load(Ordering::SeqCst).then(|| lock(&LOST_REASON).clone())
}

/// Marca a perda à mão (bridge `gpu_simulate lost`: exercita o caminho de saída sem precisar esgotar a placa).
pub fn simulate_lost(reason: &str) {
    *lock(&LOST_REASON) = format!("simulado: {reason}");
    LOST.store(true, Ordering::SeqCst);
}

/// Simula falta de memória (bridge `gpu_simulate oom`): o mesmo caminho do tratador de erros.
pub fn simulate_oom() {
    OOM_EVENTS.fetch_add(1, Ordering::SeqCst);
    OOM_PENDING.store(true, Ordering::SeqCst);
    *lock(&LAST_ERROR) = "simulado: Out of Memory".into();
}

/// Simula um uso de `bytes` na próxima amostra (0 desliga).
pub fn simulate_usage(bytes: u64) {
    SIM_USED.store(bytes, Ordering::SeqCst);
}

// ------------------------------------------------------------------------------------------------ reabrir e marcador

#[derive(Debug, Clone, Default)]
struct Relaunch {
    exe: Option<PathBuf>,
    /// Argumentos originais sem `--project`/`--wait-pid` (o projeto atual entra na hora).
    args: Vec<String>,
    allowed: bool,
    /// Esta execução já é uma reabertura depois de perder a GPU.
    is_relaunch: bool,
    started: Option<Instant>,
}

static RELAUNCH: Mutex<Option<Relaunch>> = Mutex::new(None);
static PROJECT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Guarda como reabrir este processo (chamado no `main`). `allowed` = execução interativa (sem bridge, sem captura):
/// instâncias de automação não se reabrem sozinhas (o agente que as abriu controla o pid).
pub fn set_relaunch(args: &[String], allowed: bool, is_relaunch: bool) {
    let mut kept = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--project" || a == "--wait-pid" {
            it.next();
            continue;
        }
        kept.push(a.clone());
    }
    *lock(&RELAUNCH) = Some(Relaunch { exe: std::env::current_exe().ok(), args: kept, allowed, is_relaunch, started: Some(Instant::now()) });
}

/// Projeto aberto agora (a reabertura volta nele).
pub fn set_project(p: Option<&Path>) {
    let mut cur = lock(&PROJECT);
    if cur.as_deref() != p {
        *cur = p.map(Path::to_path_buf);
    }
}

/// Marcador deixado por uma execução que perdeu a GPU: a próxima abertura explica o que houve.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LostMarker {
    pub pid: u32,
    pub at_ms: u64,
    pub reason: String,
    pub project: Option<String>,
    /// Uso de VRAM do processo e orçamento na última amostra (MB).
    pub used_mb: u64,
    pub limit_mb: u64,
    pub instances: u32,
    pub relaunched: bool,
    /// "frame" (saída limpa antes de pintar) ou "panic" (perda dentro da pintura do egui).
    pub path: String,
}

static LAST_USED: AtomicU64 = AtomicU64::new(0);
static LAST_LIMIT: AtomicU64 = AtomicU64::new(0);
static LAST_INSTANCES: AtomicU32 = AtomicU32::new(1);

fn marker_path(pid: u32) -> PathBuf {
    logs_dir().join(format!("gpu-lost-{pid}.json"))
}

/// Escreve o marcador e, se permitido, reabre o app no mesmo projeto. Devolve o marcador gravado.
pub fn record_loss_and_relaunch(path_kind: &str) -> LostMarker {
    let reason = lock(&LOST_REASON).clone();
    let project = lock(&PROJECT).clone();
    let rl = lock(&RELAUNCH).clone().unwrap_or_default();
    // reabertura que perde a GPU de novo em menos de 2 minutos não se reabre outra vez (sem laço de quedas)
    let too_soon = rl.is_relaunch && rl.started.is_some_and(|s| s.elapsed() < Duration::from_secs(120));
    let mut m = LostMarker {
        pid: std::process::id(),
        at_ms: now_ms(),
        reason: if reason.is_empty() { "desconhecido".into() } else { reason },
        project: project.as_ref().map(|p| p.display().to_string()),
        used_mb: LAST_USED.load(Ordering::Relaxed) / MB,
        limit_mb: LAST_LIMIT.load(Ordering::Relaxed) / MB,
        instances: LAST_INSTANCES.load(Ordering::Relaxed),
        relaunched: false,
        path: path_kind.into(),
    };
    if rl.allowed && !too_soon {
        if let Some(exe) = &rl.exe {
            let mut cmd = std::process::Command::new(exe);
            cmd.args(&rl.args);
            if let Some(p) = &project {
                cmd.arg("--project").arg(p);
            }
            cmd.arg("--wait-pid").arg(std::process::id().to_string());
            m.relaunched = cmd.spawn().is_ok();
        }
    }
    let _ = std::fs::create_dir_all(logs_dir());
    if let Ok(text) = serde_json::to_string_pretty(&m) {
        let _ = std::fs::write(marker_path(m.pid), text);
    }
    log_line(&format!("LOSS RECORDED path={path_kind} relaunched={}", m.relaunched));
    // a saída não passa pelo Drop do `GpuMem`: o registro desta instância sai aqui
    registry_remove();
    m
}

/// Chamado pelo gancho de pânico: se o pânico vem da GPU (dispositivo perdido ou sem memória), grava o marcador,
/// reabre o app e devolve o cabeçalho que explica a queda no relatório.
pub fn panic_note(message: &str) -> Option<String> {
    let gpu_msg = ["Failed to create staging buffer", "Out of Memory", "OutOfMemory", "Device lost", "DeviceLost", "Parent device is lost"];
    let lost = LOST.load(Ordering::SeqCst);
    if !lost && !gpu_msg.iter().any(|k| message.contains(k)) {
        return None;
    }
    if !lost {
        *lock(&LOST_REASON) = "pânico do renderer com a GPU sem memória".into();
    }
    let m = record_loss_and_relaunch("panic");
    Some(format!(
        "CAUSA: memória de vídeo esgotada / dispositivo de vídeo perdido ({}).\nUso de VRAM do processo na última medida: {} MB de {} MB de limite; instâncias do AnimeIAI abertas: {}.\nO diário do projeto (wal.ndjson) é gravado a cada comando: nada confirmado se perdeu. {}\nPara reduzir: feche outras instâncias do AnimeIAI e programas que usam a placa, ou baixe o orçamento em Preferências > Desempenho.\n",
        m.reason,
        m.used_mb,
        m.limit_mb,
        m.instances,
        if m.relaunched { "O app foi reaberto no mesmo projeto." } else { "Abra o app de novo." }
    ))
}

/// Marcadores pendentes: o desta reabertura (`wait_pid`) e, numa abertura normal com sessão, os de processos que já
/// saíram. Cada marcador lido é apagado.
pub fn take_markers(wait_pid: Option<u32>, consume_others: bool) -> Vec<LostMarker> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(logs_dir()) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix("gpu-lost-").and_then(|s| s.strip_suffix(".json")).and_then(|s| s.parse::<u32>().ok()) else { continue };
        let mine = wait_pid == Some(pid);
        if !mine && (!consume_others || process_alive(pid)) {
            continue;
        }
        if let Some(m) = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<LostMarker>(&b).ok()) {
            out.push(m);
        }
        let _ = std::fs::remove_file(e.path());
    }
    out.sort_by_key(|m| m.at_ms);
    out
}

// ------------------------------------------------------------------------------------------------ processos

/// O processo `pid` ainda está vivo?
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 {
        return false;
    }
    // SAFETY: handle aberto aqui e fechado logo depois; `code` é nosso.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(windows))]
pub fn process_alive(pid: u32) -> bool {
    pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
}

/// Espera o processo `pid` sair (até `timeout`). Usado pela reabertura: a trava do projeto e o diário são soltos antes.
#[cfg(windows)]
pub fn wait_for_exit(pid: u32, timeout: Duration) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: handle aberto aqui e fechado logo depois.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return;
        }
        WaitForSingleObject(h, timeout.as_millis().min(u32::MAX as u128) as u32);
        CloseHandle(h);
    }
}

#[cfg(not(windows))]
pub fn wait_for_exit(pid: u32, timeout: Duration) {
    let t = Instant::now();
    while process_alive(pid) && t.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ------------------------------------------------------------------------------------------------ instâncias

fn instances_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("AnimeIAI").join("instances")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InstanceInfo {
    pid: u32,
    exe: String,
    started_ms: u64,
    used_mb: u64,
    limit_mb: u64,
}

/// Grava/atualiza o registro desta instância e devolve quantas instâncias vivas há (esta inclusive). Registros de
/// processos mortos (queda) são apagados.
fn registry_update(used: u64, limit: u64, started_ms: u64) -> (u32, u64) {
    let dir = instances_dir();
    let me = std::process::id();
    if std::fs::create_dir_all(&dir).is_ok() {
        let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let info = InstanceInfo { pid: me, exe, started_ms, used_mb: used / MB, limit_mb: limit / MB };
        if let Ok(t) = serde_json::to_vec(&info) {
            let tmp = dir.join(format!("{me}.json.tmp"));
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join(format!("{me}.json")));
            }
        }
    }
    let (mut n, mut others_mb) = (1u32, 0u64);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pid) = name.strip_suffix(".json").and_then(|s| s.parse::<u32>().ok()) else { continue };
            if pid == me {
                continue;
            }
            if process_alive(pid) {
                n += 1;
                if let Some(i) = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<InstanceInfo>(&b).ok()) {
                    others_mb += i.used_mb;
                }
            } else {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    (n, others_mb)
}

fn registry_remove() {
    let _ = std::fs::remove_file(instances_dir().join(format!("{}.json", std::process::id())));
}

// ------------------------------------------------------------------------------------------------ DXGI

/// Memória do adaptador e do processo segundo o sistema.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OsVram {
    /// Memória dedicada do adaptador.
    pub dedicated: u64,
    /// Memória compartilhada (RAM do sistema que a placa pode usar).
    pub shared: u64,
    /// Uso do processo na memória da placa.
    pub local_usage: u64,
    /// Orçamento do sistema para este processo na memória da placa (encolhe com outros programas na placa).
    pub local_budget: u64,
    /// Uso do processo na memória compartilhada (o que transbordou ou foi posto na RAM).
    pub nonlocal_usage: u64,
}

#[cfg(windows)]
mod dxgi {
    use super::OsVram;
    use std::cell::RefCell;
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
        DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    thread_local! {
        /// O adaptador DXGI do dispositivo do egui (COM: fica na thread que o achou, a da interface).
        static ADAPTER: RefCell<Option<(IDXGIAdapter3, u64, u64)>> = const { RefCell::new(None) };
    }

    /// Acha o adaptador DXGI pelo fabricante/dispositivo PCI (ou pelo nome) do adaptador do wgpu.
    pub fn bind(vendor: u32, device: u32, name: &str) -> bool {
        // SAFETY: chamadas COM com valores próprios; os ponteiros devolvidos são donos (Drop faz Release).
        let found = unsafe {
            let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else { return false };
            let mut found = None;
            for i in 0..16 {
                let Ok(a) = f.EnumAdapters1(i) else { break };
                let Ok(d) = a.GetDesc1() else { continue };
                if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let n = String::from_utf16_lossy(&d.Description);
                let n = n.trim_end_matches('\0');
                let by_id = vendor != 0 && d.VendorId == vendor && (device == 0 || d.DeviceId == device);
                if by_id || (!name.is_empty() && n == name) {
                    if let Ok(a3) = a.cast::<IDXGIAdapter3>() {
                        found = Some((a3, d.DedicatedVideoMemory as u64, d.SharedSystemMemory as u64));
                        break;
                    }
                }
            }
            found
        };
        let ok = found.is_some();
        ADAPTER.with(|c| *c.borrow_mut() = found);
        ok
    }

    pub fn query() -> Option<OsVram> {
        ADAPTER.with(|c| {
            let c = c.borrow();
            let (a, dedicated, shared) = c.as_ref()?;
            let mut l = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            let mut n = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            // SAFETY: estruturas próprias, adaptador vivo.
            unsafe {
                a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut l).ok()?;
                a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut n).ok()?;
            }
            Some(OsVram { dedicated: *dedicated, shared: *shared, local_usage: l.CurrentUsage, local_budget: l.Budget, nonlocal_usage: n.CurrentUsage })
        })
    }
}

#[cfg(not(windows))]
mod dxgi {
    pub fn bind(_: u32, _: u32, _: &str) -> bool {
        false
    }
    pub fn query() -> Option<super::OsVram> {
        None
    }
}

// ------------------------------------------------------------------------------------------------ contabilidade

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubUse {
    pub key: &'static str,
    pub bytes: u64,
    pub count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Texturas do egui (todas) e por subsistema.
    pub egui_bytes: u64,
    pub egui_count: u32,
    pub subs: Vec<SubUse>,
    /// As maiores texturas do egui: (nome, [l, a], bytes).
    pub largest: Vec<(String, [usize; 2], u64)>,
    /// Recursos que o serviço 3D guarda entre quadros.
    pub native_3d: u64,
    /// Relatório do alocador do wgpu: alocado e reservado (blocos).
    pub wgpu_allocated: Option<u64>,
    pub wgpu_reserved: Option<u64>,
    /// Blocos de memória do alocador e as maiores alocações agrupadas pelo rótulo do wgpu (nome, bytes, quantas).
    pub wgpu_blocks: u32,
    pub wgpu_top: Vec<(String, u64, u32)>,
    pub os: Option<OsVram>,
    /// Orçamento (automático ou das Preferências) e limite efetivo (menor entre orçamento e 90% do sistema).
    pub budget: u64,
    pub budget_auto: bool,
    pub limit: u64,
    /// Uso que conta para a pressão: o do sistema quando há, senão alocado pelo wgpu (ou estimado).
    pub used: u64,
    pub pressure: Pressure,
    /// Instâncias do AnimeIAI vivas (esta inclusive) e quanto as outras dizem usar.
    pub instances: u32,
    pub others_mb: u64,
}

/// Contabilidade e orçamento de VRAM do app (vive no `Editor`).
pub struct GpuMem {
    device: Option<wgpu::Device>,
    os_bound: bool,
    /// Memória dedicada informada pelo wgpu/DXGI (0 = desconhecida).
    dedicated_hint: u64,
    /// Orçamento escolhido nas Preferências (MB; 0 = automático).
    pub budget_pref_mb: u32,
    pub snap: Snapshot,
    last_sample: Option<Instant>,
    last_report: Option<Instant>,
    last_registry: Option<Instant>,
    started_ms: u64,
    /// Bytes do egui na última amostra do sistema (o que cresceu depois ainda não aparece no uso do sistema).
    egui_at_sample: u64,
    /// Falta de memória vista: crítica até este instante.
    oom_until: Option<Instant>,
    /// Passadas de despejo e bytes soltos (telemetria).
    pub evictions: u64,
    pub evicted_bytes: u64,
    pub last_eviction: Option<(Pressure, u64)>,
    registered: bool,
}

impl Default for GpuMem {
    fn default() -> Self {
        GpuMem {
            device: None,
            os_bound: false,
            dedicated_hint: 0,
            budget_pref_mb: 0,
            snap: Snapshot { instances: 1, ..Default::default() },
            last_sample: None,
            last_report: None,
            last_registry: None,
            started_ms: now_ms(),
            egui_at_sample: 0,
            oom_until: None,
            evictions: 0,
            evicted_bytes: 0,
            last_eviction: None,
            registered: false,
        }
    }
}

impl Drop for GpuMem {
    fn drop(&mut self) {
        if self.registered {
            registry_remove();
        }
    }
}

impl GpuMem {
    /// Liga ao dispositivo do egui: ganchos de erro/perda, adaptador DXGI e registro de instância.
    pub fn install(&mut self, rs: &egui_wgpu::RenderState) {
        if self.device.is_some() {
            return;
        }
        install_device_hooks(&rs.device);
        let info = rs.adapter.get_info();
        self.os_bound = dxgi::bind(info.vendor, info.device, &info.name);
        if let Some(os) = dxgi::query() {
            self.dedicated_hint = os.dedicated;
        }
        self.device = Some(rs.device.clone());
        self.registered = true;
        tracing::info!("VRAM: adaptador {} ({}), memória dedicada {} MB", info.name, if self.os_bound { "DXGI" } else { "sem DXGI" }, self.dedicated_hint / MB);
    }

    /// Orçamento para `instances` instâncias vivas: o das Preferências, ou o automático.
    pub fn budget_for(&self, instances: u32) -> (u64, bool) {
        if self.budget_pref_mb > 0 {
            return (self.budget_pref_mb as u64 * MB, false);
        }
        (auto_budget(self.dedicated_hint, instances), true)
    }

    /// Uma vez por quadro, no fim (antes da pintura): soma as texturas do egui (barato), amostra sistema e alocador a
    /// cada segundo (4×/s sob pressão) e devolve a pressão atual.
    pub fn tick(&mut self, ctx: &egui::Context, native_3d: u64) -> Pressure {
        let now = Instant::now();
        self.sample_egui(ctx);
        self.snap.native_3d = native_3d;
        let every = if self.snap.pressure == Pressure::Normal { Duration::from_millis(1000) } else { Duration::from_millis(250) };
        if self.last_sample.is_none_or(|t| now - t >= every) {
            self.last_sample = Some(now);
            self.snap.os = if self.os_bound { dxgi::query() } else { None };
            self.egui_at_sample = self.snap.egui_bytes;
            if let Some(d) = &self.device {
                if self.last_report.is_none_or(|t| now - t >= Duration::from_secs(2)) {
                    self.last_report = Some(now);
                    if let Some(r) = d.generate_allocator_report() {
                        self.snap.wgpu_allocated = Some(r.total_allocated_bytes);
                        self.snap.wgpu_reserved = Some(r.total_reserved_bytes);
                        self.snap.wgpu_blocks = r.blocks.len() as u32;
                        let mut by: std::collections::HashMap<&str, (u64, u32)> = std::collections::HashMap::new();
                        for a in &r.allocations {
                            let e = by.entry(a.name.as_str()).or_default();
                            e.0 += a.size;
                            e.1 += 1;
                        }
                        let mut top: Vec<(String, u64, u32)> = by.into_iter().map(|(n, (b, c))| (n.to_owned(), b, c)).collect();
                        top.sort_by(|a, b| b.1.cmp(&a.1));
                        top.truncate(10);
                        self.snap.wgpu_top = top;
                    }
                }
            }
            if self.registered && self.last_registry.is_none_or(|t| now - t >= Duration::from_secs(5)) {
                self.last_registry = Some(now);
                let (n, others) = registry_update(self.snap.used, self.snap.limit, self.started_ms);
                self.snap.instances = n;
                self.snap.others_mb = others;
            }
        }
        let (budget, auto) = self.budget_for(self.snap.instances);
        self.snap.budget = budget;
        self.snap.budget_auto = auto;
        let os_cap = self.snap.os.map(|o| o.local_budget / 10 * 9).filter(|&b| b > 0);
        self.snap.limit = os_cap.map_or(budget, |c| c.min(budget));
        // uso: o do sistema (mais o que o egui criou depois da amostra, ainda não enviado à placa); sem sistema, o
        // reservado pelo wgpu; sem nada disso, a soma do que se conhece
        let grown = self.snap.egui_bytes.saturating_sub(self.egui_at_sample);
        let base = match (self.snap.os, self.snap.wgpu_reserved) {
            (Some(o), _) => o.local_usage,
            (None, Some(r)) => r,
            (None, None) => self.snap.egui_bytes + self.snap.native_3d,
        };
        let sim = SIM_USED.load(Ordering::Relaxed);
        self.snap.used = if sim > 0 { sim } else { base + grown };
        let mut p = Pressure::from_ratio(self.snap.used, self.snap.limit, self.snap.pressure);
        if OOM_PENDING.swap(false, Ordering::SeqCst) {
            self.oom_until = Some(now + Duration::from_secs(10));
        }
        if self.oom_until.is_some_and(|t| now < t) {
            p = Pressure::Critical;
        }
        self.snap.pressure = p;
        CURRENT_PRESSURE.store(p as u32, Ordering::Relaxed);
        LAST_USED.store(self.snap.used, Ordering::Relaxed);
        LAST_LIMIT.store(self.snap.limit, Ordering::Relaxed);
        LAST_INSTANCES.store(self.snap.instances, Ordering::Relaxed);
        p
    }

    fn sample_egui(&mut self, ctx: &egui::Context) {
        let tm = ctx.tex_manager();
        let tm = tm.read();
        let mut subs: Vec<SubUse> = SUBSYSTEMS.iter().map(|(k, _)| SubUse { key: k, bytes: 0, count: 0 }).collect();
        subs.push(SubUse { key: "other", bytes: 0, count: 0 });
        let (mut total, mut count) = (0u64, 0u32);
        let mut largest: Vec<(String, [usize; 2], u64)> = Vec::new();
        for (_, meta) in tm.allocated() {
            let b = (meta.size[0] * meta.size[1] * meta.bytes_per_pixel) as u64;
            total += b;
            count += 1;
            let k = classify(&meta.name);
            if let Some(s) = subs.iter_mut().find(|s| s.key == k) {
                s.bytes += b;
                s.count += 1;
            }
            if largest.len() < 8 || largest.last().is_some_and(|l| l.2 < b) {
                largest.push((meta.name.clone(), meta.size, b));
                largest.sort_by(|a, b| b.2.cmp(&a.2));
                largest.truncate(8);
            }
        }
        self.snap.egui_bytes = total;
        self.snap.egui_count = count;
        self.snap.subs = subs;
        self.snap.largest = largest;
    }

    /// Bytes do egui num subsistema.
    pub fn sub_bytes(&self, key: &str) -> u64 {
        self.snap.subs.iter().find(|s| s.key == key).map_or(0, |s| s.bytes)
    }

    pub fn pressure(&self) -> Pressure {
        self.snap.pressure
    }

    /// Registra uma passada de despejo (`freed` = bytes do egui que sumiram).
    pub fn note_eviction(&mut self, p: Pressure, freed: u64) {
        self.evictions += 1;
        self.evicted_bytes += freed;
        self.last_eviction = Some((p, freed));
        if freed > 0 {
            tracing::info!("VRAM: pressão {} soltou {} MB de texturas", p.key(), freed / MB);
        }
    }

    /// Estado para a bridge (`ui_state["gpu"]`) e para o painel das Preferências.
    pub fn to_json(&self) -> Value {
        let s = &self.snap;
        let mb = |b: u64| (b as f64 / MB as f64 * 10.0).round() / 10.0;
        let last_error = Some(lock(&LAST_ERROR).clone()).filter(|l| !l.is_empty());
        json!({
            "pressure": s.pressure.key(),
            "preview_scale": s.pressure.preview_scale(),
            "budget_mb": mb(s.budget),
            "budget_auto": s.budget_auto,
            "limit_mb": mb(s.limit),
            "used_mb": mb(s.used),
            "instances": s.instances,
            "other_instances_used_mb": s.others_mb,
            "egui": {
                "total_mb": mb(s.egui_bytes),
                "textures": s.egui_count,
                "by_subsystem": s.subs.iter().map(|u| (u.key.to_owned(), json!({ "mb": mb(u.bytes), "textures": u.count }))).collect::<serde_json::Map<String, Value>>(),
                "largest": s.largest.iter().map(|(n, sz, b)| json!({ "name": n, "size": sz, "mb": mb(*b) })).collect::<Vec<_>>(),
            },
            "service3d_mb": mb(s.native_3d),
            "wgpu_allocator": {
                "allocated_mb": s.wgpu_allocated.map(mb), "reserved_mb": s.wgpu_reserved.map(mb), "blocks": s.wgpu_blocks,
                "largest": s.wgpu_top.iter().map(|(n, b, c)| json!({ "name": n, "mb": mb(*b), "count": c })).collect::<Vec<_>>(),
            },
            "os": s.os.map(|o| json!({
                "dedicated_mb": mb(o.dedicated), "shared_mb": mb(o.shared), "process_local_mb": mb(o.local_usage),
                "process_budget_mb": mb(o.local_budget), "process_nonlocal_mb": mb(o.nonlocal_usage),
            })),
            "errors": {
                "out_of_memory": OOM_EVENTS.load(Ordering::Relaxed),
                "other": OTHER_ERRORS.load(Ordering::Relaxed),
                "last": last_error,
                "device_lost": device_lost(),
            },
            "evictions": { "passes": self.evictions, "freed_mb": mb(self.evicted_bytes), "last": self.last_eviction.map(|(p, b)| json!({ "pressure": p.key(), "freed_mb": mb(b) })) },
        })
    }
}

/// Orçamento automático: 60% da memória dedicada (1,5 GB se desconhecida) dividido pelas instâncias vivas, com piso.
pub fn auto_budget(dedicated: u64, instances: u32) -> u64 {
    let total = if dedicated > 0 { (dedicated as f64 * AUTO_FRACTION) as u64 } else { FALLBACK_BUDGET };
    (total / instances.max(1) as u64).max(MIN_AUTO_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_texture_names() {
        assert_eq!(classify("canvas-image"), "canvas");
        assert_eq!(classify("live-stroke"), "overlays");
        assert_eq!(classify("move-below"), "overlays");
        assert_eq!(classify("lib-thumb-abc"), "thumbs_library");
        assert_eq!(classify("lib-turn-x"), "view3d");
        assert_eq!(classify("cut-thumb-3"), "thumbs_production");
        assert_eq!(classify("fx_thumb_glow"), "thumbs_effects");
        assert_eq!(classify("egui_font_texture"), "ui");
        assert_eq!(classify("whatever"), "other");
    }

    #[test]
    fn auto_budget_shrinks_with_instances_and_has_floor() {
        let four_gb = 4096 * MB;
        assert_eq!(auto_budget(four_gb, 1), (four_gb as f64 * 0.6) as u64);
        assert!(auto_budget(four_gb, 2) < auto_budget(four_gb, 1));
        assert_eq!(auto_budget(four_gb, 8), MIN_AUTO_BUDGET);
        assert_eq!(auto_budget(0, 1), FALLBACK_BUDGET);
    }

    #[test]
    fn pressure_has_hysteresis() {
        let l = 1000;
        assert_eq!(Pressure::from_ratio(500, l, Pressure::Normal), Pressure::Normal);
        assert_eq!(Pressure::from_ratio(810, l, Pressure::Normal), Pressure::High);
        assert_eq!(Pressure::from_ratio(960, l, Pressure::High), Pressure::Critical);
        // desce só abaixo de 85% / 70%
        assert_eq!(Pressure::from_ratio(900, l, Pressure::Critical), Pressure::Critical);
        assert_eq!(Pressure::from_ratio(800, l, Pressure::Critical), Pressure::High);
        assert_eq!(Pressure::from_ratio(750, l, Pressure::High), Pressure::High);
        assert_eq!(Pressure::from_ratio(600, l, Pressure::High), Pressure::Normal);
    }

    #[test]
    fn downscale_halves_and_averages() {
        let img = egui::ColorImage::new([4, 2], vec![egui::Color32::WHITE, egui::Color32::BLACK, egui::Color32::WHITE, egui::Color32::BLACK, egui::Color32::WHITE, egui::Color32::BLACK, egui::Color32::WHITE, egui::Color32::BLACK]);
        let d = downscale(&img, 0.5);
        assert_eq!(d.size, [2, 1]);
        assert!(d.pixels.iter().all(|c| (126..=128).contains(&c.r())));
    }

    #[test]
    fn cache_cap_shrinks_under_pressure() {
        let b = 1000 * MB;
        assert!(cache_cap(b, 5, Pressure::Critical) < cache_cap(b, 5, Pressure::High));
        assert!(cache_cap(b, 5, Pressure::High) < cache_cap(b, 5, Pressure::Normal));
    }

    #[test]
    fn relaunch_args_drop_project_and_wait_pid() {
        set_relaunch(&["--project".into(), "C:/a".into(), "--lang".into(), "en".into(), "--wait-pid".into(), "12".into()], true, false);
        let r = lock(&RELAUNCH).clone().unwrap();
        assert_eq!(r.args, vec!["--lang".to_string(), "en".to_string()]);
    }
}

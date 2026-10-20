use aiai_studio::app::StudioApp;
use aiai_studio::cli;

const USAGE: &str = "uso: aiai-studio [--no-session] [--session-file <json>] [--project <pasta>] [--lang pt|en] [--theme light|dark] \
[--workspace <chave>] [--size LxA] [--bridge] [--screenshot <png>] [--frames <n>] [--splash | --no-splash] [--wait-pid <pid>]";

/// Pânico vira arquivo em `%LOCALAPPDATA%\AnimeIAI\logs\crash-<unix>.txt` (mensagem, local, backtrace): aberto pelo
/// Explorer ou pelo harness o app não tem console, e uma queda sumia sem rastro (S20 P6e).
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("AnimeIAI").join("logs");
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        // pânico vindo da GPU (memória de vídeo esgotada / dispositivo perdido): a causa vai no topo do relatório, um
        // marcador explica na próxima abertura e o app se reabre no mesmo projeto (gpu_mem)
        let gpu = aiai_studio::gpu_mem::panic_note(&info.to_string()).unwrap_or_default();
        let text = format!("AnimeIAI Studio {} (pid {})\n{gpu}thread '{thread}': {info}\n\n{bt}\n", env!("CARGO_PKG_VERSION"), std::process::id());
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join(format!("crash-{secs}.txt")), text);
        }
        default(info);
    }));
}

/// Tela de abertura (ver `splash.rs`): sobe antes da GPU e do projeto; o tema e o idioma vêm da linha de comando ou das
/// preferências já lidas.
fn start_splash(cli: &cli::Cli, startup: &aiai_studio::prefs::Prefs) {
    if !cli.wants_splash() {
        return;
    }
    aiai_studio::i18n::set_lang(cli.lang.unwrap_or(startup.lang));
    let dark = cli.theme.unwrap_or(startup.theme) == aiai_studio::theme::ThemeKind::Dark;
    use aiai_studio::i18n::tr;
    aiai_studio::splash::start(aiai_studio::splash::SplashInfo {
        dark,
        version: format!("{} {}", tr("about.version"), aiai_studio::brand::VERSION),
        tagline: tr("about.tagline").to_owned(),
        status: tr("splash.gpu").to_owned(),
    });
}

fn main() -> eframe::Result {
    aiai_studio::brand::enable_dpi_awareness();
    install_crash_log();
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,naga=warn".into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        // erros do wgpu/gpu-allocator também em logs\gpu-errors.log: o app aberto pelo Explorer não tem console, e a causa
        // de uma perda de dispositivo (ex.: ERROR_OUT_OF_HOST_MEMORY no vkAllocateMemory) só aparece nesse log
        .with(aiai_studio::gpu_mem::WgpuLogLayer.with_filter(tracing_subscriber::filter::LevelFilter::ERROR))
        .init();
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("erro: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    // reabertura depois de perder a GPU: o processo anterior solta a trava do projeto e sai antes
    if let Some(pid) = cli.wait_pid {
        aiai_studio::gpu_mem::wait_for_exit(pid, std::time::Duration::from_secs(15));
    }
    // execuções interativas se reabrem sozinhas se a GPU se perder; as de automação (bridge, captura) não
    let args: Vec<String> = std::env::args().skip(1).collect();
    aiai_studio::gpu_mem::set_relaunch(&args, !cli.bridge && cli.screenshot.is_none(), cli.wait_pid.is_some());
    let (w, h) = cli.size.unwrap_or((1440.0, 900.0));
    let mut wgpu_setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    // Teste da resiliência de VRAM sem esgotar a placa de verdade (que é dividida com outros programas): o wgpu trata o
    // processo como sem memória quando o uso dele passa da porcentagem dada do orçamento do sistema. `..._LOSS_PCT` perde
    // o dispositivo no envio seguinte; `..._OOM_PCT` faz criar textura/buffer falhar (buffer de passagem perde o
    // dispositivo, como na queda real). Sem as variáveis, nada muda.
    let pct = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u8>().ok()).filter(|p| (1..=100).contains(p));
    wgpu_setup.instance_descriptor.memory_budget_thresholds.for_device_loss = pct("AIAI_GPU_TEST_LOSS_PCT");
    wgpu_setup.instance_descriptor.memory_budget_thresholds.for_resource_creation = pct("AIAI_GPU_TEST_OOM_PCT");
    // Blocos pequenos no alocador do wgpu (gpu-allocator). O padrão (`Performance`) reserva 128 MB de VRAM + 64 MB de
    // memória mapeável já no primeiro quadro (medido: 192 MB reservados para 12 MB usados) e pede blocos novos desse
    // tamanho; com a RAM apertada um pedido de 64 MB mapeável falha com ERROR_OUT_OF_HOST_MEMORY, que o gpu-allocator
    // devolve como erro interno e o wgpu-hal transforma em dispositivo perdido. `MemoryUsage` usa blocos de 8-64 MB
    // (VRAM) e 4-32 MB (mapeável): várias instâncias abertas reservam bem menos e cada pedido ao driver é menor.
    let base_device = std::sync::Arc::clone(&wgpu_setup.device_descriptor);
    wgpu_setup.device_descriptor = std::sync::Arc::new(move |adapter| wgpu::DeviceDescriptor { memory_hints: wgpu::MemoryHints::MemoryUsage, ..base_device(adapter) });
    // GTX 1650 (GPU dedicada) em vez da Intel UHD (licao 24), salvo se o usuário escolheu a integrada nas Preferências
    let startup = aiai_studio::prefs::load_for_startup(cli.no_session, cli.session_file.as_deref());
    start_splash(&cli, &startup);
    wgpu_setup.power_preference = match startup.gpu {
        aiai_studio::prefs::GpuPref::Dedicated => wgpu::PowerPreference::HighPerformance,
        aiai_studio::prefs::GpuPref::Integrated => wgpu::PowerPreference::LowPower,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnimeIAI")
            // ícone inicial (variante do tema do Windows, no tamanho do ícone grande do sistema); `StudioApp::new` refaz o par
            // pequeno + grande certo e acompanha o tema (brand.rs). O ícone do exe vem do recurso embutido (build.rs).
            .with_icon(std::sync::Arc::new(aiai_studio::brand::window_icon()))
            .with_inner_size([w, h])
            .with_min_inner_size([1024.0, 680.0]),
        renderer: eframe::Renderer::Wgpu,
        // O app só persiste `prefs.json` (prefs.rs). O armazenamento do eframe nunca é escrito (nem posição de janela, nem
        // memória do egui) e aponta para um caminho temporário para não criar pastas em %APPDATA% com --no-session.
        persist_window: false,
        persistence_path: Some(std::env::temp_dir().join("aiai-eframe-unused.ron")),
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(wgpu_setup),
            // 1 quadro na fila em vez de 2: o que se desenha acompanha o mouse um quadro mais cedo (o canvas de
            // pintura pede latência, não vazão)
            surface: egui_wgpu::SurfaceConfig::LOW_LATENCY,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native("AnimeIAI", options, Box::new(move |cc| Ok(Box::new(StudioApp::new(cc, cli)))))
}

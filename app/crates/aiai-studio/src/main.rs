use aiai_studio::app::StudioApp;
use aiai_studio::cli;

const USAGE: &str = "uso: aiai-studio [--no-session] [--session-file <json>] [--project <pasta>] [--lang pt|en] [--theme light|dark] \
[--workspace <chave>] [--size LxA] [--bridge] [--screenshot <png>] [--frames <n>]";

/// Pânico vira arquivo em `%LOCALAPPDATA%\AnimeIAI\logs\crash-<unix>.txt` (mensagem, local, backtrace): aberto pelo
/// Explorer ou pelo harness o app não tem console, e uma queda sumia sem rastro (S20 P6e).
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("AnimeIAI").join("logs");
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!("AnimeIAI Studio {} (pid {})\nthread '{thread}': {info}\n\n{bt}\n", env!("CARGO_PKG_VERSION"), std::process::id());
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join(format!("crash-{secs}.txt")), text);
        }
        default(info);
    }));
}

fn main() -> eframe::Result {
    install_crash_log();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,naga=warn".into()))
        .init();
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("erro: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let (w, h) = cli.size.unwrap_or((1440.0, 900.0));
    let mut wgpu_setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    // GTX 1650 (GPU dedicada) em vez da Intel UHD (licao 24), salvo se o usuário escolheu a integrada nas Preferências
    let startup = aiai_studio::prefs::load_for_startup(cli.no_session, cli.session_file.as_deref());
    wgpu_setup.power_preference = match startup.gpu {
        aiai_studio::prefs::GpuPref::Dedicated => wgpu::PowerPreference::HighPerformance,
        aiai_studio::prefs::GpuPref::Integrated => wgpu::PowerPreference::LowPower,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnimeIAI")
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
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native("AnimeIAI", options, Box::new(move |cc| Ok(Box::new(StudioApp::new(cc, cli)))))
}

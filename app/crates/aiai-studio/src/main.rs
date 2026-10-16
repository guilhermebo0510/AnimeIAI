use aiai_studio::app::StudioApp;
use aiai_studio::cli;

const USAGE: &str = "uso: aiai-studio [--no-session] [--session-file <json>] [--project <pasta>] [--lang pt|en] [--theme light|dark] \
[--workspace <chave>] [--size LxA] [--bridge] [--screenshot <png>] [--frames <n>]";

fn main() -> eframe::Result {
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
    // GTX 1650 (GPU dedicada) em vez da Intel UHD (licao 24)
    wgpu_setup.power_preference = wgpu::PowerPreference::HighPerformance;
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

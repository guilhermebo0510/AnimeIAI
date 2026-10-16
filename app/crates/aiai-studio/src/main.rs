fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("AnimeIAI").with_inner_size([1440.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("aiai-studio", options, Box::new(|_cc| Ok(Box::new(Hello))))
}

struct Hello;
impl eframe::App for Hello {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| ui.heading("AnimeIAI"));
    }
}

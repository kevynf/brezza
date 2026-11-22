#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("brezza")
            .with_inner_size([1280.0, 800.0])
            .with_icon(egui::IconData::default()),
        persistence_path: std::env::var_os("BREZZA_STORAGE").map(Into::into),
        ..Default::default()
    };
    eframe::run_native(
        "brezza",
        options,
        Box::new(|cc| Ok(Box::new(brezza::app::App::new(cc)))),
    )
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod report;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RedPulse")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RedPulse",
        options,
        Box::new(|cc| Ok(Box::new(app::RedPulseApp::new(cc)))),
    )
}

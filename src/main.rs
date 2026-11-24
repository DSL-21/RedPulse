#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod report;
mod recon;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RedPulse")
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([840.0, 580.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RedPulse",
        options,
        Box::new(|cc| Ok(Box::new(app::RedPulseApp::new(cc)))),
    )
}

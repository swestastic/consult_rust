#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod consult;
mod logfile;
mod logging;
mod pages;
mod widgets;

use app::ConsultApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 740.0])
            .with_min_inner_size([820.0, 620.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Consult Rust",
        options,
        Box::new(|_cc| Ok(Box::new(ConsultApp::new()))),
    )
}

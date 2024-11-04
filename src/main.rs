#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod logic;
mod ui;

fn main() -> eframe::Result {
    dotenvy::dotenv().ok();
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };

    eframe::run_native(
        "RoboJules",
        native_options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )?;

    Ok(())
}

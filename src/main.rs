fn main() -> eframe::Result<()> {
    eframe::run_native(
        "Stock & Currency Tracker",
        eframe::NativeOptions::default(),
        Box::new(|_cc| {
            let _ = egui::ViewportBuilder::default();
            Ok(Box::new(app::App::new()))
        }),
    )
}

mod app;
mod data;
mod db;
mod models;

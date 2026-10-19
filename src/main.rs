//! Native entry point.
//!
//! Wires together the eframe window (dark theme, sensible default size) with the
//! [`app::App`], which owns the bundled database, the background workers and the
//! render loop.

fn main() -> eframe::Result<()> {
    // Initialise logging so data-fetch / database problems surface on stderr.
    let _ = env_logger::init();

    let native_options = eframe::NativeOptions {
        // A large default window gives the watchlist side panel and the chart
        // area room to breathe.
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Stock & Currency Tracker",
        native_options,
        Box::new(|cc| {
            // egui's default theme is light; the app is designed for a dark UI.
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app::App::new()))
        }),
    )
}

mod app;
mod charts;
mod data;
mod db;
mod models;

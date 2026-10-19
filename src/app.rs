//! Application entry point.
//!
//! A minimal `eframe` application shell. UI panels, charts and data plumbing are
//! wired in during the integration phase; for now the window renders empty.

/// Root application object. Implements `eframe::App` so it can drive the
/// egui render loop.
pub struct App;

impl App {
    /// Zero-arg inherent constructor used by the native entry point.
    pub fn new() -> Self {
        Self
    }
}

impl eframe::App for App {
    fn update(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {}
}

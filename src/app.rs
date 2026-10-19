//! Application shell: state, background workers, and the egui render loop.
//!
//! All database access happens on the UI thread. Network fetches run on
//! short-lived `std::thread` workers and report their results back over an
//! `mpsc` channel, so the UI never blocks on HTTP. The `Database` owns a single
//! SQLite connection which is `!Sync`, so it cannot be shared directly with the
//! workers — they only produce `PricePoint`s, and the UI thread persists them.

use std::sync::mpsc;

use chrono::NaiveDate;
use egui::{Color32, ComboBox, CentralPanel, RichText, ScrollArea, SidePanel, Window};

use crate::data::{frankfurter, yahoo, Client};
use crate::db::Database;
use crate::models::{Asset, AssetType, PricePoint, SearchResult, TimeRange};

/// A message produced by a background worker thread and consumed on the UI thread.
enum WorkerMsg {
    /// Result of a Yahoo Finance symbol search.
    SearchLoaded(Result<Vec<SearchResult>, String>),
    /// Result of a history fetch (stock via Yahoo, currency pair via Frankfurter).
    HistoryLoaded {
        symbol: String,
        points: Result<Vec<PricePoint>, String>,
    },
    /// Result of the Frankfurter currency-code list.
    CurrenciesLoaded(Result<Vec<(String, String)>, String>),
}

/// Root application object. Implements [`eframe::App`] to drive the egui loop.
pub struct App {
    /// The single on-disk (or in-memory) SQLite connection, used on the UI thread.
    db: Database,
    /// A shared HTTP client cloned into each worker thread.
    client: Client,
    /// Assets currently in the watchlist, refreshed from the database.
    assets: Vec<Asset>,

    /// Background channel: the UI drains `results_rx` each frame.
    results_rx: mpsc::Receiver<WorkerMsg>,
    results_tx: mpsc::Sender<WorkerMsg>,

    /// The asset whose history is currently displayed.
    selected_asset_id: Option<u32>,
    /// The (time-range filtered) history currently shown in the chart.
    selected_prices: Vec<PricePoint>,
    time_range: TimeRange,

    // --- "add asset" UI state ---
    search_query: String,
    last_search_query: String,
    search_results: Vec<SearchResult>,
    search_status: String,
    searching: bool,

    currencies: Vec<(String, String)>,
    selected_base: String,
    selected_target: String,
    show_add_window: bool,

    /// Symbols whose history is still being fetched.
    pending: Vec<String>,

    /// A short status / error message shown at the bottom of the UI.
    status: String,
}

/// A platform-appropriate path for the SQLite database file, created if missing.
fn default_db_path() -> std::path::PathBuf {
    const APP_DIR: &str = "StockAndCurrencyTracker";
    const FILE: &str = "tracker.db";

    let base: Option<std::path::PathBuf> = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Library").join("Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local").join("share")))
    };

    match base {
        Some(mut dir) => {
            dir.push(APP_DIR);
            // Best-effort; the DB open will create it too.
            let _ = std::fs::create_dir_all(&dir);
            dir.push(FILE);
            dir
        }
        None => std::path::PathBuf::from(FILE),
    }
}
impl App {
    /// Build the application, opening (and migrating) the on-disk database.
    ///
    /// If the persistent database cannot be opened for any reason, the app falls
    /// back to an in-memory database so the UI still renders — data simply won't
    /// survive a restart.
    pub fn new() -> Self {
        let client = Client::new();
        let db = Database::new(default_db_path()).unwrap_or_else(|e| {
            eprintln!("[db] failed to open persistent database: {e}; using in-memory db");
            Database::new(":memory:").expect("an in-memory SQLite database should always open")
        });
        let assets = db.list_assets().unwrap_or_default();
        let (results_tx, results_rx) = mpsc::channel();

        Self {
            db,
            client,
            assets,
            results_rx,
            results_tx,
            selected_asset_id: None,
            selected_prices: Vec::new(),
            time_range: TimeRange::Year,
            search_query: String::new(),
            last_search_query: String::new(),
            search_results: Vec::new(),
            search_status: String::new(),
            searching: false,
            currencies: Vec::new(),
            selected_base: "EUR".to_string(),
            selected_target: "USD".to_string(),
            show_add_window: false,
            pending: Vec::new(),
            status: "Ready. Add an asset to get started.".to_string(),
        }
    }

    /// Kick off a Yahoo symbol search on a background thread (debounced).
    fn spawn_search(&mut self, query: String) {
        let query = query.trim().to_string();
        if query.is_empty() || query == self.last_search_query {
            return;
        }
        self.last_search_query = query.clone();
        self.searching = true;
        let client = self.client.clone();
        let tx = self.results_tx.clone();
        std::thread::spawn(move || {
            let res = yahoo::search(&client, &query).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::SearchLoaded(res));
        });
    }

    /// Fetch history for a symbol on a background thread. Stocks use Yahoo;
    /// currency pairs use Frankfurter. No-op if a fetch for `symbol` is pending.
    fn spawn_history(&mut self, symbol: String, asset_type: AssetType, currency_pair: Option<(String, String)>) {
        if self.pending.iter().any(|s| s == &symbol) {
            return;
        }
        self.pending.push(symbol.clone());
        let client = self.client.clone();
        let tx = self.results_tx.clone();
        std::thread::spawn(move || {
            let res: Result<Vec<PricePoint>, String> = match asset_type {
                AssetType::Stock => {
                    yahoo::fetch_history(&client, &symbol, "5y", "1d").map_err(|e| e.to_string())
                }
                AssetType::Currency => {
                    let (base, target) =
                        currency_pair.unwrap_or_else(|| ("EUR".to_string(), "USD".to_string()));
                    let today = chrono::Utc::now().date_naive();
                    let from = today - chrono::TimeDelta::days(5 * 365);
                    frankfurter::convert(&client, &base, &target, from, today).map_err(|e| e.to_string())
                }
            };
            let _ = tx.send(WorkerMsg::HistoryLoaded { symbol, points: res });
        });
    }

    /// Fetch the Frankfurter currency-code list once, on a background thread.
    fn spawn_currencies(&mut self) {
        if !self.currencies.is_empty() {
            return;
        }
        let client = self.client.clone();
        let tx = self.results_tx.clone();
        std::thread::spawn(move || {
            let res = frankfurter::available_currencies(&client).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::CurrenciesLoaded(res));
        });
    }
}
impl App {
    /// Drain any background results that have arrived and apply them to the
    /// database and UI state.
    fn apply_results(&mut self) {
        let messages: Vec<WorkerMsg> = self.results_rx.try_iter().collect();
        if messages.is_empty() {
            return;
        }
        for msg in messages {
            match msg {
                WorkerMsg::SearchLoaded(res) => match res {
                    Ok(results) => {
                        self.search_results = results.clone();
                        self.searching = false;
                        self.search_status = format!("{} result(s)", results.len());
                    }
                    Err(e) => {
                        self.search_results.clear();
                        self.searching = false;
                        self.search_status = format!("Search failed: {e}");
                    }
                },
                WorkerMsg::HistoryLoaded { symbol, points } => {
                    match points {
                        Ok(mut points) => {
                            if let Ok(Some(id)) = self.db.asset_id(&symbol) {
                                for p in &mut points {
                                    p.asset_id = id;
                                }
                                let n = self.db.insert_prices(id, &points).unwrap_or(0);
                                self.status = format!("Saved {n} point(s) for {symbol}.");
                            } else {
                                self.status = format!("{symbol}: no matching asset in database.");
                            }
                        }
                        Err(e) => self.status = format!("Failed to load {symbol}: {e}"),
                    }
                    self.pending.retain(|s| s != &symbol);
                }
                WorkerMsg::CurrenciesLoaded(res) => match res {
                    Ok(list) => {
                        self.currencies = list;
                        self.status = format!("Loaded {} currencies.", self.currencies.len());
                    }
                    Err(e) => self.status = format!("Failed to load currencies: {e}"),
                },
            }
        }
        // A history load may have updated the latest prices / last_updated stamps.
        self.reload_assets();
        self.refresh_selected();
    }

    /// Reload the watchlist from the database and drop a selection that no longer
    /// exists.
    fn reload_assets(&mut self) {
        if let Ok(assets) = self.db.list_assets() {
            self.assets = assets;
        }
        if let Some(id) = self.selected_asset_id {
            if !self.assets.iter().any(|a| a.id == id) {
                self.selected_asset_id = None;
            }
        }
    }

    /// Refresh the chart's data for the currently selected asset and time range.
    fn refresh_selected(&mut self) {
        let Some(id) = self.selected_asset_id else {
            return;
        };
        let (start, end) = self.range_bounds();
        if let Ok(prices) = self.db.get_prices(id, start, end) {
            self.selected_prices = prices;
        }
    }

    /// The `(start, end)` date bounds implied by the current [`TimeRange`].
    /// `Max`/`Custom` leave the start open so the full history is used.
    fn range_bounds(&self) -> (Option<NaiveDate>, Option<NaiveDate>) {
        let today = chrono::Utc::now().date_naive();
        (self.time_range.start_date(today), None)
    }

    /// Persist a searched asset and start fetching its history.
    fn add_from_search(&mut self, result: &SearchResult) {
        let symbol = result.symbol.clone();
        let name = result.name.clone();
        let asset_type = result.asset_type;
        let base_currency = result.currency.clone().unwrap_or_else(|| "USD".to_string());
        if self.db.ensure_asset(&symbol, asset_type, &name, &base_currency).is_ok() {
            self.reload_assets();
            self.selected_asset_id = self.db.asset_id(&symbol).ok().flatten();
            self.spawn_history(symbol.clone(), asset_type, None);
            self.status = format!("Adding {symbol} — fetching 5-year history…");
            self.show_add_window = false;
        } else {
            self.status = format!("Could not add {symbol} to the database.");
        }
    }

    /// Persist a currency pair and start fetching its historical rates.
    fn add_currency_pair(&mut self, base: &str, target: &str) {
        if base.is_empty() || target.is_empty() || base == target {
            self.status = "Choose two different currencies.".to_string();
            return;
        }
        let symbol = format!("{base}/{target}");
        let name = format!("{base} → {target}");
        if self.db.ensure_asset(&symbol, AssetType::Currency, &name, target).is_ok() {
            self.reload_assets();
            self.selected_asset_id = self.db.asset_id(&symbol).ok().flatten();
            self.spawn_history(symbol.clone(), AssetType::Currency, Some((base.to_string(), target.to_string())));
            self.status = format!("Adding currency {symbol} — fetching 5-year history…");
            self.show_add_window = false;
        } else {
            self.status = format!("Could not add currency {symbol} to the database.");
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Apply any background results that arrived since the last frame.
        self.apply_results();

        // 2. Keep redrawing while work is in flight so the UI updates when a
        //    result arrives (no pointer event would otherwise trigger a repaint).
        if !self.pending.is_empty() || self.searching {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }

        // 3. Layout: a left watchlist panel, a central chart area, and an
        //    optional floating "add asset" window.
        SidePanel::left(egui::Id::new("watchlist_panel"))
            .default_width(300.0)
            .show(ctx, |ui| self.render_watchlist(ui));

        CentralPanel::default().show(ctx, |ui| self.render_chart_area(ui));

        if self.show_add_window {
            Window::new("Add Asset")
                .resizable(true)
                .default_width(440.0)
                .show(ctx, |ui| self.render_add_window(ui));
        }
    }
}

impl App {
    fn render_watchlist(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Watchlist").heading());
        ui.separator();

        if self.assets.is_empty() {
            self.render_onboarding(ui);
            return;
        }

        let mut clicked_id: Option<u32> = None;
        ScrollArea::vertical()
            .max_height(700.0)
            .show(ui, |ui| {
                for asset in &self.assets {
                    let selected = self.selected_asset_id == Some(asset.id);
                    let mut label = RichText::new(asset.symbol.clone()).strong();
                    if selected {
                        label = label.color(Color32::from_rgb(80, 160, 255));
                    }
                    let row = ui.horizontal(|ui| {
                        ui.selectable_label(selected, label);
                        if let Some(price) = self.db.latest_price(asset.id).ok().flatten() {
                            ui.label(RichText::new(format!("{:.4}", price.close)).small().weak());
                        }
                    });
                    if row.response.clicked() {
                        clicked_id = Some(asset.id);
                    }
                }
            });

        if let Some(id) = clicked_id {
            self.selected_asset_id = Some(id);
            self.refresh_selected();
        }

        ui.separator();
        if ui.button("+ Add asset").clicked() {
            self.show_add_window = true;
        }
    }

    fn render_onboarding(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.0);
        ui.label(RichText::new("Welcome to Stock & Currency Tracker").heading());
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Your watchlist is empty. Search Yahoo Finance for a stock, crypto, \
                 or fund — or add a currency pair — to start tracking history.",
            )
            .weak(),
        );
        ui.add_space(16.0);
        self.render_search(ui);
        ui.add_space(8.0);
        self.render_currency_picker(ui);
        ui.add_space(12.0);
        ui.separator();
        ui.label(RichText::new(self.status.as_str()).small().weak());
    }

    fn render_search(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            let mut query = self.search_query.clone();
            if ui.text_edit_singleline(&mut query).changed() {
                self.search_query = query.clone();
                self.spawn_search(query);
            }
            if ui.button("Search").clicked() {
                self.spawn_search(self.search_query.clone());
            }
        });
        ui.label(RichText::new(self.search_status.as_str()).small().weak());
        if let Some(result) = self.render_search_results(ui) {
            self.add_from_search(&result);
        }
    }

    fn render_search_results(&self, ui: &mut egui::Ui) -> Option<SearchResult> {
        for result in &self.search_results {
            let display = result.display_name();
            if ui.selectable_label(false, display).clicked() {
                return Some(result.clone());
            }
        }
        None
    }
}
impl App {
    fn render_currency_picker(&mut self, ui: &mut egui::Ui) {
        let mut chosen_base: Option<String> = None;
        let mut chosen_target: Option<String> = None;
        ui.horizontal(|ui| {
            ComboBox::from_id_salt("currency_base")
                .selected_text(self.selected_base.clone())
                .show_ui(ui, |ui| {
                    if self.currencies.is_empty() {
                        if ui.button("Load list…").clicked() {
                            self.spawn_currencies();
                        }
                    } else {
                        for (code, name) in &self.currencies {
                            if ui.selectable_label(false, format!("{code} · {name}")).clicked() {
                                chosen_base = Some(code.clone());
                            }
                        }
                    }
                });
            ui.label("to");
            ComboBox::from_id_salt("currency_target")
                .selected_text(self.selected_target.clone())
                .show_ui(ui, |ui| {
                    if self.currencies.is_empty() {
                        if ui.button("Load list…").clicked() {
                            self.spawn_currencies();
                        }
                    } else {
                        for (code, name) in &self.currencies {
                            if ui.selectable_label(false, format!("{code} · {name}")).clicked() {
                                chosen_target = Some(code.clone());
                            }
                        }
                    }
                });
            if ui.button("Add pair").clicked() {
                let base = self.selected_base.clone();
                let target = self.selected_target.clone();
                self.add_currency_pair(&base, &target);
            }
        });
        if let Some(base) = chosen_base {
            self.selected_base = base;
        }
        if let Some(target) = chosen_target {
            self.selected_target = target;
        }
    }

    fn render_chart_area(&mut self, ui: &mut egui::Ui) {
        // Top toolbar.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Stock & Currency Tracker").heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.pending.is_empty() || self.searching {
                    ui.spinner();
                    ui.label(RichText::new("loading…").small().weak());
                }
                if ui.button("+ Add asset").clicked() {
                    self.show_add_window = true;
                }
            });
        });
        ui.separator();

        // Empty state (the watchlist panel also shows onboarding).
        if self.assets.is_empty() {
            ui.add_space(40.0);
            ui.label(RichText::new("Add an asset to see its history here.").weak());
            return;
        }

        // Auto-select the first asset so the chart is never blank.
        if self.selected_asset_id.is_none() {
            if let Some(first) = self.assets.first() {
                self.selected_asset_id = Some(first.id);
                self.refresh_selected();
            }
        }

        // Time-range selector.
        ui.horizontal(|ui| {
            ui.label("Range:");
            for range in TimeRange::all() {
                let active = self.time_range == range;
                if ui.selectable_label(active, range.label()).clicked() {
                    self.time_range = range;
                    self.refresh_selected();
                }
            }
        });
        ui.add_space(6.0);

        // Resolve the selection into an owned `Asset` so we don't hold a `&self`
        // borrow while calling other `&self` / `&mut self` methods below.
        let selected_id = self.selected_asset_id;
        let asset = self
            .assets
            .iter()
            .find(|a| a.id == selected_id.unwrap_or(0))
            .cloned();
        match asset {
            Some(asset) => {
                ui.label(RichText::new(format!("{} · {}", asset.symbol, asset.name)).heading());
                if let Some(last) = self.db.latest_price(asset.id).ok().flatten() {
                    ui.label(
                        RichText::new(format!("Latest close: {:.4} {}", last.close, asset.base_currency))
                            .strong()
                            .monospace(),
                    );
                }
                crate::charts::render_line_chart(
                    ui,
                    &self.selected_prices,
                    Color32::from_rgb(70, 160, 230),
                    &asset.symbol,
                );
            }
            None => {
                ui.label(RichText::new("Select an asset to view its history.").weak());
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(self.status.as_str()).small().weak());
    }

    fn render_add_window(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Add Asset").heading());
        ui.separator();
        ui.label("Search Yahoo Finance (stocks, crypto, funds):");
        self.render_search(ui);
        ui.separator();
        ui.label("Or add a currency pair (Frankfurter):");
        self.render_currency_picker(ui);
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(self.status.as_str()).small().weak());
    }
}

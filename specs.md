# Stock & Currency Tracker - Complete Technical Specification

**Version:** 1.0  
**Target Platforms:** macOS, Windows, Linux  
**Language:** Rust  
**Purpose:** Desktop application for tracking stocks and currencies, analyzing historical evolution, comparing assets, and performing historical currency conversions. Built for personal investment tracking and decision support (buy/sell assessment).

This specification is written so that an LLM (e.g. Qwen) using Cline in VS Code can implement the entire application from scratch with minimal or no human intervention after reading this document.

---

## 1. Project Overview

The application allows the user to:

- Search for stocks and currencies and add them to a personal "Favorites" list.
- Download historical data (and refresh the latest data) from free online sources.
- View interactive charts for a single asset.
- Build, save, and reopen custom multi-asset charts (e.g. Google vs Meta, or CAD vs MXN).
- Perform historical currency conversion queries (e.g. “What was 2,000,000 MXN worth in CAD on a specific date?”).
- Store all data locally in SQLite for offline use after the initial download.

The UI must feel like a modern finance application: dynamic charts with zoom, pan, hover tooltips, time-range selectors, and legend interaction.

---

## 2. Technology Stack

| Component          | Crate / Library              | Notes |
|--------------------|------------------------------|-------|
| GUI Framework      | `eframe` + `egui`            | Immediate-mode, cross-platform |
| Interactive Charts | `egui_plot`                  | Built-in zoom, pan, hover, boxed zoom |
| HTTP Client        | `reqwest` (with `json` feature) | Async or blocking |
| Async Runtime      | `tokio`                      | For HTTP calls |
| Database           | `rusqlite` (bundled)         | SQLite, no external dependency |
| Serialization      | `serde` + `serde_json`       | |
| Date/Time          | `chrono`                     | |
| Error Handling     | `anyhow` + `thiserror`       | |
| Logging            | `env_logger` or `tracing`    | |
| Yahoo Finance      | `yahoo_finance_api` (or direct HTTP) | Prefer the crate if stable; otherwise implement thin wrapper |

**Cargo.toml dependencies (minimum):**

```toml
[dependencies]
eframe = "0.29"          # or latest stable compatible
egui = "0.29"
egui_plot = "0.29"
rusqlite = { version = "0.32", features = ["bundled"] }
reqwest = { version = "0.12", features = ["json", "blocking"] }  # or async
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
thiserror = "1"
yahoo_finance_api = "2"   # optional; can be replaced by direct HTTP
```

Use the latest compatible versions at the time of implementation. Prefer blocking `reqwest` if it simplifies the eframe integration, or properly spawn tokio tasks.

---

## 3. Data Sources

### 3.1 Stocks
- **Primary source:** Yahoo Finance unofficial API (no key required).
- **Historical chart endpoint:**  
  `https://query1.finance.yahoo.com/v8/finance/chart/{SYMBOL}?period1={unix_start}&period2={unix_end}&interval=1d&events=div,split`
- **Search endpoint:**  
  `https://query1.finance.yahoo.com/v1/finance/search?q={query}&quotesCount=10&newsCount=0`
- Recommended Rust crate: `yahoo_finance_api` (provides high-level methods for history and search). If the crate is problematic, implement a thin `reqwest` wrapper around the endpoints above.
- Store adjusted close when available. Daily interval is the default.

### 3.2 Currencies
- **Primary source:** Frankfurter API (free, no key, excellent historical coverage).
- Base URL: `https://api.frankfurter.dev`
- Key endpoints:
  - Latest / specific date: `/v2/rates?date=YYYY-MM-DD&base=USD&quotes=CAD,MXN`
  - Time series: `/v2/rates?from=YYYY-MM-DD&to=YYYY-MM-DD&base=USD&quotes=CAD,MXN`
  - Supported currencies include CAD, MXN, USD, EUR, etc. (full list available via the API).
- For charting a currency, treat it as an exchange rate against a configurable base (default: USD). Example: chart of CAD shows USD/CAD or CAD/USD (document the convention used).

### 3.3 Data Download Strategy
- On adding an asset: download the maximum reasonable history (e.g. last 10 years or “max” available) and store it.
- “Refresh Data” button: for each favorite (or selected assets), fetch only from the last stored date + 1 day until today and append the new rows.
- Handle rate limiting / temporary failures gracefully (retry with backoff, show user-friendly messages).
- All data is stored locally after download so the app works offline.

---

## 4. Database Schema (SQLite)

Use a single file `data.db` in the application’s data directory (platform-specific via `dirs` crate or hard-coded relative path for simplicity).

```sql
CREATE TABLE assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_type TEXT NOT NULL CHECK(asset_type IN ('stock', 'currency')),
    symbol TEXT NOT NULL UNIQUE,          -- e.g. "AAPL", "GOOGL", "CAD", "MXN"
    name TEXT,                            -- full name
    base_currency TEXT,                   -- for currencies: the base (default USD); for stocks: the trading currency
    created_at TEXT NOT NULL
);

CREATE TABLE price_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL,
    date TEXT NOT NULL,                   -- YYYY-MM-DD
    open REAL,
    high REAL,
    low REAL,
    close REAL NOT NULL,
    adj_close REAL,
    volume INTEGER,
    FOREIGN KEY(asset_id) REFERENCES assets(id) ON DELETE CASCADE,
    UNIQUE(asset_id, date)
);

CREATE TABLE custom_charts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    asset_ids TEXT NOT NULL,              -- JSON array of asset IDs, e.g. "[1,5,12]"
    settings TEXT,                        -- JSON: { "normalize": true, "base_currency": "USD", "interval": "1d" }
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Optional: simple settings table
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT
);
```

Indexes on `(asset_id, date)` for fast range queries.

---

## 5. Core Features

### 5.1 Search & Favorites
- Search bar that queries Yahoo Finance search for stocks and also allows direct entry of currency codes (CAD, MXN, EUR, etc.).
- Display results with symbol, name, type, and exchange.
- “Add to Favorites” button. Prevent duplicates.
- Favorites panel (sidebar or dedicated view) listing all added assets with type icon, last price (if available), and quick actions (view chart, remove).

### 5.2 Single Asset Chart
- Select any favorite → show interactive line chart of close (or adj_close) prices.
- Time range presets: 1M, 3M, 6M, 1Y, 5Y, Max, Custom.
- Hover shows date + value.
- Zoom (scroll), pan (drag), boxed zoom (right-drag or shift-drag), double-click reset.
- Optional: toggle between absolute price and percentage change from the start of the visible range.

### 5.3 Custom Multi-Asset Charts
- “Create Custom Chart” mode: multi-select from favorites.
- Choose visualization mode:
  - Absolute prices (may need dual Y-axes or warning when scales differ greatly).
  - Normalized (percentage change from first common date) — preferred default for comparison.
- Save the chart with a user-provided name. Store the list of asset IDs + settings.
- List of saved custom charts; click to reopen.
- Ability to edit or delete saved charts.

### 5.4 Currency Conversion Calculator
- Dedicated panel or modal:
  - Amount (number input)
  - From currency (dropdown or search)
  - To currency
  - Date picker (or “latest”)
- Query Frankfurter for the historical rate on that date (or nearest available business day).
- Display the converted amount and the exact rate used.
- Example use case: 2 000 000 MXN → CAD on 2024-10-18.

### 5.5 Data Management
- Global “Refresh All Data” button.
- Per-asset refresh.
- Progress indication during downloads.
- Display last update timestamp per asset.

### 5.6 Chart Interactivity (required)
Using `egui_plot::Plot`:
- `allow_zoom(true)`
- `allow_drag(true)` (pan)
- `allow_boxed_zoom(true)`
- `allow_double_click_reset(true)`
- Hover labels with formatted date and value
- Legend that can toggle series visibility
- X-axis as dates (format nicely)

---

## 6. User Interface Structure

Recommended layout (egui):

- **Left sidebar (fixed or collapsible):**
  - Search input
  - Search results
  - Favorites list (with filter)
  - Saved Custom Charts list

- **Main area:**
  - Top bar: selected asset / chart name, time-range buttons, Refresh button, Normalize toggle
  - Large interactive plot
  - Bottom or side panel for the Currency Converter (always accessible or via tab)

- Dark theme by default (egui supports it easily).
- Status bar showing last refresh time and any error messages.

---

## 7. Architecture & Module Structure

```
src/
├── main.rs                 # eframe app entry
├── app.rs                  # Main App struct implementing eframe::App
├── db/
│   ├── mod.rs
│   └── schema.rs           # rusqlite setup + queries
├── data/
│   ├── mod.rs
│   ├── yahoo.rs            # Stock data fetching
│   └── frankfurter.rs      # Currency data fetching
├── models.rs               # Asset, PricePoint, CustomChart structs
├── charts.rs               # Chart rendering helpers
└── ui/
    ├── sidebar.rs
    ├── chart_view.rs
    ├── converter.rs
    └── custom_chart.rs
```

- Keep data layer independent of UI.
- All database access through a small repository layer.
- Use `Arc<Mutex<...>>` or channels if background refreshing is needed so the UI stays responsive.

---

## 8. Implementation Guidelines for the LLM

1. Start by creating the project with `cargo new stock_currency_tracker`.
2. Add all dependencies and ensure it compiles on the target platforms.
3. Implement the SQLite layer first (create tables, insert/query helpers).
4. Implement Yahoo Finance and Frankfurter clients with proper error handling and User-Agent headers.
5. Build a minimal egui window that lists favorites and shows a dummy plot.
6. Connect real data loading and plotting with `egui_plot`.
7. Add search, add-to-favorites, refresh logic, and custom chart save/load.
8. Implement the currency converter.
9. Polish UI, add loading indicators, and basic error toasts/messages.
10. Test with real symbols: AAPL, GOOGL, META, CAD, MXN, EUR.
11. Ensure the app can be built for Windows, macOS, and Linux with standard `cargo build --release`.

**Important constraints:**
- No external services that require API keys.
- All historical data must be persisted locally.
- Charts must remain interactive even with several thousand data points.
- Handle weekends/holidays gracefully for currencies (use nearest available rate).
- Make the code modular and well-commented so future extensions (technical indicators, portfolio tracking, etc.) are easy.

---

## 9. Non-Functional Requirements

- Startup time < 2 seconds with a few dozen assets.
- Smooth scrolling and chart interaction (60 FPS target).
- Offline capability after initial data download.
- Data directory: use platform conventions or a simple `./data` folder next to the executable for simplicity.
- Logging of errors to console or a log file.

---

## 10. Future Extensions (not required for v1)

- Technical indicators (SMA, RSI, etc.)
- Portfolio holdings and performance tracking
- Export charts as PNG
- Alerts on price thresholds
- Multiple base currencies for FX charts

---


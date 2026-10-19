//! SQLite persistence layer: schema, migrations, and data access helpers.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{Asset, AssetType, ChartSettings, CustomChart, PricePoint};

/// The SQL used to create / migrate the database schema.
///
/// `INSERT OR IGNORE` semantics combined with the `UNIQUE` constraints on
/// `(asset_id, date)` and `symbol` mean running the migration repeatedly is
/// safe.
const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS assets (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_type    TEXT NOT NULL,
    symbol        TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    base_currency TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    last_updated  TEXT
);

CREATE TABLE IF NOT EXISTS prices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id    INTEGER NOT NULL,
    date        TEXT NOT NULL,
    open        REAL NOT NULL,
    high        REAL NOT NULL,
    low         REAL NOT NULL,
    close       REAL NOT NULL,
    adj_close   REAL NOT NULL,
    volume      INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (asset_id) REFERENCES assets(id) ON DELETE CASCADE,
    UNIQUE (asset_id, date)
);

CREATE INDEX IF NOT EXISTS idx_prices_asset_date ON prices (asset_id, date);

CREATE TABLE IF NOT EXISTS custom_charts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    description TEXT,
    asset_ids   TEXT NOT NULL,
    settings    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);
"#;

/// Wrapper around a single SQLite connection. The application performs all
/// database access on the UI thread, so one connection is enough. WAL mode and
/// a busy timeout keep it robust against the occasional transient lock.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open database at {}", path.as_ref().display()))?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.execute_batch(MIGRATION)?;
        Ok(Self { conn })
    }

    // ------------------------------------------------------------------ assets

    /// Insert an asset if it does not already exist (matched by symbol) and
    /// return its id.
    pub fn ensure_asset(
        &self,
        symbol: &str,
        asset_type: AssetType,
        name: &str,
        base_currency: &str,
    ) -> Result<u32> {
        let today = chrono::Utc::now().date_naive();
        let created = today.to_string();
        self.conn.execute(
            "INSERT OR IGNORE INTO assets (asset_type, symbol, name, base_currency, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![asset_type.as_str(), symbol, name, base_currency, created],
        )?;
        let id = self
            .asset_id(symbol)?
            .ok_or_else(|| anyhow::anyhow!("asset not found after insert: {symbol}"))?;
        Ok(id)
    }

    pub fn asset_id(&self, symbol: &str) -> Result<Option<u32>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM assets WHERE lower(symbol) = lower(?1)",
                params![symbol],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id.map(|v| v as u32))
    }

    pub fn get_asset(&self, id: u32) -> Result<Option<Asset>> {
        let asset = self
            .conn
            .query_row(
                "SELECT id, asset_type, symbol, name, base_currency, created_at, last_updated FROM assets WHERE id = ?1",
                params![id],
                map_asset,
            )
            .optional()?;
        Ok(asset)
    }

    pub fn list_assets(&self) -> Result<Vec<Asset>> {
        let mut stmt = self.conn.prepare("SELECT id, asset_type, symbol, name, base_currency, created_at, last_updated FROM assets ORDER BY symbol")?;
        let rows = stmt.query_map([], map_asset)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn remove_asset(&mut self, id: u32) -> Result<()> {
        // Remove references to this asset from saved custom charts, then delete
        // the asset (and its prices via the ON DELETE CASCADE foreign key).
        self.purge_asset_from_charts(id)?;
        self.conn.execute("DELETE FROM assets WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn purge_asset_from_charts(&mut self, id: u32) -> Result<()> {
        let charts = self.list_custom_charts()?;
        for chart in charts {
            let remaining: Vec<u32> = chart.asset_ids.iter().copied().filter(|a| *a != id).collect();
            self.conn
                .execute("UPDATE custom_charts SET asset_ids = ?1 WHERE id = ?2", params![encode_ids(&remaining), chart.id])?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ prices

    pub fn count_prices(&self, asset_id: u32) -> Result<u64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM prices WHERE asset_id = ?1", params![asset_id], |row| row.get(0))?;
        Ok(n as u64)
    }

    pub fn last_price_date(&self, asset_id: u32) -> Result<Option<NaiveDate>> {
        let s: Option<String> = self
            .conn
            .query_row("SELECT MAX(date) FROM prices WHERE asset_id = ?1", params![asset_id], |row| row.get(0))
            .optional()?;
        Ok(s.as_deref().and_then(|v| parse_date(Some(v))))
    }

    /// Return price points for an asset within `[from, to]` (inclusive), ordered
    /// by date ascending. `from`/`to` are optional bounds.
    pub fn get_prices(&self, asset_id: u32, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<Vec<PricePoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, asset_id, date, open, high, low, close, adj_close, volume FROM prices WHERE asset_id = ?1 AND (?2 IS NULL OR date >= ?2) AND (?3 IS NULL OR date <= ?3) ORDER BY date ASC",
        )?;
        let from_s = from.map(|d| d.to_string());
        let to_s = to.map(|d| d.to_string());
        let rows = stmt.query_map(params![asset_id, from_s, to_s], map_price)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Return the most recent (latest by date) price point for an asset, if any.
    pub fn latest_price(&self, asset_id: u32) -> Result<Option<PricePoint>> {
        let mut stmt = self.conn.prepare("SELECT id, asset_id, date, open, high, low, close, adj_close, volume FROM prices WHERE asset_id = ?1 ORDER BY date DESC LIMIT 1")?;
        let rows = stmt.query_map(params![asset_id], map_price)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out.into_iter().next())
    }

    /// Insert price points, ignoring duplicates. Returns the number actually
    /// inserted.
    pub fn insert_prices(&mut self, asset_id: u32, points: &[PricePoint]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0usize;
        for p in points {
            let n = tx.execute(
                "INSERT OR IGNORE INTO prices (asset_id, date, open, high, low, close, adj_close, volume) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![asset_id, p.date.to_string(), p.open, p.high, p.low, p.close, p.adj_close, p.volume],
            )?;
            inserted += n;
        }
        tx.commit()?;
        Ok(inserted)
    }

    pub fn update_last_updated(&self, asset_id: u32, date: NaiveDate) -> Result<()> {
        self.conn.execute("UPDATE assets SET last_updated = ?1 WHERE id = ?2", params![date.to_string(), asset_id])?;
        Ok(())
    }

    // -------------------------------------------------------------- custom charts

    pub fn save_custom_chart(&mut self, name: &str, description: Option<&str>, asset_ids: &[u32], settings: &ChartSettings) -> Result<u32> {
        let settings_json = serde_json::to_string(settings)?;
        self.conn.execute(
            "INSERT INTO custom_charts (name, description, asset_ids, settings) VALUES (?1, ?2, ?3, ?4)",
            params![name, description, encode_ids(asset_ids), settings_json],
        )?;
        Ok(self.conn.last_insert_rowid() as u32)
    }

    pub fn list_custom_charts(&self) -> Result<Vec<CustomChart>> {
        let mut stmt = self.conn.prepare("SELECT id, name, description, asset_ids, settings FROM custom_charts ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let description: Option<String> = row.get(2)?;
            let ids_str: String = row.get(3)?;
            let settings_str: String = row.get(4)?;
            let settings = serde_json::from_str::<ChartSettings>(&settings_str).unwrap_or_default();
            Ok(CustomChart {
                id: id as u32,
                name,
                description,
                asset_ids: decode_ids(&ids_str),
                settings,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn delete_custom_chart(&mut self, id: u32) -> Result<()> {
        self.conn.execute("DELETE FROM custom_charts WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ----------------------------------------------------------------- settings

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .optional()?;
        Ok(v)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }
}

// --------------------------------------------------------------------- helpers

fn map_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    let id: i64 = row.get(0)?;
    let asset_type: String = row.get(1)?;
    let name: String = row.get(3)?;
    let base_currency: String = row.get(4)?;
    let created_at: String = row.get(5)?;
    let last_updated: Option<String> = row.get(6)?;
    Ok(Asset {
        id: id as u32,
        asset_type: AssetType::from_str(&asset_type),
        symbol: row.get(2)?,
        name,
        base_currency,
        created_at: parse_date(Some(&created_at)).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
        last_updated: last_updated.as_deref().and_then(|v| parse_date(Some(v))),
    })
}

fn map_price(row: &rusqlite::Row<'_>) -> rusqlite::Result<PricePoint> {
    let asset_id: i64 = row.get(1)?;
    let date: String = row.get(2)?;
    Ok(PricePoint {
        asset_id: asset_id as u32,
        date: parse_date(Some(&date)).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
        open: row.get(3)?,
        high: row.get(4)?,
        low: row.get(5)?,
        close: row.get(6)?,
        adj_close: row.get(7)?,
        volume: row.get(8)?,
    })
}

fn parse_date(s: Option<&str>) -> Option<NaiveDate> {
    s.and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
}

/// Encode a list of asset ids as a comma-separated string with leading/trailing
/// commas so substring membership checks are unambiguous.
fn encode_ids(ids: &[u32]) -> String {
    let mut s = String::from(",");
    for id in ids {
        s.push_str(&id.to_string());
        s.push(',');
    }
    s
}

fn decode_ids(s: &str) -> Vec<u32> {
    s.trim_matches(',').split(',').filter_map(|p| p.trim().parse::<u32>().ok()).collect()
}


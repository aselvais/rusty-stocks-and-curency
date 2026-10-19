//! Shared data models used across the application.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// The kind of tracked asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    Stock,
    Currency,
}

impl AssetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetType::Stock => "Stock",
            AssetType::Currency => "Currency",
        }
    }

    pub fn from_str(s: &str) -> AssetType {
        if s.eq_ignore_ascii_case("currency") {
            AssetType::Currency
        } else {
            AssetType::Stock
        }
    }
}

/// A tracked asset (a stock ticker or a currency code) stored in the database.
#[derive(Debug, Clone)]
pub struct Asset {
    pub id: u32,
    pub asset_type: AssetType,
    pub symbol: String,
    pub name: String,
    pub base_currency: String,
    pub created_at: NaiveDate,
    pub last_updated: Option<NaiveDate>,
}

/// A single daily observation for an asset.
#[derive(Debug, Clone)]
pub struct PricePoint {
    pub asset_id: u32,
    pub date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub adj_close: f64,
    pub volume: i64,
}

/// Persisted settings for a custom chart. Serialized to JSON in the database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChartSettings {
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub base_currency: Option<String>,
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default)]
    pub custom_start: Option<String>,
    #[serde(default)]
    pub custom_end: Option<String>,
}

fn default_interval() -> String {
    "1d".to_string()
}

/// A user-saved chart that compares multiple assets.
#[derive(Debug, Clone)]
pub struct CustomChart {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub asset_ids: Vec<u32>,
    pub settings: ChartSettings,
}

/// A result returned by an asset search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub symbol: String,
    pub name: String,
    pub asset_type: AssetType,
    pub exchange_name: Option<String>,
    pub currency: Option<String>,
}

impl SearchResult {
    pub fn display_name(&self) -> String {
        if self.exchange_name.is_some() {
            format!("{} · {} · {}", self.symbol, self.name, self.exchange_name.as_deref().unwrap_or(""))
        } else {
            format!("{} · {}", self.symbol, self.name)
        }
    }
}

/// Time ranges offered in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRange {
    Month,
    ThreeMonths,
    SixMonths,
    Year,
    FiveYears,
    Max,
    Custom,
}

impl TimeRange {
    pub fn all() -> [TimeRange; 7] {
        [
            TimeRange::Month,
            TimeRange::ThreeMonths,
            TimeRange::SixMonths,
            TimeRange::Year,
            TimeRange::FiveYears,
            TimeRange::Max,
            TimeRange::Custom,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            TimeRange::Month => "1M",
            TimeRange::ThreeMonths => "3M",
            TimeRange::SixMonths => "6M",
            TimeRange::Year => "1Y",
            TimeRange::FiveYears => "5Y",
            TimeRange::Max => "Max",
            TimeRange::Custom => "Custom",
        }
    }

    /// The start date for this range relative to `today`. `Max` and `Custom`
    /// return `None` here (they are handled specially by the caller).
    pub fn start_date(&self, today: NaiveDate) -> Option<NaiveDate> {
        use chrono::TimeDelta;
        match self {
            TimeRange::Month => Some(today - TimeDelta::days(31)),
            TimeRange::ThreeMonths => Some(today - TimeDelta::days(90)),
            TimeRange::SixMonths => Some(today - TimeDelta::days(180)),
            TimeRange::Year => Some(today - TimeDelta::days(365)),
            TimeRange::FiveYears => Some(today - TimeDelta::days(1825)),
            TimeRange::Max => None,
            TimeRange::Custom => None,
        }
    }
}

//! Yahoo Finance data source: symbol search and daily historical prices.
//!
//! Uses two public JSON endpoints:
//! * `v1/finance/search`  – autocomplete-style symbol search.
//! * `v8/finance/chart`   – OHLCV time series for a symbol.
//!
//! These endpoints do not require an API key. They are rate-limited and may
//! occasionally return empty results or HTTP errors; callers treat any failure
//! as a non-fatal, retryable condition.

use anyhow::{anyhow, Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;

use super::Client;
use crate::models::{AssetType, PricePoint, SearchResult};

const SEARCH_URL: &str = "https://query1.finance.yahoo.com/v1/finance/search";
const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/";

/// Search Yahoo Finance for symbols matching `query`.
///
/// Returns matches ordered by relevance as returned by the upstream service.
/// Empty `query` returns an empty list (the upstream call is skipped).
pub fn search(client: &Client, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!("{SEARCH_URL}?q={}", urlencoded(query));
    let resp = client.get(&url).with_context(|| format!("search request failed: {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Yahoo search returned HTTP {status} for query '{query}'"));
    }
    let body: SearchResponse = resp.json().context("failed to parse Yahoo search response")?;
    Ok(body
        .quotes
        .into_iter()
        .map(SearchResult::from)
        .filter(|r| !r.symbol.is_empty())
        .collect())
}

/// Fetch daily OHLCV history for a symbol over `range` (e.g. "1y", "5y", "10y").
///
/// Returns points ordered by date ascending with `asset_id` left as 0 (the
/// database layer assigns the real id on insert).
pub fn fetch_history(client: &Client, symbol: &str, range: &str, interval: &str) -> Result<Vec<PricePoint>> {
    let url = format!("{CHART_URL}{}?range={range}&interval={interval}", urlencoded(symbol));
    let resp = client.get(&url).with_context(|| format!("chart request failed: {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Yahoo chart returned HTTP {status} for symbol '{symbol}'"));
    }
    let body: ChartResponse = resp.json().context("failed to parse Yahoo chart response")?;

    if let Some(err) = body.chart.error {
        return Err(anyhow!("Yahoo chart error for '{symbol}': {err}"));
    }

    let result = body.chart.result.into_iter().next().ok_or_else(|| anyhow!("no chart data for '{symbol}'"))?;
    let timestamps = result.timestamp;
    let quote = result
        .indicators
        .quote
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("missing quote series for '{symbol}'"))?;
    let adj_close = result
        .indicators
        .adj_close
        .into_iter()
        .next()
        .map(|a| a.adj_close)
        .unwrap_or_default();

    let mut points: Vec<PricePoint> = Vec::with_capacity(timestamps.len());
    for (i, &ts) in timestamps.iter().enumerate() {
        let close = quote.close.get(i).copied().unwrap_or(f64::NAN);
        // Yahoo uses null for missing values; skip rows with no close.
        if close.is_nan() {
            continue;
        }
        let point = PricePoint {
            asset_id: 0,
            date: timestamp_to_date(ts),
            open: quote.open.get(i).copied().unwrap_or(close),
            high: quote.high.get(i).copied().unwrap_or(close),
            low: quote.low.get(i).copied().unwrap_or(close),
            close,
            adj_close: adj_close.get(i).copied().unwrap_or(close),
            volume: quote.volume.get(i).copied().unwrap_or(0.0) as i64,
        };
        points.push(point);
    }
    Ok(points)
}

// --------------------------------------------------------------- serde types

#[derive(Deserialize, Default)]
struct SearchResponse {
    #[serde(default)]
    quotes: Vec<Quote>,
}

#[derive(Deserialize, Default)]
struct Quote {
    #[serde(default)]
    symbol: String,
    #[serde(default, rename = "shortname")]
    short_name: String,
    #[serde(default, rename = "longname")]
    long_name: String,
    #[serde(default, rename = "exchangeName")]
    exchange_name: Option<String>,
    #[serde(default, rename = "exchDisp")]
    exchange_display: Option<String>,
    #[serde(default, rename = "quoteType")]
    quote_type: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}
impl From<Quote> for SearchResult {
    fn from(q: Quote) -> Self {
        let name = if !q.long_name.is_empty() {
            q.long_name
        } else {
            q.short_name
        };
        let exchange = q.exchange_display.or(q.exchange_name);
        SearchResult {
            asset_type: asset_type_from_quote(q.quote_type.as_deref().unwrap_or("")),
            symbol: q.symbol,
            name,
            exchange_name: exchange,
            currency: q.currency,
        }
    }
}

#[derive(Deserialize, Default)]
struct ChartResponse {
    chart: Chart,
}

#[derive(Deserialize, Default)]
struct Chart {
    #[serde(default)]
    result: Vec<ChartResult>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChartResult {
    #[serde(default)]
    timestamp: Vec<i64>,
    #[serde(default)]
    indicators: Indicators,
}

#[derive(Deserialize, Default)]
struct Indicators {
    #[serde(default)]
    quote: Vec<QuoteSeries>,
    #[serde(default, rename = "adjclose")]
    adj_close: Vec<AdjSeries>,
}

#[derive(Deserialize, Default)]
struct QuoteSeries {
    #[serde(default)]
    open: Vec<f64>,
    #[serde(default)]
    high: Vec<f64>,
    #[serde(default)]
    low: Vec<f64>,
    #[serde(default)]
    close: Vec<f64>,
    #[serde(default)]
    volume: Vec<f64>,
}

#[derive(Deserialize, Default)]
struct AdjSeries {
    #[serde(default, rename = "adjclose")]
    adj_close: Vec<f64>,
}

// --------------------------------------------------------------- helpers

/// Map Yahoo's `quoteType` to our coarse `AssetType`. Anything that is not an
/// equity or currency is treated as a stock/fund-like instrument.
fn asset_type_from_quote(quote_type: &str) -> AssetType {
    match quote_type.to_uppercase().as_str() {
        "CURRENCY" | "FX" | "FOREX" | "CRYPTOCURRENCY" => AssetType::Currency,
        _ => AssetType::Stock,
    }
}

/// Convert a unix timestamp (seconds) to a `NaiveDate` in UTC.
fn timestamp_to_date(ts: i64) -> NaiveDate {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
}

/// Minimal percent-encoding for the query/symbol segments we send upstream.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.bytes() {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(c as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{c:02X}")),
        }
    }
    out
}

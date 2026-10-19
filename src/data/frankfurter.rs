//! Frankfurter.dev data source: historical currency exchange rates.
//!
//! Endpoints (no API key required, current API v2 — v1 is now frozen):
//! * `v2/currencies` – a list of `{ iso_code, name }` currency records.
//! * `v2/rates`      – a rate time series for one base and one (or many) quote
//!   currencies over a date range (`from`/`to` query parameters, ISO dates).
//!
//! The rate for a pair is modelled as a price series where the "close" (and the
//! other OHLC fields, plus adjusted close) all equal the daily rate. This lets
//! the charting and comparison logic treat currencies identically to stocks.

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;

use super::Client;
use crate::models::PricePoint;

const RATES_URL: &str = "https://api.frankfurter.dev/v2/rates";
const CURRENCIES_URL: &str = "https://api.frankfurter.dev/v2/currencies";

/// List all currency codes the service knows about, as `(code, name)` pairs
/// sorted by code.
pub fn available_currencies(client: &Client) -> Result<Vec<(String, String)>> {
    let resp = client
        .get(CURRENCIES_URL)
        .with_context(|| format!("currencies request failed: {CURRENCIES_URL}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Frankfurter returned HTTP {status} for currencies list"));
    }
    let list: Vec<CurrencyInfo> =
        resp.json().context("failed to parse Frankfurter currencies response")?;
    let mut out: Vec<(String, String)> =
        list.into_iter().map(|c| (c.iso_code, c.name)).filter(|(code, _)| !code.is_empty()).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Fetch the daily `base`-to-`target` rate series over `[from, to]`.
///
/// Each point has `close == open == high == low == adj_close == rate` and a
/// zero volume. Dates without a published rate are skipped by the upstream
/// service (the returned map only contains trading days).
pub fn convert(
    client: &Client,
    base: &str,
    target: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<PricePoint>> {
    let from = from.to_string();
    let to = to.to_string();
    let url = format!(
        "{RATES_URL}?base={}&quotes={}&from={from}&to={to}",
        urlencoded(base),
        urlencoded(target)
    );
    let resp = client.get(&url).with_context(|| format!("convert request failed: {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Frankfurter returned HTTP {status} for {base}->{target}"));
    }
    // v2 `/rates` returns a top-level JSON array of `{ date, base, quote, rate }`.
    let rows: Vec<RateEntry> = resp.json().context("failed to parse Frankfurter response")?;

    let mut points: Vec<PricePoint> = rows
        .into_iter()
        .map(|r| {
            let date = NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").ok()?;
            Some(PricePoint {
                asset_id: 0,
                date,
                open: r.rate,
                high: r.rate,
                low: r.rate,
                close: r.rate,
                adj_close: r.rate,
                volume: 0,
            })
        })
        .flatten()
        .collect();

    points.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(points)
}

// --------------------------------------------------------------- serde types

/// A single currency record returned by `v2/currencies`.
#[derive(Deserialize)]
struct CurrencyInfo {
    #[serde(default)]
    iso_code: String,
    #[serde(default)]
    name: String,
}

/// A single daily observation returned by `v2/rates` (one entry per date).
#[derive(Deserialize)]
struct RateEntry {
    #[serde(default)]
    date: String,
    #[serde(default)]
    rate: f64,
}

// --------------------------------------------------------------- helpers

/// Minimal percent-encoding for currency codes (normally plain 3-letter codes).
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

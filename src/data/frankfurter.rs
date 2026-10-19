//! Frankfurter.dev data source: historical currency exchange rates.
//!
//! Endpoints (no API key required):
//! * `v1/currencies` – a map of ISO code to currency name.
//! * `v1/{base}/{target}` – a rate time series for a currency pair over a date
//!   range (`from`/`to` query parameters, ISO dates).
//!
//! The rate for a pair is modelled as a price series where the "close" (and the
//! other OHLC fields, plus adjusted close) all equal the daily rate. This lets
//! the charting and comparison logic treat currencies identically to stocks.

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;

use super::Client;
use crate::models::PricePoint;

const BASE_URL: &str = "https://api.frankfurter.dev/v1";
const CURRENCIES_URL: &str = "https://api.frankfurter.dev/v1/currencies";

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
    let map: std::collections::BTreeMap<String, String> =
        resp.json().context("failed to parse Frankfurter currencies response")?;
    Ok(map.into_iter().collect())
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
    let url = format!("{BASE_URL}/{}/{}/?from={from}&to={to}", urlencoded(base), urlencoded(target));
    let resp = client.get(&url).with_context(|| format!("convert request failed: {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Frankfurter returned HTTP {status} for {base}->{target}"));
    }
    let body: ConvertResponse = resp.json().context("failed to parse Frankfurter response")?;

    let mut points: Vec<PricePoint> = body
        .timeseries
        .into_iter()
        .map(|(date_str, rate)| {
            let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").ok()?;
            Some(PricePoint {
                asset_id: 0,
                date,
                open: rate,
                high: rate,
                low: rate,
                close: rate,
                adj_close: rate,
                volume: 0,
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();

    points.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(points)
}

// --------------------------------------------------------------- serde types

#[derive(Deserialize, Default)]
struct ConvertResponse {
    #[serde(default)]
    timeseries: std::collections::BTreeMap<String, f64>,
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

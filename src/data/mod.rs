//! Network data sources.
//!
//! Two backends are implemented:
//! * [`yahoo`] – equity / crypto quotes and historical prices via Yahoo Finance.
//! * [`frankfurter`] – historical currency exchange rates via frankfurter.dev.
//!
//! All network access is done through a single shared blocking HTTP client so
//! that the data functions can be invoked from a worker `std::thread` without
//! pulling an async runtime onto the eframe UI thread.

pub mod frankfurter;
pub mod yahoo;

use std::time::Duration;

/// A shared HTTP client configured with a sensible user agent and timeouts.
/// Cloning it is cheap (it shares the connection pool via an interior handle),
/// so each spawned worker thread can take its own clone.
#[derive(Clone)]
pub struct Client {
    inner: reqwest::blocking::Client,
}

impl Client {
    /// Build a client. Falls back to a default client if construction fails
    /// (e.g. native TLS unavailable on some platforms).
    pub fn new() -> Self {
        let user_agent = "stock_currency_tracker/1.0 (rust-desktop-finance)";
        let builder = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        let inner = builder
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { inner }
    }

    pub fn get(&self, url: &str) -> reqwest::Result<reqwest::blocking::Response> {
        self.inner.get(url).send()
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

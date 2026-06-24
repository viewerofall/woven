//! Weather fetch with 10-minute in-memory cache.
//!
//! Uses wttr.in (no API key, free, returns plain text).
//! Format: "Partly Cloudy 72°F RH 65%"
//!
//! Cache policy: one fetch per 10 minutes, non-blocking (returns stale on miss,
//! spawns a thread to refresh). On startup returns None until first fetch completes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

const CACHE_TTL: Duration = Duration::from_secs(600); // 10 minutes
const WTTR_URL: &str = "https://wttr.in/?format=%C+%t+RH+%h";

#[derive(Clone)]
struct CachedWeather {
    text:     String,
    fetched:  Instant,
}

#[derive(Clone)]
pub struct WeatherCache {
    inner: Arc<Mutex<Option<CachedWeather>>>,
}

impl WeatherCache {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(None)) }
    }

    /// Returns the cached weather string, or None if not yet fetched.
    /// Spawns a background refresh if the cache is stale or empty.
    pub fn get(&self) -> Option<String> {
        let needs_refresh = {
            let guard = self.inner.lock().unwrap();
            match &*guard {
                None => true,
                Some(c) => c.fetched.elapsed() >= CACHE_TTL,
            }
        };

        if needs_refresh {
            self.spawn_refresh();
        }

        let guard = self.inner.lock().unwrap();
        guard.as_ref().map(|c| c.text.clone())
    }

    fn spawn_refresh(&self) {
        let cache = self.inner.clone();
        std::thread::Builder::new()
            .name("woven-lite-weather".into())
            .spawn(move || {
                match fetch_weather() {
                    Ok(text) => {
                        info!("weather: fetched \"{}\"", text);
                        let mut guard = cache.lock().unwrap();
                        *guard = Some(CachedWeather { text, fetched: Instant::now() });
                    }
                    Err(e) => warn!("weather fetch failed: {e:#}"),
                }
            })
            .ok();
    }
}

fn fetch_weather() -> anyhow::Result<String> {
    let resp = ureq::get(WTTR_URL)
        .call()?
        .body_mut()
        .read_to_string()?;
    // wttr.in may include ANSI escapes in some formats; strip them just in case.
    let clean = strip_ansi(&resp).trim().to_string();
    Ok(if clean.is_empty() { "Weather unavailable".into() } else { clean })
}

fn strip_ansi(s: &str) -> String {
    // Remove ESC[ ... m sequences
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            // consume until 'm'
            for ch in chars.by_ref() {
                if ch == 'm' { break; }
            }
        } else {
            out.push(c);
        }
    }
    out
}

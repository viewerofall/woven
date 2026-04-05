//! Synchronous HTTP client for Lua plugins.

use std::time::Duration;

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub ok: bool,
}

/// Perform a GET request. Blocks the calling thread.
pub fn get(url: &str, timeout_secs: u64, headers: &[(String, String)]) -> HttpResponse {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(timeout_secs)))
            .build(),
    );

    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.header(k, v);
    }

    match req.call() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.into_body().read_to_string().unwrap_or_default();
            HttpResponse { status, body, ok: status < 400 }
        }
        Err(e) => HttpResponse {
            status: 0,
            body: e.to_string(),
            ok: false,
        },
    }
}

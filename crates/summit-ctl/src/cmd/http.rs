//! Shared HTTP request helpers for CLI commands.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/api", port)
}

pub async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    reqwest::get(url)
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<T>()
        .await
        .context("failed to parse response")
}

pub async fn post_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    reqwest::Client::new()
        .post(url)
        .send()
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<T>()
        .await
        .context("failed to parse response")
}

pub async fn post_json_body<T, R>(url: &str, body: &T) -> Result<R>
where
    T: Serialize,
    R: for<'de> Deserialize<'de>,
{
    reqwest::Client::new()
        .post(url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<R>()
        .await
        .context("failed to parse response")
}

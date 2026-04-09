use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::time::Duration;

pub struct NodeClient {
    base_url: String,
    client:   Client,
}

impl NodeClient {
    pub fn new(url: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connection_verbose(false)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(NodeClient {
            base_url: url.trim_end_matches('/').to_string(),
            client,
        })
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.client.get(&url).send().context("GET request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: serde_json::Value = resp.json().unwrap_or_default();
            let msg = body["error"].as_str().unwrap_or("unknown error").to_string();
            anyhow::bail!("HTTP {}: {}", status, msg);
        }

        resp.json::<T>().context("JSON parse")
    }

    pub fn post<T: DeserializeOwned>(&self, path: &str, payload: &serde_json::Value) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.client.post(&url).json(payload).send().context("POST request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: serde_json::Value = resp.json().unwrap_or_default();
            let msg = body["error"].as_str().unwrap_or("unknown error").to_string();
            anyhow::bail!("HTTP {}: {}", status, msg);
        }

        resp.json::<T>().context("JSON parse")
    }

    pub fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

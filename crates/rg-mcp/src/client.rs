//! Thin async client for the IronForge REST API.
//!
//! Wraps `reqwest::Client` and adds:
//! - `/api/v1/` prefix
//! - Bearer token injection
//! - Status-code → `crate::Error` conversion

use super::AppState;
use serde::de::DeserializeOwned;

pub struct ApiClient {
    inner: reqwest::Client,
    base: String,
}

impl ApiClient {
    pub fn new(state: &AppState) -> Self {
        Self {
            inner: state.http_client(),
            base: state.api_base.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/{}", self.base, path.trim_start_matches('/'))
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> crate::Result<T> {
        let resp = self.inner.get(self.url(path)).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(super::Error::Api { status, body });
        }
        Ok(resp.json().await?)
    }

    pub async fn get_raw(&self, path: &str) -> crate::Result<String> {
        let resp = self.inner.get(self.url(path)).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(super::Error::Api { status, body });
        }
        Ok(resp.text().await?)
    }

    pub async fn get_bytes(&self, path: &str) -> crate::Result<Vec<u8>> {
        let resp = self.inner.get(self.url(path)).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(super::Error::Api { status, body });
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

impl ApiClient {
    /// Send any request and hand back `(status, body)` without turning non-2xx
    /// into an error, so callers can read the API's structured error body.
    ///
    /// `session` is the `(id, secret)` pair for collaboration calls; the secret
    /// travels only in a header marked sensitive, never in the URL.
    pub async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        session: Option<(&str, &str)>,
        body: Option<&serde_json::Value>,
    ) -> crate::Result<(u16, String)> {
        let mut request = self.inner.request(method, self.url(path));
        if let Some((id, secret)) = session {
            let mut secret_value = reqwest::header::HeaderValue::from_str(secret)
                .map_err(|_| super::Error::Config("session secret is not a valid header value".into()))?;
            secret_value.set_sensitive(true);
            request = request
                .header("x-ironforge-session", id)
                .header("x-ironforge-session-secret", secret_value);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let resp = request.send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }
}

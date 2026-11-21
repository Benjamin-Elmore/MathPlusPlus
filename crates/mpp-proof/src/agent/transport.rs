//! The HTTP layer, behind a trait so the agent loop can be tested with a scripted fake.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

/// One HTTP request.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    /// Headers other than the API key, which the transport adds itself.
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish()
    }
}

/// One HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    /// `retry-after` in seconds, when the server sent one.
    pub retry_after: Option<u64>,
}

/// A transport failure (no response at all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TransportError {}

/// Sends requests. The API key is handed over separately so that requests, logs and
/// test recordings never hold it.
pub trait Transport: Send + Sync {
    fn send(
        &self,
        req: &HttpRequest,
        api_key: &super::key::ApiKey,
    ) -> Result<HttpResponse, TransportError>;
}

/// The real transport: blocking `reqwest` over TLS.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// A client with a generous timeout (a model turn can take minutes).
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| TransportError(format!("could not start the HTTP client: {e}")))?;
        Ok(ReqwestTransport { client })
    }
}

impl Transport for ReqwestTransport {
    fn send(
        &self,
        req: &HttpRequest,
        api_key: &super::key::ApiKey,
    ) -> Result<HttpResponse, TransportError> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|_| TransportError(format!("bad method {}", req.method)))?;
        let mut b = self
            .client
            .request(method, &req.url)
            .header("x-api-key", api_key.expose());
        for (k, v) in &req.headers {
            b = b.header(k.as_str(), v.as_str());
        }
        if let Some(body) = &req.body {
            b = b.body(body.clone());
        }
        // Errors from reqwest name the URL, never the headers, so the key cannot leak.
        let resp = b
            .send()
            .map_err(|e| TransportError(format!("the request failed: {e}")))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        let body = resp
            .text()
            .map_err(|e| TransportError(format!("the response could not be read: {e}")))?;
        Ok(HttpResponse {
            status,
            body,
            retry_after,
        })
    }
}

/// A scripted fake server for tests: answers requests in order from a queue, and
/// records every request it saw (without the key).
pub struct ScriptedTransport {
    replies: Mutex<VecDeque<Scripted>>,
    seen: Mutex<Vec<HttpRequest>>,
    keys_seen: Mutex<usize>,
}

/// One scripted reply: which request it expects (method and URL suffix) and what to
/// answer.
#[derive(Debug, Clone)]
pub struct Scripted {
    pub method: String,
    pub url_ends_with: String,
    pub status: u16,
    pub body: String,
}

impl Scripted {
    /// `200` with a JSON body.
    pub fn ok(method: &str, url_ends_with: &str, body: &str) -> Self {
        Scripted {
            method: method.into(),
            url_ends_with: url_ends_with.into(),
            status: 200,
            body: body.into(),
        }
    }
}

impl ScriptedTransport {
    pub fn new(replies: Vec<Scripted>) -> Self {
        ScriptedTransport {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
            keys_seen: Mutex::new(0),
        }
    }

    /// The requests sent so far.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// How many requests carried a non-empty key.
    pub fn keyed_requests(&self) -> usize {
        self.keys_seen.lock().map(|k| *k).unwrap_or(0)
    }

    /// Replies not used yet.
    pub fn remaining(&self) -> usize {
        self.replies.lock().map(|r| r.len()).unwrap_or(0)
    }
}

impl Transport for ScriptedTransport {
    fn send(
        &self,
        req: &HttpRequest,
        api_key: &super::key::ApiKey,
    ) -> Result<HttpResponse, TransportError> {
        if let Ok(mut s) = self.seen.lock() {
            s.push(req.clone());
        }
        if !api_key.expose().is_empty() {
            if let Ok(mut k) = self.keys_seen.lock() {
                *k += 1;
            }
        }
        let next = self
            .replies
            .lock()
            .map_err(|_| TransportError("the script is poisoned".into()))?
            .pop_front();
        let Some(r) = next else {
            return Err(TransportError(format!(
                "the script has no reply left for {} {}",
                req.method, req.url
            )));
        };
        if r.method != req.method || !req.url.ends_with(&r.url_ends_with) {
            return Err(TransportError(format!(
                "the script expected {} …{}, got {} {}",
                r.method, r.url_ends_with, req.method, req.url
            )));
        }
        Ok(HttpResponse {
            status: r.status,
            body: r.body,
            retry_after: None,
        })
    }
}

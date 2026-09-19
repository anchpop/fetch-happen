//! The native transport: a streaming reqwest client with the same API shape
//! as the web transport. `send()` resolves once the response headers arrive;
//! the body is read on demand, either whole (`bytes`, `text`, `json`) or
//! chunk by chunk (`stream_reader`), and abort signals are honoured while
//! awaiting headers and every body read.
use crate::{AbortSignal, Error, Method, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use web_sys::RequestMode;

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Transport(err.to_string())
    }
}

/// One shared client so requests reuse connection pools and TLS sessions.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// A builder for HTTP requests
pub struct RequestBuilder {
    url: String,
    method: Method,
    headers: HashMap<String, String>,
    body: Option<String>,
    signal: Option<AbortSignal>,
}

impl RequestBuilder {
    pub(crate) fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method,
            headers: HashMap::new(),
            body: None,
            signal: None,
        }
    }

    /// Set a header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set multiple headers
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Set the request mode. CORS is a browser concept; accepted for API
    /// parity and ignored natively.
    pub fn mode(self, _mode: RequestMode) -> Self {
        self
    }

    /// Set the request body as a string
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the request body as JSON
    pub fn json<T: Serialize>(mut self, json: &T) -> Result<Self> {
        let body = serde_json::to_string(json)?;
        self.body = Some(body);
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        Ok(self)
    }

    /// Abort pending headers or body reads when the signal fires.
    pub fn abort_signal(mut self, signal: impl Into<AbortSignal>) -> Self {
        self.signal = Some(signal.into());
        self
    }

    /// Send the request and get a Response once its headers arrive.
    pub async fn send(mut self) -> Result<Response> {
        let signal = self.signal.take();
        let response = until(signal.as_ref(), self.send_inner()).await?;
        Ok(Response { signal, ..response })
    }

    async fn send_inner(self) -> Result<Response> {
        let method = reqwest::Method::from_bytes(self.method.as_str().as_bytes())
            .expect("Method::as_str is always a valid HTTP method");
        let mut request = client().request(method, &self.url);
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        if let Some(body) = self.body {
            request = request.body(body);
        }

        let response = request.send().await?;
        Ok(Response {
            status: response.status().as_u16(),
            ok: response.status().is_success(),
            headers: response.headers().clone(),
            body: Body::new(response),
            signal: None,
        })
    }
}

/// Run `future`, failing with [`Error::Aborted`] if `signal` fires first.
async fn until<T>(
    signal: Option<&AbortSignal>,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    match signal {
        Some(signal) => signal.until(future).await.map_err(|_| Error::Aborted)?,
        None => future.await,
    }
}

/// The unread remainder of a response body. Like the browser's, a body can
/// be consumed once; reading it again is an error rather than empty data.
struct Body(Mutex<Option<reqwest::Response>>);

impl Body {
    fn new(response: reqwest::Response) -> Self {
        Self(Mutex::new(Some(response)))
    }

    fn take(&self) -> Result<reqwest::Response> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| Error::Transport("body already consumed".to_string()))
    }

    fn put_back(&self, response: reqwest::Response) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(response);
    }
}

/// A response from a fetch request. Headers are available immediately; the
/// body streams from the socket as it is read.
pub struct Response {
    status: u16,
    ok: bool,
    headers: reqwest::header::HeaderMap,
    body: Body,
    signal: Option<AbortSignal>,
}

impl Response {
    /// Get the status code
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Check if the response was successful (status 200-299)
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// Get a header value
    pub fn header(&self, name: &str) -> Result<Option<String>> {
        match self.headers.get(name) {
            Some(value) => Ok(Some(
                value
                    .to_str()
                    .map_err(|e| Error::Transport(format!("non-UTF-8 header value: {e}")))?
                    .to_string(),
            )),
            None => Ok(None),
        }
    }

    /// Get the response body as text. Malformed UTF-8 is replaced rather
    /// than rejected, matching the browser's `Response.text()`.
    pub async fn text(&self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes().await?).into_owned())
    }

    /// Get the response body as JSON
    pub async fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        Ok(serde_json::from_slice(&self.bytes().await?)?)
    }

    /// Get the response body as a dynamic JSON value
    pub async fn json_value(&self) -> Result<Value> {
        self.json().await
    }

    /// Read the whole response body
    pub async fn bytes(&self) -> Result<Vec<u8>> {
        let response = self.body.take()?;
        let bytes = until(self.signal.as_ref(), async { Ok(response.bytes().await?) }).await?;
        Ok(bytes.to_vec())
    }

    /// Ensure the response was successful, returning an error if not
    pub fn error_for_status(self) -> Result<Self> {
        if self.ok() {
            Ok(self)
        } else {
            let status = self.status();
            let text = format!("HTTP Error {}", status);
            Err(Error::HttpError(status, text))
        }
    }

    /// Get a stream reader for reading chunks from the response as they
    /// arrive. Takes over the body, so it can't be read from `self` again.
    pub fn stream_reader(&self) -> Result<StreamReader> {
        Ok(StreamReader {
            body: Body::new(self.body.take()?),
            signal: self.signal.clone(),
        })
    }
}

/// A reader for streaming response bodies chunk by chunk
pub struct StreamReader {
    body: Body,
    signal: Option<AbortSignal>,
}

impl StreamReader {
    /// Read the next chunk from the stream
    /// Returns Ok(Some(bytes)) if a chunk is available
    /// Returns Ok(None) if the stream is finished
    pub async fn read_chunk(&self) -> Result<Option<Vec<u8>>> {
        let mut response = self.body.take()?;
        let chunk = until(self.signal.as_ref(), async { Ok(response.chunk().await?) }).await?;
        // Kept after the end too, so reading past it stays `None` as in the browser.
        self.body.put_back(response);
        Ok(chunk.map(|chunk| chunk.to_vec()))
    }

    /// Release the reader; the connection is dropped with it.
    pub fn cancel(self) -> Result<()> {
        Ok(())
    }
}

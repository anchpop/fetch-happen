use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortSignal, ReadableStream, ReadableStreamDefaultReader, Request as WebRequest, RequestInit,
    Response as WebResponse,
};

pub use web_sys::{AbortController, RequestMode};

pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when making a request
#[derive(Debug)]
pub enum Error {
    /// JavaScript error
    JsError(JsValue),
    /// HTTP error with status code
    HttpError(u16, String),
    /// JSON parsing error
    JsonError(String),
    /// Request was aborted
    Aborted,
}

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::JsError(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::JsonError(err.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::JsError(e) => write!(f, "JavaScript error: {:?}", e),
            Error::HttpError(status, msg) => write!(f, "HTTP error {}: {}", status, msg),
            Error::JsonError(e) => write!(f, "JSON error: {}", e),
            Error::Aborted => write!(f, "Request was aborted"),
        }
    }
}

impl std::error::Error for Error {}

/// HTTP methods
#[derive(Debug, Clone, Copy)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

impl Method {
    fn as_str(&self) -> &'static str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::PATCH => "PATCH",
            Method::HEAD => "HEAD",
            Method::OPTIONS => "OPTIONS",
        }
    }
}

/// A builder for HTTP requests
pub struct RequestBuilder {
    url: String,
    method: Method,
    headers: HashMap<String, String>,
    body: Option<String>,
    mode: RequestMode,
    signal: Option<AbortSignal>,
}

impl RequestBuilder {
    fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method,
            headers: HashMap::new(),
            body: None,
            mode: RequestMode::Cors,
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

    /// Set the request mode (Cors, NoCors, SameOrigin)
    pub fn mode(mut self, mode: RequestMode) -> Self {
        self.mode = mode;
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

    /// Set an abort signal for the request
    pub fn abort_signal(mut self, signal: AbortSignal) -> Self {
        self.signal = Some(signal);
        self
    }

    /// Send the request and get a Response
    pub async fn send(self) -> Result<Response> {
        let opts = RequestInit::new();
        opts.set_method(self.method.as_str());
        opts.set_mode(self.mode);

        if let Some(body) = &self.body {
            opts.set_body(&JsValue::from_str(body));
        }

        if let Some(signal) = &self.signal {
            opts.set_signal(Some(signal));
        }

        let request = WebRequest::new_with_str_and_init(&self.url, &opts)?;
        let headers = request.headers();

        for (key, value) in &self.headers {
            headers.set(key, value)?;
        }

        let window = web_sys::window()
            .ok_or_else(|| Error::JsError(JsValue::from_str("Failed to get window")))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                // Check if this is an abort error
                if let Some(error) = e.dyn_ref::<js_sys::Error>() {
                    if error.name() == "AbortError" {
                        return Error::Aborted;
                    }
                }
                Error::JsError(e)
            })?;
        let web_response: WebResponse = resp_value
            .dyn_into()
            .map_err(|_| Error::JsError(JsValue::from_str("Response conversion failed")))?;

        Ok(Response::from_web_response(web_response))
    }
}

/// A response from a fetch request
pub struct Response {
    inner: WebResponse,
}

impl Response {
    fn from_web_response(response: WebResponse) -> Self {
        Self { inner: response }
    }

    /// Get the status code
    pub fn status(&self) -> u16 {
        self.inner.status()
    }

    /// Check if the response was successful (status 200-299)
    pub fn ok(&self) -> bool {
        self.inner.ok()
    }

    /// Get a header value
    pub fn header(&self, name: &str) -> Result<Option<String>> {
        Ok(self.inner.headers().get(name)?)
    }

    /// Get the response body as text
    pub async fn text(&self) -> Result<String> {
        let promise = self.inner.text().map_err(Error::JsError)?;
        let text = JsFuture::from(promise).await?;

        text.as_string()
            .ok_or_else(|| Error::JsError(JsValue::from_str("Failed to convert to string")))
    }

    /// Get the response body as JSON
    pub async fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        let text = self.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Get the response body as a dynamic JSON value
    pub async fn json_value(&self) -> Result<Value> {
        self.json().await
    }

    /// Get the response body as bytes
    pub async fn bytes(&self) -> Result<Vec<u8>> {
        let promise = self.inner.array_buffer().map_err(Error::JsError)?;
        let array_buffer = JsFuture::from(promise).await?;
        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
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

    /// Get the response body as a readable stream
    pub fn stream(&self) -> Result<ReadableStream> {
        self.inner
            .body()
            .ok_or_else(|| Error::JsError(JsValue::from_str("No body in response")))
    }

    /// Get a stream reader for reading chunks from the response
    pub fn stream_reader(&self) -> Result<StreamReader> {
        let stream = self.stream()?;
        let reader = stream
            .get_reader()
            .dyn_into::<ReadableStreamDefaultReader>()
            .map_err(|_| Error::JsError(JsValue::from_str("Failed to get stream reader")))?;
        Ok(StreamReader { reader })
    }
}

/// A reader for streaming response bodies chunk by chunk
pub struct StreamReader {
    reader: ReadableStreamDefaultReader,
}

impl StreamReader {
    /// Read the next chunk from the stream
    /// Returns Ok(Some(bytes)) if a chunk is available
    /// Returns Ok(None) if the stream is finished
    pub async fn read_chunk(&self) -> Result<Option<Vec<u8>>> {
        let result = JsFuture::from(self.reader.read()).await?;

        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))?
            .as_bool()
            .unwrap_or(false);

        if done {
            return Ok(None);
        }

        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))?;
        let uint8_array = js_sys::Uint8Array::new(&value);
        Ok(Some(uint8_array.to_vec()))
    }

    /// Release the reader lock
    pub fn cancel(self) -> Result<()> {
        self.reader.release_lock();
        Ok(())
    }
}

/// Main client for making HTTP requests
pub struct Client;

impl Client {
    /// Make a GET request
    pub fn get(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::GET, url)
    }

    /// Make a POST request
    pub fn post(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::POST, url)
    }

    /// Make a PUT request
    pub fn put(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::PUT, url)
    }

    /// Make a DELETE request
    pub fn delete(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::DELETE, url)
    }

    /// Make a PATCH request
    pub fn patch(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::PATCH, url)
    }

    /// Make a HEAD request
    pub fn head(&self, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::HEAD, url)
    }
}

/// Convenience function for making a GET request
pub async fn get(url: impl Into<String>) -> Result<Response> {
    Client.get(url).send().await
}

/// Convenience function for making a POST request with JSON body
pub async fn post_json<T: Serialize>(url: impl Into<String>, json: &T) -> Result<Response> {
    Client.post(url).json(json)?.send().await
}

#[cfg(all(feature = "examples", target_arch = "wasm32"))]
pub mod examples {
    use super::*;
    use web_sys::console;
    use wasm_bindgen::prelude::wasm_bindgen;

    /// Example of streaming a large response body in chunks
    #[wasm_bindgen]
    pub async fn stream_large_file() {
        let client = Client;
        let url = "https://raw.githubusercontent.com/yaptown/yap/refs/heads/main/out/deu/frequency_lists/combined/frequencies.jsonl";

        console::log_1(&"Starting streaming download...".into());

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Request failed: {}", e).into());
                return;
            }
        };

        let response = match response.error_for_status() {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("HTTP error: {}", e).into());
                return;
            }
        };

        // Get a stream reader
        let reader = match response.stream_reader() {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Failed to get stream reader: {}", e).into());
                return;
            }
        };

        let mut total_bytes = 0;
        let mut chunk_count = 0;

        // Read chunks until the stream is done
        loop {
            match reader.read_chunk().await {
                Ok(Some(chunk)) => {
                    total_bytes += chunk.len();
                    chunk_count += 1;
                    console::log_1(&format!("Received chunk {}: {} bytes", chunk_count, chunk.len()).into());
                }
                Ok(None) => break,
                Err(e) => {
                    console::error_1(&format!("Error reading chunk: {}", e).into());
                    return;
                }
            }
        }

        console::log_1(&format!("✓ Total: {} bytes in {} chunks", total_bytes, chunk_count).into());
    }

    /// Example of streaming text content line by line
    #[wasm_bindgen]
    pub async fn stream_text_content() {
        let client = Client;
        let url = "https://raw.githubusercontent.com/yaptown/yap/refs/heads/main/out/deu/frequency_lists/combined/frequencies.jsonl";

        console::log_1(&"Starting line-by-line streaming...".into());

        let response = match client.get(url).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Request failed: {}", e).into());
                return;
            }
        };

        let reader = match response.stream_reader() {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Failed to get stream reader: {}", e).into());
                return;
            }
        };

        let mut buffer = Vec::new();
        let mut line_count = 0;

        loop {
            let chunk = match reader.read_chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    console::error_1(&format!("Error reading chunk: {}", e).into());
                    return;
                }
            };

            buffer.extend_from_slice(&chunk);

            // Process complete lines from the buffer
            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line_bytes);
                line_count += 1;

                // Only log first few lines to avoid spam
                if line_count <= 5 {
                    console::log_1(&format!("Line {}: {}", line_count, line.trim()).into());
                }
            }
        }

        // Process any remaining data in the buffer
        if !buffer.is_empty() {
            let line = String::from_utf8_lossy(&buffer);
            line_count += 1;
            console::log_1(&format!("Last line: {}", line.trim()).into());
        }

        console::log_1(&format!("✓ Processed {} lines total", line_count).into());
    }

    /// Example of downloading with progress tracking
    #[wasm_bindgen]
    pub async fn download_with_progress() {
        let client = Client;
        let url = "https://raw.githubusercontent.com/yaptown/yap/refs/heads/main/out/deu/frequency_lists/combined/frequencies.jsonl";

        console::log_1(&"Starting download with progress tracking...".into());

        let response = match client.get(url).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Request failed: {}", e).into());
                return;
            }
        };

        // Get content length if available
        let content_length = response
            .header("content-length")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<usize>().ok());

        if let Some(total) = content_length {
            console::log_1(&format!("Content-Length: {} bytes", total).into());
        } else {
            console::log_1(&"Content-Length not available".into());
        }

        let reader = match response.stream_reader() {
            Ok(r) => r,
            Err(e) => {
                console::error_1(&format!("Failed to get stream reader: {}", e).into());
                return;
            }
        };

        let mut downloaded = Vec::new();
        let mut last_logged_percent = 0;

        loop {
            let chunk = match reader.read_chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    console::error_1(&format!("Error reading chunk: {}", e).into());
                    return;
                }
            };

            downloaded.extend_from_slice(&chunk);

            if let Some(total) = content_length {
                let progress = (downloaded.len() as f64 / total as f64) * 100.0;
                let progress_int = progress as u32;

                // Only log every 10%
                if progress_int >= last_logged_percent + 10 {
                    console::log_1(&format!("Progress: {:.1}% ({}/{})", progress, downloaded.len(), total).into());
                    last_logged_percent = progress_int;
                }
            }
        }

        console::log_1(&format!("✓ Download complete: {} bytes", downloaded.len()).into());
    }
}

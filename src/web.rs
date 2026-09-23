//! The wasm transport: the browser's `fetch` API via web-sys.
use crate::{AbortSignal, Error, Method, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ReadableStream, ReadableStreamDefaultReader, Request as WebRequest, RequestInit, RequestMode,
    Response as WebResponse,
};

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::Transport(format!("{value:?}"))
    }
}

fn js_error(value: JsValue) -> Error {
    Error::Transport(format!("{value:?}"))
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
    pub(crate) fn new(method: Method, url: impl Into<String>) -> Self {
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
    pub fn abort_signal(mut self, signal: impl Into<AbortSignal>) -> Self {
        self.signal = Some(signal.into());
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
            opts.set_signal(Some(signal.as_web()));
        }

        let request = WebRequest::new_with_str_and_init(&self.url, &opts)?;
        let headers = request.headers();

        for (key, value) in &self.headers {
            headers.set(key, value)?;
        }

        // Main thread or Web Worker: fetch lives on whichever global this is.
        let global = js_sys::global();
        let promise = if let Some(window) = global.dyn_ref::<web_sys::Window>() {
            window.fetch_with_request(&request)
        } else if let Some(worker) = global.dyn_ref::<web_sys::WorkerGlobalScope>() {
            worker.fetch_with_request(&request)
        } else {
            return Err(Error::Transport(
                "No window or worker global scope".to_string(),
            ));
        };

        let resp_value = JsFuture::from(promise).await.map_err(|e| {
            // Check if this is an abort error
            if let Some(error) = e.dyn_ref::<js_sys::Error>() {
                if error.name() == "AbortError" {
                    return Error::Aborted;
                }
            }
            js_error(e)
        })?;
        let web_response: WebResponse = resp_value
            .dyn_into()
            .map_err(|_| Error::Transport("Response conversion failed".to_string()))?;

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
        let promise = self.inner.text().map_err(js_error)?;
        let text = JsFuture::from(promise).await?;

        text.as_string()
            .ok_or_else(|| Error::Transport("Failed to convert to string".to_string()))
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
        let promise = self.inner.array_buffer().map_err(js_error)?;
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
            .ok_or_else(|| Error::Transport("No body in response".to_string()))
    }

    /// Get a stream reader for reading chunks from the response
    pub fn stream_reader(&self) -> Result<StreamReader> {
        let stream = self.stream()?;
        let reader = stream
            .get_reader()
            .dyn_into::<ReadableStreamDefaultReader>()
            .map_err(|_| Error::Transport("Failed to get stream reader".to_string()))?;
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

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request as WebRequest, RequestInit, Response as WebResponse};

pub use web_sys::RequestMode;

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
}

impl RequestBuilder {
    fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method,
            headers: HashMap::new(),
            body: None,
            mode: RequestMode::Cors,
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

    /// Send the request and get a Response
    pub async fn send(self) -> Result<Response> {
        let opts = RequestInit::new();
        opts.set_method(self.method.as_str());
        opts.set_mode(self.mode);

        if let Some(body) = &self.body {
            opts.set_body(&JsValue::from_str(body));
        }

        let request = WebRequest::new_with_str_and_init(&self.url, &opts)?;
        let headers = request.headers();

        for (key, value) in &self.headers {
            headers.set(key, value)?;
        }

        let window = web_sys::window()
            .ok_or_else(|| Error::JsError(JsValue::from_str("Failed to get window")))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
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
    pub async fn text(self) -> Result<String> {
        let promise = self.inner.text().map_err(Error::JsError)?;
        let text = JsFuture::from(promise).await?;

        text.as_string()
            .ok_or_else(|| Error::JsError(JsValue::from_str("Failed to convert to string")))
    }

    /// Get the response body as JSON
    pub async fn json<T: for<'de> Deserialize<'de>>(self) -> Result<T> {
        let text = self.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Get the response body as a dynamic JSON value
    pub async fn json_value(self) -> Result<Value> {
        self.json().await
    }

    /// Get the response body as bytes
    pub async fn bytes(self) -> Result<Vec<u8>> {
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
}

/// Convenience function for making a GET request
pub async fn get(url: impl Into<String>) -> Result<Response> {
    Client.get(url).send().await
}

/// Convenience function for making a POST request with JSON body
pub async fn post_json<T: Serialize>(url: impl Into<String>, json: &T) -> Result<Response> {
    Client.post(url).json(json)?.send().await
}

#![allow(dead_code, unused)]

use fetch_happen::{Client, Error, Result};
use wasm_bindgen::prelude::*;
use web_sys::AbortController;

/// Example of using AbortController to cancel a request
async fn abortable_request() -> Result<String> {
    let client = Client;

    // Create an AbortController
    let abort_controller = AbortController::new().map_err(|e| Error::JsError(e))?;
    let signal = abort_controller.signal();

    // Create the request with the abort signal
    let request_future = client
        .get("https://api.github.com/repos/rust-lang/rust/branches/master")
        .header("Accept", "application/vnd.github.v3+json")
        .abort_signal(signal)
        .send();

    // Simulate deciding to abort after 500ms
    // In a real application, this could be triggered by user action
    let abort_controller_clone = abort_controller.clone();
    wasm_bindgen_futures::spawn_local(async move {
        // Wait 500ms
        js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500)
                .unwrap();
        });

        // Abort the request
        abort_controller_clone.abort();
    });

    // Try to complete the request
    match request_future.await {
        Ok(response) => response.text().await,
        Err(Error::Aborted) => Err(Error::Aborted),
        Err(e) => Err(e),
    }
}

/// Example of using AbortController with timeout
async fn request_with_timeout(timeout_ms: i32) -> Result<String> {
    let client = Client;

    // Create an AbortController
    let abort_controller = AbortController::new().map_err(|e| Error::JsError(e))?;
    let signal = abort_controller.signal();

    // Set up timeout
    let abort_controller_clone = abort_controller.clone();
    let timeout_handle = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            &js_sys::Function::new_no_args(&format!(
                "(() => {{ const controller = arguments[0]; controller.abort(); }})"
            ))
            .bind1(&JsValue::undefined(), &abort_controller_clone),
            timeout_ms,
        )
        .map_err(|e| Error::JsError(e))?;

    // Make the request
    let result = client
        .get("https://api.github.com/repos/rust-lang/rust/branches/master")
        .signal(signal)
        .send()
        .await;

    // Clear the timeout if request completed
    web_sys::window()
        .unwrap()
        .clear_timeout_with_handle(timeout_handle);

    match result {
        Ok(response) => response.text().await,
        Err(e) => Err(e),
    }
}

/// Example of sharing an AbortController between multiple requests
async fn multiple_requests_with_shared_abort() -> Result<()> {
    let client = Client;

    // Create a single AbortController for multiple requests
    let abort_controller = AbortController::new().map_err(|e| Error::JsError(e))?;
    let signal = abort_controller.signal();

    // Launch multiple requests with the same signal
    let request1 = client
        .get("https://api.github.com/repos/rust-lang/rust")
        .signal(signal.clone())
        .send();

    let request2 = client
        .get("https://api.github.com/repos/rust-lang/cargo")
        .signal(signal.clone())
        .send();

    let request3 = client
        .get("https://api.github.com/repos/rust-lang/rustup")
        .signal(signal)
        .send();

    // If we abort, all three requests will be cancelled
    // abort_controller.abort();

    // Wait for all requests
    let (result1, result2, result3) = futures::join!(request1, request2, request3);

    // Handle results
    match (result1, result2, result3) {
        (Ok(_), Ok(_), Ok(_)) => Ok(()),
        _ => Err(Error::JsError(JsValue::from_str(
            "One or more requests failed",
        ))),
    }
}

fn main() {}

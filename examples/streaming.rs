#![allow(dead_code, unused)]

use fetch_happen::{Client, Result};
use wasm_bindgen::prelude::*;
use web_sys::console;

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
                console::log_1(
                    &format!("Received chunk {}: {} bytes", chunk_count, chunk.len()).into(),
                );
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

    let response = match client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
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

    let response = match client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
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
                console::log_1(
                    &format!(
                        "Progress: {:.1}% ({}/{})",
                        progress,
                        downloaded.len(),
                        total
                    )
                    .into(),
                );
                last_logged_percent = progress_int;
            }
        }
    }

    console::log_1(&format!("✓ Download complete: {} bytes", downloaded.len()).into());
}

fn main() {
    println!("This is a WASM example that needs to run in a browser.");
    println!("\nTo test the streaming examples:");
    println!("1. Build WASM: wasm-pack build --target web --dev --features examples");
    println!("2. Serve the directory: python3 -m http.server 8000");
    println!("3. Open http://localhost:8000/examples/streaming.html");
    println!("4. Open browser DevTools console to see output");
    println!("5. Click the buttons to test different streaming methods");
}

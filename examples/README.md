# Examples

## Running the Streaming Examples

The streaming examples demonstrate how to use `fetch-happen` to stream large responses in chunks.

### Setup

1. Install wasm-pack if you haven't already:
```bash
cargo install wasm-pack
```

2. Build the WASM package with the examples feature:
```bash
wasm-pack build --target web --dev --features examples
```

3. Serve the examples directory with a local web server:
```bash
# Using Python
python3 -m http.server 8000

# Or using Node.js http-server
npx http-server -p 8000
```

4. Open your browser to:
```
http://localhost:8000/examples/streaming.html
```

5. Open the browser's Developer Tools Console (F12) to see the output

### What's Demonstrated

- **Stream Large File**: Downloads a large file in chunks, logging each chunk's size
- **Stream Text Content**: Processes a JSONL file line-by-line as it streams
- **Download with Progress**: Shows download progress with percentage updates

All examples use the same large file from GitHub (a JSONL frequency list).

# fetch-happen

This is a library that provides a comfortable wrapper for the javascript "fetch" api.

On wasm it uses the browser's `fetch`; on native targets the same API is
backed by [reqwest](https://crates.io/crates/reqwest), so code written
against fetch-happen runs unchanged in both places.

Native caveats (the API is identical, the semantics are slightly simpler):

- Responses are fully buffered at `send()` time; `stream_reader()` yields
  the whole body as a single chunk followed by `None`.
- `mode(...)` and `abort_signal(...)` are accepted for parity and ignored —
  CORS is a browser concept, and a buffered request can't be aborted.
- `Response::stream()` is wasm-only: it returns a `web_sys::ReadableStream`,
  which has no native counterpart. Use `stream_reader()` in shared code.

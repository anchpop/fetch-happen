# fetch-happen

This is a library that provides a comfortable wrapper for the javascript "fetch" api.

On wasm it uses the browser's `fetch`; on native targets the same API is
backed by [reqwest](https://crates.io/crates/reqwest), so code written
against fetch-happen runs unchanged in both places.

Native caveats (the API is identical, the semantics are slightly simpler):

- Responses are fully buffered at `send()` time; `stream_reader()` yields
  the whole body as a single chunk followed by `None`.
- `mode(...)` is ignored natively because CORS is a browser concept.
- `abort_signal(...)` interrupts both pending headers and body reads.
- `Response::stream()` is wasm-only: it returns a `web_sys::ReadableStream`,
  which has no native counterpart. Use `stream_reader()` in shared code.

Cancellation uses the small `abort-signal` workspace crate, which has no HTTP
client dependency. It wraps a real browser signal on wasm and a Tokio
cancellation token natively. Existing `web_sys::AbortSignal` inputs still work
with `.abort_signal(signal)` on wasm. `AbortController::new()`, `.signal()`,
`.abort()`, and `AbortSignal::aborted()` work on both platforms; `.until(future)`
can cancel other asynchronous work. Dropping a controller does not abort it.
Native child controllers follow parent cancellation without propagating their
own cancellation back to the parent or siblings.

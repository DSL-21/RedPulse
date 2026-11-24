# RedPulse 0.1.1

A maintenance and usability update to the native Windows Rust desktop HTTP benchmarking application.

## Improvements

- Compact default window size: 1040 × 700 logical pixels.
- Updated dark UI palette, softer panels, rounded cards and a clearer status indicator.
- More balanced metric cards and improved dashboard hierarchy.
- Results now preserve and display the exact configuration used for the completed test.
- JSON exports include the target, method, worker count, duration, request-rate ceiling and timeout from that run.
- Network errors now include clearer categories and chained causes where available, helping distinguish request timeouts, connection failures, TLS issues and lower-level network errors.

## Validation

The release workflow checks Rust formatting, runs unit and local HTTP integration tests, runs Clippy with warnings treated as errors, and produces an optimized Windows x64 executable. The Windows build statically links the Microsoft C runtime.

## Existing capabilities

- HTTP GET and POST
- Configurable workers, duration, global request-rate ceiling and timeout
- Live throughput and latency dashboard
- Average, P50, P95 and P99
- HTTP status and network error classification
- JSON and CSV export
- Cancellation of in-flight requests

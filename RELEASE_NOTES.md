# RedPulse 0.1.0

First public release of RedPulse, a native Windows HTTP benchmarking desktop application written entirely in Rust.

## Included

- `RedPulse.exe` — Windows x64 GUI application.
- `RedPulse.exe.sha256` — SHA-256 checksum for verifying the executable.

## Capabilities

- HTTP GET and POST benchmark runs.
- Configurable asynchronous worker count.
- Global request-rate ceiling shared by all workers.
- Configurable test duration and request timeout.
- Live observed requests/second and response-time chart.
- Average, P50, P95 and P99 latency.
- HTTP status and network error classification.
- Per-request results.
- JSON and CSV report export.
- Immediate stop, including cancellation of in-flight requests.

## Build and validation

The release workflow checks Rust formatting, runs the unit and local HTTP integration tests, runs Clippy with warnings treated as errors, and builds the optimized Windows x64 executable.

The Windows x64 build statically links the Microsoft C runtime. Standard Windows system DLLs remain operating-system dependencies.

## Initial version limits

- 1–64 workers.
- 1–200 scheduled requests per second.
- 1–300 seconds per run.
- 1–60 second request timeout.
- 2 MiB measured response-body limit per request.
- Up to 20,000 retained request samples in the UI and exported reports.

See the repository README for build instructions and implementation details.

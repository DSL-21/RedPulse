# RedPulse 0.1.2

RedPulse 0.1.2 improves benchmark timing accuracy and the response-time analytics dashboard.

## Improvements

- The engine now emits its monotonic start instant and measured elapsed duration.
- Final observed throughput is calculated using the engine's measured elapsed duration rather than the last GUI repaint time.
- HTTP response latency statistics include actual HTTP responses (including 4xx/5xx) and exclude transport failures such as DNS, connection and timeout errors.
- Network failures remain visible in their own count and request details.
- Added minimum and maximum HTTP response latency alongside average, P50, P95 and P99.
- The response-time chart now labels elapsed time in seconds and response latency in milliseconds.
- P50 and P95 reference lines are shown on the chart.
- Added Reset View and enabled double-click chart reset.
- Empty charts display a waiting state instead of a default coordinate grid.
- JSON exports now contain the actual engine-measured elapsed duration.

## Validation

The release workflow checks Rust formatting, runs unit and local HTTP integration tests, runs Clippy with warnings treated as errors, and builds the optimized Windows x64 executable. The Windows x64 build statically links the Microsoft C runtime.

## Existing capabilities

- HTTP GET and POST
- Configurable workers, duration, global request-rate ceiling and timeout
- Live throughput and latency dashboard
- HTTP status and network error classification
- JSON and CSV export
- Cancellation of in-flight requests

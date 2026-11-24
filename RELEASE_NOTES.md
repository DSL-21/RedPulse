# RedPulse 0.1.3

A focused usability update that makes the response-time dashboard chart static and predictable.

## Changes

- Removed the Reset View button.
- Disabled plot zoom, wheel scaling, pointer dragging, boxed zoom and double-click reset.
- Removed the chart view revision state used only for manual resetting.
- The response-time chart retains its normal automatic data bounds while the benchmark samples update.
- Mouse-wheel scrolling over the chart no longer modifies its axes; the surrounding dashboard can scroll normally.

## Validation

The release workflow checks Rust formatting, runs unit and local HTTP integration tests, runs Clippy with warnings treated as errors, and builds the optimized Windows x64 executable. The Windows x64 build statically links the Microsoft C runtime.

## Existing capabilities

- HTTP GET and POST
- Configurable workers, duration, global request-rate ceiling and timeout
- Live throughput and latency metrics
- Minimum, average, P50, P95, P99 and maximum HTTP response latency
- HTTP status and network error classification
- JSON and CSV export
- Cancellation of in-flight requests

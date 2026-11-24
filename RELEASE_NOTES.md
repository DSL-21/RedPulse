# RedPulse 0.1.6

RedPulse 0.1.6 hardens request generation, bounds per-run resource use and improves latency chart fidelity.

## Global engine guardrails

- Both load models share a hard global ceiling of 200 request starts per second.
- Every run is limited to 10,000 scheduled request attempts.
- Constant Arrival Rate continues to use its bounded queue and records queue overflow or missed slots as dropped arrivals.
- Closed Loop uses a shared monotonic start pacer across all workers and cannot exceed the global request-start ceiling.
- When the request budget is exhausted, no further request is scheduled; already in-flight requests are allowed to finish or are accounted as cancelled when the run ends.

## Request and response validation

- POST bodies must parse as valid JSON.
- POST request bodies are limited to 256 KiB.
- Measured response bodies remain limited to 2 MiB per request.
- Existing timeout, worker, duration and URL validation remain in place.

## Full-run chart history

- Each sample now records elapsed completion time in microseconds, while retaining the millisecond field for compatibility.
- The chart keeps the full run timeline rather than only the latest 600 samples.
- The graph renders at most 2,000 points using min/max-preserving buckets, retaining latency spikes while limiting UI work.
- CSV exports include both microsecond and millisecond elapsed fields.
- JSON and final engine statistics identify when the per-run request budget was reached.

## Automated validation

- Request-budget reservation tests.
- Invalid and oversized POST JSON tests.
- Closed Loop shared-pacer test.
- Local HTTP timeout integration test.
- Full timeline and latency-spike downsampling tests.
- Existing GET/POST, slow-server, cancellation and scheduler tests.
- Rust formatting, Clippy with warnings denied and Windows x64 release build.

# RedPulse 0.5.0

RedPulse 0.5.0 advances the Windows HTTP performance engine with generator telemetry, HTTP/2 support, lower-overhead result delivery, and clearer measurement attribution.

## Hardware and generator diagnostics

- Samples average host CPU usage across logical cores and RedPulse process CPU usage once per second.
- Shows host used/total memory and RedPulse process memory.
- Displays network receive/transmit throughput summed across visible host interfaces. These are host-wide counters, not process-only traffic measurements.
- Warns when CPU telemetry indicates low generator headroom. This is a diagnostic heuristic, not a definitive saturation test.
- Retains up to 1,800 hardware samples (30 minutes at the one-second sampling interval) in the UI and JSON report.

## HTTP protocol and backend attribution

- Enables HTTP/2 support in Reqwest alongside HTTP/1.1.
- Records the negotiated HTTP protocol for each response and displays it in Results.
- Includes HTTP protocol, selected DNS origin IP, and optional application-provided backend identity in request reports.
- Preserves optional response-header backend attribution (for example, `X-Backend-ID`) and per-backend distribution.

HTTP/2 is negotiated when supported by the endpoint and TLS configuration. HTTP/3 is not enabled in this stable build. Backend identifiers are observed only when the tested application returns the configured response header; RedPulse does not infer hidden backend nodes.

## Engine performance and measurement quality

- Replaces the shared async-mutex worker receiver with a bounded multi-consumer queue.
- Batches up to 64 response samples and flushes at least every 50 ms, reducing UI event overhead without discarding retained samples.
- Publishes request counters every 250 ms rather than emitting a statistics event for each request.
- Captures the workload duration before telemetry helper shutdown, so monitor cleanup does not distort observed RPS.
- Keeps request samples and hardware history bounded to protect memory during long runs.
- Continues to distinguish scheduled, started, completed, queued, in-flight, cancelled, and dropped requests.

## Validation

The release workflow runs Rust formatting, the complete unit and local HTTP integration test suite, Clippy with warnings denied, and an optimized Windows x64 release build. The generated executable is packaged with a SHA-256 checksum.

Use RedPulse only against systems you own or are explicitly authorized to test. Configure a request budget and duration appropriate for the environment before starting a run.

# RedPulse 0.1.5

RedPulse 0.1.5 adds explicit load-model selection so users can compare fixed request arrivals with worker-paced traffic.

## Constant Arrival Rate

- Preserves the independent monotonic scheduler introduced in 0.1.4.
- Attempts request arrivals at the configured requests-per-second interval.
- Uses a bounded queue shared by HTTP workers.
- Counts missed or rejected arrivals as dropped; it does not generate catch-up bursts.
- Worker count limits concurrent in-flight requests.

## Closed Loop

- Each worker starts its next request only after its previous request completes.
- No arrival scheduler or waiting queue is used.
- Throughput emerges from worker count and response time.
- The arrival-rate slider is disabled because it does not apply to this mode.
- The final accounting still reports scheduled, started, completed, in-flight, cancelled and dropped requests.

## Reporting and UI

- The Tester page has a Load model selector.
- Results show the selected load model and distinguish an arrival-rate target from worker-paced throughput.
- JSON run metadata contains the load model and an optional request-rate target.

## Validation

The release workflow checks Rust formatting, runs unit and local HTTP integration tests for both load models, runs Clippy with warnings treated as errors, and builds the optimized Windows x64 executable with the Microsoft C runtime statically linked.

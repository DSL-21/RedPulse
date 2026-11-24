# RedPulse 0.2.1

RedPulse 0.2.1 makes the configurable HTTP traffic engine the primary product direction.

## Configurable traffic generation

- Removed fixed application-level ceilings of 64 workers, 200 requests/second, 300 seconds, and 10,000 requests per run.
- Worker count, duration, arrival rate, timeout, and total request budget are now numeric user-configurable settings.
- The total request budget defaults to 10,000 as a conservative starting value and can be increased or reduced by the operator.
- Tokio runtime worker threads automatically match the logical CPU parallelism reported by the operating system.
- The arrival queue is bounded and sized from the configured worker count.
- Closed Loop no longer applies a global 200-RPS start pacer; concurrency and response time determine its natural throughput.
- The old Recon module remains available as a secondary tab, while Traffic Lab is now the startup screen.

## Measurement and safeguards

- The UI keeps the most recent 10,000 samples and chart points while aggregate counters and latency histogram statistics cover the complete run.
- Per-request response measurement remains bounded to 2 MiB.
- POST JSON remains validated and bounded to 256 KiB.
- Redirect following remains disabled.
- The configured request budget and duration remain visible in run metadata and exported JSON.

## Validation

The release pipeline runs formatting, Rust unit tests, Clippy with warnings denied, and the optimized Windows x64 release build.

# RedPulse 0.4.0

RedPulse 0.4.0 completes the first backend-attribution milestone on top of the 0.3.0 configurable HTTP load engine and DNS-based Multi-IP routing.

## Backend attribution

- Configure an optional response header in Traffic Lab, such as `X-Backend-ID` or `X-Served-By`.
- RedPulse captures the header value on every HTTP response and aggregates request counts by backend identifier.
- The live Traffic Lab view and Results view display the observed backend distribution.
- JSON and CSV samples include the backend identifier alongside the selected origin IP.
- Leave the header field empty to disable backend attribution.

This feature reports only information explicitly returned by the target application. It does not claim to discover hidden backend nodes or infer load-balancer routing when no backend identifier is exposed.

## Engine and report quality

- Refactored request options into a dedicated request specification, keeping the engine interface maintainable and passing strict Clippy checks without lint suppression.
- CSV text fields are consistently quoted and escaped, including values containing commas, quotes, or line breaks.

## Validation

The release pipeline runs Rust formatting, the complete unit test suite, Clippy with warnings denied, and an optimized Windows x64 release build. The generated executable is packaged with a SHA-256 checksum.

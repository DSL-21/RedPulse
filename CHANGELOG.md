# Changelog

All notable RedPulse changes are documented here.

## [0.1.0] - 2026-11-24

### Added
- Native Rust Windows desktop application built with egui / eframe.
- HTTP GET and POST benchmarking through Reqwest and Tokio.
- Configurable workers, run duration, global request-rate ceiling and timeout.
- Live request, throughput, status and latency metrics.
- Average, P50, P95 and P99 latency statistics.
- Per-request result table.
- JSON and CSV report export.
- Immediate cancellation of queued and in-flight operations.
- Disabled automatic redirect following for clear endpoint-level measurements.
- Bounded response measurement and UI sample retention.
- Windows x64 CI and release workflows.
- Static Microsoft C runtime linking for the Windows x64 release executable.

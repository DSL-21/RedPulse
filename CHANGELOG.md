# Changelog

All notable RedPulse changes are documented here.

## [0.1.1] - 2026-11-24

### Added
- More compact default desktop window size (1040 × 700 logical pixels).
- Refined dark color palette, rounded metric cards, status pill and updated dashboard hierarchy.
- Detailed network error messages with timeout/connection classification and chained causes.
- Per-run configuration snapshot shown with results.
- Run profile metadata included in JSON exports.

### Fixed
- Results no longer display slider values changed after the completed run.
- JSON export now records the configuration that actually produced its samples.

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

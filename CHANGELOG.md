# Changelog

All notable RedPulse changes are documented here.

## [0.1.5] - 2026-11-24

### Added
- Explicit Constant Arrival Rate and Closed Loop load models.
- Closed Loop workers immediately schedule their next request after completing the previous request.
- Load-model selection and mode-specific guidance in the Tester UI.
- Load model and optional arrival-rate target in JSON run metadata.
- Closed Loop validation and local HTTP integration tests.

### Changed
- Arrival-rate control is disabled and omitted from run metadata in Closed Loop mode.
- Results identify the load model used for each benchmark.

## [0.1.4] - 2026-11-24

### Added
- Independent monotonic arrival scheduler that is decoupled from worker completion.
- Bounded asynchronous worker queue (up to twice the worker count, maximum 128 jobs).
- Explicit request accounting: scheduled, started, completed, queued, in-flight, cancelled and dropped.
- Separate HTTP 2xx, 3xx, 4xx and 5xx classification.
- Run accounting included in JSON exports and HTTP status class included in CSV exports.
- Tests for scheduler slot accounting, final cancellation accounting and HTTP status classification.

### Changed
- A full worker queue drops and records an arrival instead of delaying later schedule slots or creating catch-up bursts.
- HTTP 3xx responses are counted separately from successful 2xx responses.

## [0.1.3] - 2026-11-24

### Changed
- Made the response-time chart non-interactive.
- Disabled mouse-wheel zoom/scroll, pointer dragging, boxed zoom and double-click reset.
- Removed the Reset View button and its state; the chart keeps its normal automatic data bounds.
- Mouse-wheel input over the chart now remains available to the surrounding dashboard scroll area.

## [0.1.2] - 2026-11-24

### Added
- Engine-reported monotonic start time and measured elapsed duration.
- Final observed throughput calculated from the engine's measured duration.
- Minimum and maximum HTTP response latency metrics.
- P50 and P95 reference lines on the response-time chart.
- Explicit elapsed-time and response-latency axis labels.
- Reset View control and double-click chart reset.
- Actual measured duration included in JSON run metadata.
- Tests for RPS calculation and engine elapsed-time reporting.

### Fixed
- Final RPS no longer depends on the last GUI repaint interval.
- Transport errors and timeouts are excluded from HTTP response latency percentiles and reported separately.
- Empty response charts show a waiting state instead of an unlabelled empty plot.

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

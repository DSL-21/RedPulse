# Changelog

All notable RedPulse changes are documented here.

## [0.2.1] - 2026-11-24

### Changed
- Repositioned the primary workflow as Traffic Lab and HTTP performance testing.
- Removed the fixed 64-worker, 200-RPS, 300-second and 10,000-request engine ceilings.
- Added a user-configurable per-run request budget, with 10,000 as the conservative default.
- Replaced bounded UI sliders with numeric controls for workers, rate, duration and timeout.
- Tokio runtime worker threads now use the logical CPU parallelism reported by the operating system.
- Closed Loop no longer uses a fixed global request-start pacer; worker concurrency and response latency determine natural throughput.
- Arrival queue capacity now scales with the configured worker count.
- Kept a bounded recent-sample window for UI/export memory stability while aggregate statistics cover the full run.
- Made Traffic Lab the application startup tab; retained the earlier Recon tab as a secondary module.

## [0.2.0] - 2026-11-24

### Added
- New Web Recon tab and Red Team Workbench positioning.
- Bounded six-probe HTTP assessment: GET, HEAD, OPTIONS, security.txt, robots.txt and sitemap.xml.
- DNS address resolution and basic HTTP technology fingerprinting.
- Security posture checks for HSTS, CSP, framing, MIME sniffing, Referrer-Policy, Permissions-Policy, cookies and CORS.
- OPTIONS-based advertised HTTP method inspection without invoking advertised methods.
- Evidence-backed findings with severity and remediation guidance.
- Five-second probe timeout, sequential execution, fixed 250 ms spacing, no redirect following and early stop on HTTP 429/503.

### Changed
- The existing HTTP benchmark is now presented as the Load Test module.
- Application branding now describes a Red Team Workbench rather than an HTTP benchmark only.

## [0.1.6] - 2026-11-24

### Added
- Global 200 request-starts/second safety ceiling shared by Constant Arrival Rate and Closed Loop.
- Hard 10,000 scheduled-request budget per run for both load models.
- POST body validation: valid JSON required, maximum request body size 256 KiB.
- Microsecond elapsed-completion timestamps in each sample and CSV export.
- Full-run response chart history with min/max-preserving downsampling to at most 2,000 plotted points.
- Request-limit reached state included in final engine statistics and JSON reports.
- Tests for request-budget reservations, invalid/oversized POST bodies, global pacing, request timeouts and chart downsampling.

### Changed
- Closed Loop can no longer generate unbounded worker-paced request volume.
- Chart no longer drops the first part of a high-throughput run when more than 600 samples arrive.
- Detailed results and exports retain the full run within the 10,000-request budget.

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

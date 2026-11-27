# Changelog

All notable RedPulse changes are documented here.

## [0.6.2] - 2026-11-27

### Improved
- Reorganized Traffic Lab into clearer configuration, measurements, request accounting, telemetry, routing and latency sections.
- Improved dashboard hierarchy, spacing, metric grouping and field labels without adding new user-facing features.
- Locked all test configuration controls while a run is active to keep the displayed configuration consistent with the active engine run.
- Clarified that aggregate metrics cover the full run while the on-screen request sample window is retained separately.

### Fixed
- Auxiliary telemetry and statistics tasks now stop when the configured request budget is reached, instead of waiting for the full duration deadline.
- Kept request-budget completion distinct from explicit user cancellation.
- Added a regression test for early request-budget completion.
- Removed the one-minute latency histogram cap and added coverage for long-duration samples.

### Notes
- Memory bounds, bounded queues and request/response measurement safeguards remain intentional stability protections.

## [0.6.1] - 2026-11-27

### Added
- Volatile session history for completed Performance, Load Balancer, and Protection runs.
- One manual Session JSON export that bundles all retained completed runs, including HTTP request samples, hardware telemetry, protection probes and findings.
- Session metadata and HTTP latency summaries (RPS, min, average, P50, P95, P99, max) in the JSON payload.
- Session-history UI showing run count, approximate memory use, retained run entries, and any evicted older runs.

### Improved
- Session history remains in RAM only and is never automatically written to disk.
- Bounded retention to 50 runs or 64 MiB, removing the oldest entries first to prevent unlimited memory growth.
- Added an explicit Clear session action.
- CSV export now clearly targets the current HTTP run only; JSON export targets the full retained session.
- Removed chart hover X/Y coordinate labels and cursor crosshairs for a cleaner dashboard.

### Notes
- Closing RedPulse releases the volatile in-app history. A Session JSON file created by manual export remains on disk until the user deletes it.
- When session limits are reached, older entries are evicted and the UI reports the count.


## [0.6.0] - 2026-11-25

### Added
- Three guided Traffic Lab profiles: Performance, Load Balancer, and Protection.
- Load Balancer profile defaults to DNS round-robin and reports per-destination-IP request share, average latency, and HTTP/network errors.
- Per-backend request share, average latency, and error metrics when the configured backend identity response header is returned.
- Protection profile counters for observed HTTP 401, 403, 429, and 503 responses.
- Passive edge-provider signature findings for Cloudflare, Fastly, Akamai, Imperva, Sucuri, and AWS WAF response markers.
- Tests for edge-provider signal interpretation, rate-limit observations, profile descriptions, and route distribution metrics.

### Improved
- Advanced traffic controls are collapsed by default, leaving target, guided profile, workers, duration, arrival rate, and request budget in the main view.
- Renamed Recon navigation and screen to Protection / Web Protection Audit, with explicit read-only scope.
- Clarified that DNS round-robin selects destination IPs, does not rotate source IPs, and does not inherently reveal hidden backends.
- Protection and edge-service findings are informational observations, not claims that a WAF is enabled or bypassed.
- Per-route errors now include transport failures as well as HTTP 4xx/5xx responses.


## [0.5.3] - 2026-11-25

### Improved
- Replaced the two-line CPU chart with one RedPulse-only utilization line; whole-host CPU remains visible as a summary metric.
- Replaced the three-line response chart with one line for individual HTTP responses; P50 and P95 remain as compact numeric summaries above the graph.
- Changed the RedPulse CPU metric from percent of one core to percent of total logical-core capacity, matching the CPU chart.
- Increased hardware telemetry sampling from once per second to twice per second and increased retained hardware history to 3,600 samples (30 minutes).
- Increased the active dashboard repaint cadence from 100 ms to 50 ms for more responsive live updates.
- Restyled Recon's red action button as a standard blue action and renamed it "Start HTTP audit".
- Clarified that Recon sends six read-only HTTP requests to inspect headers, cookies, public metadata and advertised methods; it does not run exploit attempts.


## [0.5.2] - 2026-11-25

### Improved
- Added visible color legends and live readings for both CPU chart lines.
- Added visible labels and current values for individual response times, P50 median, and P95 percentile.
- Explained P50 and P95 in plain language directly in Traffic Lab.
- Clarified that percentile lines summarize HTTP responses observed during the current run; they are not configured thresholds, and transport errors/timeouts are excluded.
- Applied explicit CPU chart line colors so their meanings stay consistent with the labels.


## [0.5.1] - 2026-11-25

### Improved
- Replaced drag-based numeric controls in Traffic Lab with directly editable text fields for workers, duration, arrival rate, timeout, and request budget.
- Preserved the existing valid defaults: 5 workers, 30 seconds, 5 requests/second, 10-second timeout, and 10,000 requests.
- Added explicit numeric parsing and clear errors for empty or invalid values; no artificial slider ceiling is imposed by the UI.
- Kept runtime and platform validation in place so values that cannot be safely represented or queued are rejected before a run starts.


## [0.5.0] - 2026-11-24

### Added
- Live once-per-second hardware telemetry for average host CPU, RedPulse process CPU, host RAM, process memory, and host-wide network RX/TX throughput.
- CPU headroom diagnostics that warn when the machine or RedPulse process may be limiting the generated load.
- HTTP/2 support through Reqwest and negotiated HTTP protocol attribution for each response.
- Negotiated HTTP version in the Results table, JSON samples, and CSV reports.
- Hardware sample history in JSON reports, bounded to the most recent 1,800 samples.

### Improved
- Replaced the shared async-mutex worker receiver with a bounded multi-consumer request queue.
- Batched up to 64 response samples, with a 50 ms flush interval, before forwarding results to the GUI.
- Published live request counters every 250 ms instead of emitting a statistics event for every arrival and response.
- Measured run duration before auxiliary telemetry shutdown so hardware-monitor cleanup does not inflate reported RPS.
- Kept request samples and hardware history bounded to protect memory during long test runs.

### Notes
- Host network throughput is summed from visible host interfaces and is not attributed exclusively to RedPulse.
- HTTP/3 is not enabled in this stable build.
- CPU headroom is a diagnostic heuristic; controlled cross-tool benchmarks are still required before making performance superiority claims.

## [0.4.0] - 2026-11-24

### Added
- Optional response-header backend identity capture, configurable in Traffic Lab (for example `X-Backend-ID`).
- Live and historical backend request distribution based on identifiers explicitly returned by the tested application.
- Backend identity retained per sample and included in JSON and CSV reports.

### Fixed
- Grouped HTTP request settings into a request specification so strict Clippy checks pass without suppressing lints.
- CSV text fields, including origin IP, backend identifier, and error details, are consistently quoted and escaped.

## [0.3.0] - 2026-11-24

### Added
- DNS routing selector: normal system DNS or round-robin across resolved A/AAAA addresses.
- Per-request origin IP attribution in live results and CSV/JSON reports.
- Pinned-origin HTTP clients that connect to a selected IP while keeping the original URL hostname for HTTP Host and TLS SNI.
- Per-IP request distribution counters in Traffic Lab.
- Tests for DNS strategy defaults, literal IP resolution, round-robin selection, and pinned client construction.

### Changed
- Traffic Lab now exposes DNS origin routing alongside the existing load model and worker controls.
- DNS-pinned mode bypasses configured system proxies so the client connects directly to the selected address.

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

# RedPulse

**RedPulse** is a native Windows HTTP benchmarking desktop application written in Rust.

It provides a compact GUI for controlled HTTP GET/POST benchmark runs, live throughput and latency metrics, request-level results, and JSON/CSV exports.

[![CI](https://github.com/DSL-21/RedPulse/actions/workflows/ci.yml/badge.svg)](https://github.com/DSL-21/RedPulse/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/DSL-21/RedPulse)](https://github.com/DSL-21/RedPulse/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Download

Open [GitHub Releases](https://github.com/DSL-21/RedPulse/releases) and download `RedPulse.exe` from the latest release.

The release includes a SHA-256 checksum file named `RedPulse.exe.sha256`.

## Features

- Native Windows desktop interface using egui / eframe
- HTTP GET and POST requests
- Configurable load model, worker count, run duration, arrival rate and timeout
- Constant Arrival Rate mode for fixed-time request arrivals
- Closed Loop mode for worker-paced requests
- Independent monotonic arrival scheduler with a bounded worker queue
- Live request count, observed RPS and a fixed, non-interactive response-time chart
- Min, average, P50, P95, P99 and max HTTP response latency
- P50 and P95 reference lines on the response-time chart
- Separate HTTP 2xx, 3xx, 4xx and 5xx counts plus network errors
- Scheduled, started, completed, queued, in-flight, cancelled and dropped request accounting
- Per-request result table
- JSON and CSV report export
- Immediate cancellation, including in-flight requests
- Redirect following disabled
- Bounded response measurement and retained UI samples
- Rustls TLS backend

## Quick start

1. Download `RedPulse.exe` from [Releases](https://github.com/DSL-21/RedPulse/releases).
2. Run the executable.
3. Enter an HTTP or HTTPS endpoint.
4. Select GET or POST.
5. Configure workers, duration, request rate and timeout.
6. Start the test and inspect the live metrics.
7. Use the Results tab to export JSON or CSV.

The initial defaults are 5 workers, 30 seconds, 5 requests per second and a 10-second timeout.

## Load models

RedPulse provides two distinct request-generation models.

### Constant Arrival Rate

A monotonic scheduler attempts to create one request arrival at each configured time slot, independently of HTTP response completion. Accepted arrivals enter a bounded queue shared by the workers. The rate ceiling is global rather than multiplied by worker count. A full queue or a missed scheduler slot is recorded as a dropped arrival; RedPulse does not create catch-up bursts. Worker count controls how many requests may be in flight concurrently, while the arrival-rate setting controls the planned pace.

### Closed Loop

Each worker starts its next request only after its previous request has completed. There is no arrival timer and no waiting queue in this mode. Throughput emerges from worker count and observed response time, so slower responses naturally reduce the number of new requests started. The arrival-rate slider is disabled because it does not apply to this model.

Both modes use the same request timeout, response measurement limit, cancellation handling, status classification and latency analytics. Final observed RPS uses the engine's monotonic elapsed duration rather than GUI refresh timing.

For each request, RedPulse records elapsed time, response latency, status code, measured response size, and any transport or body-read error. Run accounting distinguishes scheduled arrivals, started attempts, completed attempts, queued jobs, in-flight requests, cancelled jobs and dropped arrivals. HTTP response latency statistics include HTTP responses (including 4xx and 5xx) and exclude transport failures such as DNS, connection and timeout errors; those failures are counted separately. Redirects are not followed, so a 3xx response is measured directly rather than silently sending another request. The chart plots the most recent HTTP response samples against elapsed time and includes P50/P95 reference lines. It is intentionally non-interactive: zoom, pan, wheel scaling and double-click reset are disabled so the mouse wheel remains available for scrolling the dashboard.

## Initial release limits

The first release uses explicit upper bounds:

| Setting | Range |
|---|---:|
| Workers | 1–64 |
| Request rate | 1–200 requests/second |
| Duration | 1–300 seconds |
| Timeout | 1–60 seconds |
| Measured response body | Up to 2 MiB per request |
| Retained request samples | Up to 20,000 |

The dashboard totals and latency histogram cover all samples received during a run. The detailed request table and exported report retain at most the most recent 20,000 samples. The chart displays the most recent 600 latency points.

## Build from source

Requirements:
- Stable Rust toolchain
- Windows 10/11 x64
- Visual Studio Build Tools with the MSVC C++ toolchain for native Windows builds

Build:

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

Run:

```powershell
cargo run --release
```

The Windows executable is written to:

```text
target/x86_64-pc-windows-msvc/release/redpulse.exe
```

The repository includes a Cargo configuration that statically links the Microsoft C runtime for the Windows x64 target. Windows system DLLs remain normal operating-system dependencies.

## Reports

- **JSON** contains application metadata, target, method, worker count, generation timestamp, and retained request samples.
- **CSV** contains one row per retained request sample.

Reports are written to the application's current working directory using the names `redpulse-report.json` and `redpulse-report.csv`.

## Project status

RedPulse 0.1.5 is the sixth public release. The desktop window opens at a compact 1040 × 700 logical-pixel size by default. Results keep the configuration snapshot from the run, including the engine-measured elapsed duration. Final observed RPS is calculated from the engine's monotonic clock, not GUI frame timing. HTTP response latency metrics exclude transport failures and timeouts, which remain visible in the separate network-error count. The response chart shows elapsed time and latency axes, P50/P95 reference lines, and fixed chart bounds with mouse and pointer interaction disabled. It focuses on a small, understandable HTTP benchmarking workflow. Scenario collections, custom headers, cookies, authentication profiles, ramp-up profiles, and additional HTTP methods are planned for later versions.

## License

MIT. See [LICENSE](LICENSE).

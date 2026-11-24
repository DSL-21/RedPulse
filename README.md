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
- Configurable worker count, run duration, global request-rate ceiling and timeout
- One shared request scheduler across all workers
- Live request count, observed RPS and response-time chart
- Average, P50, P95 and P99 response latency
- HTTP status classes and network error counts
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

## How the engine works

RedPulse starts a Tokio runtime on a dedicated engine thread. A configured number of asynchronous workers share one scheduler. Each worker reserves a time slot from that shared scheduler, so the configured request-rate ceiling is global rather than multiplied by the number of workers.

Worker count controls how many requests may be in flight concurrently. The request-rate setting controls the maximum pace at which new requests are scheduled. Actual observed throughput can be lower than the configured ceiling when the server, network, or client cannot complete requests quickly enough.

For each request, RedPulse records elapsed time, response latency, status code, measured response size, and any transport or body-read error. Redirects are not followed, so a 3xx response is measured directly rather than silently sending another request.

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

RedPulse 0.1.0 is the first public release. It focuses on a small, understandable HTTP benchmarking workflow. Scenario collections, custom headers, cookies, authentication profiles, ramp-up profiles, and additional HTTP methods are not part of this initial version.

## License

MIT. See [LICENSE](LICENSE).

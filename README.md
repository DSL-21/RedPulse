# RedPulse

**RedPulse** is a native Windows HTTP load-testing and infrastructure-resilience workbench written in Rust. It is a portfolio project focused on a high-performance traffic engine, accurate measurements, and practical testing of HTTP services and load-balanced deployments.

[![CI](https://github.com/DSL-21/RedPulse/actions/workflows/ci.yml/badge.svg)](https://github.com/DSL-21/RedPulse/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/DSL-21/RedPulse)](https://github.com/DSL-21/RedPulse/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Download

Open [GitHub Releases](https://github.com/DSL-21/RedPulse/releases) and download `RedPulse.exe`. Releases include a SHA-256 checksum.

## Current capabilities

- Native Windows desktop UI built with egui / eframe.
- Rust async HTTP engine built on Tokio and Reqwest with Rustls.
- Constant Arrival Rate and Closed Loop traffic-generation models.
- User-configurable worker count, arrival rate, run duration, timeout, and request budget.
- No fixed application-level 64-worker or 200-RPS ceiling.
- Tokio runtime worker threads automatically match the logical CPU parallelism reported by the operating system.
- Bounded per-run queue sized from the configured worker count.
- Global request-start pacing removed from Closed Loop; concurrency and response time determine its natural throughput.
- Live RPS, HTTP status counts, errors, and latency percentiles.
- Min, average, P50, P95, P99 and max response latency.
- Request accounting: scheduled, started, completed, queued, in-flight, cancelled and dropped.
- JSON and CSV export of retained samples.
- Immediate cancellation and redirect following disabled.
- Optional Web Recon tab retained from the earlier prototype.

The current release concentrates on the configurable HTTP traffic engine. Dedicated multi-origin and load-balancer distribution/failover analysis is the next development milestone; the existing Recon tab is not the main product direction.

## Quick start

1. Download `RedPulse.exe` from [Releases](https://github.com/DSL-21/RedPulse/releases).
2. Run the executable and open **Traffic Lab**.
3. Enter the HTTP or HTTPS URL of a test endpoint.
4. Choose GET or POST and select a load model.
5. Configure workers, duration, arrival rate, timeout, and total request budget.
6. Start the run, inspect live measurements, and export the report.

Defaults are intentionally modest: 5 workers, 30 seconds, 5 requests/second, a 10-second timeout, and a 10,000-request budget. These are starting values, not fixed worker/RPS/request-budget ceilings. The controls accept larger values; actual throughput remains subject to the machine, operating system, network, and target service.

## Load models

### Constant Arrival Rate

A monotonic scheduler creates request arrivals at the configured rate independently of response completion. Arrivals enter a bounded queue sized from the configured worker count. When the queue is full or a scheduler slot is missed, the arrival is accounted as dropped rather than producing a catch-up burst. Worker count controls concurrency; the arrival-rate setting controls the planned request pace.

The rate interval uses nanosecond duration granularity. Values beyond the clock's effective resolution cannot produce a faster distinct schedule.

### Closed Loop

Each worker starts its next request after its previous request completes. There is no arrival timer and no global 200-RPS start pacer. Throughput emerges from configured concurrency and observed response time: slower responses naturally reduce request starts.

### Configurable run budget

The total request budget is user-configurable. It starts at 10,000 requests but can be increased or reduced in the Traffic Lab UI. The engine stops scheduling when the configured budget is reached. Duration and timeout are also user-controlled; values that exceed the platform monotonic clock's representable range are rejected at run start.

The queue remains bounded to prevent unbounded backlog growth. A bounded queue does not impose a fixed global RPS ceiling; it makes overload visible through the dropped-arrival counter.

## Hardware and performance notes

The Tokio runtime uses the logical CPU parallelism reported by the operating system instead of clamping runtime threads to eight. Worker tasks are asynchronous and are not one operating-system thread each. Increasing workers can raise concurrent network activity, but it does not guarantee higher throughput: connection limits, target capacity, local socket limits, CPU, memory, and available bandwidth all matter.

A useful benchmark must identify whether the generator, network path, or target is the bottleneck. RedPulse's next performance milestone is to add local generator telemetry and reproducible side-by-side benchmarks against established tools on identical hardware.

## Measurement and retention

For each completed request, RedPulse records elapsed time, response latency, HTTP status, measured response bytes, and transport/body-read errors. HTTP latency percentiles include HTTP responses (including 4xx and 5xx) and exclude transport failures. Redirects are not followed, so the configured endpoint response is measured directly.

The interface retains the most recent 10,000 request samples and the most recent 10,000 chart points for display and export; aggregate counters and latency histogram statistics continue to cover the full run. The response body measurement limit is 2 MiB per request. POST JSON bodies are validated and limited to 256 KiB. These are per-sample memory/measurement safeguards, not global request-rate or worker ceilings.

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

## Reports

- **JSON** contains application metadata, target, method, configured worker count, traffic model, configured request budget, generation timestamp, final counters, and retained samples.
- **CSV** contains one row per retained request sample.

Reports are written to the application's current working directory using `redpulse-report.json` and `redpulse-report.csv`.

## Project status

RedPulse 0.2.1 removes the old fixed worker, RPS, duration, and global Closed Loop pacing ceilings. The default profile remains conservative, while operators can choose their own run settings. This version is the foundation for the next stages: multi-origin testing, load-balancer distribution and failover analysis, hardware telemetry, traffic profiles, and controlled benchmark comparisons.

Use the tool only against systems you own or are explicitly authorized to test. Set a request budget and duration appropriate for the environment before starting a run.

## License

MIT. See `LICENSE`.

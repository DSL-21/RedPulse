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
- HTTP/1.1 and HTTP/2 support, with the negotiated protocol recorded per response.
- Constant Arrival Rate and Closed Loop traffic-generation models.
- User-configurable worker count, arrival rate, run duration, timeout, and request budget.
- Directly editable numeric text fields for workers, arrival rate, duration, timeout, and request budget; no slider-imposed GUI ceiling.
- No fixed application-level 64-worker or 200-RPS ceiling.
- Tokio runtime worker threads automatically match the logical CPU parallelism reported by the operating system.
- Bounded per-run queue sized from the configured worker count.
- Global request-start pacing removed from Closed Loop; concurrency and response time determine its natural throughput.
- Live RPS, HTTP status counts, errors, and latency percentiles.
- Simple, single-line CPU and response-time charts. The graphs show RedPulse CPU usage and individual HTTP response times; P50/P95 stay in a compact text summary.
- Min, average, P50, P95, P99 and max response latency.
- Request accounting: scheduled, started, completed, queued, in-flight, cancelled and dropped.
- JSON and CSV export of retained samples.
- Immediate cancellation and redirect following disabled.
- System DNS mode or round-robin routing across DNS-resolved IPv4/IPv6 addresses.
- Direct pinned-IP connections that preserve the original URL hostname for HTTP Host and TLS SNI.
- Per-request origin IP attribution and live per-IP request distribution.
- Optional response-header backend attribution (for example `X-Backend-ID`) with live backend distribution.
- Origin IP, negotiated HTTP protocol, and detected backend identity included in sample tables and CSV/JSON reports.
- Live host and RedPulse-process CPU/RAM telemetry, plus host-wide network receive/transmit throughput.
- Generator-headroom warning when CPU measurements indicate that the test machine may be limiting the run.
- Batched request-sample events and periodic counter updates to reduce GUI/event overhead at high response rates.
- Optional Web Recon tab retained from the earlier prototype.

The current release combines the configurable HTTP traffic engine, DNS origin routing, response-header backend attribution, negotiated HTTP protocol reporting, and local generator telemetry. Backend attribution depends on the target application exposing a stable response header; it cannot reveal private backend nodes that the application does not identify. The existing Recon tab is not the main product direction.

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

### DNS origin routing

**System DNS** uses the normal system resolver and shared client behavior.

**Round-robin DNS IPs** resolves the hostname at the start of the run, collects unique IPv4 and IPv6 addresses, and creates a pinned client for each address. Successive request starts are assigned cyclically to those clients. The URL itself is not rewritten, so the hostname remains the source for HTTP Host and HTTPS TLS SNI. Pinned mode bypasses configured system proxy settings so the selected IP is reached directly.

The live Traffic Lab view counts requests per origin IP. Each retained sample records its selected IP, and the IP is included in result tables and report exports. If the DNS name returns only one address, this mode naturally uses that one address.

This feature tests the IP addresses published for a hostname. A public load balancer often hides private backend addresses; DNS rotation therefore does not prove how the balancer distributes traffic among those hidden backends. For that reason RedPulse can optionally inspect a response header such as `X-Backend-ID` or `X-Served-By`. Configure the header in Traffic Lab to count responses by the identifier returned by the application. This is observed backend attribution, not inference: responses without that header remain unattributed.

### Configurable run budget

The total request budget is user-configurable. It starts at 10,000 requests but can be increased or reduced in the Traffic Lab UI. The engine stops scheduling when the configured budget is reached. Duration and timeout are also user-controlled; values that exceed the platform monotonic clock's representable range are rejected at run start.

The queue remains bounded to prevent unbounded backlog growth. A bounded queue does not impose a fixed global RPS ceiling; it makes overload visible through the dropped-arrival counter.

## Hardware and performance notes

The Tokio runtime uses the logical CPU parallelism reported by the operating system instead of clamping runtime threads to eight. Worker tasks are asynchronous and are not one operating-system thread each. Increasing workers can raise concurrent network activity, but it does not guarantee higher throughput: connection limits, target capacity, local socket limits, CPU, memory, and available bandwidth all matter.

RedPulse samples hardware telemetry twice per second (every 500 ms) while a run is active. It records average host CPU usage across logical cores, RedPulse process CPU usage, host memory usage, RedPulse process memory, and host-wide network RX/TX throughput. Network counters are summed across the host's visible interfaces; they are not process-only measurements. Process CPU can exceed 100% because one fully utilized logical core is represented as 100%; the UI also normalizes process CPU against the total logical-core capacity. The CPU chart shows this normalized RedPulse usage as one line; whole-host CPU remains a summary value above the chart.

The CPU headroom indicator is a diagnostic heuristic, not a proof that the generator is saturated. Treat a run with very high host or process CPU as potentially generator-limited. The runtime and worker tasks are asynchronous; adding workers does not automatically mean more CPU throughput.

Request samples are passed to the UI in batches of up to 64, or after 50 ms, whichever comes first. Live request counters are published every 250 ms instead of generating a separate statistics event for every request. This reduces UI/event overhead while preserving each retained response sample. The arrival queue is bounded and uses a multi-consumer receiver without a shared async mutex.

Reqwest's HTTP/2 support is enabled. The protocol shown for each response is the protocol actually negotiated with the endpoint; HTTP/2 requires the endpoint and TLS negotiation to support it. HTTP/3 is not enabled in this stable build.

A useful benchmark should identify whether the generator, network path, or target is the bottleneck. RedPulse now records local generator telemetry to help distinguish these cases; reproducible side-by-side comparisons against established tools on identical hardware remain a future benchmark milestone.

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

- **JSON** contains application metadata, target, method, configured worker count, traffic model, configured request budget, generation timestamp, final counters, retained request samples (including origin IP, negotiated HTTP protocol, and optional backend identity), and retained hardware samples.
- **CSV** contains one row per retained request sample, including selected origin IP, negotiated HTTP protocol, optional backend identity, status class, and correctly quoted text fields. Hardware history is currently included in JSON, not the request-row CSV.

Reports are written to the application's current working directory using `redpulse-report.json` and `redpulse-report.csv`.

## Project status

RedPulse 0.5.0 adds live generator/host hardware telemetry, generator-headroom diagnostics, batched result delivery, periodic counters, HTTP/2 support, and per-response negotiated protocol attribution. It retains DNS-based multi-IP routing and optional backend identity distribution. Backend attribution is observational: a backend is identified only when the application returns the configured header. Controlled traffic profiles, explicit failover experiments, and reproducible side-by-side benchmark comparisons remain future work.

Use the tool only against systems you own or are explicitly authorized to test. Set a request budget and duration appropriate for the environment before starting a run.

## License

MIT. See `LICENSE`.

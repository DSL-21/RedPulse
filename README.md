# RedPulse

Native Windows desktop HTTP benchmarking workbench written in Rust.

## Features

- Native GUI using egui / eframe
- HTTP GET and POST requests
- Configurable workers, duration, global request-rate ceiling and timeout
- Live throughput and latency chart
- Average, P50, P95 and P99 latency
- HTTP status classes and transport errors
- JSON and CSV report export
- Immediate cancellation
- Redirect following disabled

## Build

Requirements: stable Rust and Windows MSVC build tools.

```powershell
cargo build --release
cargo run --release
```

Executable: `target/release/redpulse.exe`.

## Engine overview

RedPulse starts Tokio on a dedicated engine thread. All asynchronous workers share one request scheduler, so the configured global request-rate ceiling is shared between workers instead of being multiplied by worker count.

Each request records elapsed time, latency, response status, response size and transport/body errors. Redirects are not followed. The initial release uses explicit upper bounds for workers, rate, duration, timeout and response sample size.

## License

MIT.

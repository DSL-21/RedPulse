# RedPulse 0.5.3

RedPulse 0.5.3 simplifies Traffic Lab and makes the live dashboard easier to read while improving telemetry responsiveness.

## Simpler dashboard

- CPU chart now has one line: RedPulse process CPU use as a percentage of total logical-core capacity.
- Whole-host CPU remains visible in the HOST CPU summary card rather than as a second confusing graph line.
- Response-time chart now shows only individual HTTP response latencies.
- P50 (median) and P95 remain visible as plain numeric summaries above the response graph; they are no longer drawn as extra horizontal lines.
- The RedPulse CPU metric now uses the same total-PC-capacity percentage as its graph.

## More responsive telemetry

- Host and process CPU, memory and network telemetry are sampled every 500 ms (twice per second).
- Hardware history retains up to 3,600 samples, preserving 30 minutes at the new sample rate.
- The dashboard requests a repaint every 50 ms while a test is running.

## Clearer Recon action

- Replaces the red Recon button with a standard blue **Start HTTP audit** action.
- Explains that the audit makes six read-only HTTP requests to inspect headers, cookies, public metadata and allowed methods.
- Explicitly states that the Recon module does not run exploit attempts.

Use RedPulse only against systems you own or are explicitly authorized to test. Set a request budget and duration appropriate for the environment before starting a run.

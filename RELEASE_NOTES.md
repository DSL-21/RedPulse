# RedPulse 0.5.2

RedPulse 0.5.2 makes the live Traffic Lab dashboard easier to understand by labeling the chart lines directly and explaining the statistics they represent.

## Clear chart legends

- CPU chart: red identifies RedPulse process CPU usage normalized against all logical cores; blue identifies whole-host CPU usage.
- Response-time chart: blue identifies individual response latencies; green identifies P50 (median); orange identifies P95 (95th percentile).
- Shows the current P50 and P95 values in milliseconds next to their labels.

## Understanding percentiles

- P50 is the median: 50% of measured HTTP responses completed in this time or less.
- P95 is the 95th percentile: 95% of measured HTTP responses completed in this time or less; the slowest 5% took longer.
- Percentile lines summarize all HTTP responses measured so far in the current run. They are statistics, not configured performance limits.
- Network errors and timeouts are excluded from HTTP response latency percentiles.

## Existing engine behavior

All HTTP engine features from 0.5.1 remain: editable numeric configuration, Constant Arrival Rate and Closed Loop models, HTTP/1.1 and HTTP/2, optional DNS round-robin routing, backend response-header attribution, live metrics, hardware telemetry, and JSON/CSV reports.

Use RedPulse only against systems you own or are explicitly authorized to test. Set a request budget and duration appropriate for the environment before starting a run.

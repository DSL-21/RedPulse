# RedPulse 0.6.6

RedPulse 0.6.6 is a focused Traffic Lab usability and rendering-performance update.

## Target endpoint and dashboard
- Reduced the Target Endpoint field maximum width to 320 px, keeping it proportionate to the other configuration fields.
- Moved the response-latency chart directly below generator telemetry and above destination-IP/backend breakdowns so the two primary charts appear together.
- Kept the existing hidden X/Y axes and hidden cursor coordinate overlay.

## Chart rendering performance
- Prepared latency and CPU plot series into cached data instead of recalculating them on every UI repaint.
- Refresh the response-latency cache at most 10 times per second while a run is active.
- Refresh CPU plot data only when new hardware telemetry arrives.
- Reuse scratch storage for rolling-median calculation, avoid sorting already chronological response samples, and select P95 without sorting the full latency set.

## Compatibility
- No changes to HTTP request generation, load scheduling, test profiles, raw measurements, routing behavior or report exports.
- Raw response samples, percentiles and exported data keep their existing semantics.

Use RedPulse only against systems you own or are explicitly authorized to test.

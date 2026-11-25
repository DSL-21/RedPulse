# RedPulse 0.5.1

RedPulse 0.5.1 improves the Traffic Lab configuration experience while preserving the existing HTTP engine and measurement behavior.

## Direct numeric configuration

- Replaces drag-based controls with ordinary editable text fields for Workers, Duration, Arrival rate, Timeout, and Request budget.
- Keeps Method, Load model, and DNS routing as dropdown selections.
- Preserves the existing valid defaults: 5 workers, 30 seconds, 5 requests/second, 10-second timeout, and a 10,000-request budget.
- Parses and validates values when a run starts and reports invalid whole-number input instead of silently reusing an old value.
- Removes slider-imposed GUI ceilings. Actual accepted values remain subject to integer representation, operating-system/runtime constraints, bounded queue capacity, and available hardware resources.

## Existing engine behavior

All HTTP engine features from 0.5.0 remain: Constant Arrival Rate and Closed Loop models, HTTP/1.1 and HTTP/2, optional DNS round-robin routing, backend response-header attribution, live latency/status metrics, hardware telemetry, and JSON/CSV reports.

Use RedPulse only against systems you own or are explicitly authorized to test. Set a request budget and duration appropriate for the environment before starting a run.

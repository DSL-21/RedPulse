# RedPulse 0.6.2

RedPulse 0.6.2 focuses on a clearer Traffic Lab interface and more reliable test completion.

## User interface
- Reorganized the Traffic Lab into distinct configuration, live measurement, request accounting, telemetry, routing and latency sections.
- Improved visual hierarchy, spacing, labels and metric grouping for easier scanning.
- Made all test configuration controls read-only while a run is active, so the displayed setup cannot drift away from the configuration already running in the engine.
- Clarified sample-retention behavior and the meaning of destination-IP and backend measurements.

## Engine reliability
- Stop auxiliary statistics and hardware-monitor tasks as soon as the workload has finished, including when the configured request budget is reached before the duration deadline.
- Preserve the distinction between a run that completed its configured budget and a run explicitly stopped by the user.
- Add a regression test for early request-budget completion.
- Keep bounded queues, response-body measurement limits and volatile session retention as stability safeguards.

Use RedPulse only against systems you own or are explicitly authorized to test.

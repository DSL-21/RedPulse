# RedPulse 0.6.4

RedPulse 0.6.4 corrects the scope of the previous dashboard update: the established application layout is restored, and the changes focus only on the line charts.

## Line charts
- Restored the Traffic Lab and workspace layout from v0.6.2; no broad GUI rearrangement.
- The latency chart overlays individual response samples with a 7-response rolling median to make the underlying trend easier to read.
- The latency chart uses a robust recent-P95-focused visual scale. Samples above that visual range are capped only in the chart, with the clipped count and actual peak displayed. Raw latency statistics, request samples and exports are unchanged.
- The process CPU chart overlays instantaneous utilization with a 7-sample rolling median and shows elapsed-time and CPU axes.

## Compatibility
- No changes to HTTP request generation, load scheduling, test profiles, measurements, routing or reporting behavior.
- Existing bounded queues, sample retention and memory safeguards remain unchanged.

Use RedPulse only against systems you own or are explicitly authorized to test.

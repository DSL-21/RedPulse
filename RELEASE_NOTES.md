# RedPulse 0.1.4

RedPulse 0.1.4 improves request scheduling accuracy and makes every stage of a benchmark run visible.

## Independent arrival scheduler

- A monotonic scheduler creates request arrivals independently of HTTP response completion.
- Worker count now limits concurrent in-flight requests; it no longer determines when the next arrival can be scheduled.
- Arrivals enter a bounded queue sized at twice the worker count, capped at 128 jobs.
- The scheduler never generates catch-up bursts. Slots missed because the scheduler wakes late are counted as dropped.
- Arrivals rejected because the queue is full are also counted as dropped.

## Request accounting

The dashboard and JSON report distinguish:

- Scheduled arrivals
- Started HTTP attempts
- Completed attempts
- Queued jobs
- In-flight requests
- Cancelled jobs (including queued and in-flight jobs not completed when the run ends)
- Dropped arrivals

## HTTP status classification

HTTP responses are now grouped separately as 2xx, 3xx, 4xx and 5xx. Network/transport failures remain a separate category. HTTP 3xx is no longer included in the successful 2xx count.

CSV exports include a status-class column. JSON exports include the final run-accounting snapshot.

## Validation

The release workflow checks formatting, runs unit and local HTTP integration tests, runs Clippy with warnings treated as errors, and builds the optimized Windows x64 executable with the Microsoft C runtime statically linked.

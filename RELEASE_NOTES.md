# RedPulse 0.6.1

RedPulse 0.6.1 adds a bounded, RAM-only session history across all test profiles and a single export containing the full retained session.

## Session history

- Every completed Performance and Load Balancer HTTP run is retained in memory with configuration, final counters, latency summary, request samples and hardware samples.
- Every completed Protection Audit is retained with DNS results, probe outcomes and findings, including failed audit attempts.
- The Results page shows previous runs, current session memory use, and the number of entries removed at the retention limit.
- **Export session JSON** writes all retained runs into one structured JSON document.
- **Export current CSV** writes request rows from the latest HTTP run only.
- **Clear session** removes the in-memory history immediately.

## Memory and privacy behavior

- No automatic log, history file or local database is created.
- Session history is held only in process RAM and disappears when RedPulse exits.
- Retention is bounded to 50 runs or 64 MiB of serialized run data, whichever comes first. Oldest entries are evicted first.
- Manual Session JSON export creates a regular file in the working directory (default `redpulse-session.json`). That file remains until the user deletes it.

## Dashboard

- Disabled chart hover X/Y coordinate labels and cursor crosshairs to remove distracting overlays.

Use RedPulse only against systems you own or are explicitly authorized to test.

# RedPulse 0.6.0

RedPulse 0.6.0 introduces a simple three-profile workflow over the existing Rust HTTP engine and makes load-balancer and protection observations more useful.

## Simple guided profiles

- **Performance** measures HTTP throughput, status distribution, latency percentiles, and local generator headroom.
- **Load Balancer** automatically selects DNS round-robin and reports the measured request share, average latency, and errors for each resolved destination IP.
- **Protection** reports observed 401, 403, 429, and 503 responses during a controlled HTTP run.

Advanced settings are collapsed by default. The main view keeps the target, profile, workers, duration, arrival rate, and request budget accessible.

## Real routing diagnostics

- Each request is still assigned to a resolved destination IP in round-robin mode; retained samples preserve the selected endpoint IP.
- Per-IP results now show request count, percentage, average response latency, and HTTP/network errors.
- Per-backend results show the same metrics when the target returns the configured identity header, such as `X-Backend-ID`.
- The UI clearly distinguishes public destination IPs from private application backends. CDN anycast IPs are not assumed to represent unique origins.
- Round-robin mode does not rotate the test machine's source IP.

## Protection observations

- The Protection Audit performs six read-only HTTP requests and checks common security headers, cookies, CORS, advertised methods, and public metadata.
- Passive response markers can identify common edge-service signatures from Cloudflare, Fastly, Akamai, Imperva, Sucuri, and AWS WAF.
- Traffic Lab records observed HTTP 401, 403, 429, and 503 responses.
- Provider fingerprints and status codes are observations only. They do not prove a WAF policy is enabled or effective; correlate with application, load-balancer, and provider logs.
- The tool does not attempt to bypass protections or exploit targets.

## Verification scope

RedPulse remains an HTTP/1.1 and HTTP/2 load-testing engine. TCP/UDP benchmark modes, automated failover orchestration, and repeatable head-to-head performance comparisons are not included in this release. Use RedPulse only against systems you own or are explicitly authorized to test.

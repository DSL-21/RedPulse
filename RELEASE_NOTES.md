# RedPulse 0.3.0

RedPulse 0.3.0 adds the first Multi-IP testing capability to Traffic Lab.

## DNS origin routing

- **System DNS** keeps the normal system resolver and HTTP client behavior.
- **Round-robin DNS IPs** resolves the target hostname once, collects unique IPv4 and IPv6 addresses, and assigns successive requests across that address list.
- The request URL remains the original hostname. Reqwest DNS overrides pin the socket destination while the HTTP Host value and HTTPS hostname remain derived from the URL.
- Pinned mode makes direct origin connections and bypasses system proxy settings, because a proxy would otherwise resolve the hostname remotely.
- Every completed request records its selected origin IP. The UI shows per-IP request counts, the Results table includes the origin IP, and CSV/JSON reports preserve this attribution.

## Important distinction

DNS IP rotation tests the addresses published for the hostname. It does not reveal private backend nodes behind a public load balancer when those nodes are not exposed through DNS. Backend distribution analysis using response headers or application-provided node identifiers remains a separate feature.

## Validation

The release pipeline runs formatting, Rust unit tests, Clippy with warnings denied, and the optimized Windows x64 release build.

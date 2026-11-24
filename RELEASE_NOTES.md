# RedPulse 0.2.0

RedPulse 0.2.0 introduces the Web Recon module and positions the application as a native red-team web assessment workbench while retaining the existing HTTP load-testing engine.

## Web Recon profile

The Recon tab performs six sequential probes:

- GET the target URL with a bounded 128 KiB body sample for technology hints.
- HEAD the target URL.
- OPTIONS the target URL to inspect the Allow header without invoking advertised methods.
- GET `/.well-known/security.txt`.
- GET `/robots.txt`.
- GET `/sitemap.xml`.

Each probe uses a five-second request timeout and the probe sequence uses a fixed 250 ms interval. Redirects are not followed. The probe sequence stops early on HTTP 429 or 503. DNS address resolution is shown separately.

## Audit checks

- HSTS and HTTPS posture signal.
- Content Security Policy.
- Framing protection via X-Frame-Options or CSP frame-ancestors.
- X-Content-Type-Options.
- Referrer-Policy and Permissions-Policy.
- Cookie Secure, HttpOnly and SameSite attributes.
- Wildcard CORS combined with credentials.
- Advertised HTTP methods from OPTIONS.
- Server and technology disclosure headers and basic HTML technology hints.
- Presence of security.txt, robots.txt and sitemap.xml.

Each finding includes severity, evidence and a recommendation. Missing headers and technology fingerprints are observations that require manual review; they are not presented as proof of exploitability.

## Bounded behavior

- Six fixed HTTP probes per assessment.
- Five-second probe timeout.
- Sequential requests with fixed spacing.
- Redirect following disabled.
- Early stop when HTTP 429 or 503 is returned.
- Response body sampling capped at 128 KiB.
- No credential guessing, exploit payloads, destructive methods, recursive wordlist discovery or high-volume traffic generation in this profile.

## Existing Load Testing

The Constant Arrival Rate and Closed Loop benchmark modes remain available in the Load Test tab, including their request accounting, rate ceiling, request budget and latency analytics.

## Validation

The release pipeline runs formatting, Rust unit tests, Clippy with warnings denied and the optimized Windows x64 release build.

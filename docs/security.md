# Security model

[English](security.md) | [Русский](ru/security.md)

## What successful verification means

A valid SHA-256 proof was submitted for a server-issued challenge and the bound token was consumed by the correct integration. It does not prove human presence, device integrity or a legitimate account. Attackers can automate the solver, outsource work or use faster hardware. Proof of work cannot replace email policies, authentication, per-account limits or ingress protection.

## Implemented controls

- Random 256-bit challenge IDs, salts and integration secrets.
- Server-selected difficulty and expiry.
- Canonical Base64URL decoding and exact byte lengths.
- Authenticated internal routes on a separate loopback listener.
- Action allowlists and integration/action/context token binding.
- Constant-time secret-hash comparison.
- Transactional single-use consumption and bounded retry receipts.
- Body limits and bounded in-memory fixed-window request buckets.
- No trust in client IP forwarding headers, fingerprints or mouse movement.
- No credentials or token plaintext in application logging.

## Limitations to understand before deployment

This is an unaudited experimental MVP. No independent security review or load qualification has been performed. Public proof submissions are deliberately unauthenticated. Apply request/concurrency/connection limits at ingress; service rate limiting happens after JSON parsing and does not stop bandwidth or connection exhaustion. In-memory counters reset on restart; persistent history survives. Fixed windows permit bursts around boundaries. The global bucket cardinality cap can temporarily reject new sources under a distributed attack.

SQLite calls are synchronous and serialized inside async request handlers. Suitable for an initial test deployment, not a high-throughput public service. Use one process per database; additional processes have independent request counters. No automatic adaptive difficulty or global disk quota is provided. Monitor disk usage and configure external resource limits.

Browser proof timing varies significantly; the Web Crypto worker prioritizes straightforward interoperability over speed. Native solvers can be much faster. Do not interpret a slow browser as higher assurance. Widget text is English-only in this version.

The application must keep operation bindings immutable, authenticate users when needed, enforce its own limits, and make its own side effects idempotent. A token may be stolen by compromised page scripts; HTTPS and prevention of XSS are responsibilities of the hosting application. No rigid IP binding is used, so network changes are tolerated.

Keep server clocks synchronized. Avoid backwards clock jumps. Protect the configuration, key, database and backups. Do not pass secrets as CLI arguments; create/rotation output must be captured securely. Use HTTPS or an authenticated local/tunneled path for server credentials.

## Reporting

Until a private security contact is configured, do not publish exploitable details or credentials in a public issue. Request a private reporting channel from the repository owner.

## Website risk policy

Website mode adds exact-origin binding, account HMACs, backend-only login outcomes
and a 24-hour default history window. Browser and behavior reports remain forgeable.
Per-source public-submission history uses the transport peer, not forwarded headers.
Outcome reports require a previously consumed verification and are immutable and
idempotent. The integration backend is trusted to report truthful outcomes and
bind the account/context to its own server session. Account targeting can cause
extra checks for victims; these thresholds need operational validation.

Additional-verification decisions never grant a token by themselves. Completion
requires a bound, expiring visual task and the original PoW solution. The current
small static illustration corpus is a prototype that bots can memorize or recognize;
it is not a production-strength CAPTCHA. No nonvisual alternative is implemented. Errors and infrastructure failures must fail closed
in the host login flow. HMAC identifiers are pseudonymous; retain the key privately.
Cleanup is batched and may lag; risk queries exclude expired history immediately.

# Architecture

The initial implementation is one Rust package with explicit modules. Splitting it into separately versioned crates is deferred until the interfaces stabilize.

| Component | Responsibility |
|---|---|
| `src/core.rs` | Canonical binary encoding, SHA-256 verification, native solver |
| `src/service.rs` | State machine, SQLite, authentication, HTTP routers and limits |
| `src/main.rs` | Initialization, service startup and integration administration |
| `web/widget.js` | Host page integration and cancellation |
| `web/worker.js` | Browser proof-of-work solver |

## Trust boundaries

Clients are untrusted. Only an authenticated application server creates challenges or consumes tokens. Applications choose action and context; clients cannot select difficulty. Each integration has an allowlist of actions. An integration cannot consume another integration's tokens.

The public and internal APIs are separate listeners and routers. The internal listener must bind to loopback. Remote application servers must use an authenticated tunnel; direct remote internal API deployment is outside this MVP.

## Persistence

SQLite uses WAL, foreign keys and a two-second busy timeout. A process-local mutex serializes database transactions. Challenge issuance is unique by integration/action/context. Solving and consuming are transactional. Competing consumption attempts with different request IDs yield one success.

The schema stores integration secret hashes, optional previous hashes with grace expiry, allowed actions, challenges, token hashes and consumption receipts. Token plaintext is deterministically reconstructed by HMAC from a private server key. Secret and token hashes use SHA-256; these are high-entropy random credentials, not human passwords.

A background task runs cleanup every minute, deleting at most 500 expired rows. Operators can also run `cleanup`. Context idempotency lasts while its row is retained; applications must never reuse operation IDs even after cleanup.

## Scope

One process, one SQLite database, fixed difficulty per deployment, local CLI administration, web widget and native solver. No clustering, adaptive risk engine, billing, hosted control panel, CAPTCHA-provider adapters or application-specific workflows. Neruvalo integration belongs in Neruvalo's repository.

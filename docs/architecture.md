# Architecture

[English](architecture.md) | [Русский](ru/architecture.md)

One Rust package implements the service. Separate versioned crates are deferred
until interfaces stabilize.

| Component | Responsibility |
|---|---|
| `src/core.rs` | Canonical encoding, SHA-256 PoW and native solver |
| `src/service.rs` | Challenges/tokens, SQLite, authentication, limits and listeners |
| `src/website.rs` | Account/origin bindings, login outcomes, website risk and CORS |
| `src/visual.rs` | Image tasks, rotation, answer budgets and illustration bank |
| `src/main.rs` | Initialization, serving and integration administration |
| `web/widget.js` | Observations, image selection, submission and cancellation |
| `web/worker.js` | Browser PoW computation |
| `sdk/server.mjs` | Optional Node.js backend client |
| `assets/visual/tiles/` | Prepared images embedded in the binary |
| `examples/prepare_visual.rs` | Offline atlas preparation |

## Trust boundaries

Clients are untrusted. An authenticated backend creates attempts, consumes tokens
and reports login results. It chooses immutable action, context and account bindings.
Clients cannot select difficulty or report password results. Secrets stay on the backend.

Public and internal APIs have separate listeners. Internal API binding is loopback
only; remote backends need private authenticated transport. Website routes check
exact origins and implement CORS. Origin is not authentication against arbitrary HTTP bots.

## Flows

The original flow is prepare -> solve PoW -> consume -> execute an application
operation. Browser, behavior and IP-history policies are optional additions.

Website mode adds account-bound login attempts. Risk can require a visual task.
A correct selection plus original PoW yields a token for the same attempt. The
website consumes it, checks credentials and reports the result. Its own session
and transaction mechanism must prevent repeated side effects.

## Persistence

SQLite uses WAL, foreign keys and a two-second busy timeout. A process-local mutex
serializes transactions. Challenge identity is unique by integration/action/context.
SHA-256 hashes protect high-entropy credentials. Token plaintext is reconstructed
using a private HMAC key.

Tables store integrations, actions/origins, policies, challenges, token receipts,
minute attempt buckets, account bindings, outcomes and visual state. Account IDs
and source IPs use HMAC pseudonyms. Outcome retries are idempotent; conflicting
results are rejected.

A wrong image answer atomically spends one attempt and, with budget remaining,
replaces the category, board and task ID. Expiry stays fixed. Stale task IDs cannot
answer the replacement, and re-fetching does not reset its budget. New attempts
exclude the previous retained category for the same account/integration.

## Cleanup and scope

Cleanup runs every minute, deleting at most 500 eligible rows per cleanup table.
Expired parent challenges cascade-delete image tasks and website bindings.
Receipt and history retention are separate; history defaults to 24 hours.
Expired history stops affecting risk even if physical deletion lags. Context IDs
must never be reused after cleanup.

This is a single-process, single-database prototype with fixed PoW difficulty and
rule-based screening. Clustering, trained classification, trusted forwarded-IP
handling, dashboards and nonvisual extra checks are not implemented. The static
illustration corpus is a visual prototype.

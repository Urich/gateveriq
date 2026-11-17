# Architecture

[English](architecture.md) | [Русский](ru/architecture.md)

One Rust package implements the service. Separate versioned crates are deferred
until interfaces stabilize.

| Component | Responsibility |
|---|---|
| `src/context.rs` | Stateless signed operation contexts and expiry |
| `src/core.rs` | Canonical encoding, SHA-256 PoW and native solver |
| `src/service.rs` | Challenges/tokens, SQLite, authentication, limits and listeners |
| `src/website.rs` | Account/origin bindings, login outcomes, website risk and CORS |
| `src/visual.rs` | Slider tasks, coordinates, rotation and answer budgets |
| `src/puzzle.rs` | Procedural landscape and puzzle PNG generation |
| `src/main.rs` | Initialization, serving and integration administration |
| `web/widget.js` | Observations, slider interaction, submission and cancellation |
| `web/slider.js` | Isolated slider styles, themes and state |
| `web/worker.js` | Browser PoW computation |
| `sdk/server.mjs` | Optional Node.js backend client |
| `assets/visual/tiles/` | Historical illustration assets, unused by runtime |
| `examples/prepare_visual.rs` | Offline atlas preparation |

## Trust boundaries

Clients are untrusted. An authenticated backend creates attempts, consumes tokens
and reports login results. It chooses immutable action, context and account bindings.
Clients cannot select difficulty or report password results. Secrets stay on the backend.

Public and internal APIs have separate listeners. The internal API defaults to loopback in native installations; Docker binds it to
`0.0.0.0:9081` for backends on the same private container network, without publishing
the port on the host. Remote backends need private authenticated transport. Website routes check
exact origins and implement CORS. Origin is not authentication against arbitrary HTTP bots.

## Flows

The original flow is prepare -> solve PoW -> consume -> execute an application
operation. Browser, behavior and IP-history policies are optional additions.

Website mode adds account-bound login attempts. Risk can require a visual task.
A correct slider position plus original PoW yields a token for the same attempt. The
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

A wrong slider position atomically spends one attempt and generates a new landscape,
cutout and task ID. Expiry stays fixed. Stale IDs fail; re-fetching does not reset
the budget. Correct coordinates remain in SQLite; PNG payloads carry no answer field.

## Cleanup and scope

Cleanup runs every minute, deleting at most 500 eligible rows per cleanup table.
Expired parent challenges cascade-delete image tasks and website bindings.
Receipt and history retention are separate; history defaults to 24 hours.
Expired history stops affecting risk even if physical deletion lags. Contexts are server-issued and authenticated with an expiry, integration and action binding.
Expired contexts remain invalid after cleanup without a permanent identifier ledger.

This is a single-process, single-database prototype with fixed PoW difficulty and
rule-based screening. Clustering, trained classification, trusted forwarded-IP
handling, dashboards and nonvisual extra checks are not implemented. Slider puzzles remain an experimental additional check.

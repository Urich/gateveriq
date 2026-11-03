# Protocol v1

[English](protocol.md) | [Русский](ru/protocol.md)

All requests use JSON, UTF-8 and a maximum body size of 4096 bytes. Unknown JSON fields are rejected. Timestamps are Unix seconds in UTC. The server clock is authoritative. The request limit does not cap image response size.

IDs, salt and tokens encode 32 bytes in canonical unpadded Base64URL (43 characters). Nonces encode exactly 8 bytes (11 characters). Action, context and request IDs are 1–128 ASCII letters, digits or characters `_-.:`.

## Create a challenge

`POST /internal/v1/challenges`, authenticated by `Authorization: Bearer <integration_secret>`:

```json
{"integration_id":"<id>","action":"register","context_id":"<unique-operation-id>"}
```

Response:

```json
{"challenge_id":"<id>","algorithm":"sha256-pow-v1","salt":"<salt>","difficulty_bits":18,"expires_at":1800000000}
```

Reusing the same integration/action/context returns the original unexpired challenge. Expired contexts are rejected while retained. Always use a new operation ID for a genuinely new attempt.

## Compute a solution

Hash the following concatenation, without delimiters or length prefixes:

1. UTF-8 bytes of `gateveriq:pow:v1`.
2. Decoded challenge ID: 32 bytes.
3. Decoded salt: 32 bytes.
4. Nonce: 8 bytes.

The first `difficulty_bits` bits of SHA-256 must be zero, most significant bit first. Native and browser solvers enumerate unsigned 64-bit counters in little-endian order; any eight-byte nonce satisfying the condition is valid.

## Submit a solution

`POST /public/v1/solutions`, no authentication:

```json
{"challenge_id":"<id>","nonce":"<nonce>"}
```

Response:

```json
{"verification_token":"<token>","expires_at":1800000060}
```

A retry with a valid proof returns the same token without extending its expiry, provided both the challenge and token remain valid and the token has not been consumed. After challenge expiry, the client must start a new operation even if an earlier solution succeeded but its response was lost.

Token bytes are `HMAC-SHA256(token_key, UTF8("gateveriq:verification:v1") || decoded_challenge_id)`. The database stores SHA-256 of the token's Base64URL string.

## Consume a token

`POST /internal/v1/verifications/consume`, authenticated:

```json
{"integration_id":"<id>","verification_token":"<token>","action":"register","context_id":"<operation-id>","request_id":"<stable-consumption-id>"}
```

Response:

```json
{"valid":true,"action":"register","context_id":"<operation-id>"}
```

The first consumption must occur before token expiry. Exact retries with the original request ID and matching binding return success for `retention_seconds` after consumption, even after token expiry. A different request ID fails. Mismatched integration is reported as `INVALID_TOKEN`; mismatched action/context as `CONTEXT_MISMATCH`.

## Errors

Domain errors use `{"error":{"code":"CODE","retryable":false}}`.

| HTTP | Codes |
|---|---|
| 400 | INVALID_REQUEST, CHALLENGE_NOT_FOUND, CHALLENGE_EXPIRED, INVALID_SOLUTION, INVALID_TOKEN, TOKEN_EXPIRED, TOKEN_ALREADY_CONSUMED, CONTEXT_MISMATCH |
| 401 | UNAUTHORIZED |
| 403 | ACTION_NOT_ALLOWED, BROWSER_SIGNALS_REQUIRED, AUTOMATION_SUSPECTED, BEHAVIOR_SIGNALS_REQUIRED, BEHAVIOR_REJECTED |
| 429 | RATE_LIMITED, ATTEMPT_RATE_LIMITED, ATTEMPT_HISTORY_BLOCKED |
| 503 | SERVICE_UNAVAILABLE |

Axum-level malformed JSON, missing content type, oversized body, unknown route and method errors may use framework text responses (400/404/405/413/415/422). Clients must handle non-JSON errors. Retry transient failures with bounded exponential backoff, retaining the original context/request ID. Never treat an error as successful verification.

### Browser screening extension

For integrations created with `--browser-check`, public solution submissions must
include `browser: {"webdriver": false, "cookies_enabled": true, "languages_count": 1}`
in addition to `challenge_id` and `nonce`. These are untrusted browser observations,
not attestations. Missing observations return 403 `BROWSER_SIGNALS_REQUIRED`;
automation indicators return 403 `AUTOMATION_SUSPECTED`. No token is issued.
Integrations without this policy accept the original solution format.

### Behavioral screening extension

`--behavior-check` enables browser screening and requires a `behavior` object on
solution submissions: `challenge_id` (the same challenge), `elapsed_ms`,
`activation_ms`, `trusted_activations`, `pointer_events`, `keyboard_events`,
`focus_changes`. All counters are unsigned integers. Timing must satisfy
300 <= activation_ms <= elapsed_ms <= configured challenge TTL in milliseconds;
trusted_activations must be 1..20 and other counters 0..4096.
Missing reports return 403 BEHAVIOR_SIGNALS_REQUIRED; inconsistent reports return
403 BEHAVIOR_REJECTED. Real challenge expiration and proof validity are checked
independently. The report is untrusted input and is not proof of human presence.

### Persistent history policy

`--history-check` enables persistent per-integration, per-transport-peer history
for public solution submissions. By default 20 submissions per UTC minute are admitted;
5 client verification failures across the current and previous 9 minute buckets
block subsequent submissions with 429 ATTEMPT_HISTORY_BLOCKED. The per-minute
limit returns 429 ATTEMPT_RATE_LIMITED. Denied requests do not extend the window.
Successful retries count toward frequency. The policy is evaluated before token
issuance; it does not invalidate already issued tokens. No client IP field or
forwarded header is accepted as identity. Thresholds are configurable under `[website]`. See README for proxy limitations.

## Website API

See [website integration](website-integration.md) for the complete flow.

- Authenticated `POST /internal/v1/web/attempts`: `integration_id`, `action`,
  `context_id` (canonical random 32-byte base64url), `account_id` (opaque 1-128
  characters, no controls), `origin` (exact configured origin).
  Returns `{decision:"verify",challenge:{...}}` or
  `{decision:"additional_verification_required",reason:"risk_policy",verification_token:null}`.
  The context binds the account, action and origin immutably.
- Public `POST /public/v1/web/solutions`: the existing solution schema, with the
  browser Origin header. Supports OPTIONS preflight. The origin must match the
  binding AND integration allowlist. The legacy solution route cannot issue website
  tokens. Risk can return HTTP 403 with the additional-verification decision above.
- Consume the token using the existing authenticated consume API with the original
  action/context and a stable server-generated request ID.
- Authenticated `POST /internal/v1/web/outcomes`: `integration_id`, `action`,
  `context_id`, `request_id`, `success` (boolean). Requires matching consumed
  verification within the consume retention window. Returns `{recorded:true}`.
  Exact retries are idempotent; changing the recorded result returns
  `OUTCOME_CONFLICT`. Missing or mismatched consumed verification returns
  `VERIFICATION_NOT_CONSUMED` (400). Outcome counts cannot be submitted by a browser.

`ORIGIN_NOT_ALLOWED` is 403. Missing origin, changed origin or an attempt prepared
through the legacy API cannot obtain a website token. Origins are canonical HTTPS
origins (HTTP only for loopback development), not arbitrary URLs or wildcard domains.
CORS/Origin is a browser policy, not proof against an HTTP bot.

### Visual additional verification

Additional-verification responses now include `challenge` and `visual`:
`task_id`, `expires_at`, `instruction`, `reference` (PNG data URL), `images`
(16 PNG data URLs in display order) and `max_attempts` (remaining budget, initially 3). There is no answer field.

Public `POST /public/v1/web/visual/solutions` accepts `challenge_id`, `task_id`,
`nonce` (original PoW), `selected` (distinct zero-based image indexes, 0..15).
Origin must match the original attempt and allowlist. OPTIONS preflight is supported.
Success returns the normal verification token. The backend must still consume it.

400 errors: INVALID_VISUAL_SOLUTION, VISUAL_NOT_FOUND, VISUAL_EXPIRED,
INVALID_SOLUTION, CHALLENGE_EXPIRED, INVALID_REQUEST. After three wrong answers,
subsequent attempts return 429 VISUAL_ATTEMPTS_EXHAUSTED. Reissuing the same
attempt does not reset this state. A correct retry before token consumption returns
the same token; a consumed token cannot be reissued. Once assigned, the visual task
cannot be bypassed through the normal solution endpoint. Visual tasks and their
state survive restarts and are deleted with their expired parent challenges.

On an incorrect visual selection with remaining attempts, HTTP 400 includes
`error.code = INVALID_VISUAL_SOLUTION` and a replacement `visual` object. The next
object category differs, the task ID changes and old task IDs become invalid.
`max_attempts` reports the remaining answer budget. The original expiry and login
binding remain unchanged. At the last wrong answer HTTP returns 429
VISUAL_ATTEMPTS_EXHAUSTED, with no replacement task. Re-fetching an offer returns
the current task and does not reset its budget. For a new login attempt, the last
retained visual category for the same account/integration is also excluded.

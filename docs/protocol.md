# Protocol v1

[English](protocol.md) | [Русский](ru/protocol.md)

All requests use JSON, UTF-8 and a maximum body size of 4096 bytes. Unknown JSON fields are rejected. Timestamps are Unix seconds in UTC. The server clock is authoritative. The request limit does not cap image response size.

IDs, salt and tokens encode 32 bytes in canonical unpadded Base64URL (43 characters). Nonces encode exactly 8 bytes (11 characters). Action and request IDs are 1–128 ASCII letters, digits or characters `_-.:`. Context IDs use the signed format below.

## Issue an operation context

First call authenticated `POST /internal/v1/contexts`:

```json
{"integration_id":"<id>","action":"register"}
```

Response: `{"context_id":"<signed-context>","expires_at":1800000000}`.
Save the context on your backend before creating a challenge. Issuance writes no
operation state. The action must be allowed and the integration active.

The context is 98 characters of canonical unpadded Base64URL encoding 73 bytes:
version byte `1`, expiry as an 8-byte big-endian signed Unix timestamp, 32 random
bytes, then a 32-byte HMAC-SHA256 tag. The MAC uses the private service key, domain
`gateveriq:context:v1:`, then integration ID, action, and the first 41 context bytes,
each prefixed by its byte length encoded as unsigned 64-bit big-endian.
Clients must treat the complete string as opaque.

Lifetime is `challenge_ttl_seconds`, starting at context issuance. Challenge
expiry equals context expiry; retries never extend it. Forged, noncanonical,
wrong-integration or wrong-action contexts return `INVALID_CONTEXT`; authentic
expired contexts return `CONTEXT_EXPIRED`, even after database cleanup/restart.
No permanent used-context ledger is required. Token expiry and exact consume
receipt retries retain their own existing deadlines.

This is a breaking change: arbitrary operation IDs and random 32-byte contexts
are rejected. Use a fresh database; no upgrade migrations are provided.

## Create a challenge

`POST /internal/v1/challenges`, authenticated by `Authorization: Bearer <integration_secret>`:

```json
{"integration_id":"<id>","action":"register","context_id":"<signed-context>"}
```

Response:

```json
{"challenge_id":"<id>","algorithm":"sha256-pow-v1","salt":"<salt>","difficulty_bits":18,"expires_at":1800000000}
```

Reusing the same integration/action/context returns the original unexpired challenge. Expired contexts are rejected even after their database records are removed. Always use a new operation ID for a genuinely new attempt.

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

Website attempts use adaptive screening instead of the legacy 300 ms minimum.
Webdriver or an untrusted activation requires an additional visual task. Otherwise,
two weak signals (disabled cookies, no languages, activation below 300 ms, or no
pointer/keyboard/focus events) require a task; one alone is allowed. Missing reports
also require a task. Unbound reports, inconsistent timing and out-of-range counters
return INVALID_REQUEST. History and account policies still apply; a pending visual
task cannot be bypassed by resubmitting better signals. Requesting an additional
task does not count as a failed verification. Proof validity, expiry and consumption
are checked before interaction screening. These initial rules need calibration
against actual traffic; client reports remain forgeable.

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
  `context_id` (server-issued signed context), `account_id` (opaque 1-128
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

### Slider additional verification

Additional-verification responses include `challenge` and a `visual` slider task:
`kind: "slider_puzzle"`, `task_id`, `expires_at`, `instruction_key: "slide_piece"`,
`background` (280×140 RGBA PNG data URL), `piece` (40×40 RGBA PNG data URL),
`width: 280`, `height: 140`, `piece_size: 40`, `piece_y`, `max_position: 240`,
and `max_attempts` (remaining budget, initially 3). The correct horizontal position
is stored only on the server. A procedural landscape is generated for each task;
there is no static category bank or external image service.

Public `POST /public/v1/web/visual/solutions` accepts:

```json
{"challenge_id":"<id>","task_id":"<task>","nonce":"<original-pow>","position":137}
```

`position` is an integer 0–240 in original image pixels, independent of CSS scaling.
The server accepts an absolute distance of at most 3 pixels from the answer.
The old `selected` array is rejected. Origin must match the attempt and allowlist;
OPTIONS is supported. Success returns a token for backend consumption, with method
`pow_visual`. The method name remains unchanged for slider puzzles.

400 errors: INVALID_VISUAL_SOLUTION, VISUAL_NOT_FOUND, VISUAL_EXPIRED,
INVALID_SOLUTION, CHALLENGE_EXPIRED, INVALID_REQUEST. Out-of-range positions are
invalid requests. An in-range incorrect position spends one answer and returns
HTTP 400 with `error.code = INVALID_VISUAL_SOLUTION` and a replacement `visual`.
The image, cutout position and task ID are regenerated. The original deadline and
remaining budget are preserved; stale task IDs fail. The final wrong answer
returns 429 VISUAL_ATTEMPTS_EXHAUSTED with no replacement.

Re-fetching an offer returns the current task without resetting its budget.
Correct retries return the same token until consumption; consumed tokens cannot
be reissued. Assigned puzzles cannot be bypassed through the ordinary solution
endpoint. Puzzle state survives restarts and is deleted with expired challenges.
Expiry is at most 120 seconds and never later than challenge expiry.

Image matching and blind guessing remain possible. A completed slider is not
proof of human presence; retain PoW, backend policy and request limits.

## Verification method and action policy

Consume returns `verification_method` (`pow` or `pow_visual`), persisted at token
issuance and stable across receipt retries. Mandatory policies are snapshotted per
challenge. See [action policies and migration](website-integration.md#action-policies-and-registration).

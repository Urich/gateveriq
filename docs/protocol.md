# Protocol v1

All requests use JSON, UTF-8 and a maximum body size of 4096 bytes. Unknown JSON fields are rejected. Timestamps are Unix seconds in UTC. The server clock is authoritative.

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
| 403 | ACTION_NOT_ALLOWED |
| 429 | RATE_LIMITED |
| 503 | SERVICE_UNAVAILABLE |

Axum-level malformed JSON, missing content type, oversized body, unknown route and method errors may use framework text responses (400/404/405/413/415/422). Clients must handle non-JSON errors. Retry transient failures with bounded exponential backoff, retaining the original context/request ID. Never treat an error as successful verification.

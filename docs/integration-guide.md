# Integration guide

[English](integration-guide.md) | [Русский](ru/integration-guide.md)

This guide covers the original PoW operation flow. For account-bound website login,
CORS and visual verification, use [website integration](website-integration.md).

## Backend contract

1. Apply your own request and business limits.
2. Generate and persist a cryptographically random operation ID, binding it to the intended action and immutable application parameters.
3. Call Gateveriq's internal challenge endpoint with your server credential.
4. Return the challenge and operation ID to the client.
5. The client solves the challenge and submits it to the public API.
6. Receive the operation ID and token at your application backend. Load the operation; do not trust a client-supplied action or payload binding.
7. Persist a consumption request ID, then consume the token server-to-server.
8. Persist verification and execute the operation once using your own database transaction/idempotency mechanism.

If the connection drops during consumption, repeat the same request ID. Gateveriq's consumption and your application's transaction are not distributed transactions. Recovery must happen within the configured receipt retention period; after that, require a new verification if the outcome remains unknown. Keep verification records in your application as needed.

Never send your integration secret to clients. A public integration ID alone does not authorize challenge creation. Domain allowlists and CORS are not authentication mechanisms.

## Browser widget

Reverse proxy the public API and assets under your application's origin. For example, `/gateveriq/widget.js`, `/gateveriq/worker.js` and `/gateveriq/public/v1/solutions` should strip `/gateveriq` upstream. The original solution route has no CORS. Website mode supports cross-origin solution submission; see [website integration](website-integration.md).

```html
<div id="verification"></div>
<button id="cancel" type="button">Cancel</button>
<script type="module">
import { GateveriqWidget } from '/gateveriq/widget.js';
const widget = new GateveriqWidget(document.querySelector('#verification'));
document.querySelector('#cancel').onclick = () => widget.cancel();
// Obtain this response from YOUR application backend, which calls the internal API.
const prepared = await fetch('/api/operation/prepare', {method: 'POST'}).then(r => {
  if (!r.ok) throw Error('Preparation failed');
  return r.json();
});
const token = await widget.verify(prepared.challenge, {
  solutionURL: '/gateveriq/public/v1/solutions'
});
// Send token + operation ID to YOUR backend. Only its consume response authorizes work.
await fetch('/api/operation/complete', {
  method: 'POST', headers: {'Content-Type': 'application/json'},
  body: JSON.stringify({operation_id: prepared.operation_id, verification_token: token})
});
</script>
```

The `/api/operation/*` endpoints are illustrative application endpoints; Gateveriq does not implement them. Catch exceptions in your host UI and offer a new attempt. An AbortSignal can also be passed to `verify`.

Allow same-origin module scripts, workers and API connections in your Content Security Policy. Serve over HTTPS; localhost is suitable for testing. The solver runs in a worker but consumes CPU and power. Benchmark on your target phones before enabling it publicly.

## Native clients

Use `core::solve` in a background task. CLI usage:

```sh
cargo run --locked -- solve --challenge challenge.json --submit-url https://example.com/gateveriq/public/v1/solutions
```

Without `--submit-url`, the command outputs solution JSON. This MVP provides a Rust module and CLI, not platform installers or dedicated Android/iOS SDK packages. Native callers must discard work on operation cancellation; the current Rust solver checks expiry but has no cancellation handle.

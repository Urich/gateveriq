# Website integration

[English](website-integration.md) | [Русский](ru/website-integration.md)

The website backend creates and stores each login attempt. The browser solves the
challenge directly against the public Gateveriq listener. The website backend
consumes the token before checking credentials and reports the actual login result.
The internal listener stays on loopback; keep Gateveriq with your application backend
or reach it through a private authenticated transport. Never expose the integration
secret or the internal API to browser code.

## Create an integration

```powershell
cargo run --locked -- integration-create --name website --actions login --origins https://your-site.example
```

Use your actual canonical origin, without a trailing slash. Multiple exact origins
can be comma-separated. HTTPS is required except for localhost development.
Website mode enables browser, behavior and persistent IP-history checks. Existing
integrations remain unchanged. Restart the upgraded server to load the new routes.

The optional `[website]` section of config.toml defaults to:

```toml
[website]
history_retention_seconds = 86400
attempts_per_minute = 20
failure_limit = 5
account_failure_limit = 5
```

IP failures use the last ten UTC minute buckets; login failures per account use
the configured retention window (24 hours by default). A success does not clear
failures. These are experimental rules, not a calibrated classifier. Cleanup runs
in bounded batches: expired rows are ignored immediately but physical deletion can
lag behind under a backlog. HMAC identifiers are pseudonymous, not anonymous.

## Backend (Node.js)

Import `sdk/server.mjs` into your backend. Use a runtime with built-in fetch and
AbortSignal.timeout. This example describes handlers inside your existing site;
it does not implement sessions, passwords or CSRF protection for you.

```javascript
import {GateveriqClient} from './sdk/server.mjs';
const gate = new GateveriqClient({
  integrationId: process.env.GATEVERIQ_ID,
  secret: process.env.GATEVERIQ_SECRET,
});

// POST /login/prepare — your normal same-origin, CSRF-protected handler.
// Resolve the account to the same opaque ID for every attempt. For nonexistent
// accounts use a stable opaque mapping too; do not reveal existence to the browser.
const login = await gate.prepareLogin({
  accountId: opaqueAccountId,
  origin: 'https://your-site.example', // your configuration, not browser input
});
session.pendingLogin = {...login, accountId: opaqueAccountId};
// Return ONLY login.attempt to the browser.
```

```javascript
// POST /login/complete — same session and CSRF checks as above.
// Serialize completion for a pending attempt; keep a stable requestId for retries.
const pending = session.pendingLogin;
if (!pending || !pending.attempt.challenge) throw Error('No pending verification');
await gate.consume({...pending, verificationToken: request.body.verification_token});
// The account being authenticated must be pending.accountId. Do not let the
// browser change the account after preparing verification.
const success = await yourPasswordCheck(pending.accountId, request.body.password);
await gate.reportOutcome({...pending, success});
// Mark this application operation completed atomically, then establish the session
// only on success. Exact Gateveriq retries are idempotent; your operation must be too.
```

Persist the pending operation server-side. Do not trust client-provided context,
account or request IDs. Queue/retry a failed outcome report using the same values
within the consume retention window (default 300 seconds). Never treat an API
failure as permission to authenticate. Passwords are handled exclusively by your
application and never sent to Gateveriq.

## Browser

Mount the widget inside your form. It renders a native checkbox that users can
activate by mouse, touch or keyboard. The checkbox starts verification. On a
high-risk attempt, the widget may then show an image challenge.

Disable the protected submit button until the widget returns `verified` and
a token. Send the token to your own backend with the form submission. If a
field bound to the pending attempt changes, cancel the widget with an
AbortSignal, discard the token and prepare a new attempt. The backend must
consume the token before performing the action. Keep separate rate limits for
actions such as posting comments and sending email.

Serve `web/widget.js` and `web/worker.js` as static files on your website, in the same
directory. The Worker is created on the website origin; do not import the widget
directly from a different origin. Its solution request goes directly to Gateveriq.

```html
<div id="verification"></div>
<script type="module">
import {verifyWebsiteAttempt} from '/assets/gateveriq/widget.js';
// Get this from your /login/prepare response after normal session/CSRF handling.
const attempt = preparedAttempt;
const result = await verifyWebsiteAttempt(document.querySelector('#verification'), attempt, {
  serviceURL: 'https://verify.your-site.example',
});
if (result.decision === 'additional_verification_required') {
  // Display a pending additional-check state. Do not submit login as verified.
} else {
  // Send result.verification_token to your /login/complete handler with credentials.
}
</script>
```

Allow the public Gateveriq URL in your site's CSP connect-src, the local Worker
in worker-src and data: images in img-src for the visual challenge. The public listener needs HTTPS for an HTTPS website. A terminating
reverse proxy makes requests share its peer IP until trusted ingress support is
implemented; do not assume forwarded headers are honored.

## Decisions and limits

Preparation returns `{"decision":"verify","challenge":{...}}` or
`{"decision":"additional_verification_required","reason":"risk_policy","verification_token":null}`.
The additional-check response includes the bound challenge and a visual task;
it cannot authenticate a login until the task is solved. Browser submission can
also return the latter with HTTP 403 if history or browser/behavior policy raises
risk. Ordinary invalid proofs, missing fields and exhausted throughput limits
remain errors. The browser helper handles both prepare-time and submit-time decisions.

The visual task asks users to select all objects of the same type as a reference illustration from
16 PNG tiles. The bundled bank has 24 illustrations across six categories (cats,
dogs, bicycles, cars, potted flowering plants and trees), generated offline and
embedded in the binary. Each board contains distinct pictures, in random order. Correct answers remain in SQLite and are never
sent as labels, filenames or vector metadata. The task expires after at most 120
seconds (and no later than the original challenge), allows at most three answer attempts before success,
and cannot be refreshed to reset its budget. After a wrong answer, the widget
receives a new object category and board with a new task ID. The original
deadline and remaining answer budget are preserved. Correct completion also requires the
original PoW solution and produces a token bound to the same login attempt. It
replaces the browser/behavior and IP-history risk rejection for that attempt only;
ordinary throughput limits remain. It does not erase account or source history.

The small static illustration bank is an experimental visual prototype. Its images
can be memorized or recognized by bots; it is not an improvement in bot resistance. It is not a production-quality CAPTCHA corpus. Keyboard
selection is supported, but no nonvisual alternative exists yet.

The account HMAC, exact origin and random attempt context are bound server-side.
Tokens remain short-lived and one-use, with exact retries only for the same request
ID. Origin validation prevents accidental browser integration on other sites, but
is not bot detection: an HTTP bot can forge Origin and browser observations.

History covers Gateveriq submissions and backend-reported login results, not every
request or every failed password anywhere on the site. The host application must
enforce this flow on every protected login path. Shared IPs and targeted account
failures can cause extra checks for legitimate users; validate thresholds before
production use.

## Try visual verification locally

Create a website-mode demo integration:

```powershell
cargo run --locked -- integration-create --name visual-demo --actions demo --origins http://127.0.0.1:8090
```

Restart the updated Rust server. In another terminal, set the returned credentials
and enable website mode:

```powershell
$env:GATEVERIQ_ID = "ID_FROM_COMMAND"
$env:GATEVERIQ_SECRET = "SECRET_FROM_COMMAND"
$env:GATEVERIQ_WEBSITE = "1"
python examples/demo.py
```

For Linux/macOS:

```sh
export GATEVERIQ_ID='ID_FROM_COMMAND'
export GATEVERIQ_SECRET='SECRET_FROM_COMMAND'
export GATEVERIQ_WEBSITE='1'
python3 examples/demo.py
```

Open http://127.0.0.1:8090. Check Simulate a failed password, complete five
verifications (or temporarily set `[website].account_failure_limit = 1` and restart
for one failure). The next verification shows the image task. Clear the checkbox
before completing the next simulated login if you want a successful result.
The failed-login switch is a local testing aid; production outcomes must come from
the backend's actual password check. Browser solutions go directly to Gateveriq
in this mode; Python still creates attempts and consumes tokens.

## Action policies and registration

Create an integration allowing `register,email_verify,login,unlock`, with the exact
Neruvalo server origin. Set the policies locally (replace ID):

```powershell
gateveriq integration-visual-policy --id ID --action register --visual always
gateveriq integration-visual-policy --id ID --action email_verify --visual always
gateveriq integration-visual-policy --id ID --action login --visual adaptive
gateveriq integration-visual-policy --id ID --action unlock --visual always
```

Only allowed actions can have a policy. Unconfigured actions default to adaptive.
Policies are snapshotted when the challenge is created, including challenges made
through the internal PoW API. Changes apply to new challenges only. `always`
requires website mode (configure origins); without a bound website attempt it
fails closed. Browser input cannot relax the policy. All token issuance routes
check the snapshot, even when no visual task exists yet.

The existing endpoints accept every allowed action:

```json
{"integration_id":"ID","action":"register","context_id":"32-byte-base64url","account_id":"opaque-id","origin":"https://neruvalo.example"}
```

POST this to `/internal/v1/web/attempts` using backend bearer authentication.
For always, prepare returns `decision: additional_verification_required`,
`challenge`, `visual`, and `verification_token: null`. Pass this payload to
`verifyWebsiteAttempt`. The widget solves PoW and submits images to
`/public/v1/web/visual/solutions`; success returns `verification_token` and
`expires_at`. Consume it through `/internal/v1/verifications/consume`:

```json
{"integration_id":"ID","action":"register","context_id":"32-byte-base64url","request_id":"stable-request-id","verification_token":"token"}
```

```json
{"valid":true,"action":"register","context_id":"32-byte-base64url","verification_method":"pow_visual"}
```

Methods are `pow` and `pow_visual`, describing actual token issuance, not the
configured policy. Exact consume retries return the same fields during receipt
retention even after token/task expiry. A different request ID is rejected.
Neruvalo should require `pow_visual` for register, email_verify and unlock.

Use `createAttempt` to generate identifiers, save them before network I/O, then
call `prepareAttempt(saved)`. Repeat with the same saved values after a network
failure; return the saved attempt on page reload. `prepareLogin` remains a
compatible wrapper; `reportOutcome` already accepts any action. The SDK returns
all consume fields. See [registration example](../examples/registration.mjs).
Its Maps are illustrative: use durable transactions, immutable request payloads,
and operation ownership checks in production. A wrong email code spends its
permission; the next submission requires a new email_verify context and images.
Registration may grant one initial code submission without a second challenge.
Never execute a business operation again merely because consume retry succeeded.

Account risk history is scoped by integration, account hash and action. Success
does not erase risk history. Transport-source/IP risk remains shared across actions.
Account lockout, emails, OPAQUE, and a protected desktop status channel belong to
Neruvalo; no passwords, codes, email addresses or recovery links go to GateVeriq.

### Upgrade

Back up the SQLite database and token key, stop the old server, replace the binary,
and start it normally. Startup transactionally migrates schema version 1 to 2;
reopening an upgraded database is safe. Existing attempts stay adaptive. Existing
issued tokens conservatively report pow: the old schema did not snapshot the
method at issuance, so a later passed task cannot prove the method of an older
token. Start a new attempt if your application requires pow_visual. Old outcomes recover action from the linked
challenge; outcomes with deleted challenges keep NULL action and do not contribute
to action-specific account risk. Configure new policies after upgrading.

Used context identifiers are retained in `used_contexts` after challenge cleanup,
so an expired operation cannot be recreated with the same context. This compact
ledger grows with operations and has no automatic pruning. Existing contexts
already deleted before upgrading cannot be recovered. Generate random contexts.
Do not downgrade the binary against this schema; restore a pre-upgrade backup.

## Visual challenge locale

Pass the application language as an option:

```js
await verifyWebsiteAttempt(element, attempt, {serviceURL, locale: 'ru-RU'});
```

Supported languages are `en` and `ru`, including regional variants. Without an
explicit locale, selection uses the closest element/ancestor `lang`, page language,
then the first supported browser preference (`navigator.languages`, `navigator.language`).
An unsupported explicit or page language falls back to English. The selected language
is kept throughout retries. `verifyVisual` accepts the same locale option.
The visual instruction, labels, confirmation, loading, retry and error messages are
translated. The server instruction key `match_reference` selects the translation;
the legacy standard English instruction is also recognized. Custom server instructions
are preserved verbatim and must be translated by their author.

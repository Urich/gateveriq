# Gateveriq

[English](README.md) | [Русский](README.ru.md)

Self-hosted experimental anti-bot verification for websites and independent applications.

Gateveriq combines SHA-256 proof of work, optional browser/behavior checks,
persistent attempt history, account-bound login attempts and visual additional
verification. Integrate a browser widget and a server API; no hosted verification
provider is required at runtime.

**Experimental MVP, not proof of human presence.** Client signals can be forged and
the small illustration bank can be memorized by bots. See [security](docs/security.md).

## Requirements

- Stable Rust and Cargo (edition 2021), with a C compiler for bundled SQLite.
- A modern browser supporting modules, Workers, Web Crypto and BigInt.
- HTTPS or localhost for browser testing.
- Python 3 for the demo/HTTP tests; Node.js for JS tests and the optional backend SDK.

## Start the website demo

From the repository root, initialize once:

```powershell
cargo build --locked
cargo run --locked -- init
cargo run --locked -- integration-create --name visual-demo --actions demo --origins http://127.0.0.1:8090
cargo run --locked -- serve
```

Skip `init` if configuration exists. Save the ID and secret printed by the create
command. Entering their JSON into the running server console does not configure
a client. In a second PowerShell terminal:

```powershell
$env:GATEVERIQ_ID = "ID_FROM_COMMAND"
$env:GATEVERIQ_SECRET = "SECRET_FROM_COMMAND"
$env:GATEVERIQ_WEBSITE = "1"
python examples/demo.py
```

Open **http://127.0.0.1:8090**, use **Run verification**, then **Confirm verification**.
The public API is on `127.0.0.1:9080`; the internal API is on `127.0.0.1:9081`.
This demo is a local test harness, not a production login backend.

To see illustrations, check **Simulate a failed password** and complete five
verifications. The next attempt shows images. For a shorter test, set
`account_failure_limit = 1` under the single `[website]` section of `config.toml`
and restart the Rust server. Wrong image answers change the object and board
without resetting the three-answer budget or deadline. After upgrades, restart
the server and reload the browser with Ctrl+F5.

For Linux/macOS environment syntax and the original demo, see [testing](docs/testing.md).

## Integrate a real website

```sh
cargo run --locked -- integration-create --name website --actions login --origins https://your-site.example
```

The backend creates an account-bound attempt. The browser submits its proof directly
to Gateveriq; the widget shows images when risk requires an extra check. The backend
consumes the token before checking credentials, reports the actual login outcome
and executes its operation once. Keep the integration secret on the backend.

Serve `web/widget.js` and `web/worker.js` on your website. The optional Node.js
backend client is `sdk/server.mjs`. [Complete examples](docs/website-integration.md).

## Modes and history

| Option | Behavior |
|---|---|
| No screening flags | Original PoW-only API |
| `--browser-check` | Requires browser observations and checks automation indicators |
| `--behavior-check` | Also requires a bounded interaction report tied to the challenge |
| `--history-check` | Persistent frequency/failure rules per integration and source IP |
| `--origins https://site.example` | Website mode: all three checks, origins, account binding and outcomes |

Defaults: 20 submissions per UTC minute; five verification failures in ten minute
buckets trigger a history rejection; five reported login failures per account in
the retained history trigger extra verification. History lasts 24 hours by default.
Configure thresholds under `[website]`. Successes do not erase failures.
Forwarded-IP headers are ignored.

The local visual bank contains 24 illustrations in six categories. Boards have 16
distinct pictures, up to three wrong answers and a maximum 120-second lifetime.
The bank is a style prototype, not a production CAPTCHA.

## Documentation

- [Documentation index](docs/README.md) / [Русская документация](docs/ru/README.md)
- [Architecture](docs/architecture.md)
- [Protocol and errors](docs/protocol.md)
- [General integration](docs/integration-guide.md)
- [Website login and visual demo](docs/website-integration.md)
- [Deployment and configuration](docs/deployment.md)
- [Security and limitations](docs/security.md)
- [Testing and troubleshooting](docs/testing.md)
- [Illustrations and generation prompts](assets/visual/README.md)

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
node tests/worker.mjs
node tests/behavior.mjs
node tests/visual.mjs
node tests/sdk.mjs
cargo build --locked
python tests/e2e.py
```

Use `python3` where needed. On Windows stop the debug server before rebuilding it,
or use a separate target directory as described in testing. No release publishing
workflow or open-source license is currently configured.

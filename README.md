# Gateveriq

Self-hosted proof-of-work request verification for independent applications.

Gateveriq issues short-lived SHA-256 challenges and single-use verification tokens. It is application-neutral: registration, email delivery, account policies and business rate limits belong to the integrating application.

**Status: experimental MVP for local testing.** Proof of work raises the cost of automation; it does not prove that a user is human. There are no image puzzles, fingerprinting, behavioral scoring or third-party tracking.

## Requirements

- Stable Rust with Cargo (edition 2021); C compiler for bundled SQLite.
- Linux: `build-essential` on Debian/Ubuntu, or GCC on Fedora.
- A modern browser with Web Workers, Web Crypto and BigInt, served over HTTPS or localhost.

## Quick start

```sh
git clone https://github.com/Urich/gateveriq.git
cd gateveriq
git checkout prototype
cargo build --locked
cargo run --locked -- init
cargo run --locked -- integration-create --name example --actions register,comment
cargo run --locked -- serve
```

Save the generated integration ID and secret. The secret is printed once; never put it in browser code. `init` creates a private configuration and token key without overwriting existing files. Keep the database and token key together across restarts. On Windows, restrict their ACL to the service account yourself.

Public API: `http://127.0.0.1:9080`. Internal API: `http://127.0.0.1:9081`.

## First verification

Replace the placeholders with the output of `integration-create`:

```sh
export GATEVERIQ_ID='YOUR_INTEGRATION_ID'
export GATEVERIQ_SECRET='YOUR_INTEGRATION_SECRET'
curl --fail-with-body http://127.0.0.1:9081/internal/v1/challenges \
  -H "Authorization: Bearer $GATEVERIQ_SECRET" \
  -H 'Content-Type: application/json' \
  -d "{\"integration_id\":\"$GATEVERIQ_ID\",\"action\":\"register\",\"context_id\":\"local-test-1\"}" \
  -o challenge.json
cargo run --locked -- solve --challenge challenge.json \
  --submit-url http://127.0.0.1:9080/public/v1/solutions
```

Copy `verification_token` from the response:

```sh
export GATEVERIQ_TOKEN='RETURNED_TOKEN'
curl --fail-with-body http://127.0.0.1:9081/internal/v1/verifications/consume \
  -H "Authorization: Bearer $GATEVERIQ_SECRET" \
  -H 'Content-Type: application/json' \
  -d "{\"integration_id\":\"$GATEVERIQ_ID\",\"verification_token\":\"$GATEVERIQ_TOKEN\",\"action\":\"register\",\"context_id\":\"local-test-1\",\"request_id\":\"consume-1\"}"
```

Repeating the consume request returns the same success. Changing `request_id` fails. Use a **new context** for the next test.

For a clickable browser test, see [Interactive browser test](docs/testing.md#interactive-browser-test).

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
node tests/worker.mjs
cargo build --locked
python3 tests/e2e.py
```

No release publishing or artifact upload workflow is configured. Build and run locally.

## Documentation

- [Architecture](docs/architecture.md)
- [Protocol](docs/protocol.md)
- [Integration guide and browser widget](docs/integration-guide.md)
- [Deployment and configuration](docs/deployment.md)
- [Security model and limitations](docs/security.md)
- [Testing guide](docs/testing.md)

Licensing has not yet been selected. This repository does not currently grant an open-source license.

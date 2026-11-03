# Testing

[English](testing.md) | [Русский](ru/testing.md)

## Automated checks

Run from the repository root:

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

Use `python3` where needed. Rust tests cover tokens, isolation, bindings, concurrent
requests, persistent history, outcomes, origins, image assets/rotation, expiry and
answer exhaustion. JavaScript tests cover the Worker, observations, image selection,
cancellation/retries and SDK. HTTP/CLI tests use real ports, restart the service,
decode PNGs and complete a login after an incorrect visual answer.
These checks validate rules, not real-world bot detection quality.

### Windows executable locking

Stop a debug server with Ctrl+C before rebuilding its executable. Alternatively:

```powershell
cargo test --locked --target-dir target/check
cargo build --locked --target-dir target/check
$env:GATEVERIQ_BINARY = (Resolve-Path 'target/check/debug/gateveriq.exe').Path
python tests/e2e.py
```

The override must be absolute. On Linux/macOS:

```sh
cargo build --locked --target-dir target/check
GATEVERIQ_BINARY="$PWD/target/check/debug/gateveriq" python3 tests/e2e.py
```

## Interactive browser test

For website mode follow the [visual demo](website-integration.md#try-visual-verification-locally).
For the original PoW-only demo, with the Rust service running:

```powershell
cargo run --locked -- integration-create --name browser-test --actions demo
$env:GATEVERIQ_ID = "RETURNED_ID"
$env:GATEVERIQ_SECRET = "RETURNED_SECRET"
$env:GATEVERIQ_WEBSITE = "0"
python examples/demo.py
```

Linux/macOS environment syntax:

```sh
export GATEVERIQ_ID='RETURNED_ID'
export GATEVERIQ_SECRET='RETURNED_SECRET'
export GATEVERIQ_WEBSITE='0'
python3 examples/demo.py
```

Open **http://127.0.0.1:8090**, use Run verification and Confirm verification,
then test cancellation/retry. Demo operations are in memory, ports are fixed and
there is no production authentication/session implementation.

## Manual assertions

1. Repeated preparation returns the same unexpired challenge for a context.
2. Invalid PoW is rejected; valid retries return the same unconsumed token.
3. Changed action/context is rejected during consumption.
4. Exact consumption retries succeed; a different request ID fails.
5. Tokens and history survive restarts.
6. Wrong/missing website origins and legacy-route bypasses are rejected.
7. Outcome reporting requires consumption; exact retries do not increase failures.
8. Wrong image answers change category/task ID, preserve the deadline and spend
   the common budget. Old task IDs cannot answer replacement boards.
9. Expired/exhausted visual tasks cannot issue tokens; cancellation stops UI work.
10. Test actual desktop/mobile browsers and keyboard selection separately.

## Troubleshooting

| Symptom | Check |
|---|---|
| ACTION_NOT_ALLOWED | Allow the client's action; the demo uses `demo` |
| Missing GATEVERIQ_ID/SECRET | Set variables in the terminal launching Python |
| ORIGIN_NOT_ALLOWED | Match scheme/host/port exactly; `localhost` differs from `127.0.0.1` |
| No image task | Enable website mode and report failed logins, or lower the account threshold for testing |
| Old images/UI | Restart the rebuilt Rust server, reload with Ctrl+F5 and start a new attempt |
| 429 | Check frequency/history limits or visual answer budget |
| Access denied compiling | Stop the executable or use another target directory |

Automated tests use low difficulty. Benchmark browser solving on target phones
and release native solving before choosing difficulty. Test artifacts are not uploaded.

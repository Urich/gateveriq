# Testing

Run `cargo test --locked`, then `node tests/worker.mjs`. Build the CLI with `cargo build --locked` and run `python3 tests/e2e.py` for a real HTTP/CLI cycle with service restart. Rust tests cover issuance retries, proof submission retries, context binding, integration isolation, expiry, database reopening, malformed input and concurrent consumption. Route tests cover separation of internal/public paths and invalid oversized submissions. The Node test runs the actual browser worker with Web Crypto against an independently computed hash.

For manual end-to-end testing follow the README. Also verify:

1. Repeat issuance for the same operation: same challenge ID.
2. Submit a wrong nonce: INVALID_SOLUTION.
3. Submit a valid solution twice: same token and expiry.
4. Change action/context when consuming: rejection.
5. Consume twice with one request ID: success both times.
6. Consume with a second request ID: TOKEN_ALREADY_CONSUMED.
7. Restart service between solution and consumption: token remains usable.
8. Disable integration: subsequent internal requests are unauthorized.
9. Lower the rate limit, restart and send requests until 429.
10. Load the widget in a real browser, cancel it, retry, and inspect console/network errors.

Automated tests use low difficulty for speed. Before deploying, benchmark release-mode native solving and the browser worker on your target devices at configured difficulty. Do not tune browser difficulty solely from native timings.

No binaries or release artifacts are uploaded by this project. GitHub Actions, if enabled by the owner, only validates source.

## Interactive browser test

With Gateveriq running, create a separate integration allowing `demo`:

```sh
cargo run --locked -- integration-create --name browser-test --actions demo
export GATEVERIQ_ID='RETURNED_ID'
export GATEVERIQ_SECRET='RETURNED_SECRET'
python3 examples/demo.py
```

Open **http://127.0.0.1:8090** and click **Run verification**. The demo backend holds the credential and operation binding; the browser receives neither the secret nor permission to issue arbitrary challenges. Test cancellation and retry too. This example has in-memory operation storage, fixed ports and no user authentication; it is strictly a local test harness, not a production integration.

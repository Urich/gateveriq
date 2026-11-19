# Deployment and configuration

For Docker, Ubuntu 24.04 packages and systemd installation, see [releases](releases.md).

[English](deployment.md) | [Русский](ru/deployment.md)

## Initialization

Follow the [quick start](../README.md). Defaults bind both listeners to localhost.
Use a dedicated unprivileged account. On Unix set `umask 077`; on Windows restrict
ACLs on the configuration, key, database directory and SQLite sidecar files.

`init` creates absolute paths and never overwrites existing configuration/key files.
Specify a custom configuration path before the subcommand:

```sh
gateveriq --config /etc/gateveriq/config.toml serve
```

## Configuration

| Root key | Default | Meaning/range |
|---|---|---|
| database | Absolute path from init | SQLite database |
| token_key_file | Absolute path from init | Canonical unpadded Base64URL key, 32 bytes |
| public_listen | 127.0.0.1:9080 | Public API and assets |
| internal_listen | 127.0.0.1:9081 | Internal API; loopback natively, private container network in Docker |
| difficulty_bits | 18 | PoW difficulty, 1-24 |
| challenge_ttl_seconds | 120 | Challenge lifetime, 10-600 seconds |
| token_ttl_seconds | 60 | Token lifetime, 10-300 seconds |
| retention_seconds | 300 | Receipt retry/cleanup retention, 300-86400 seconds |
| requests_per_minute | 120 | Positive in-memory fixed-window bucket limit |
| max_active_challenges | 1000 | Positive per-integration unexpired challenge limit |

`GATEVERIQ_INTERNAL_LISTEN` (or `--internal-listen ADDRESS` before the subcommand)
overrides `internal_listen` at runtime. The Alpine Docker image sets it to
`0.0.0.0:9081` so backends in the same container network can connect; Compose does
not publish this port on the host. See [Docker deployment](releases.md#docker).

Optional website configuration, with defaults:

```toml
[website]
history_retention_seconds = 86400
attempts_per_minute = 20
failure_limit = 5
account_failure_limit = 5
reset_account_failures_on_slider_success = false
```

History retention accepts 600–86400 seconds, and numeric limits accept 1–10000.
The boolean `reset_account_failures_on_slider_success` enables resetting the
account login-failure count after successful slider and PoW verification.
IP failures use ten UTC minute buckets. Account failures use the full
retention window. The attempt limit applies per UTC minute. These settings also
control the thresholds for legacy `--history-check` integrations.

Configuration is loaded at startup; changes require restart. Existing challenge
difficulty/expiry do not change. The key file allows an optional trailing newline.
History retention is separate from receipt retention. Cleanup runs in batches;
physical removal can lag while expired history is already excluded from decisions.

## HTTPS and reverse proxies

For an HTTPS website expose the public listener through HTTPS, for example on a
dedicated hostname. The website helper constructs root-relative public API paths;
use a service origin rather than a URL subdirectory.

```caddyfile
verify.example.com {
    reverse_proxy 127.0.0.1:9080
}
```

Never publish the internal listener. Serve widget/worker files on the integrating
website origin, as shown in [website integration](website-integration.md).
The service does not serve the demo application at `/`.

Forwarded-IP headers are ignored. Behind a proxy, clients share the proxy peer IP
for service limits/history. Apply client limits at trusted ingress; trusted
forwarded-address support is not implemented. Configure ingress connection/body
limits and resource controls independently of application-level JSON limits.

## Integration administration

```sh
gateveriq integration-create --name forum --actions signup,comment
gateveriq integration-create --name screened --actions login --browser-check --behavior-check --history-check
gateveriq integration-create --name website --actions login --origins https://site.example
gateveriq integration-rotate --id INTEGRATION_ID --grace-seconds 300
gateveriq integration-disable --id INTEGRATION_ID
gateveriq cleanup
```

`--actions` and `--origins` accept comma-separated lists. Origins are exact canonical
HTTPS origins without paths or trailing slashes; HTTP is allowed only for loopback
development. `--origins` enables website mode and all screening policies.

Creation and rotation print secrets once. Rotation accepts 0-3600 grace seconds,
with one previous secret; another rotation replaces it. Disabled integrations
cannot issue/consume internally. Re-enabling, editing actions/origins and updating
existing integration policy are not exposed by this CLI. Create a new integration
when a different configuration is needed.

## Backups, upgrades and operations

Use SQLite's backup API or stop the service before copying it. Back up the token
key with the database. Copying only the database while WAL is active is unsafe.
Startup initializes a fresh schema; migrations from older schemas are not supported.
Use a fresh database for this protocol revision. Do not share databases between versions.

Token-key rotation is not implemented. Replacing it changes token reconstruction
and history pseudonyms. For emergency rotation stop issuance/service, expire or
purge outstanding challenges, securely replace the key and require new operations.

`GET /health` returns `ok` for liveness, not database readiness. Cleanup failures
are written to stderr. Metrics, audit dashboards, graceful draining, clustering
remain outside this prototype. Keep clocks synchronized and monitor disk/CPU usage.
Version tags trigger the tested Ubuntu/Docker release workflow.

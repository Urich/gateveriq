# Docker and Ubuntu releases

## Docker

Images are published to `ghcr.io/urich/gateveriq:v<VERSION>` after checks succeed
on pushes to `main` and versioned release tags. A push to `main` updates the image
for the version in `Cargo.toml` and also publishes `sha-<COMMIT>` for an immutable
reference to that build. The image is built and runs on Alpine 3.23 (musl) as UID/GID 10001.
Docker builds require BuildKit; Cargo downloads and build artifacts are cached.
The native Ubuntu archive remains a separate glibc build.
After the first publication, ensure the package visibility in GitHub Packages is
**Public**, so users can pull without signing in.

From a checkout of the matching release:

```sh
docker compose pull
docker compose up -d --wait
curl --fail http://127.0.0.1:9080/health
```

To build from source instead:

```sh
docker compose up --build -d --wait
```

The public API is published on host loopback port 9080; put an HTTPS reverse proxy
in front of it for external access. There is no demo website at `/`.
Configuration, SQLite and the token key persist in the `data` named volume.
`docker compose down` preserves it; `docker compose down -v` deletes it.
Back up the volume with the service stopped. A bind-mounted `/data` directory
must be writable by UID/GID 10001 and should have mode 0700.

Administer integrations using the running container:

```sh
docker compose exec gateveriq gateveriq --config /data/config.toml integration-create \
  --name website --actions login --origins https://your-site.example
```

Save the printed secret on your backend. Inside Docker the internal API listens on
`0.0.0.0:9081` and is reachable by other containers on the same Compose network at
`http://gateveriq:9081`. Port 9081 is **not published on the host**; internal routes
still require the integration secret. For example, add your backend to this Compose
project (adapt the image and environment variable to your application):

```yaml
services:
  backend:
    image: your-backend-image
    environment:
      GATEVERIQ_INTERNAL_URL: http://gateveriq:9081
    depends_on:
      gateveriq:
        condition: service_healthy
```

Containers from another Compose project must join this project's network
(`<project>_default`) to use the same address. A reverse proxy in that network can
reach the public API at `http://gateveriq:9080`; proxy only the public port.

The image sets `GATEVERIQ_INTERNAL_LISTEN=0.0.0.0:9081`, overriding the config file
without rewriting it, including on existing volumes. Override that environment
variable to change the internal bind address. Native installations still default
to loopback. The CLI also accepts `--internal-listen ADDRESS` before the subcommand.

To change configuration, copy `/data/config.toml` out with `docker compose cp`,
edit it, copy it back, then run `docker compose restart`. Keep the public listener
on `0.0.0.0:9080` inside the container.
Existing configuration and keys are never regenerated on restart.

SIGTERM and Ctrl+C stop both listeners and allow up to 25 seconds for active
requests to finish; Compose allows 30 seconds before forcing termination.

Run the isolated container smoke test (requires Docker Compose, BuildKit, Python 3
and curl):

```sh
bash tests/docker.sh
```

It checks Alpine startup, authenticated access from a neighboring container,
rejection without credentials, clean SIGTERM exit, and persistence after restart.
Its temporary Compose project and data volume are removed afterwards.

## Ubuntu 24.04 amd64 without Docker

Download the `.tar.gz` and its `.sha256` file from GitHub Releases, then:

```sh
sha256sum -c gateveriq-0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf gateveriq-0.2.0-x86_64-unknown-linux-gnu.tar.gz
cd gateveriq-0.2.0-x86_64-unknown-linux-gnu
sudo bash scripts/install.sh
curl --fail http://127.0.0.1:9080/health
journalctl -u gateveriq -f
```

The installer creates a dedicated account and systemd service. It keeps config,
key and database in `/var/lib/gateveriq`, with both APIs on loopback by default.
For this release, back up existing data while stopped and use a fresh database.
v0.1.0 database migrations are not supported. Create new integrations against the
fresh database before connecting applications. Then run the release installer.
Existing config and keys are preserved. Updating from a development checkout does
not automatically migrate its database; move existing data separately with the
service stopped and update paths in the configuration.

## Build and publish

On Ubuntu 24.04 install Rust 1.93.0, `build-essential`, `ca-certificates`, `curl`
and `python3`, then run:

```sh
bash scripts/build-release.sh
```

The archive and checksum are written to `dist/`. Runtime assets are compiled into
the binary; widget and SDK sources are also included in the archive.

The release workflow supports a manual run for testing without publication.
To publish, update the Cargo package/lockfile version and default image version in
Compose, add `docs/releases/v<VERSION>.md` with release notes, commit, then push a matching tag (for example `v0.2.0`). The workflow
tests Rust, the release binary, Docker persistence and systemd installation before
publishing the versioned GHCR image and GitHub Release assets. It uses the
repository's `GITHUB_TOKEN` with `contents: write` and `packages: write`.
Only amd64 is currently built by CI. The native release binary is built on Ubuntu 24.04; older distributions are not
a supported native binary target. The Docker image uses Alpine independently.

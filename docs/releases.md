# Docker and Ubuntu releases

## Docker

Release images are published to `ghcr.io/urich/gateveriq:v<VERSION>` after the
release checks succeed. The image runs on Ubuntu 24.04 as UID/GID 10001.
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

Save the printed secret on your backend. The internal API listens on
`127.0.0.1:9081` **inside the container** and is not published. A backend container
can use Compose `network_mode: "service:gateveriq"` to share that network namespace
and access it at `http://127.0.0.1:9081`. A host backend should use the native
systemd installation below. Ordinary Docker bridge networking does not provide
access to the internal listener.

To change configuration, copy `/data/config.toml` out with `docker compose cp`,
edit it, copy it back, then run `docker compose restart`. Keep the internal listener
on loopback and the public listener on `0.0.0.0:9080` inside the container.
Existing configuration and keys are never regenerated on restart.

## Ubuntu 24.04 amd64 without Docker

Download the `.tar.gz` and its `.sha256` file from GitHub Releases, then:

```sh
sha256sum -c gateveriq-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf gateveriq-0.1.0-x86_64-unknown-linux-gnu.tar.gz
cd gateveriq-0.1.0-x86_64-unknown-linux-gnu
sudo bash scripts/install.sh
curl --fail http://127.0.0.1:9080/health
journalctl -u gateveriq -f
```

The installer creates a dedicated account and systemd service. It keeps config,
key and database in `/var/lib/gateveriq`, with both APIs on loopback by default.
For upgrades, back up the data while stopped, then run the new release's installer.
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
Compose, commit, then push a matching tag (for example `v0.1.0`). The workflow
tests Rust, the release binary, Docker persistence and systemd installation before
publishing the versioned GHCR image and GitHub Release assets. It uses the
repository's `GITHUB_TOKEN` with `contents: write` and `packages: write`.
Only amd64 is currently built by CI. The runner and final Docker image both use
Ubuntu 24.04; older distributions are not a supported binary target.

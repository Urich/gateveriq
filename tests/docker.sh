#!/usr/bin/env bash
# Isolated Docker lifecycle and bridge-network smoke test. Requires Compose v2.
set -euo pipefail
cd "$(dirname "$0")/.."
export COMPOSE_PROJECT_NAME="gateveriq-smoke-$$"
export GATEVERIQ_IMAGE="${GATEVERIQ_IMAGE:-gateveriq:smoke}"
test_dir=$(mktemp -d)
cleanup() {
    docker compose down -v --remove-orphans
    rm -rf "$test_dir"
}
trap cleanup EXIT

docker compose up --build -d --wait --wait-timeout 120
container=$(docker compose ps -q gateveriq)
test "$(docker compose exec -T gateveriq id -u)" = 10001
docker compose exec -T gateveriq test -f /etc/alpine-release
docker compose exec -T gateveriq curl --fail --silent http://127.0.0.1:9080/health
# Only the public listener may be published to the host.
docker inspect "$container" | python3 -c '
import json, sys
ports = json.load(sys.stdin)[0]["HostConfig"]["PortBindings"]
assert not ports.get("9081/tcp"), ports
'
docker compose exec -T gateveriq gateveriq --config /data/config.toml integration-create \
    --name smoke --actions test > "$test_dir/integration.json"
id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["integration_id"])' "$test_dir/integration.json")
secret=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["integration_secret"])' "$test_dir/integration.json")
body="{\"integration_id\":\"$id\",\"action\":\"test\"}"
# A separate container must reach the internal API over the normal bridge network.
# Supply the secret via stdin rather than a Docker command-line argument.
status=$(printf 'header = "Authorization: Bearer %s"\n' "$secret" | \
    docker compose run --rm --no-deps -T --entrypoint curl gateveriq \
    --config - --fail --silent --show-error --output /dev/null --write-out '%{http_code}' \
    --header 'Content-Type: application/json' --data "$body" \
    http://gateveriq:9081/internal/v1/contexts)
test "$status" = 200
status=$(docker compose run --rm --no-deps -T --entrypoint curl gateveriq \
    --silent --show-error --output /dev/null --write-out '%{http_code}' \
    --header 'Content-Type: application/json' --data "$body" \
    http://gateveriq:9081/internal/v1/contexts)
test "$status" = 401

docker compose exec -T gateveriq sha256sum /data/token.key /data/config.toml > "$test_dir/before"
docker compose stop
# SIGTERM must produce a clean exit, rather than signal termination or SIGKILL.
test "$(docker inspect --format '{{.State.ExitCode}}' "$container")" = 0
docker compose down
docker compose up -d --wait --wait-timeout 120
docker compose exec -T gateveriq sha256sum /data/token.key /data/config.toml > "$test_dir/after"
diff "$test_dir/before" "$test_dir/after"
docker compose exec -T gateveriq gateveriq --config /data/config.toml integration-disable --id "$id"
echo 'Alpine, bridge networking, authentication, SIGTERM and persistence checks passed'

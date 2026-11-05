#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
target=${TARGET:-x86_64-unknown-linux-gnu}
case "$target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;; *) echo 'Unsupported Linux target' >&2; exit 1;; esac
cargo build --locked --release --target "$target"
target_dir=$(cargo metadata --no-deps --format-version 1 --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
name="gateveriq-${version}-${target}"
mkdir -p dist
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name/scripts" "$stage/$name/packaging"
install -m 755 "$target_dir/$target/release/gateveriq" "$stage/$name/gateveriq"
install -m 755 scripts/install.sh "$stage/$name/scripts/install.sh"
cp packaging/gateveriq.service "$stage/$name/packaging/"
cp README.md README.ru.md "$stage/$name/"
cp -r docs web sdk "$stage/$name/"
tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "Created dist/$name.tar.gz"

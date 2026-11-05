#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID != 0 ]]; then echo 'Run with sudo: sudo bash scripts/install.sh [binary]' >&2; exit 1; fi
root=$(cd "$(dirname "$0")/.." && pwd)
binary=${1:-$root/gateveriq}
[[ -f "$binary" ]] || { echo "Binary missing: $binary" >&2; exit 1; }
command -v systemctl >/dev/null || { echo 'systemd is required' >&2; exit 1; }
getent group gateveriq >/dev/null || groupadd --system gateveriq
id gateveriq >/dev/null 2>&1 || useradd --system --gid gateveriq --home-dir /var/lib/gateveriq --shell /usr/sbin/nologin gateveriq
install -d -o gateveriq -g gateveriq -m 700 /var/lib/gateveriq
# Stop before replacing the executable and restart only after installation succeeds.
if systemctl is-active --quiet gateveriq; then systemctl stop gateveriq; fi
install -m 755 "$binary" /usr/local/bin/gateveriq.new
mv -f /usr/local/bin/gateveriq.new /usr/local/bin/gateveriq
if [[ ! -e /var/lib/gateveriq/config.toml ]]; then
    runuser -u gateveriq -- /usr/local/bin/gateveriq --config /var/lib/gateveriq/config.toml init
fi
install -m 644 "$root/packaging/gateveriq.service" /etc/systemd/system/gateveriq.service
systemctl daemon-reload
systemctl enable --now gateveriq
echo 'Installed. Configuration and persistent data: /var/lib/gateveriq'
echo 'Logs: journalctl -u gateveriq -f'

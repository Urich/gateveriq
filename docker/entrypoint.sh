#!/bin/sh
set -eu
umask 077
if [ ! -f /data/config.toml ]; then
    gateveriq --config /data/config.toml init
    sed -i 's/public_listen = "127.0.0.1:9080"/public_listen = "0.0.0.0:9080"/' /data/config.toml
fi
exec gateveriq --config /data/config.toml "$@"

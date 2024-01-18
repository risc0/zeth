#!/usr/bin/env bash
set -ex

/restart_aesm.sh

if [[ $# -eq 1 && $1 == "--init" ]]; then
    cd /opt/raiko/guests/sgx
    gramine-sgx ./raiko-guest bootstrap
else
    /opt/raiko/bin/raiko-host "$@"
fi

#!/usr/bin/env bash
set -ex

RAIKO_CONFIG_DIR_PATH="/root/.config/raiko/config"
RAIKO_INPUT_MANIFEST_FILENAME="raiko-guest.manifest"
RAIKO_OUTPUT_MANIFEST_FILENAME="raiko-guest.manifest.sgx"
RAIKO_SIGNED_MANIFEST_FILENAME="raiko-guest.sig"
SGX_DIR_PATH="/opt/raiko/guests/sgx"

/restart_aesm.sh

if [[ $# -eq 1 && $1 == "--init" ]]; then
    cd "$SGX_DIR_PATH"
    gramine-sgx-gen-private-key
    gramine-sgx-sign --manifest "$RAIKO_INPUT_MANIFEST_FILENAME" --output "$RAIKO_OUTPUT_MANIFEST_FILENAME"
    cp "$RAIKO_OUTPUT_MANIFEST_FILENAME" "$RAIKO_SIGNED_MANIFEST_FILENAME" "$RAIKO_CONFIG_DIR_PATH"
    gramine-sgx ./raiko-guest bootstrap
else
    ln -sf "$RAIKO_CONFIG_DIR_PATH/$RAIKO_OUTPUT_MANIFEST_FILENAME" "$SGX_DIR_PATH"
    ln -sf "$RAIKO_CONFIG_DIR_PATH/$RAIKO_SIGNED_MANIFEST_FILENAME" "$SGX_DIR_PATH"
    /opt/raiko/bin/raiko-host "$@"
fi

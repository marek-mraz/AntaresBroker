#!/usr/bin/env bash
# Fetch the wasm toolchain — wasm-bindgen-cli EXACTLY matching the
# Cargo.lock wasm-bindgen version (the CLI refuses a mismatch at bindgen
# time), plus binaryen's wasm-opt. Prebuilt binaries into ~/.local/bin.
# Used by CI and this dev box identically (one-pipeline rule).
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="${BIN:-$HOME/.local/bin}"
mkdir -p "$BIN"
# A private unpack directory, not a name an attacker can predict: these
# archives are unpacked and the binaries inside are then copied onto PATH and
# run by the build, so a symlink waiting at a fixed path would decide which
# binary that is.
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
ARCH=$(uname -m)
case "$ARCH" in
  aarch64|arm64) WBG_ARCH=aarch64-unknown-linux-gnu; BINARYEN_ARCH=aarch64-linux ;;
  x86_64)        WBG_ARCH=x86_64-unknown-linux-musl; BINARYEN_ARCH=x86_64-linux ;;
  *) echo "unsupported arch $ARCH" >&2; exit 1 ;;
esac

WBG_VER=$(grep -A2 '^name = "wasm-bindgen"$' Cargo.lock | grep version | head -1 | cut -d'"' -f2)
if ! "$BIN/wasm-bindgen" --version 2>/dev/null | grep -q "$WBG_VER"; then
  curl -sSfL "https://github.com/rustwasm/wasm-bindgen/releases/download/${WBG_VER}/wasm-bindgen-${WBG_VER}-${WBG_ARCH}.tar.gz" \
    | tar xz -C "$TMP"
  cp "$TMP/wasm-bindgen-${WBG_VER}-${WBG_ARCH}/wasm-bindgen" "$BIN/"
fi

BINARYEN_VER=123
if ! "$BIN/wasm-opt" --version 2>/dev/null | grep -q "version ${BINARYEN_VER}"; then
  curl -sSfL "https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VER}/binaryen-version_${BINARYEN_VER}-${BINARYEN_ARCH}.tar.gz" \
    | tar xz -C "$TMP"
  cp "$TMP/binaryen-version_${BINARYEN_VER}/bin/wasm-opt" "$BIN/"
fi

rustup target add wasm32-unknown-unknown
"$BIN/wasm-bindgen" --version && "$BIN/wasm-opt" --version

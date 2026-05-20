#!/usr/bin/env bash
# Cross-compile a static musl x86_64 binary suitable for scp to any Linux host.
# Requires: zig, cargo-zigbuild, and the x86_64-unknown-linux-musl rustup target.

set -euo pipefail
cd "$(dirname "$0")"

TARGET=x86_64-unknown-linux-musl
cargo zigbuild --release --target "$TARGET"

BIN="target/$TARGET/release/fleetbench"
ls -lh "$BIN"
file "$BIN"
echo
echo "scp this binary to a target host and run ./smoke.sh --binary fleetbench"

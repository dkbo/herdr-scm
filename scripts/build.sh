#!/bin/sh
# build.sh — the herdr [[build]] step for herdr-scm.
#
# v1 builds from source: no prebuilt download and no network. `~/.cargo/env` is sourced first so
# cargo is found even when herdr was launched without ~/.cargo/bin on PATH (a GUI or login-less
# launch); the `[ -f ]` guard means a missing env file cannot abort the build.
set -eu

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

if ! command -v cargo >/dev/null 2>&1; then
  echo "herdr-scm needs Rust to build, but cargo was not found. Install it from https://rustup.rs, then re-run the install." >&2
  exit 1
fi

exec cargo build --release

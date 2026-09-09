#!/usr/bin/env bash
# Idempotent launcher for the SCM panel — "launch-or-focus, toggle on repeat", scoped to the
# current tab:
#   - no SCM pane in this tab      -> open a split (focused)
#   - an SCM pane exists, unfocused -> focus it
#   - the focused pane IS the panel -> close it (herdr has no hide-without-close, and reopening
#                                      just re-walks the tree — cheap)
#
# The decision is computed in-process by the binary itself (`--launch-decision`, fed the
# `pane list` JSON on stdin), so it is unit-tested and the pane id it returns is already
# validated flag-safe. Any failure degrades to OPEN.
#
# herdr has no focus-by-id, so a focus is a `zoom <id> --on/--off` cycle: `--on` focuses (and
# maximizes), `--off` un-maximizes while keeping focus.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
scm_bin="$script_dir/../target/release/herdr-scm"

open_pane() {
  exec "$herdr_bin" plugin pane open \
    --plugin herdr-scm \
    --entrypoint scm \
    --placement split \
    --direction right \
    --focus
}

decision="OPEN"
if [ -x "$scm_bin" ]; then
  # NOTE: `herdr pane list` takes no `--json` flag on 0.9.0 — it already emits JSON.
  panes="$("$herdr_bin" pane list 2>/dev/null || true)"
  if [ -n "$panes" ]; then
    decision="$(printf '%s' "$panes" | "$scm_bin" --launch-decision 2>/dev/null || echo OPEN)"
  fi
fi

case "$decision" in
  "FOCUS "*)
    pid="${decision#FOCUS }"
    "$herdr_bin" pane zoom "$pid" --on >/dev/null 2>&1 || true
    exec "$herdr_bin" pane zoom "$pid" --off
    ;;
  "CLOSE "*)
    exec "$herdr_bin" pane close "${decision#CLOSE }"
    ;;
  *)
    open_pane
    ;;
esac

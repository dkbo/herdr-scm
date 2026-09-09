#!/usr/bin/env bash
# Idempotent launcher for the SCM panel in its own TAB — "open-or-switch, toggle on repeat",
# scoped across the tabs of the CURRENT WORKSPACE:
#   - no SCM pane in this workspace        -> open the panel in a new tab (focused)
#   - an SCM pane in another tab here      -> switch to that tab (no duplicate panel)
#   - an SCM pane in this tab, unfocused   -> focus it in place
#   - the focused pane IS the panel        -> close it (herdr auto-closes the emptied tab)
# A panel open in a DIFFERENT workspace is left alone and a fresh one opens here — the action
# reaches this workspace's panel, it never switches you across workspaces.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
scm_bin="$script_dir/../target/release/herdr-scm"

open_tab() {
  exec "$herdr_bin" plugin pane open \
    --plugin herdr-scm \
    --entrypoint scm \
    --placement tab \
    --focus
}

decision="OPEN"
if [ -x "$scm_bin" ]; then
  panes="$("$herdr_bin" pane list 2>/dev/null || true)"
  if [ -n "$panes" ]; then
    decision="$(printf '%s' "$panes" | "$scm_bin" --launch-decision-tab 2>/dev/null || echo OPEN)"
  fi
fi

case "$decision" in
  "SWITCHTAB "*)
    # If the target tab vanished between the snapshot and now, fall back to opening a fresh
    # tab rather than leaving the keypress a silent no-op. (No `exec`, so `||` can run.)
    "$herdr_bin" tab focus "${decision#SWITCHTAB }" || open_tab
    ;;
  "FOCUS "*)
    pid="${decision#FOCUS }"
    "$herdr_bin" pane zoom "$pid" --on >/dev/null 2>&1 || true
    exec "$herdr_bin" pane zoom "$pid" --off
    ;;
  "CLOSE "*)
    exec "$herdr_bin" pane close "${decision#CLOSE }"
    ;;
  *)
    open_tab
    ;;
esac

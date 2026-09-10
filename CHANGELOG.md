# Changelog

All notable changes to herdr-scm are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — 2026-09-10

Colour, and the four presentation defects that shipped with 0.1.0. Still read-only, still
Linux, still herdr ≥ 0.9.0; no new configuration.

### Added

- **A semantic palette for the tree half.** It uses your terminal's own sixteen ANSI colours
  rather than a palette of its own, so it follows whatever theme you have set, stays legible on
  a light background or over a 16-colour SSH session, and cannot drift out of step with the
  `delta`-coloured diff beside it. Green, red and yellow are file-change semantics; cyan and
  magenta are git refs and sync state; dark grey is structure and information that has gone to
  zero (`↑0 ↓0` recedes, because level with the upstream is nothing to act on); repo names and
  file paths take no colour at all. A merge conflict (`U`) is bold red. There is deliberately
  nothing to configure — change your terminal theme and both halves change with it.
- **A permanent status bar** along the bottom: whatever the last action reported on the left,
  and the cursor's position in the tree (`N/M`) on the right. Permanent rather than carved out
  on demand, so a notice appearing and disappearing no longer reflows the tree.
- **The diff pane is titled with the file it belongs to**, in both the side-by-side and the
  stacked layout. An over-wide path is cut from the left with a leading `…`, because the file
  name matters more than the directories above it.

### Fixed

- **`Tab` now visibly changes the focus.** The tree's focus cue was bold on a borderless block
  that the pane then drew straight over, so switching focus changed nothing on screen. The
  cursor row is now reversed while the tree has focus and merely underlined when it does not,
  and the diff's divider brightens reciprocally. Both are modifiers rather than colours, so a
  row keeps its own semantic colours while selected.
- **The tree cursor is no longer pinned to the bottom row.** Once the repo list outgrew the
  pane, the cursor sat permanently on the last visible line and nothing below it was ever
  drawn. It now keeps three rows of context from either edge, vim-style, and the window holds
  still while the cursor stays inside that margin.
- **A notice no longer costs a row of content.** It used to clear the pane's bottom line,
  drawing over whichever tree or diff row was already there; it now lives in the status bar.
  Panes too short to afford that row (fewer than five lines) keep the old overlay behaviour.
- **The help overlay is only as wide as its key list**, rather than stretched to nearly the
  full pane, which had pushed every key away from its own description.
- **Wide-character paths and notices no longer lose their tail.** The diff title and the status
  bar now measure display columns rather than characters, so a CJK path keeps its file name and
  its `…`, and the position indicator can no longer be pushed off the right edge.
- The README's mockups match what the panel actually draws.

[0.2.0]: https://github.com/dkbo/herdr-scm/releases/tag/v0.2.0

## [0.1.0] — 2026-09-09

First public release. Requires herdr ≥ 0.9.0; Linux only.

### Added

- A read-only source-control panel covering **every** git repo under the current herdr
  workspace at once: each repo's branch, ahead/behind counts, and changed files in one tree.
- A diff pane for the selected file, rendered through `delta` when it is installed and falling
  back to plain text when it is not. Large and binary diffs are capped and refused rather than
  wedging the renderer, and git's stderr is kept out of the pane.
- Gitignore-aware repo discovery with three-tier pruning (`scan_excludes`, gitignore rules,
  `scan_depth`), so a workspace full of `node_modules` and `target` stays cheap to scan.
- Responsive layout: tree and diff side by side at or above `split_threshold_cols`
  (default 120), stacked below it.
- A background poller that refreshes status on an interval (`poll_interval_secs`) and re-walks
  the filesystem for the repo list only every `rescan_every` rounds.
- Two herdr actions: `open-scm` (split beside the current pane, scoped to the tab) and
  `open-scm-tab` (its own tab, idempotent across the workspace's tabs — it switches to an
  existing panel instead of duplicating it, and toggles off when already focused).
- Keys for navigation, expand/collapse, cross-repo next/previous change (`]` / `[`), zoom,
  refresh, `repo:path` copy via OSC 52, and `$EDITOR` hand-off. Every action is rebindable
  under `[keys]` in `config.toml`.
- Read-only TOML configuration: a parse error degrades the whole file to the built-in defaults
  rather than failing to start. `config.example.toml` documents every setting at its default.
- CI on both `stable` and the declared MSRV (1.88), plus a weekly scheduled run so a `stable`
  regression surfaces here rather than in a user's install.

### Deliberately not included

No write operations of any kind (no stage, commit, push/pull or discard), no commit-graph or
log view, and no cross-workspace aggregation.

[0.1.0]: https://github.com/dkbo/herdr-scm/releases/tag/v0.1.0

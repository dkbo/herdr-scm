# Changelog

All notable changes to herdr-scm are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

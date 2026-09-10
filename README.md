# herdr-scm

[![CI](https://github.com/dkbo/herdr-scm/actions/workflows/ci.yml/badge.svg)](https://github.com/dkbo/herdr-scm/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A read-only, multi-repo source-control overview panel for [herdr](https://herdr.dev) — the
equivalent of VS Code's Source Control sidebar (Changes tree + diff), but for every git repo
under your current herdr workspace at once. At a glance: every repo's branch and ahead/behind
counts, every changed file, and a diff for the one you've selected.

The value it adds over a single-repo view: when several agents are editing different repos in
the same tree simultaneously, this panel is the one place that shows the whole tree's dirty
state at once.

## What it looks like

Wide pane (≥ `split_threshold_cols`, default 120 columns) — tree and diff side by side:

```
┌──────────────────────────────────────────────────────────────────────┐
│ SCM — 6 repos · 3 dirty                                              │
│ ▾ teleagent  master  ↑6 ↓1  root  1 │ e2e/specs/07-authz.spec.ts     │
│   ▾ Changes  1                      │ @@ -12,6 +12,9 @@              │
│     M e2e/specs/07-authz.spec.ts    │  test('B3 rejects cross-tenant'│
│ ▸ pencil  sub/pencil  main  nested  │ +  await expect(page).toHaveURL│
│                                     │                                │
│ copied teleagent:e2e/specs/07-authz.spec.ts                       3/8│
└──────────────────────────────────────────────────────────────────────┘
```

Narrow pane (below the threshold) — tree stacked above diff:

```
┌──────────────────────────────────────────────┐
│ SCM — 6 repos · 3 dirty                      │
│ ▾ teleagent  master  ↑6 ↓1  root  1          │
│   ▾ Changes  1                               │
│     M e2e/specs/07-authz.spec.ts             │
│ ▸ pencil  sub/pencil  main  nested           │
│ e2e/specs/07-authz.spec.ts───────────────────│
│ @@ -12,6 +12,9 @@                            │
│  test('B3 rejects cross-tenant', async () => │
│                                           3/8│
└──────────────────────────────────────────────┘
```

The bottom row is a status bar: whatever the last action reported on the left, and the cursor's
position in the tree on the right, flush to the last column. A notice too long for the row is
simply cut short: the position is the half that is always true, so it is the half that keeps its
columns. The outer box above is only there to mark the pane's edges — the panel itself draws no
outer border.

### Colours

The panel uses your terminal's own sixteen ANSI colours rather than a palette of its own, so it
follows whatever theme you have set and stays legible on a light background or over a 16-colour
SSH session. That is also the only way the tree can stay in step with the diff beside it, which
`delta` colours according to your `delta` config and terminal theme — neither of which this
plugin can see.

What the colours mean: green and red match `delta`'s own `+` and `-`, and yellow marks a
modification; cyan and magenta are git refs and sync state; dark grey is structure and information
that has gone to zero (`↑0 ↓0` recedes, because level with the upstream is nothing to act on);
and repo names and file paths take no colour at all, so they render in your terminal's own
foreground. A merge conflict (`U`) is bold red — the loudest thing on the screen.

There is nothing to configure here, deliberately: change your terminal theme and both halves of
the pane change with it.

## Install

```bash
herdr plugin install dkbo/herdr-scm
```

Then bind a key in `~/.config/herdr/config.toml`:

```toml
[[keys]]
key = "prefix+g"
action = "open-scm"

[[keys]]
key = "prefix+shift+g"
action = "open-scm-tab"
```

The install compiles the plugin from source — there is no prebuilt download and no network
access beyond fetching crates. The first build resolves around 200 crates and takes a few
minutes; after that it is incremental. See [Requirements](#requirements) for what you need on
the machine before installing.

`open-scm` opens (or focuses, or closes) the panel as a split alongside your current pane,
scoped to the current tab. `open-scm-tab` opens it in its own tab instead: pressing it again
switches to an already-open panel elsewhere in the same workspace rather than duplicating it,
and closes it when it's already focused.

### Requirements

| What | Needed for | Where it comes from |
|---|---|---|
| herdr ≥ 0.9.0 | the host this plugin is a pane inside; 0.9.0 is where the `herdr pane list --workspace <id>` payload it parses was verified | <https://herdr.dev> |
| Rust ≥ 1.88 with `cargo` | the install-time build (`cargo build --release`). Missing cargo fails the install with a message rather than a broken plugin | <https://rustup.rs> |
| `git` | every branch, ahead/behind count and diff — the panel shells out to git and never reimplements it | <https://git-scm.com> |
| `delta` | optional, but it is the default `diff_tool`. Without it, set `diff_tool = ""` for plain-text diffs | [dandavison/delta](https://github.com/dandavison/delta) |
| `$EDITOR` | optional — the `e` key hands the selected file to it | your shell environment |
| `bash` | the two launcher scripts the manifest's actions run | preinstalled on Linux |

Linux only in v1 (developed under WSL2); the manifest declares `platforms = ["linux"]`.

### Building from a clone

For working on the plugin itself, link the checkout instead of installing from GitHub:

```bash
git clone https://github.com/dkbo/herdr-scm.git
cd herdr-scm
herdr plugin link "$PWD"
```

After a source change, `cargo build --release` is enough — the linked plugin picks up the new
binary on the next launch.

## Keybindings

These are the panel's own keys, once it has focus (distinct from the herdr-level keys above
that open/close the panel itself). They come from the registry in `src/input.rs`
(`input::REGISTRY`) and can be overridden per-action via the config's `[keys]` table.

| Key | Action |
|---|---|
| `j` `k` `↓` `↑` | Move the cursor down / up |
| `Enter` `Space` | Expand or collapse the selected repo or group |
| `Tab` | Move focus between the tree and the diff |
| `]` | Jump to the next changed file, across repos |
| `[` | Jump to the previous changed file, across repos |
| `r` | Re-scan now, including the repo list |
| `a` | Expand or collapse everything |
| `Z` | Toggle this pane's full-screen zoom |
| `y` | Copy `repo:path` for the selected file |
| `e` | Open the selected file in `$EDITOR` |
| `?` | Show this help |
| `Esc` | Close the overlay |
| `q` | Quit |

## Configuration

Copy `config.example.toml` to `$HERDR_PLUGIN_CONFIG_DIR/config.toml` (find the directory with
`herdr plugin config-dir herdr-scm`), or to `~/.config/herdr-scm/config.toml`. The file is
read-only to herdr-scm — it is never written back — and every value in the example is the
built-in default, so a fresh copy changes nothing until you edit it. See the comments in
`config.example.toml` for what each setting does, including how to rebind keys under `[keys]`.

A parse error in the config file degrades the whole file to the built-in defaults rather than
failing to start.

## What this does NOT do

- **Any write operation.** No stage, no commit, no push/pull/sync, no discard. It is read-only,
  full stop.
- **A commit graph / log view.** Outgoing-changes-style history is out of scope for v1 — it's a
  separate concern from the working-tree overview this panel provides, and could be added later
  without touching the existing structure.
- **Cross-workspace aggregation.** It shows only the current herdr workspace, never repos that
  belong to another workspace.
- **macOS or Windows.** v1 is Linux-only (developed under WSL2). The manifest keeps the
  `platforms` field in place for when that changes, but there is no other-platform build script
  or `-windows` action yet.

## Third-party code and attribution

The panel is a [ratatui](https://github.com/ratatui/ratatui) TUI over `git`'s own output. Credit
where it's due — these are the crates herdr-scm depends on directly:

| Crate | What it does here | License |
|---|---|---|
| [ratatui](https://github.com/ratatui/ratatui) | the whole terminal UI: layout, widgets, the frame loop | MIT |
| [crossterm](https://github.com/crossterm-rs/crossterm) | the terminal backend — raw mode, the alternate screen, key events | MIT |
| [ansi-to-tui](https://github.com/ratatui/ansi-to-tui) | turns `delta`'s coloured patch output into styled ratatui text | MIT |
| [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) | the gitignore matcher that prunes the repo walk (from ripgrep) | Unlicense OR MIT |
| [serde](https://github.com/serde-rs/serde) + [serde_json](https://github.com/serde-rs/json) | parsing the `herdr pane list` payload | MIT OR Apache-2.0 |
| [toml](https://github.com/toml-rs/toml) | reading `config.toml` | MIT OR Apache-2.0 |
| [tempfile](https://github.com/Stebalien/tempfile) | dev-dependency only — the tests build real git repositories in temp dirs | MIT OR Apache-2.0 |

Diffs are rendered by [delta](https://github.com/dandavison/delta) (MIT) when it is installed,
and the host itself is [herdr](https://github.com/herdrdev/herdr) (Apache-2.0). Neither is
bundled — herdr-scm runs whichever copy is on your machine.

[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) lists every crate in the resolved dependency
graph with its version, license and source, plus the external programs above.

## License

MIT — see [LICENSE](LICENSE).

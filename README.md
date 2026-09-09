# herdr-scm

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
┌ SCM · herdr-p ── 6 repos · 3 dirty ─────────────────────────────────┐
│ ▾ teleagent    master ↑6↓1 root │ e2e/specs/07-authz-boundary.spec.ts│
│   ▾ Changes                  1  │ @@ -12,6 +12,9 @@                  │
│     U 07-authz-boundary.spec.ts │  test('B3 拒絕跨租戶', async () => {│
│ ▸ pencil       main    nested   │ +  await expect(page).toHaveURL(…) │
└─────────────────────────────────┴────────────────────────────────────┘
```

Narrow pane (below the threshold) — tree stacked above diff:

```
┌ SCM · herdr-p ── 6 repos · 3 dirty ──────────┐
│ ▾ teleagent      master ↑6 ↓1  root       1  │
│   ▾ Changes                                  │
│     U  e2e/specs/07-authz-boundary.spec.ts   │
├─ 07-authz-boundary.spec.ts ──────────────────┤
│ @@ -12,6 +12,9 @@                            │
└──────────────────────────────────────────────┘
```

## Install

```bash
git clone <this repo> ~/project/herdr-plugin2
herdr plugin link ~/project/herdr-plugin2
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

Building needs Rust (https://rustup.rs). After a source change, `cargo build --release` is
enough — the linked plugin picks it up on the next launch.

`open-scm` opens (or focuses, or closes) the panel as a split alongside your current pane,
scoped to the current tab. `open-scm-tab` opens it in its own tab instead: pressing it again
switches to an already-open panel elsewhere in the same workspace rather than duplicating it,
and closes it when it's already focused.

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

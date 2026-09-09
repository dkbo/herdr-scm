//! Drawing. Every line the panel shows is produced by a pure string function here, so the
//! layout and content assertions are plain string tests; `draw` only places them.
//!
//! Every piece of untrusted text — repo directory names, file paths, git's stderr — passes
//! through the control-sequence scanner before it reaches the buffer.

use crate::controller::{Controller, Focus};
use crate::input::{Bindings, REGISTRY, key_label};
use crate::layout::{Orientation, geometry};
use crate::model::{FileEntry, RepoEntry, StatusGroup};
use crate::render::neutralize_plain_text;
use crate::tree::{Row, RowId};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::path::PathBuf;

/// Where things ended up this frame, for mouse hit-testing.
#[derive(Debug, Clone, Default)]
pub struct Hits {
    pub tree: Rect,
    pub diff: Rect,
    /// `(screen row, index into the tree's row list)` for every drawn row.
    pub rows: Vec<(u16, usize)>,
}

/// The title bar's text.
pub fn title((repos, dirty): (usize, usize)) -> String {
    let noun = if repos == 1 { "repo" } else { "repos" };
    format!("SCM — {repos} {noun} · {dirty} dirty")
}

/// A repo row (spec §5.2): marker, name, relative path, branch, ahead/behind, kind, count.
pub fn repo_line(repo: &RepoEntry, collapsed: bool) -> String {
    let marker = if collapsed { '▸' } else { '▾' };
    let mut parts = vec![format!("{marker} {}", safe(&repo.display_name))];
    if !repo.rel_path.is_empty() {
        parts.push(safe(&repo.rel_path));
    }
    if let Some(branch) = &repo.branch {
        parts.push(safe(branch));
    }
    // No upstream means no arrows at all; `↑0 ↓0` means level with one.
    if let (Some(ahead), Some(behind)) = (repo.ahead, repo.behind) {
        parts.push(format!("↑{ahead} ↓{behind}"));
    }
    parts.push(repo.kind.label().to_string());
    if repo.stale {
        parts.push("stale".to_string());
    }
    if let Some(error) = &repo.error {
        parts.push(format!("! {}", safe(error)));
    }
    let count = repo.dirty_count();
    if count > 0 {
        parts.push(count.to_string());
    }
    parts.join("  ")
}

/// A group row: its title and how many files it holds.
pub fn group_line(group: &StatusGroup, collapsed: bool) -> String {
    let marker = if collapsed { '▸' } else { '▾' };
    format!("{marker} {}  {}", group.kind.title(), group.files.len())
}

/// A file row: the status letter and the repo-relative path.
pub fn file_line(file: &FileEntry) -> String {
    let path = match &file.orig_path {
        Some(orig) => format!("{} ← {}", safe(&file.path), safe(orig)),
        None => safe(&file.path),
    };
    format!("{} {path}", file.status)
}

/// The empty state (spec §5.4): why it is empty, where we looked, and how to retry.
pub fn empty_state_lines(scan_roots: &[PathBuf]) -> Vec<String> {
    let mut lines = vec![
        "No git repositories found in this workspace.".to_string(),
        String::new(),
        "Scanned from:".to_string(),
    ];
    if scan_roots.is_empty() {
        lines.push("  (no scan start could be determined)".to_string());
    } else {
        for root in scan_roots {
            lines.push(format!("  {}", safe(&root.to_string_lossy())));
        }
    }
    lines.push(String::new());
    lines.push("Press r to scan again.".to_string());
    lines
}

/// The help overlay: every action with its effective keys.
pub fn help_lines(bindings: &Bindings) -> Vec<String> {
    REGISTRY
        .iter()
        .map(|row| {
            let keys = bindings
                .keys_for(row.intent)
                .into_iter()
                .map(key_label)
                .collect::<Vec<_>>()
                .join(" / ");
            format!("{keys:<12}  {}", row.description)
        })
        .collect()
}

/// Draw one frame and report where everything landed.
pub fn draw(frame: &mut Frame, controller: &Controller, bindings: &Bindings) -> Hits {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return Hits::default();
    }
    // One row of title, the rest for the body. On a one-row pane the title wins: it still
    // says how many repos are dirty, which beats a single unreadable tree row.
    let title_area = Rect::new(area.x, area.y, area.width, 1);
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(title(controller.title_counts()))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        title_area,
    );
    if body.height == 0 {
        return Hits::default();
    }

    if controller.is_empty() {
        let text = Text::from(
            empty_state_lines(controller.scan_roots())
                .into_iter()
                .map(Line::from)
                .collect::<Vec<_>>(),
        );
        frame.render_widget(Paragraph::new(text), body);
        return Hits {
            tree: body,
            ..Hits::default()
        };
    }

    let geo = geometry(body, controller.settings().split_threshold_cols);
    let hits = draw_tree(frame, controller, geo.tree);
    draw_diff(frame, controller, geo.diff, geo.orientation);
    if let Some(notice) = controller.notice() {
        draw_notice(frame, area, notice);
    }
    if controller.help_open() {
        draw_help(frame, area, bindings);
    }
    Hits {
        tree: geo.tree,
        diff: geo.diff,
        rows: hits,
    }
}

/// Draw the tree, scrolled so the cursor stays visible, and report each row's screen line.
fn draw_tree(frame: &mut Frame, controller: &Controller, area: Rect) -> Vec<(u16, usize)> {
    let focused = controller.focus() == Focus::Tree;
    let block = Block::default()
        .borders(Borders::NONE)
        .style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return Vec::new();
    }
    let tree = controller.tree();
    let repos = controller.repos();
    let height = inner.height as usize;
    let (lines, hits) = windowed_rows(tree.rows(), repos, tree.cursor(), height, inner.y, |id| {
        tree.is_collapsed(id)
    });
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
    hits
}

/// The pure heart of `draw_tree`: which rows are visible in a `height`-tall window starting at
/// the cursor's neighborhood, formatted to text, alongside where each landed on screen.
///
/// `base_y` is the first screen row of the window (`inner.y` in `draw_tree`; `0` in tests).
///
/// A row whose indices do not resolve against `repos` (which cannot happen through the public
/// API today, since the tree and the presenter always read the same repo list, but nothing in
/// the type system rules it out for a future caller) is skipped — but skipping must never let
/// `hits`' `y` diverge from the row's ACTUAL screen line. `lines` is packed contiguously from
/// `base_y`, so deriving `y` from `lines.len()` (rather than from the loop's `offset`, which
/// counts skipped rows too) keeps that invariant true by construction: `hits.len() ==
/// lines.len()` always, and every `y` is `base_y + 0, base_y + 1, …` with no gaps.
fn windowed_rows(
    rows: &[Row],
    repos: &[RepoEntry],
    cursor: usize,
    height: usize,
    base_y: u16,
    is_collapsed: impl Fn(&RowId) -> bool,
) -> (Vec<Line<'static>>, Vec<(u16, usize)>) {
    // Keep the cursor on screen with the simplest rule that never jumps: scroll only far
    // enough to include it.
    let first = cursor.saturating_sub(height.saturating_sub(1));
    let mut lines = Vec::new();
    let mut hits = Vec::new();
    for (offset, row) in rows.iter().skip(first).take(height).enumerate() {
        let Some(repo) = repos.get(row.repo_idx) else {
            continue;
        };
        let text = match &row.id {
            RowId::Repo { .. } => repo_line(repo, is_collapsed(&row.id)),
            RowId::Group { .. } => match row.group_idx.and_then(|i| repo.groups.get(i)) {
                Some(group) => format!("  {}", group_line(group, is_collapsed(&row.id))),
                None => continue,
            },
            RowId::File { .. } => {
                let file = row
                    .group_idx
                    .and_then(|g| repo.groups.get(g))
                    .and_then(|group| row.file_idx.and_then(|f| group.files.get(f)));
                match file {
                    Some(file) => format!("    {}", file_line(file)),
                    None => continue,
                }
            }
        };
        let index = first + offset;
        let style = if index == cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        // Derived from what has actually been pushed to `lines` so far, not from `offset` —
        // `offset` counts rows skipped by a `continue` above, `lines.len()` does not.
        let y = base_y + lines.len() as u16;
        hits.push((y, index));
        lines.push(Line::styled(text, style));
    }
    (lines, hits)
}

/// Draw the diff region, with a title in the stacked layout where it doubles as the divider.
fn draw_diff(frame: &mut Frame, controller: &Controller, area: Rect, orientation: Orientation) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let focused = controller.focus() == Focus::Diff;
    let block = match orientation {
        Orientation::Stacked => Block::default()
            .borders(Borders::TOP)
            .style(border_style(focused)),
        Orientation::SideBySide => Block::default()
            .borders(Borders::LEFT)
            .style(border_style(focused)),
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(controller.diff_text().clone()).scroll((controller.diff_scroll(), 0)),
        inner,
    );
}

/// A one-line transient notice along the bottom.
fn draw_notice(frame: &mut Frame, area: Rect, notice: &str) {
    if area.height < 2 {
        return;
    }
    let line = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    frame.render_widget(Clear, line);
    frame.render_widget(Paragraph::new(safe(notice)), line);
}

/// The help overlay, centered.
fn draw_help(frame: &mut Frame, area: Rect, bindings: &Bindings) {
    let lines = help_lines(bindings);
    let width = area.width.saturating_sub(4).max(1);
    let height = ((lines.len() + 2) as u16).min(area.height);
    let overlay = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(Text::from(
            lines.into_iter().map(Line::from).collect::<Vec<_>>(),
        ))
        .block(Block::default().borders(Borders::ALL).title("Keys")),
        overlay,
    );
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

/// Neutralize untrusted text and collapse it to one line — every row here has exactly one.
fn safe(raw: &str) -> String {
    neutralize_plain_text(raw).replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::default_bindings;
    use crate::model::{FileEntry, RepoEntry, RepoKind, StatusGroup};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn entry() -> RepoEntry {
        RepoEntry {
            path: PathBuf::from("/w/teleagent"),
            display_name: "teleagent".to_string(),
            rel_path: String::new(),
            kind: RepoKind::Root,
            branch: Some("master".to_string()),
            ahead: Some(6),
            behind: Some(1),
            groups: vec![StatusGroup {
                kind: crate::model::GroupKind::Changes,
                files: vec![FileEntry {
                    path: "e2e/specs/07-authz.spec.ts".to_string(),
                    status: 'M',
                    orig_path: None,
                }],
            }],
            ..RepoEntry::blank()
        }
    }

    // ---- the title -----------------------------------------------------------------------

    #[test]
    fn the_title_reports_the_repo_and_dirty_counts() {
        assert_eq!(title((6, 3)), "SCM — 6 repos · 3 dirty");
    }

    #[test]
    fn the_title_is_singular_for_one_repo() {
        assert_eq!(title((1, 0)), "SCM — 1 repo · 0 dirty");
    }

    // ---- the repo row (spec §5.2) ------------------------------------------------------------

    #[test]
    fn a_repo_row_shows_name_branch_ahead_behind_kind_and_count() {
        let line = repo_line(&entry(), false);
        for part in ["teleagent", "master", "↑6", "↓1", "root", "1"] {
            assert!(line.contains(part), "{part:?} missing from {line:?}");
        }
    }

    #[test]
    fn a_repo_row_shows_its_relative_path_except_at_the_scan_start() {
        assert!(!repo_line(&entry(), false).contains('/'));
        let nested = RepoEntry {
            rel_path: "sub/pencil".to_string(),
            ..entry()
        };
        assert!(repo_line(&nested, false).contains("sub/pencil"));
    }

    #[test]
    fn a_repo_with_no_upstream_shows_no_arrows_at_all() {
        // Distinguishable from "level with upstream", which shows ↑0 ↓0.
        let no_upstream = RepoEntry {
            ahead: None,
            behind: None,
            ..entry()
        };
        let line = repo_line(&no_upstream, false);
        assert!(!line.contains('↑'), "{line}");
        assert!(!line.contains('↓'), "{line}");

        let level = RepoEntry {
            ahead: Some(0),
            behind: Some(0),
            ..entry()
        };
        assert!(repo_line(&level, false).contains("↑0"));
    }

    #[test]
    fn a_clean_repo_shows_no_dirty_count() {
        let clean = RepoEntry {
            groups: vec![],
            ..entry()
        };
        let line = repo_line(&clean, false);
        assert!(line.contains("teleagent"), "{line}");
        assert!(!line.ends_with('0'), "{line}");
    }

    #[test]
    fn the_expansion_marker_reflects_the_collapsed_state() {
        assert!(repo_line(&entry(), false).starts_with('▾'));
        assert!(repo_line(&entry(), true).starts_with('▸'));
    }

    #[test]
    fn a_failing_repo_is_marked_and_carries_its_message() {
        let broken = RepoEntry {
            error: Some("fatal: not a git repository".to_string()),
            ..entry()
        };
        let line = repo_line(&broken, false);
        assert!(line.contains('!'), "{line}");
        assert!(line.contains("not a git repository"), "{line}");
    }

    #[test]
    fn a_stale_repo_is_labelled_stale() {
        let stale = RepoEntry {
            stale: true,
            ..entry()
        };
        assert!(repo_line(&stale, false).contains("stale"));
    }

    #[test]
    fn a_repo_row_is_a_single_line_even_when_its_name_or_error_contains_control_characters() {
        // Repo directory names and git's stderr are both untrusted text.
        let hostile = RepoEntry {
            display_name: "ev\x1b]52;c;x\x07il\nsecond".to_string(),
            error: Some("bad\nlines".to_string()),
            ..entry()
        };
        let line = repo_line(&hostile, false);
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.contains('\x1b'), "{line:?}");
    }

    // ---- the group and file rows -----------------------------------------------------------

    #[test]
    fn a_group_row_shows_its_title_and_file_count() {
        let group = &entry().groups[0];
        let line = group_line(group, false);
        assert!(line.contains("Changes"), "{line}");
        assert!(line.contains('1'), "{line}");
    }

    #[test]
    fn a_file_row_shows_its_status_letter_and_repo_relative_path() {
        let file = &entry().groups[0].files[0];
        let line = file_line(file);
        assert!(line.starts_with('M'), "{line}");
        assert!(line.contains("e2e/specs/07-authz.spec.ts"), "{line}");
    }

    #[test]
    fn a_renamed_file_shows_where_it_came_from() {
        let renamed = FileEntry {
            path: "new.rs".to_string(),
            status: 'R',
            orig_path: Some("old.rs".to_string()),
        };
        let line = file_line(&renamed);
        assert!(line.contains("old.rs"), "{line}");
        assert!(line.contains("new.rs"), "{line}");
    }

    #[test]
    fn a_file_name_carrying_control_characters_is_neutralized() {
        let hostile = FileEntry {
            path: "a\x1b[2Jb\nc".to_string(),
            status: '?',
            orig_path: None,
        };
        let line = file_line(&hostile);
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.contains('\x1b'), "{line:?}");
    }

    // ---- the empty state (spec §5.4) ----------------------------------------------------------

    #[test]
    fn the_empty_state_explains_itself_lists_where_we_looked_and_offers_a_rescan() {
        let lines = empty_state_lines(&[PathBuf::from("/w/one"), PathBuf::from("/w/two")]);
        let all = lines.join("\n");
        assert!(all.contains("No git repositories"), "{all}");
        assert!(all.contains("/w/one") && all.contains("/w/two"), "{all}");
        assert!(all.contains('r'), "{all}");
        assert!(!lines.is_empty());
    }

    #[test]
    fn the_empty_state_still_says_something_when_there_were_no_scan_starts_at_all() {
        assert!(!empty_state_lines(&[]).is_empty());
    }

    // ---- the help overlay -----------------------------------------------------------------------

    #[test]
    fn the_help_overlay_lists_every_action_with_its_effective_keys() {
        let lines = help_lines(&default_bindings());
        let all = lines.join("\n");
        for row in crate::input::REGISTRY {
            assert!(all.contains(row.description), "{} missing", row.name);
        }
        assert!(all.contains('j') && all.contains(']'), "{all}");
    }

    // ---- windowed_rows: the pure row-selection core of draw_tree ---------------------------------

    /// A minimal one-repo entry, distinguishable only by its path, for `windowed_rows` tests.
    fn named_repo(name: &str) -> RepoEntry {
        RepoEntry {
            path: PathBuf::from(format!("/w/{name}")),
            display_name: name.to_string(),
            ..RepoEntry::blank()
        }
    }

    fn repo_row(repo_idx: usize) -> Row {
        Row {
            id: RowId::Repo {
                repo: PathBuf::from(format!("/w/row-{repo_idx}")),
            },
            depth: 0,
            repo_idx,
            group_idx: None,
            file_idx: None,
        }
    }

    #[test]
    fn a_row_that_does_not_resolve_does_not_desync_the_reported_screen_rows_from_what_was_drawn() {
        // The middle row's `repo_idx` (99) does not resolve against `repos` (only 0 and 1 are
        // valid) — the one skip path reachable with hand-built data, standing in for all three
        // `continue`s in the loop, which share the same reporting logic.
        let rows = vec![repo_row(0), repo_row(99), repo_row(1)];
        let repos = vec![named_repo("a"), named_repo("b")];
        let (lines, hits) = windowed_rows(&rows, &repos, 0, 10, 5, |_| false);

        // The invariant `Hits` exists for: every drawn line has exactly one hit, and the hits'
        // `y` values are exactly the screen rows `lines` actually occupies — contiguous from
        // `base_y`, with no gap left by the skipped row.
        assert_eq!(hits.len(), lines.len(), "one hit per drawn line");
        let ys: Vec<u16> = hits.iter().map(|(y, _)| *y).collect();
        assert_eq!(
            ys,
            vec![5, 6],
            "{ys:?}: must be contiguous from base_y, not leave a gap"
        );
    }

    // ---- smoke tests through a real backend -------------------------------------------------------

    /// The visible characters of the rendered buffer, joined by newlines.
    fn screen(width: u16, height: u16, controller: &crate::controller::Controller) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| {
                draw(f, controller, &default_bindings());
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_wide_pane_draws_the_tree_and_the_diff_side_by_side() {
        let controller = crate::controller::tests_support::loaded_controller();
        let out = screen(140, 12, &controller);
        assert!(out.contains("teleagent"), "{out}");
        // A side-by-side layout draws a vertical divider between tree and diff; a stacked one
        // does not. This distinguishes the two orientations without depending on exact column
        // arithmetic (a LEFT-bordered block's divider sits in its own first column, so the
        // longest trimmed row here is only ~57 chars, never > 60).
        assert!(
            out.contains('\u{2502}'),
            "a wide pane draws a vertical divider: {out}"
        );
    }

    #[test]
    fn a_narrow_pane_stacks_without_panicking() {
        let controller = crate::controller::tests_support::loaded_controller();
        let out = screen(60, 12, &controller);
        assert!(out.contains("teleagent"), "{out}");
        assert!(
            out.contains('\u{2500}'),
            "a narrow pane draws a horizontal divider: {out}"
        );
    }

    #[test]
    fn a_tiny_pane_draws_without_panicking() {
        // A herdr split can be dragged arbitrarily small; every size must be survivable.
        let controller = crate::controller::tests_support::loaded_controller();
        for (w, h) in [(1u16, 1u16), (2, 1), (1, 2), (10, 3), (0, 0)] {
            let _ = screen(w.max(1), h.max(1), &controller);
        }
    }

    #[test]
    fn an_empty_panel_draws_the_empty_state_rather_than_a_blank_pane() {
        let controller = crate::controller::tests_support::empty_controller();
        let out = screen(80, 12, &controller);
        assert!(out.contains("No git repositories"), "{out}");
    }
}

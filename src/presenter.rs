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
use crate::theme::{self, Role};
use crate::tree::{Row, RowId};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
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

/// One styled run inside a row.
///
/// It carries a semantic [`Role`], never a `Style`: the palette lives in `theme.rs`, and nothing
/// in this module knows what colour anything is. That is also what keeps every layout and content
/// assertion here a plain string test — see [`plain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub role: Role,
}

fn seg(text: impl Into<String>, role: Role) -> Segment {
    Segment {
        text: text.into(),
        role,
    }
}

/// A row's characters with its styling dropped — what the content tests assert on.
pub fn plain(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.text.as_str()).collect()
}

/// Resolve a row's roles into a drawable line, with `base` patched over each segment's own style.
///
/// `base` is how the cursor row gets its selection treatment without any segment losing its
/// semantic colour: `Style::patch` unions modifiers and only fills a colour the segment left
/// unset, and `theme::selection` deliberately sets no colours at all.
fn to_line(segments: Vec<Segment>, base: Style) -> Line<'static> {
    Line::from(
        segments
            .into_iter()
            .map(|s| Span::styled(s.text, theme::style(s.role).patch(base)))
            .collect::<Vec<_>>(),
    )
}

/// Prefix a row's segments with `n` spaces of tree indentation.
fn indent(n: usize, mut segments: Vec<Segment>) -> Vec<Segment> {
    segments.insert(0, seg(" ".repeat(n), Role::Chrome));
    segments
}

/// The title bar's text.
pub fn title((repos, dirty): (usize, usize)) -> Vec<Segment> {
    let noun = if repos == 1 { "repo" } else { "repos" };
    vec![
        seg("SCM", Role::RepoName),
        seg(format!(" — {repos} {noun} · "), Role::Chrome),
        // A clean tree should have nothing lit up in the title.
        seg(
            format!("{dirty} dirty"),
            if dirty > 0 {
                Role::DirtyCount
            } else {
                Role::Chrome
            },
        ),
    ]
}

/// A repo row (spec §5.2): marker, name, relative path, branch, ahead/behind, kind, count.
pub fn repo_line(repo: &RepoEntry, collapsed: bool) -> Vec<Segment> {
    let marker = if collapsed { '▸' } else { '▾' };
    let mut out = vec![
        seg(format!("{marker} "), Role::Marker),
        seg(safe(&repo.display_name), Role::RepoName),
    ];
    if !repo.rel_path.is_empty() {
        out.push(gap());
        out.push(seg(safe(&repo.rel_path), Role::RelPath));
    }
    if let Some(branch) = &repo.branch {
        out.push(gap());
        out.push(seg(safe(branch), Role::Branch));
    }
    // No upstream means no arrows at all; `↑0 ↓0` means level with one. Each side is lit
    // independently, because being level one way is not being level both ways.
    if let (Some(ahead), Some(behind)) = (repo.ahead, repo.behind) {
        out.push(gap());
        out.push(seg(format!("↑{ahead}"), sync_role(ahead)));
        out.push(seg(" ", Role::Chrome));
        out.push(seg(format!("↓{behind}"), sync_role(behind)));
    }
    out.push(gap());
    out.push(seg(repo.kind.label(), Role::Kind));
    if repo.stale {
        out.push(gap());
        out.push(seg("stale", Role::Stale));
    }
    if let Some(error) = &repo.error {
        out.push(gap());
        out.push(seg(format!("! {}", safe(error)), Role::Error));
    }
    let count = repo.dirty_count();
    if count > 0 {
        out.push(gap());
        out.push(seg(count.to_string(), Role::DirtyCount));
    }
    out
}

/// The two-space separator between a repo row's fields.
fn gap() -> Segment {
    seg("  ", Role::Chrome)
}

/// Lit when there is something to act on, receded when level with the upstream.
fn sync_role(n: u32) -> Role {
    if n > 0 { Role::Sync } else { Role::SyncIdle }
}

/// A group row: its title and how many files it holds.
pub fn group_line(group: &StatusGroup, collapsed: bool) -> Vec<Segment> {
    let marker = if collapsed { '▸' } else { '▾' };
    vec![
        seg(format!("{marker} "), Role::Marker),
        seg(group.kind.title(), Role::GroupTitle),
        seg("  ", Role::Chrome),
        seg(group.files.len().to_string(), Role::GroupCount),
    ]
}

/// A file row: the status letter and the repo-relative path.
pub fn file_line(file: &FileEntry) -> Vec<Segment> {
    let mut out = vec![
        seg(file.status.to_string(), Role::Status(file.status)),
        seg(" ", Role::Chrome),
        seg(safe(&file.path), Role::Path),
    ];
    if let Some(orig) = &file.orig_path {
        out.push(seg(" ← ", Role::Chrome));
        out.push(seg(safe(orig), Role::OrigPath));
    }
    out
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
        Paragraph::new(to_line(title(controller.title_counts()), Style::default())),
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
    if area.height == 0 {
        return Vec::new();
    }
    let tree = controller.tree();
    let win = Window {
        cursor: tree.cursor(),
        height: area.height as usize,
        base_y: area.y,
        // The tree has no border to brighten, so the selection bar is its whole focus cue.
        focused: controller.focus() == Focus::Tree,
    };
    let (lines, hits) = windowed_rows(tree.rows(), controller.repos(), win, |id| {
        tree.is_collapsed(id)
    });
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    hits
}

/// Which slice of the row list to draw, where it lands on screen, and how the cursor row should
/// look. Bundled rather than passed loose so the parameter count stays under clippy's limit.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// Index of the cursor within the row list.
    cursor: usize,
    /// How many rows fit.
    height: usize,
    /// The screen row the window's first drawn line lands on.
    base_y: u16,
    /// Whether the TREE has focus, which decides the cursor row's treatment.
    focused: bool,
}

/// The pure heart of `draw_tree`: which rows are visible in a `height`-tall window starting at
/// the cursor's neighborhood, formatted to text, alongside where each landed on screen.
///
/// `win.base_y` is the first screen row of the window (`inner.y` in `draw_tree`; `0` in tests).
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
    win: Window,
    is_collapsed: impl Fn(&RowId) -> bool,
) -> (Vec<Line<'static>>, Vec<(u16, usize)>) {
    // Keep the cursor on screen with the simplest rule that never jumps: scroll only far
    // enough to include it.
    let first = win.cursor.saturating_sub(win.height.saturating_sub(1));
    let mut lines = Vec::new();
    let mut hits = Vec::new();
    for (offset, row) in rows.iter().skip(first).take(win.height).enumerate() {
        let Some(repo) = repos.get(row.repo_idx) else {
            continue;
        };
        let segments = match &row.id {
            RowId::Repo { .. } => repo_line(repo, is_collapsed(&row.id)),
            RowId::Group { .. } => match row.group_idx.and_then(|i| repo.groups.get(i)) {
                Some(group) => indent(2, group_line(group, is_collapsed(&row.id))),
                None => continue,
            },
            RowId::File { .. } => {
                let file = row
                    .group_idx
                    .and_then(|g| repo.groups.get(g))
                    .and_then(|group| row.file_idx.and_then(|f| group.files.get(f)));
                match file {
                    Some(file) => indent(4, file_line(file)),
                    None => continue,
                }
            }
        };
        let index = first + offset;
        let base = if index == win.cursor {
            theme::selection(win.focused)
        } else {
            Style::default()
        };
        // Derived from what has actually been pushed to `lines` so far, not from `offset` —
        // `offset` counts rows skipped by a `continue` above, `lines.len()` does not.
        let y = win.base_y + lines.len() as u16;
        hits.push((y, index));
        lines.push(to_line(segments, base));
    }
    (lines, hits)
}

/// Draw the diff region, with a title in the stacked layout where it doubles as the divider.
fn draw_diff(frame: &mut Frame, controller: &Controller, area: Rect, orientation: Orientation) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let focused = controller.focus() == Focus::Diff;
    let borders = match orientation {
        Orientation::Stacked => Borders::TOP,
        Orientation::SideBySide => Borders::LEFT,
    };
    let block = Block::default()
        .borders(borders)
        .border_style(theme::pane_border(focused));
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
    use ratatui::style::Modifier;

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
        assert_eq!(plain(&title((6, 3))), "SCM — 6 repos · 3 dirty");
    }

    #[test]
    fn the_title_is_singular_for_one_repo() {
        assert_eq!(plain(&title((1, 0))), "SCM — 1 repo · 0 dirty");
    }

    // ---- the repo row (spec §5.2) ------------------------------------------------------------

    #[test]
    fn a_repo_row_shows_name_branch_ahead_behind_kind_and_count() {
        let line = plain(&repo_line(&entry(), false));
        for part in ["teleagent", "master", "↑6", "↓1", "root", "1"] {
            assert!(line.contains(part), "{part:?} missing from {line:?}");
        }
    }

    #[test]
    fn a_repo_row_shows_its_relative_path_except_at_the_scan_start() {
        assert!(!plain(&repo_line(&entry(), false)).contains('/'));
        let nested = RepoEntry {
            rel_path: "sub/pencil".to_string(),
            ..entry()
        };
        assert!(plain(&repo_line(&nested, false)).contains("sub/pencil"));
    }

    #[test]
    fn a_repo_with_no_upstream_shows_no_arrows_at_all() {
        // Distinguishable from "level with upstream", which shows ↑0 ↓0.
        let no_upstream = RepoEntry {
            ahead: None,
            behind: None,
            ..entry()
        };
        let line = plain(&repo_line(&no_upstream, false));
        assert!(!line.contains('↑'), "{line}");
        assert!(!line.contains('↓'), "{line}");

        let level = RepoEntry {
            ahead: Some(0),
            behind: Some(0),
            ..entry()
        };
        assert!(plain(&repo_line(&level, false)).contains("↑0"));
    }

    #[test]
    fn a_clean_repo_shows_no_dirty_count() {
        let clean = RepoEntry {
            groups: vec![],
            ..entry()
        };
        let line = plain(&repo_line(&clean, false));
        assert!(line.contains("teleagent"), "{line}");
        assert!(!line.ends_with('0'), "{line}");
    }

    #[test]
    fn the_expansion_marker_reflects_the_collapsed_state() {
        assert!(plain(&repo_line(&entry(), false)).starts_with('▾'));
        assert!(plain(&repo_line(&entry(), true)).starts_with('▸'));
    }

    #[test]
    fn a_failing_repo_is_marked_and_carries_its_message() {
        let broken = RepoEntry {
            error: Some("fatal: not a git repository".to_string()),
            ..entry()
        };
        let line = plain(&repo_line(&broken, false));
        assert!(line.contains('!'), "{line}");
        assert!(line.contains("not a git repository"), "{line}");
    }

    #[test]
    fn a_stale_repo_is_labelled_stale() {
        let stale = RepoEntry {
            stale: true,
            ..entry()
        };
        assert!(plain(&repo_line(&stale, false)).contains("stale"));
    }

    #[test]
    fn a_repo_row_is_a_single_line_even_when_its_name_or_error_contains_control_characters() {
        // Repo directory names and git's stderr are both untrusted text.
        let hostile = RepoEntry {
            display_name: "ev\x1b]52;c;x\x07il\nsecond".to_string(),
            error: Some("bad\nlines".to_string()),
            ..entry()
        };
        let line = plain(&repo_line(&hostile, false));
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.contains('\x1b'), "{line:?}");
    }

    // ---- the group and file rows -----------------------------------------------------------

    #[test]
    fn a_group_row_shows_its_title_and_file_count() {
        let group = &entry().groups[0];
        let line = plain(&group_line(group, false));
        assert!(line.contains("Changes"), "{line}");
        assert!(line.contains('1'), "{line}");
    }

    #[test]
    fn a_file_row_shows_its_status_letter_and_repo_relative_path() {
        let file = &entry().groups[0].files[0];
        let line = plain(&file_line(file));
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
        let line = plain(&file_line(&renamed));
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
        let line = plain(&file_line(&hostile));
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.contains('\x1b'), "{line:?}");
    }

    // ---- semantic roles ------------------------------------------------------------------

    /// The role carried by the first segment whose text is exactly `text`.
    fn role_of(segments: &[Segment], text: &str) -> Role {
        segments
            .iter()
            .find(|s| s.text == text)
            .unwrap_or_else(|| panic!("no segment reads exactly {text:?} in {segments:?}"))
            .role
    }

    #[test]
    fn plain_reproduces_exactly_what_the_string_version_rendered() {
        // The one guarantee that keeps every content assertion in this module honest.
        assert_eq!(
            plain(&repo_line(&entry(), false)),
            "▾ teleagent  master  ↑6 ↓1  root  1"
        );
        assert_eq!(
            plain(&group_line(&entry().groups[0], false)),
            "▾ Changes  1"
        );
        assert_eq!(
            plain(&file_line(&entry().groups[0].files[0])),
            "M e2e/specs/07-authz.spec.ts"
        );
        assert_eq!(plain(&title((6, 3))), "SCM — 6 repos · 3 dirty");
    }

    #[test]
    fn each_field_of_a_repo_row_carries_its_own_role() {
        let segs = repo_line(&entry(), false);
        assert_eq!(role_of(&segs, "teleagent"), Role::RepoName);
        assert_eq!(role_of(&segs, "master"), Role::Branch);
        assert_eq!(role_of(&segs, "root"), Role::Kind);
        assert_eq!(role_of(&segs, "1"), Role::DirtyCount);
    }

    #[test]
    fn a_relative_path_is_structure_rather_than_identity() {
        let nested = RepoEntry {
            rel_path: "sub/pencil".to_string(),
            ..entry()
        };
        assert_eq!(
            role_of(&repo_line(&nested, false), "sub/pencil"),
            Role::RelPath
        );
    }

    #[test]
    fn an_arrow_at_zero_recedes_while_a_pending_one_stays_lit() {
        let segs = repo_line(&entry(), false); // ahead 6, behind 1
        assert_eq!(role_of(&segs, "↑6"), Role::Sync);
        assert_eq!(role_of(&segs, "↓1"), Role::Sync);

        let level = RepoEntry {
            ahead: Some(0),
            behind: Some(0),
            ..entry()
        };
        let segs = repo_line(&level, false);
        assert_eq!(role_of(&segs, "↑0"), Role::SyncIdle);
        assert_eq!(role_of(&segs, "↓0"), Role::SyncIdle);

        // Each side recedes on its own: being level one way is not being level both ways.
        let ahead_only = RepoEntry {
            ahead: Some(2),
            behind: Some(0),
            ..entry()
        };
        let segs = repo_line(&ahead_only, false);
        assert_eq!(role_of(&segs, "↑2"), Role::Sync);
        assert_eq!(role_of(&segs, "↓0"), Role::SyncIdle);
    }

    #[test]
    fn a_failing_repos_message_carries_the_error_role() {
        let broken = RepoEntry {
            error: Some("fatal: not a git repository".to_string()),
            ..entry()
        };
        let segs = repo_line(&broken, false);
        assert_eq!(role_of(&segs, "! fatal: not a git repository"), Role::Error);
    }

    #[test]
    fn a_stale_repo_carries_the_stale_role() {
        let stale = RepoEntry {
            stale: true,
            ..entry()
        };
        assert_eq!(role_of(&repo_line(&stale, false), "stale"), Role::Stale);
    }

    #[test]
    fn a_status_letter_carries_its_own_letters_role() {
        for code in ['M', 'A', 'D', 'R', 'C', 'U', '?'] {
            let file = FileEntry {
                path: "x".to_string(),
                status: code,
                orig_path: None,
            };
            let segs = file_line(&file);
            assert_eq!(role_of(&segs, &code.to_string()), Role::Status(code));
            assert_eq!(role_of(&segs, "x"), Role::Path);
        }
    }

    #[test]
    fn the_place_a_rename_came_from_is_dimmer_than_where_it_went() {
        let renamed = FileEntry {
            path: "new.rs".to_string(),
            status: 'R',
            orig_path: Some("old.rs".to_string()),
        };
        let segs = file_line(&renamed);
        assert_eq!(role_of(&segs, "new.rs"), Role::Path);
        assert_eq!(role_of(&segs, "old.rs"), Role::OrigPath);
    }

    #[test]
    fn a_group_title_outranks_its_count() {
        let segs = group_line(&entry().groups[0], false);
        assert_eq!(role_of(&segs, "Changes"), Role::GroupTitle);
        assert_eq!(role_of(&segs, "1"), Role::GroupCount);
    }

    #[test]
    fn a_clean_tree_does_not_light_up_the_dirty_count_in_the_title() {
        assert_eq!(role_of(&title((3, 0)), "0 dirty"), Role::Chrome);
        assert_eq!(role_of(&title((3, 2)), "2 dirty"), Role::DirtyCount);
        assert_eq!(role_of(&title((3, 2)), "SCM"), Role::RepoName);
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
        let (lines, hits) = windowed_rows(
            &rows,
            &repos,
            Window {
                cursor: 0,
                height: 10,
                base_y: 5,
                focused: true,
            },
            |_| false,
        );

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

    /// The style of the cell at `(x, y)` after one draw.
    fn cell_style(
        width: u16,
        height: u16,
        controller: &crate::controller::Controller,
        x: u16,
        y: u16,
    ) -> Style {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| {
                draw(f, controller, &default_bindings());
            })
            .expect("draw");
        terminal.backend().buffer()[(x, y)].style()
    }

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

    #[test]
    fn the_tree_says_it_has_focus_even_though_it_has_no_border_to_brighten() {
        // Before this, the tree's focus cue was BOLD on a Borders::NONE block, which the
        // Paragraph drew straight over: pressing Tab changed nothing on screen.
        let mut controller = crate::controller::tests_support::loaded_controller();
        // The cursor starts on row 0 of the tree, which is screen row 1 (row 0 is the title).
        let focused = cell_style(140, 12, &controller, 0, 1);
        assert!(
            focused.add_modifier.contains(Modifier::REVERSED),
            "the tree has focus, so its cursor row is reversed: {focused:?}"
        );

        controller.handle(crate::intent::Intent::FocusToggle);
        let unfocused = cell_style(140, 12, &controller, 0, 1);
        assert!(
            !unfocused.add_modifier.contains(Modifier::REVERSED),
            "focus moved to the diff: {unfocused:?}"
        );
        assert!(
            unfocused.add_modifier.contains(Modifier::UNDERLINED),
            "the tree keeps a quieter selection bar: {unfocused:?}"
        );
    }

    #[test]
    fn the_divider_says_which_pane_has_focus() {
        let mut controller = crate::controller::tests_support::loaded_controller();
        // The diff block's LEFT border sits in the first column of the diff region.
        let divider_x = (140u32 * u32::from(crate::layout::TREE_PCT) / 100) as u16;
        let tree_has_focus = cell_style(140, 12, &controller, divider_x, 1);
        controller.handle(crate::intent::Intent::FocusToggle);
        let diff_has_focus = cell_style(140, 12, &controller, divider_x, 1);
        assert_ne!(
            tree_has_focus, diff_has_focus,
            "the divider must reflect which side is active"
        );
    }

    #[test]
    fn a_rows_own_colours_survive_being_selected() {
        // The selection is a modifier, not a colour, precisely so this holds.
        let controller = crate::controller::tests_support::loaded_controller();
        let branch_style = theme::style(Role::Branch).patch(theme::selection(true));
        assert_eq!(branch_style.fg, theme::style(Role::Branch).fg);
        assert!(branch_style.add_modifier.contains(Modifier::REVERSED));
        // And the pane still draws.
        let _ = screen(140, 12, &controller);
    }
}

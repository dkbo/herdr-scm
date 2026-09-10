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

/// The diff region's title: which file (or repo) the patch below belongs to.
///
/// Truncated from the LEFT when it does not fit, because the file name matters more than the
/// directories leading to it.
pub fn diff_title(selected: Option<&Row>, repos: &[RepoEntry], width: u16) -> String {
    let text = match selected {
        Some(row) => match &row.id {
            RowId::File { path, .. } => safe(path),
            // A repo or group row has no diff of its own; naming the repo is more use than
            // saying nothing.
            _ => repos
                .get(row.repo_idx)
                .map(|repo| safe(&repo.display_name))
                .unwrap_or_default(),
        },
        None => "no file selected".to_string(),
    };
    truncate_left(&text, width as usize)
}

/// Keep the last `width` characters, marking the cut with a leading `…`.
fn truncate_left(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    match width {
        0 => String::new(),
        1 => "…".to_string(),
        _ => {
            let tail: String = text.chars().skip(count - (width - 1)).collect();
            format!("…{tail}")
        }
    }
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

/// The help overlay's width: as wide as its widest line, never wider than the pane can hold.
///
/// `+ 4` covers the two borders and a column of padding each side. Content-driven because a key
/// list is around sixty columns, and stretching it across a 160-column pane just puts the keys
/// and their descriptions too far apart to read together.
pub fn help_width(lines: &[String], available: u16) -> u16 {
    let content = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    ((content + 4) as u16)
        .min(available.saturating_sub(2))
        .max(1)
}

/// The bottom line: what just happened on the left, where the cursor is on the right.
///
/// A permanent row rather than one carved out only when a notice exists — a notice appearing and
/// disappearing would otherwise reflow the whole tree. It also gives the tree its position
/// indicator without costing a column of width, which matters in the stacked layout.
///
/// Character counts, not display widths: a neutralized notice is git's own prose and the position
/// is ASCII digits, so the two agree for everything this actually renders.
pub fn status_bar_line(notice: Option<&str>, cursor: usize, total: usize, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }
    let position = format!("{}/{}", cursor + 1, total);
    if position.chars().count() >= width {
        // No room for both: the position is the part that is always true.
        return position.chars().take(width).collect();
    }
    // One column of gap so a full-width notice cannot run into the position.
    let left_room = width - position.chars().count() - 1;
    let left: String = notice
        .map(safe)
        .unwrap_or_default()
        .chars()
        .take(left_room)
        .collect();
    let pad = width - position.chars().count() - left.chars().count();
    format!("{left}{}{position}", " ".repeat(pad))
}

/// Draw one frame and report where everything landed.
pub fn draw(frame: &mut Frame, controller: &mut Controller, bindings: &Bindings) -> Hits {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return Hits::default();
    }
    // One row of title, one of status bar, the rest for the body. On a one-row pane the title
    // wins: it still says how many repos are dirty, which beats a single unreadable tree row.
    //
    // The status bar costs a row, so it is only carved out when the pane can spare one —
    // title + status + three rows of content. Below that a notice overlays the bottom line
    // instead, as it always did.
    let title_area = Rect::new(area.x, area.y, area.width, 1);
    let status_rows: u16 = if area.height >= 5 { 1 } else { 0 };
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1 + status_rows),
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
    if status_rows == 1 {
        let bar = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        let (cursor, total) = {
            let tree = controller.tree();
            (tree.cursor(), tree.rows().len())
        };
        frame.render_widget(
            Paragraph::new(status_bar_line(
                controller.notice(),
                cursor,
                total,
                area.width,
            ))
            .style(theme::style(Role::Chrome)),
            bar,
        );
    } else if let Some(notice) = controller.notice() {
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

/// Draw the tree, scrolled so the cursor keeps its context, and report each row's screen line.
fn draw_tree(frame: &mut Frame, controller: &mut Controller, area: Rect) -> Vec<(u16, usize)> {
    if area.height == 0 {
        return Vec::new();
    }
    let height = area.height as usize;
    let first = {
        let tree = controller.tree();
        window_start(
            controller.tree_scroll(),
            tree.rows().len(),
            tree.cursor(),
            height,
            SCROLLOFF,
        )
    };
    controller.set_tree_scroll(first);
    let focused = controller.focus() == Focus::Tree;
    let tree = controller.tree();
    let win = Window {
        first,
        cursor: tree.cursor(),
        height,
        base_y: area.y,
        focused,
    };
    let (lines, hits) = windowed_rows(tree.rows(), controller.repos(), win, |id| {
        tree.is_collapsed(id)
    });
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    hits
}

/// How many rows of context to keep above and below the cursor.
///
/// Not configurable: a panel this size has one right answer, and spec §3 keeps the config file
/// out of this change entirely.
const SCROLLOFF: usize = 3;

/// Where a `height`-tall window should start, given where it started on the previous frame.
///
/// vim's scrolloff rule: the window holds still while the cursor stays at least `scrolloff` rows
/// from either edge, and otherwise moves the minimum needed to restore that margin. The
/// alternative — deriving the start from the cursor alone — is stateless but makes the whole pane
/// move on every keypress.
///
/// Pure by construction: the caller owns the `prev` cell ([`Controller::tree_scroll`]), so this
/// stays a table-testable function rather than becoming a stateful widget.
///
/// The final clamp is load-bearing rather than defensive. The panel re-polls every few seconds
/// and `total` shrinks under the user — a repo goes clean, a group collapses — so a remembered
/// `prev` is routinely out of range by the next frame.
pub fn window_start(
    prev: usize,
    total: usize,
    cursor: usize,
    height: usize,
    scrolloff: usize,
) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    let max_first = total - height;
    // Capped so the top and bottom margins can both hold in a short window; without this a
    // scrolloff wider than the window would fight itself.
    let off = scrolloff.min((height - 1) / 2);
    let mut first = prev.min(max_first);
    if cursor < first + off {
        first = cursor.saturating_sub(off);
    } else if cursor + off + 1 > first + height {
        first = (cursor + off + 1).saturating_sub(height);
    }
    first.min(max_first)
}

/// Which slice of the row list to draw, where it lands on screen, and how the cursor row should
/// look. Bundled rather than passed loose so the parameter count stays under clippy's limit.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// Index of the first row to draw.
    first: usize,
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
    let mut lines = Vec::new();
    let mut hits = Vec::new();
    for (offset, row) in rows.iter().skip(win.first).take(win.height).enumerate() {
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
        let index = win.first + offset;
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

/// Draw the diff region, titled with the file it belongs to.
fn draw_diff(frame: &mut Frame, controller: &Controller, area: Rect, orientation: Orientation) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let focused = controller.focus() == Focus::Diff;
    // In the stacked layout the border doubles as the divider and the title sits inside it; in
    // the side-by-side one the title is simply the column's first row, as the README draws it.
    let (borders, border_cols) = match orientation {
        Orientation::Stacked => (Borders::TOP, 0),
        Orientation::SideBySide => (Borders::LEFT, 1),
    };
    let title = diff_title(
        controller.tree().selected(),
        controller.repos(),
        area.width.saturating_sub(border_cols),
    );
    let block = Block::default()
        .borders(borders)
        .border_style(theme::pane_border(focused))
        // The title's style must not inherit the border's, or an unfocused pane would grey out
        // its own file name — hence `border_style` above rather than `style`.
        .title(Line::styled(title, theme::style(Role::DiffTitle)));
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

/// A one-line transient notice along the bottom, for panes too short to afford a status bar.
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
    let width = help_width(&lines, area.width);
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::style(Role::Chrome))
                .title("Keys"),
        ),
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

    // ---- window_start: where the tree's viewport begins ---------------------------------------

    #[test]
    fn a_window_that_fits_every_row_starts_at_the_top() {
        assert_eq!(window_start(0, 5, 4, 10, 3), 0);
        assert_eq!(
            window_start(7, 5, 4, 10, 3),
            0,
            "a stale prev is clamped, not trusted"
        );
        assert_eq!(
            window_start(0, 0, 0, 10, 3),
            0,
            "an empty list has nowhere to scroll"
        );
    }

    #[test]
    fn the_window_holds_still_while_the_cursor_stays_inside_the_margins() {
        // total 100, height 10, scrolloff 3: rows 3..=6 of a window at 0 need no scroll.
        for cursor in 3..=6 {
            assert_eq!(window_start(0, 100, cursor, 10, 3), 0, "cursor {cursor}");
        }
    }

    #[test]
    fn the_window_scrolls_just_far_enough_to_keep_the_bottom_margin() {
        // The bug this replaces pinned the cursor to the last row, so nothing was ever visible
        // below it.
        assert_eq!(window_start(0, 100, 7, 10, 3), 1);
        assert_eq!(window_start(0, 100, 8, 10, 3), 2);
    }

    #[test]
    fn the_window_scrolls_back_to_keep_the_top_margin() {
        assert_eq!(window_start(90, 100, 92, 10, 3), 89);
    }

    #[test]
    fn the_cursor_still_reaches_the_last_row_at_the_end_of_the_list() {
        // No phantom rows below the end: the window stops at total - height.
        assert_eq!(window_start(0, 100, 99, 10, 3), 90);
    }

    #[test]
    fn a_remembered_start_past_the_end_is_clamped_rather_than_showing_a_blank_pane() {
        // Load-bearing, not defensive: the panel re-polls every few seconds and `total` shrinks
        // under the user when a repo goes clean or a group collapses.
        assert_eq!(window_start(95, 20, 19, 10, 3), 10);
    }

    #[test]
    fn a_one_row_window_still_tracks_the_cursor() {
        assert_eq!(window_start(0, 100, 5, 1, 3), 5);
    }

    #[test]
    fn a_scrolloff_taller_than_the_window_degrades_instead_of_pinning_the_cursor() {
        // off is capped at (height - 1) / 2 so the top and bottom margins can both hold.
        assert_eq!(window_start(0, 100, 0, 3, 99), 0);
        assert_eq!(window_start(0, 100, 2, 3, 99), 1);
    }

    #[test]
    fn a_zero_height_window_has_nowhere_to_start() {
        assert_eq!(window_start(4, 100, 50, 0, 3), 0);
    }

    #[test]
    fn the_cursor_is_no_longer_pinned_to_the_bottom_row_of_the_tree() {
        // A window at the end of a long list: with the old rule the cursor sat on the last
        // visible row and nothing below it was ever drawn.
        let total = 100;
        let height = 10;
        let cursor = 50;
        let first = window_start(0, total, cursor, height, SCROLLOFF);
        assert!(
            cursor < first + height - 1,
            "rows below the cursor must be visible: first {first}, cursor {cursor}"
        );
        assert!(cursor >= first, "the cursor must be inside the window");
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
                first: 0,
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
        controller: &mut crate::controller::Controller,
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
    fn screen(width: u16, height: u16, controller: &mut crate::controller::Controller) -> String {
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
        let mut controller = crate::controller::tests_support::loaded_controller();
        let out = screen(140, 12, &mut controller);
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
        let mut controller = crate::controller::tests_support::loaded_controller();
        let out = screen(60, 12, &mut controller);
        assert!(out.contains("teleagent"), "{out}");
        assert!(
            out.contains('\u{2500}'),
            "a narrow pane draws a horizontal divider: {out}"
        );
    }

    #[test]
    fn a_tiny_pane_draws_without_panicking() {
        // A herdr split can be dragged arbitrarily small; every size must be survivable.
        let mut controller = crate::controller::tests_support::loaded_controller();
        for (w, h) in [(1u16, 1u16), (2, 1), (1, 2), (10, 3), (0, 0)] {
            let _ = screen(w.max(1), h.max(1), &mut controller);
        }
    }

    #[test]
    fn an_empty_panel_draws_the_empty_state_rather_than_a_blank_pane() {
        let mut controller = crate::controller::tests_support::empty_controller();
        let out = screen(80, 12, &mut controller);
        assert!(out.contains("No git repositories"), "{out}");
    }

    #[test]
    fn the_tree_says_it_has_focus_even_though_it_has_no_border_to_brighten() {
        // Before this, the tree's focus cue was BOLD on a Borders::NONE block, which the
        // Paragraph drew straight over: pressing Tab changed nothing on screen.
        let mut controller = crate::controller::tests_support::loaded_controller();
        // The cursor starts on row 0 of the tree, which is screen row 1 (row 0 is the title).
        let focused = cell_style(140, 12, &mut controller, 0, 1);
        assert!(
            focused.add_modifier.contains(Modifier::REVERSED),
            "the tree has focus, so its cursor row is reversed: {focused:?}"
        );

        controller.handle(crate::intent::Intent::FocusToggle);
        let unfocused = cell_style(140, 12, &mut controller, 0, 1);
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
        let tree_has_focus = cell_style(140, 12, &mut controller, divider_x, 1);
        controller.handle(crate::intent::Intent::FocusToggle);
        let diff_has_focus = cell_style(140, 12, &mut controller, divider_x, 1);
        assert_ne!(
            tree_has_focus, diff_has_focus,
            "the divider must reflect which side is active"
        );
    }

    #[test]
    fn a_rows_own_colours_survive_being_selected() {
        // The selection is a modifier, not a colour, precisely so this holds.
        let mut controller = crate::controller::tests_support::loaded_controller();
        let branch_style = theme::style(Role::Branch).patch(theme::selection(true));
        assert_eq!(branch_style.fg, theme::style(Role::Branch).fg);
        assert!(branch_style.add_modifier.contains(Modifier::REVERSED));
        // And the pane still draws.
        let _ = screen(140, 12, &mut controller);
    }

    // ---- the status bar ----------------------------------------------------------------------

    #[test]
    fn the_status_bar_reports_the_cursor_position_on_the_right() {
        let bar = status_bar_line(None, 0, 48, 20);
        assert_eq!(bar.chars().count(), 20, "{bar:?}");
        assert!(bar.ends_with("1/48"), "{bar:?}");
        assert_eq!(bar.trim_start(), "1/48", "{bar:?}");
    }

    #[test]
    fn a_notice_sits_on_the_left_without_pushing_the_position_off() {
        let bar = status_bar_line(Some("copied"), 11, 48, 20);
        assert!(bar.starts_with("copied"), "{bar:?}");
        assert!(bar.ends_with("12/48"), "{bar:?}");
        assert_eq!(bar.chars().count(), 20, "{bar:?}");
    }

    #[test]
    fn a_long_notice_is_truncated_rather_than_evicting_the_position() {
        let bar = status_bar_line(Some(&"x".repeat(200)), 0, 9, 20);
        assert!(bar.ends_with("1/9"), "{bar:?}");
        assert_eq!(bar.chars().count(), 20, "{bar:?}");
    }

    #[test]
    fn a_notice_carrying_control_characters_is_neutralized_in_the_status_bar() {
        // A notice can quote a file name or git's stderr, both untrusted.
        let bar = status_bar_line(Some("a\x1b[2Jb\nc"), 0, 1, 20);
        assert!(!bar.contains('\x1b'), "{bar:?}");
        assert_eq!(bar.lines().count(), 1, "{bar:?}");
    }

    #[test]
    fn a_bar_too_narrow_for_both_keeps_the_position() {
        // The position is the part that is always true, so it is the part that survives.
        assert_eq!(status_bar_line(Some("copied"), 0, 9, 3), "1/9");
        assert_eq!(status_bar_line(None, 0, 9, 0), "");
    }

    #[test]
    fn the_status_bar_has_its_own_row_instead_of_eating_a_tree_row() {
        // draw_notice used to Clear the bottom line of the pane, silently costing a tree row.
        let mut controller = crate::controller::tests_support::loaded_controller();
        let out = screen(80, 12, &mut controller);
        let last = out.lines().last().expect("a last line");
        assert!(
            last.contains("1/"),
            "the bottom row is the status bar: {out}"
        );
        assert!(out.contains("teleagent"), "{out}");
    }

    #[test]
    fn a_pane_with_no_room_for_a_status_bar_still_draws_the_tree() {
        let mut controller = crate::controller::tests_support::loaded_controller();
        let out = screen(80, 4, &mut controller);
        assert!(out.contains("teleagent"), "{out}");
    }

    // ---- the diff title ------------------------------------------------------------------------

    fn file_row(path: &str) -> Row {
        Row {
            id: RowId::File {
                repo: PathBuf::from("/w/teleagent"),
                group: crate::model::GroupKind::Changes,
                path: path.to_string(),
            },
            depth: 2,
            repo_idx: 0,
            group_idx: Some(0),
            file_idx: Some(0),
        }
    }

    #[test]
    fn the_diff_title_names_the_selected_file() {
        let row = file_row("e2e/specs/07-authz.spec.ts");
        assert_eq!(
            diff_title(Some(&row), &[entry()], 80),
            "e2e/specs/07-authz.spec.ts"
        );
    }

    #[test]
    fn the_diff_title_falls_back_to_the_repo_when_a_repo_row_is_selected() {
        assert_eq!(
            diff_title(Some(&repo_row(0)), &[named_repo("teleagent")], 80),
            "teleagent"
        );
    }

    #[test]
    fn the_diff_title_says_so_when_nothing_is_selected() {
        assert_eq!(diff_title(None, &[], 80), "no file selected");
    }

    #[test]
    fn a_too_long_diff_title_keeps_the_file_name_and_marks_the_cut() {
        // Truncated from the LEFT: the file name matters more than the directories above it.
        let row = file_row("a/very/deep/path/to/the/file.rs");
        let title = diff_title(Some(&row), &[entry()], 12);
        assert_eq!(title.chars().count(), 12, "{title:?}");
        assert_eq!(title, "…the/file.rs");
    }

    #[test]
    fn a_diff_title_with_no_room_at_all_does_not_panic() {
        let row = file_row("some/file.rs");
        assert_eq!(diff_title(Some(&row), &[entry()], 1), "…");
        assert_eq!(diff_title(Some(&row), &[entry()], 0), "");
    }

    #[test]
    fn a_diff_title_carrying_control_characters_is_neutralized() {
        let row = file_row("a\x1b[2Jb");
        let title = diff_title(Some(&row), &[entry()], 80);
        assert!(!title.contains('\x1b'), "{title:?}");
    }

    #[test]
    fn a_row_whose_repo_no_longer_resolves_gets_an_empty_title_rather_than_a_panic() {
        assert_eq!(diff_title(Some(&repo_row(99)), &[], 80), "");
    }

    #[test]
    fn both_layouts_name_the_selected_file_above_its_diff() {
        // draw_diff's doc comment claimed a title in the stacked layout, but neither branch set
        // one — and the README drew it in both.
        let mut controller = crate::controller::tests_support::loaded_controller();
        controller.handle(crate::intent::Intent::NextChange);
        for (w, h) in [(140u16, 12u16), (60u16, 16u16)] {
            let out = screen(w, h, &mut controller);
            assert_eq!(
                out.matches("07-authz.spec.ts").count(),
                2,
                "{w}x{h}: once in the tree row, once as the diff's title: {out}"
            );
        }
    }

    // ---- the help overlay's width ---------------------------------------------------------------

    #[test]
    fn the_help_overlay_is_only_as_wide_as_its_widest_line() {
        // It used to be area.width - 4, stretching a ~60-column key list across the whole pane.
        let lines = help_lines(&default_bindings());
        let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        assert_eq!(help_width(&lines, 200), (widest + 4) as u16);
    }

    #[test]
    fn the_help_overlay_never_outgrows_the_pane() {
        let lines = help_lines(&default_bindings());
        assert_eq!(help_width(&lines, 20), 18);
        assert_eq!(help_width(&lines, 1), 1, "never zero-width");
        assert_eq!(help_width(&lines, 0), 1);
    }

    #[test]
    fn the_help_overlay_draws_narrower_than_the_pane() {
        let mut controller = crate::controller::tests_support::loaded_controller();
        controller.handle(crate::intent::Intent::Help);
        let out = screen(160, 30, &mut controller);
        assert!(out.contains("Keys"), "{out}");
        // Measure the width of the overlay's own top border row (the one containing "Keys"),
        // not the maximum over every rendered line: the permanent status bar (task 5) renders a
        // full-width row ending in a digit, so trim_end() leaves all of it and the maximum over
        // every line is always the pane width regardless of the overlay's own width.
        let widest = out
            .lines()
            .find(|l| l.contains("Keys"))
            .map(|l| l.trim_end().chars().count())
            .unwrap_or(0);
        assert!(
            widest < 156,
            "the overlay still spans nearly the whole pane: {widest}"
        );
    }
}

//! Intent -> state. The controller owns everything the presenter draws and nothing that
//! touches the outside world directly: git, the clipboard, herdr and the editor all arrive as
//! injected seams, so the whole thing is unit-tested with stubs (spec §11).

use crate::config::Settings;
use crate::herdr::{self, HerdrCli};
use crate::intent::Intent;
use crate::model::{GroupKind, RepoEntry, Snapshot};
use crate::poller::{DiffJob, DiffResult, JobSink, PollMsg};
use crate::render::{self, Rendered};
use crate::tree::Tree;
use ratatui::text::Text;
use std::path::{Path, PathBuf};

/// Which region the keys act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Diff,
}

/// The terminal clipboard seam (OSC 52 in production).
pub trait Clipboard {
    fn copy(&mut self, text: &str) -> std::io::Result<()>;
}

/// The `$EDITOR` hand-off seam. The controller never performs the hand-off itself — it happens
/// in the run loop, which owns the terminal.
pub trait EditorHandoff {
    fn open(&mut self, path: &Path) -> Result<(), String>;
}

/// What the run loop should do after an intent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Effects {
    pub redraw: bool,
    pub quit: bool,
    /// Ask the poller for a round now.
    pub refresh: bool,
    /// Hand the terminal to `$EDITOR` for this file.
    pub editor: Option<PathBuf>,
}

/// The injected seams.
pub struct Deps {
    pub sink: Box<dyn JobSink>,
    pub clipboard: Box<dyn Clipboard>,
    pub herdr: Box<dyn HerdrCli>,
    pub editor: Box<dyn EditorHandoff>,
}

/// The panel's state.
pub struct Controller {
    settings: Settings,
    deps: Deps,
    repos: Vec<RepoEntry>,
    tree: Tree,
    focus: Focus,
    generation: u64,
    /// The next diff job's sequence number; also the only result sequence accepted.
    diff_seq: u64,
    /// What the current diff belongs to, so a refresh does not re-request the same file.
    diff_target: Option<(PathBuf, GroupKind, String)>,
    diff_text: Text<'static>,
    diff_scroll: u16,
    notice: Option<String>,
    help_open: bool,
    scan_roots: Vec<PathBuf>,
}

impl Controller {
    pub fn new(settings: Settings, deps: Deps) -> Controller {
        Controller {
            settings,
            deps,
            repos: Vec::new(),
            tree: Tree::new(),
            focus: Focus::Tree,
            generation: 0,
            diff_seq: 0,
            diff_target: None,
            diff_text: Text::raw(""),
            diff_scroll: 0,
            notice: None,
            help_open: false,
            scan_roots: Vec::new(),
        }
    }

    // ---- readers for the presenter -------------------------------------------------------

    pub fn repos(&self) -> &[RepoEntry] {
        &self.repos
    }
    pub fn tree(&self) -> &Tree {
        &self.tree
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub fn diff_text(&self) -> &Text<'static> {
        &self.diff_text
    }
    pub fn diff_scroll(&self) -> u16 {
        self.diff_scroll
    }
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
    pub fn help_open(&self) -> bool {
        self.help_open
    }
    pub fn scan_roots(&self) -> &[PathBuf] {
        &self.scan_roots
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// No repos at all — the spec §5.4 empty state.
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }
    /// `(repo count, dirty repo count)` for the title bar.
    pub fn title_counts(&self) -> (usize, usize) {
        (
            self.repos.len(),
            self.repos.iter().filter(|r| r.dirty_count() > 0).count(),
        )
    }

    // ---- inbound messages -----------------------------------------------------------------

    /// Record the scan starts, so the empty state can show where we looked (spec §5.4).
    pub fn set_scan_roots(&mut self, roots: Vec<PathBuf>) {
        self.scan_roots = roots;
    }

    /// Apply a poller message. An out-of-order snapshot is dropped.
    pub fn apply(&mut self, msg: PollMsg) {
        match msg {
            // The repo list itself only matters via the snapshot that follows it, but the scan
            // starts it carries drive the empty state's "we looked here" list.
            PollMsg::Roots(roots) => {
                // First-seen-order de-duplication: `Vec::dedup()` only removes CONSECUTIVE
                // duplicates, and scan starts are not sorted, so a repeat further down the list
                // would otherwise survive.
                let mut seen = std::collections::HashSet::new();
                let starts: Vec<PathBuf> = roots
                    .iter()
                    .map(|r| r.scan_root.clone())
                    .filter(|p| seen.insert(p.clone()))
                    .collect();
                if !starts.is_empty() {
                    self.scan_roots = starts;
                }
            }
            PollMsg::Snapshot(Snapshot { repos, generation }) => {
                if generation <= self.generation {
                    return;
                }
                self.generation = generation;
                self.repos = repos;
                self.tree.rebuild(&self.repos);
                self.sync_diff();
            }
        }
    }

    /// Accept a finished diff, unless the user has already moved on (spec §7).
    pub fn apply_diff(&mut self, result: DiffResult) {
        if result.seq != self.diff_seq {
            return;
        }
        let Rendered { text, notice } = result.rendered;
        self.diff_text = text;
        if let Some(notice) = notice {
            self.notice = Some(notice);
        }
    }

    // ---- intents ----------------------------------------------------------------------------

    pub fn handle(&mut self, intent: Intent) -> Effects {
        // A fresh keystroke clears the previous action's notice; a diff notice re-appears with
        // its own result.
        self.notice = None;
        let mut effects = Effects {
            redraw: true,
            ..Effects::default()
        };
        // An open overlay swallows navigation: only Close, Help and Quit get through.
        if self.help_open && !matches!(intent, Intent::Close | Intent::Help | Intent::Quit) {
            return effects;
        }
        match intent {
            Intent::NavDown => self.nav(1),
            Intent::NavUp => self.nav(-1),
            Intent::Activate => {
                self.tree.toggle_selected(&self.repos);
                self.sync_diff();
            }
            Intent::ToggleAll => {
                let collapse = !self.tree.all_collapsed(&self.repos);
                self.tree.set_all_collapsed(collapse, &self.repos);
                self.sync_diff();
            }
            Intent::FocusToggle => {
                self.focus = match self.focus {
                    Focus::Tree => Focus::Diff,
                    Focus::Diff => Focus::Tree,
                }
            }
            Intent::NextChange => {
                self.tree.next_file(&self.repos);
                self.sync_diff();
            }
            Intent::PrevChange => {
                self.tree.prev_file(&self.repos);
                self.sync_diff();
            }
            Intent::Refresh => effects.refresh = true,
            Intent::Zoom => herdr::zoom_current(self.deps.herdr.as_ref()),
            Intent::CopyPath => self.copy_path(),
            Intent::OpenEditor => effects.editor = self.editor_target(),
            Intent::Help => self.help_open = true,
            Intent::Close => self.help_open = false,
            Intent::Quit => effects.quit = true,
        }
        effects
    }

    /// Navigation routes by focus: the tree cursor, or the diff's scroll offset.
    fn nav(&mut self, delta: isize) {
        match self.focus {
            Focus::Tree => {
                self.tree.move_cursor(delta);
                self.sync_diff();
            }
            Focus::Diff => {
                self.diff_scroll = self
                    .diff_scroll
                    .saturating_add_signed(delta.clamp(-1, 1) as i16);
            }
        }
    }

    /// Request a diff for whatever is selected now — but only when the target actually changed,
    /// so a background refresh does not re-run git diff and re-spawn the renderer every round.
    fn sync_diff(&mut self) {
        let target = self.tree.selected_file();
        if target == self.diff_target {
            return;
        }
        self.diff_target = target.clone();
        self.diff_scroll = 0;
        let Some((repo, group, path)) = target else {
            self.diff_text = Text::raw("");
            return;
        };
        self.diff_seq += 1;
        self.deps.sink.submit(DiffJob {
            seq: self.diff_seq,
            repo,
            group,
            path,
        });
    }

    /// Copy `repo:path` for the selected file (spec §5.3's `y`).
    ///
    /// Both the clipboard payload and the notice are neutralized first: a file name is chosen by
    /// whoever wrote the repo, and an OSC sequence inside one must never reach the terminal.
    fn copy_path(&mut self) {
        let Some((repo, _, path)) = self.tree.selected_file() else {
            self.notice = Some("no file selected".to_string());
            return;
        };
        let name = self
            .repos
            .iter()
            .find(|r| r.path == repo)
            .map(|r| r.display_name.clone())
            .unwrap_or_else(|| repo.to_string_lossy().into_owned());
        let text = render::neutralize_plain_text(&format!("{name}:{path}"));
        self.notice = Some(match self.deps.clipboard.copy(&text) {
            Ok(()) => format!("copied {text}"),
            Err(e) => format!("could not copy: {e}"),
        });
    }

    /// The absolute path to hand to `$EDITOR`, or `None` with a notice explaining why not.
    fn editor_target(&mut self) -> Option<PathBuf> {
        match self.tree.selected_file() {
            Some((repo, _, path)) => Some(repo.join(path)),
            None => {
                self.notice = Some("no file selected".to_string());
                None
            }
        }
    }

    /// Report the outcome of the run loop's editor hand-off.
    pub fn editor_finished(&mut self, path: &Path) {
        if let Err(e) = self.deps.editor.open(path) {
            self.notice = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::model::{FileEntry, RepoEntry, RepoKind, RepoRoot, StatusGroup};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Recorder {
        jobs: RefCell<Vec<DiffJob>>,
        copied: RefCell<Vec<String>>,
        herdr_calls: RefCell<Vec<Vec<String>>>,
        opened: RefCell<Vec<PathBuf>>,
        /// When set, `StubEditor::open` fails instead of recording the hand-off.
        fail_editor: RefCell<bool>,
    }

    struct StubSink(Rc<Recorder>);
    impl JobSink for StubSink {
        fn submit(&self, job: DiffJob) {
            self.0.jobs.borrow_mut().push(job);
        }
    }

    struct StubClipboard(Rc<Recorder>);
    impl Clipboard for StubClipboard {
        fn copy(&mut self, text: &str) -> std::io::Result<()> {
            self.0.copied.borrow_mut().push(text.to_string());
            Ok(())
        }
    }

    struct StubHerdr(Rc<Recorder>);
    impl crate::herdr::HerdrCli for StubHerdr {
        fn run_json(&self, args: &[&str]) -> std::io::Result<String> {
            self.0
                .herdr_calls
                .borrow_mut()
                .push(args.iter().map(|a| a.to_string()).collect());
            Ok(String::new())
        }
    }

    struct StubEditor(Rc<Recorder>);
    impl EditorHandoff for StubEditor {
        fn open(&mut self, path: &Path) -> Result<(), String> {
            if *self.0.fail_editor.borrow() {
                return Err("editor exploded".to_string());
            }
            self.0.opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
    }

    /// A `RepoRoot` for a scan root at `path`, for `PollMsg::Roots` tests.
    fn root(path: &str) -> RepoRoot {
        RepoRoot {
            path: PathBuf::from(path),
            scan_root: PathBuf::from(path),
            kind: RepoKind::Root,
        }
    }

    fn controller() -> (Controller, Rc<Recorder>) {
        let rec = Rc::new(Recorder::default());
        let c = Controller::new(
            Settings::default(),
            Deps {
                sink: Box::new(StubSink(rec.clone())),
                clipboard: Box::new(StubClipboard(rec.clone())),
                herdr: Box::new(StubHerdr(rec.clone())),
                editor: Box::new(StubEditor(rec.clone())),
            },
        );
        (c, rec)
    }

    fn repo(name: &str, files: &[(GroupKind, &str)]) -> RepoEntry {
        let mut groups: Vec<StatusGroup> = Vec::new();
        for (kind, path) in files {
            let entry = FileEntry {
                path: path.to_string(),
                status: 'M',
                orig_path: None,
            };
            match groups.iter_mut().find(|g| g.kind == *kind) {
                Some(g) => g.files.push(entry),
                None => groups.push(StatusGroup {
                    kind: *kind,
                    files: vec![entry],
                }),
            }
        }
        RepoEntry {
            path: PathBuf::from(format!("/w/{name}")),
            display_name: name.to_string(),
            rel_path: name.to_string(),
            kind: RepoKind::Nested,
            branch: Some("main".to_string()),
            groups,
            ..RepoEntry::blank()
        }
    }

    fn snapshot(repos: Vec<RepoEntry>, generation: u64) -> PollMsg {
        PollMsg::Snapshot(Snapshot { repos, generation })
    }

    fn loaded() -> (Controller, Rc<Recorder>) {
        let (mut c, rec) = controller();
        c.apply(snapshot(
            vec![
                repo(
                    "a",
                    &[(GroupKind::Changes, "x.rs"), (GroupKind::Changes, "y.rs")],
                ),
                repo("b", &[(GroupKind::Untracked, "z.rs")]),
            ],
            1,
        ));
        (c, rec)
    }

    // ---- snapshots -----------------------------------------------------------------------

    #[test]
    fn a_snapshot_populates_the_tree_and_the_header_counts() {
        let (c, _) = loaded();
        assert_eq!(c.tree().rows().len(), 7);
        assert_eq!(c.title_counts(), (2, 2)); // 2 repos, 2 dirty
        assert!(!c.is_empty());
    }

    #[test]
    fn an_empty_repo_list_puts_the_panel_in_its_empty_state() {
        // spec §5.4: never a blank pane.
        let (mut c, _) = controller();
        c.set_scan_roots(vec![PathBuf::from("/w")]);
        c.apply(snapshot(vec![], 1));
        assert!(c.is_empty());
        assert_eq!(c.scan_roots(), [PathBuf::from("/w")]);
    }

    #[test]
    fn a_repo_with_no_changes_is_not_counted_as_dirty() {
        let (mut c, _) = controller();
        c.apply(snapshot(vec![repo("clean", &[])], 1));
        assert_eq!(c.title_counts(), (1, 0));
    }

    #[test]
    fn a_stale_snapshot_arriving_late_is_ignored() {
        let (mut c, _) = loaded();
        c.apply(snapshot(vec![repo("z", &[])], 0)); // an older generation
        assert_eq!(c.repos().len(), 2);
    }

    // ---- PollMsg::Roots --------------------------------------------------------------------

    #[test]
    fn roots_message_deduplicates_scan_roots_preserving_first_seen_order() {
        // A NON-CONTIGUOUS duplicate: plain `Vec::dedup()` only removes consecutive runs, so
        // this must be a first-seen-order dedup, not a sort-then-dedup.
        let (mut c, _) = controller();
        c.apply(PollMsg::Roots(vec![
            root("/w/a"),
            root("/w/b"),
            root("/w/a"),
        ]));
        assert_eq!(
            c.scan_roots(),
            [PathBuf::from("/w/a"), PathBuf::from("/w/b")]
        );
    }

    #[test]
    fn an_empty_roots_message_does_not_wipe_previously_recorded_scan_roots() {
        let (mut c, _) = controller();
        c.apply(PollMsg::Roots(vec![root("/w/a")]));
        c.apply(PollMsg::Roots(vec![]));
        assert_eq!(c.scan_roots(), [PathBuf::from("/w/a")]);
    }

    // ---- navigation and diff jobs -------------------------------------------------------

    #[test]
    fn landing_on_a_file_row_requests_its_diff_with_the_right_baseline() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NavDown); // group row
        c.handle(Intent::NavDown); // x.rs
        let jobs = rec.jobs.borrow();
        let last = jobs.last().expect("a diff job");
        assert_eq!(last.repo, PathBuf::from("/w/a"));
        assert_eq!(last.group, GroupKind::Changes);
        assert_eq!(last.path, "x.rs");
    }

    #[test]
    fn moving_onto_a_repo_or_group_row_requests_no_diff() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NavDown); // the Changes group row
        assert!(rec.jobs.borrow().is_empty());
    }

    #[test]
    fn each_diff_job_carries_a_higher_sequence_than_the_last() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        c.handle(Intent::NextChange);
        let jobs = rec.jobs.borrow();
        assert!(jobs.len() >= 2);
        assert!(jobs[1].seq > jobs[0].seq);
    }

    #[test]
    fn a_diff_result_for_a_superseded_job_is_discarded() {
        // spec §7: the user moved on; the late result must not overwrite what they are reading.
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        c.handle(Intent::NextChange);
        let stale_seq = rec.jobs.borrow()[0].seq;
        let current_seq = rec.jobs.borrow()[1].seq;

        c.apply_diff(DiffResult {
            seq: current_seq,
            rendered: Rendered {
                text: ratatui::text::Text::raw("CURRENT"),
                notice: None,
            },
        });
        c.apply_diff(DiffResult {
            seq: stale_seq,
            rendered: Rendered {
                text: ratatui::text::Text::raw("STALE"),
                notice: None,
            },
        });
        assert!(flatten(c.diff_text()).contains("CURRENT"));
        assert!(!flatten(c.diff_text()).contains("STALE"));
    }

    #[test]
    fn a_diff_notice_for_the_current_job_is_forwarded() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        let seq = rec.jobs.borrow().last().expect("a diff job").seq;
        c.apply_diff(DiffResult {
            seq,
            rendered: Rendered {
                text: ratatui::text::Text::raw("body"),
                notice: Some("delta is not available — showing plain text".to_string()),
            },
        });
        assert_eq!(
            c.notice(),
            Some("delta is not available — showing plain text")
        );
    }

    #[test]
    fn a_diff_notice_for_a_superseded_job_is_not_forwarded() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        c.handle(Intent::NextChange);
        let stale_seq = rec.jobs.borrow()[0].seq;
        c.apply_diff(DiffResult {
            seq: stale_seq,
            rendered: Rendered {
                text: ratatui::text::Text::raw("STALE"),
                notice: Some("must not surface".to_string()),
            },
        });
        assert!(c.notice().is_none());
    }

    #[test]
    fn re_selecting_the_same_file_after_a_refresh_does_not_re_request_its_diff() {
        // Otherwise every poll round re-runs git diff and re-spawns delta for the same file.
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        let before = rec.jobs.borrow().len();
        c.apply(snapshot(
            vec![
                repo(
                    "a",
                    &[(GroupKind::Changes, "x.rs"), (GroupKind::Changes, "y.rs")],
                ),
                repo("b", &[(GroupKind::Untracked, "z.rs")]),
            ],
            2,
        ));
        assert_eq!(rec.jobs.borrow().len(), before);
    }

    #[test]
    fn cross_repo_jumping_reaches_the_other_repos_file() {
        let (mut c, rec) = loaded();
        for _ in 0..3 {
            c.handle(Intent::NextChange);
        }
        assert_eq!(rec.jobs.borrow().last().expect("job").path, "z.rs");
    }

    // ---- focus and diff scrolling -------------------------------------------------------

    #[test]
    fn tab_moves_focus_between_the_tree_and_the_diff() {
        let (mut c, _) = loaded();
        assert_eq!(c.focus(), Focus::Tree);
        c.handle(Intent::FocusToggle);
        assert_eq!(c.focus(), Focus::Diff);
        c.handle(Intent::FocusToggle);
        assert_eq!(c.focus(), Focus::Tree);
    }

    #[test]
    fn with_the_diff_focused_the_navigation_keys_scroll_the_diff_not_the_tree() {
        let (mut c, _) = loaded();
        let cursor = c.tree().cursor();
        c.handle(Intent::FocusToggle);
        c.handle(Intent::NavDown);
        assert_eq!(c.tree().cursor(), cursor);
        assert_eq!(c.diff_scroll(), 1);
        c.handle(Intent::NavUp);
        assert_eq!(c.diff_scroll(), 0);
    }

    #[test]
    fn the_diff_scroll_never_goes_negative() {
        let (mut c, _) = loaded();
        c.handle(Intent::FocusToggle);
        for _ in 0..5 {
            c.handle(Intent::NavUp);
        }
        assert_eq!(c.diff_scroll(), 0);
    }

    #[test]
    fn selecting_a_different_file_resets_the_diff_scroll() {
        let (mut c, _) = loaded();
        c.handle(Intent::NextChange);
        c.handle(Intent::FocusToggle);
        c.handle(Intent::NavDown);
        assert_eq!(c.diff_scroll(), 1);
        c.handle(Intent::FocusToggle);
        c.handle(Intent::NextChange);
        assert_eq!(c.diff_scroll(), 0);
    }

    // ---- the action keys -----------------------------------------------------------------

    #[test]
    fn copying_a_path_writes_repo_colon_path_to_the_clipboard() {
        let (mut c, rec) = loaded();
        c.handle(Intent::NextChange);
        c.handle(Intent::CopyPath);
        assert_eq!(rec.copied.borrow().as_slice(), ["a:x.rs"]);
        assert!(c.notice().is_some());
    }

    #[test]
    fn copying_with_no_file_selected_says_so_instead_of_copying_something_wrong() {
        let (mut c, rec) = loaded();
        c.handle(Intent::CopyPath); // the cursor is on a repo row
        assert!(rec.copied.borrow().is_empty());
        assert!(c.notice().is_some());
    }

    #[test]
    fn a_path_carrying_control_characters_is_neutralized_before_it_reaches_the_clipboard() {
        // A file name is attacker-choosable; it must not be able to drive the terminal.
        let (mut c, rec) = controller();
        c.apply(snapshot(
            vec![repo("a", &[(GroupKind::Changes, "evil\x1b]52;c;x\x07.rs")])],
            1,
        ));
        c.handle(Intent::NextChange);
        c.handle(Intent::CopyPath);
        let copied = &rec.copied.borrow()[0];
        assert!(!copied.contains('\x1b'), "{copied:?}");
        assert!(!copied.contains('\x07'), "{copied:?}");
    }

    #[test]
    fn opening_the_editor_hands_over_the_absolute_path_of_the_selected_file() {
        let (mut c, _) = loaded();
        c.handle(Intent::NextChange);
        let effects = c.handle(Intent::OpenEditor);
        assert_eq!(effects.editor, Some(PathBuf::from("/w/a/x.rs")));
    }

    #[test]
    fn opening_the_editor_with_no_file_selected_is_a_notice_not_a_handoff() {
        let (mut c, _) = loaded();
        let effects = c.handle(Intent::OpenEditor);
        assert_eq!(effects.editor, None);
        assert!(c.notice().is_some());
    }

    #[test]
    fn editor_finished_records_the_handed_off_path_and_leaves_no_notice_on_success() {
        let (mut c, rec) = loaded();
        c.editor_finished(Path::new("/w/a/x.rs"));
        assert_eq!(rec.opened.borrow().as_slice(), [PathBuf::from("/w/a/x.rs")]);
        assert!(c.notice().is_none());
    }

    #[test]
    fn editor_finished_surfaces_a_failed_handoff_as_a_notice() {
        let (mut c, rec) = loaded();
        *rec.fail_editor.borrow_mut() = true;
        c.editor_finished(Path::new("/w/a/x.rs"));
        assert!(rec.opened.borrow().is_empty());
        assert_eq!(c.notice(), Some("editor exploded"));
    }

    #[test]
    fn zoom_asks_herdr_to_toggle_this_panes_zoom() {
        let (mut c, rec) = loaded();
        c.handle(Intent::Zoom);
        assert_eq!(
            rec.herdr_calls.borrow()[0],
            ["pane", "zoom", "--current", "--toggle"]
        );
    }

    #[test]
    fn refresh_asks_the_run_loop_to_poll_now() {
        let (mut c, _) = loaded();
        assert!(c.handle(Intent::Refresh).refresh);
    }

    #[test]
    fn toggle_all_collapses_everything_and_then_expands_it_again() {
        let (mut c, _) = loaded();
        c.handle(Intent::ToggleAll);
        assert_eq!(c.tree().rows().len(), 2);
        c.handle(Intent::ToggleAll);
        assert_eq!(c.tree().rows().len(), 7);
    }

    #[test]
    fn activate_collapses_the_selected_repo() {
        let (mut c, _) = loaded();
        c.handle(Intent::Activate);
        assert_eq!(c.tree().rows().len(), 4);
    }

    // ---- overlays and quitting -------------------------------------------------------------

    #[test]
    fn help_opens_and_escape_closes_it() {
        let (mut c, _) = loaded();
        c.handle(Intent::Help);
        assert!(c.help_open());
        c.handle(Intent::Close);
        assert!(!c.help_open());
    }

    #[test]
    fn escape_with_no_overlay_open_does_not_quit() {
        // Otherwise Esc becomes an accidental exit.
        let (mut c, _) = loaded();
        assert!(!c.handle(Intent::Close).quit);
    }

    #[test]
    fn quit_requests_an_exit() {
        let (mut c, _) = loaded();
        assert!(c.handle(Intent::Quit).quit);
    }

    #[test]
    fn while_help_is_open_the_navigation_keys_do_not_move_the_tree_cursor() {
        let (mut c, _) = loaded();
        c.handle(Intent::Help);
        let cursor = c.tree().cursor();
        c.handle(Intent::NavDown);
        assert_eq!(c.tree().cursor(), cursor);
    }

    /// The plain text of a `Text`, for assertions.
    fn flatten(text: &ratatui::text::Text<'_>) -> String {
        text.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

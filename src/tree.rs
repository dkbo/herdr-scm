//! The flattened row list, its expansion state and cursor (spec §5.2, §5.5).
//!
//! State is keyed by row IDENTITY (repo path + group + file path), never by index: a background
//! refresh reshuffles indices constantly, and index-keyed state would move the user's cursor
//! and re-open their collapsed repos on every poll round.

use crate::model::{GroupKind, RepoEntry};
use std::collections::HashSet;
use std::path::PathBuf;

/// A row's stable identity across rebuilds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowId {
    Repo {
        repo: PathBuf,
    },
    Group {
        repo: PathBuf,
        group: GroupKind,
    },
    File {
        repo: PathBuf,
        group: GroupKind,
        path: String,
    },
}

impl RowId {
    /// The row that must be expanded for this row to be visible, if any.
    fn parent(&self) -> Option<RowId> {
        match self {
            RowId::Repo { .. } => None,
            RowId::Group { repo, .. } => Some(RowId::Repo { repo: repo.clone() }),
            RowId::File { repo, group, .. } => Some(RowId::Group {
                repo: repo.clone(),
                group: *group,
            }),
        }
    }
}

/// One visible row, with the indices needed to reach its model data without a second lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: RowId,
    /// 0 = repo, 1 = group, 2 = file.
    pub depth: u16,
    pub repo_idx: usize,
    pub group_idx: Option<usize>,
    pub file_idx: Option<usize>,
}

/// The visible rows plus the state that survives a rebuild.
#[derive(Debug, Default)]
pub struct Tree {
    rows: Vec<Row>,
    /// Rows the user has collapsed. Absence means expanded, so a newly discovered repo starts
    /// open — which is what the user wants from a panel whose job is to surface changes.
    collapsed: HashSet<RowId>,
    cursor: usize,
}

impl Tree {
    pub fn new() -> Self {
        Tree::default()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    pub fn is_collapsed(&self, id: &RowId) -> bool {
        self.collapsed.contains(id)
    }

    /// Rebuild the visible rows from a fresh snapshot, restoring the cursor by identity.
    ///
    /// When the previously selected row is gone, the cursor falls back to the same index,
    /// clamped — the closest thing to "where the user was looking".
    pub fn rebuild(&mut self, repos: &[RepoEntry]) {
        let previous = self.selected().map(|r| r.id.clone());
        let previous_index = self.cursor;
        self.rows = flatten(repos, &self.collapsed);
        self.cursor = previous
            .and_then(|id| self.rows.iter().position(|r| r.id == id))
            .unwrap_or(previous_index)
            .min(self.rows.len().saturating_sub(1));
    }

    pub fn set_cursor(&mut self, index: usize) {
        self.cursor = index.min(self.rows.len().saturating_sub(1));
    }

    /// Move the cursor, clamping at both ends. Deliberately does NOT wrap: `j` at the bottom
    /// jumping to the top would lose the user's place in a long list.
    pub fn move_cursor(&mut self, delta: isize) {
        let target = self.cursor as isize + delta;
        let last = self.rows.len().saturating_sub(1) as isize;
        self.cursor = target.clamp(0, last.max(0)) as usize;
    }

    /// Expand or collapse the selected repo or group. A file row has no children, so it is a
    /// no-op.
    pub fn toggle_selected(&mut self, repos: &[RepoEntry]) {
        let Some(id) = self.selected().map(|r| r.id.clone()) else {
            return;
        };
        if matches!(id, RowId::File { .. }) {
            return;
        }
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
        self.rebuild(repos);
    }

    /// Collapse or expand every repo and group at once (the `a` key).
    pub fn set_all_collapsed(&mut self, collapsed: bool, repos: &[RepoEntry]) {
        self.collapsed.clear();
        if collapsed {
            for repo in repos {
                self.collapsed.insert(RowId::Repo {
                    repo: repo.path.clone(),
                });
                for group in &repo.groups {
                    self.collapsed.insert(RowId::Group {
                        repo: repo.path.clone(),
                        group: group.kind,
                    });
                }
            }
        }
        self.rebuild(repos);
    }

    /// Whether every repo is currently collapsed — what the `a` key toggles against.
    pub fn all_collapsed(&self, repos: &[RepoEntry]) -> bool {
        !repos.is_empty()
            && repos.iter().all(|r| {
                self.collapsed.contains(&RowId::Repo {
                    repo: r.path.clone(),
                })
            })
    }

    /// Move to the next changed file across every repo, wrapping (spec §5.3's `]`).
    pub fn next_file(&mut self, repos: &[RepoEntry]) {
        self.jump(repos, true);
    }

    /// Move to the previous changed file across every repo, wrapping (spec §5.3's `[`).
    pub fn prev_file(&mut self, repos: &[RepoEntry]) {
        self.jump(repos, false);
    }

    /// The selected row's diff target: which repo, which baseline, which path (spec §4.1).
    pub fn selected_file(&self) -> Option<(PathBuf, GroupKind, String)> {
        match self.selected().map(|r| &r.id) {
            Some(RowId::File { repo, group, path }) => Some((repo.clone(), *group, path.clone())),
            _ => None,
        }
    }

    /// The shared body of `next_file` / `prev_file`.
    ///
    /// The walk runs over the FULLY EXPANDED row list, so a collapsed repo cannot make `]` a
    /// dead key; whatever was hiding the target is then expanded before the cursor lands.
    fn jump(&mut self, repos: &[RepoEntry], forward: bool) {
        let all = flatten(repos, &HashSet::new());
        let files: Vec<&Row> = all
            .iter()
            .filter(|r| matches!(r.id, RowId::File { .. }))
            .collect();
        if files.is_empty() {
            return;
        }
        // Where the cursor sits within the fully expanded list, so the "next" file is next
        // relative to the user's actual position even when rows are hidden.
        let here = self
            .selected()
            .and_then(|sel| all.iter().position(|r| r.id == sel.id))
            .unwrap_or(0);
        let target = if forward {
            files
                .iter()
                .find(|r| position_of(&all, &r.id) > here)
                .or_else(|| files.first())
        } else {
            files
                .iter()
                .rev()
                .find(|r| position_of(&all, &r.id) < here)
                .or_else(|| files.last())
        };
        let Some(target) = target.map(|r| r.id.clone()) else {
            return;
        };
        // Expand every ancestor that was hiding the target.
        let mut ancestor = target.parent();
        while let Some(id) = ancestor {
            self.collapsed.remove(&id);
            ancestor = id.parent();
        }
        self.rows = flatten(repos, &self.collapsed);
        if let Some(index) = self.rows.iter().position(|r| r.id == target) {
            self.cursor = index;
        }
    }
}

/// The index of `id` in `rows`, or `usize::MAX` when absent (never, for rows drawn from the
/// same list).
fn position_of(rows: &[Row], id: &RowId) -> usize {
    rows.iter().position(|r| &r.id == id).unwrap_or(usize::MAX)
}

/// Flatten the snapshot into visible rows, honoring `collapsed`.
fn flatten(repos: &[RepoEntry], collapsed: &HashSet<RowId>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (repo_idx, repo) in repos.iter().enumerate() {
        let repo_id = RowId::Repo {
            repo: repo.path.clone(),
        };
        let repo_collapsed = collapsed.contains(&repo_id);
        rows.push(Row {
            id: repo_id,
            depth: 0,
            repo_idx,
            group_idx: None,
            file_idx: None,
        });
        if repo_collapsed {
            continue;
        }
        for (group_idx, group) in repo.groups.iter().enumerate() {
            let group_id = RowId::Group {
                repo: repo.path.clone(),
                group: group.kind,
            };
            let group_collapsed = collapsed.contains(&group_id);
            rows.push(Row {
                id: group_id,
                depth: 1,
                repo_idx,
                group_idx: Some(group_idx),
                file_idx: None,
            });
            if group_collapsed {
                continue;
            }
            for (file_idx, file) in group.files.iter().enumerate() {
                rows.push(Row {
                    id: RowId::File {
                        repo: repo.path.clone(),
                        group: group.kind,
                        path: file.path.clone(),
                    },
                    depth: 2,
                    repo_idx,
                    group_idx: Some(group_idx),
                    file_idx: Some(file_idx),
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileEntry, RepoEntry, RepoKind, StatusGroup};

    fn repo(name: &str, groups: &[(GroupKind, &[&str])]) -> RepoEntry {
        RepoEntry {
            path: PathBuf::from(format!("/w/{name}")),
            display_name: name.to_string(),
            rel_path: name.to_string(),
            kind: RepoKind::Nested,
            groups: groups
                .iter()
                .map(|(kind, files)| StatusGroup {
                    kind: *kind,
                    files: files
                        .iter()
                        .map(|p| FileEntry {
                            path: p.to_string(),
                            status: 'M',
                            orig_path: None,
                        })
                        .collect(),
                })
                .collect(),
            ..RepoEntry::blank()
        }
    }

    /// A two-repo tree: `a` has Changes[x.rs, y.rs], `b` has Untracked[z.rs].
    fn fixture() -> Vec<RepoEntry> {
        vec![
            repo("a", &[(GroupKind::Changes, &["x.rs", "y.rs"])]),
            repo("b", &[(GroupKind::Untracked, &["z.rs"])]),
        ]
    }

    fn built(repos: &[RepoEntry]) -> Tree {
        let mut tree = Tree::new();
        tree.rebuild(repos);
        tree
    }

    /// A compact, readable rendering of the flattened rows: `depth:label`.
    fn shape(tree: &Tree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|r| {
                let label = match &r.id {
                    RowId::Repo { repo } => repo.to_string_lossy().into_owned(),
                    RowId::Group { group, .. } => group.title().to_string(),
                    RowId::File { path, .. } => path.clone(),
                };
                format!("{}:{}", r.depth, label)
            })
            .collect()
    }

    // ---- flattening ----------------------------------------------------------------------

    #[test]
    fn everything_is_expanded_on_a_first_build() {
        assert_eq!(
            shape(&built(&fixture())),
            vec![
                "0:/w/a",
                "1:Changes",
                "2:x.rs",
                "2:y.rs",
                "0:/w/b",
                "1:Untracked",
                "2:z.rs",
            ]
        );
    }

    #[test]
    fn a_clean_repo_contributes_only_its_own_row() {
        assert_eq!(shape(&built(&[repo("clean", &[])])), vec!["0:/w/clean"]);
    }

    #[test]
    fn an_empty_repo_list_yields_no_rows_and_no_selection() {
        let tree = built(&[]);
        assert!(tree.rows().is_empty());
        assert!(tree.selected().is_none());
        assert!(tree.selected_file().is_none());
    }

    #[test]
    fn each_row_carries_the_indices_needed_to_reach_its_model_data() {
        let repos = fixture();
        let tree = built(&repos);
        let file_row = tree
            .rows()
            .iter()
            .find(|r| matches!(&r.id, RowId::File { path, .. } if path == "z.rs"))
            .expect("z.rs row");
        assert_eq!(file_row.repo_idx, 1);
        assert_eq!(
            repos[file_row.repo_idx].groups[file_row.group_idx.unwrap()].files
                [file_row.file_idx.unwrap()]
            .path,
            "z.rs"
        );
    }

    // ---- collapsing ---------------------------------------------------------------------

    #[test]
    fn collapsing_a_repo_hides_its_groups_and_files_but_not_the_next_repo() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        tree.toggle_selected(&repos);
        assert_eq!(
            shape(&tree),
            vec!["0:/w/a", "0:/w/b", "1:Untracked", "2:z.rs"]
        );
    }

    #[test]
    fn collapsing_a_group_hides_only_that_groups_files() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(1); // the Changes group of repo a
        tree.toggle_selected(&repos);
        assert_eq!(
            shape(&tree),
            vec!["0:/w/a", "1:Changes", "0:/w/b", "1:Untracked", "2:z.rs"]
        );
    }

    #[test]
    fn toggling_a_file_row_does_nothing_because_a_file_has_no_children() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(2);
        let before = shape(&tree);
        tree.toggle_selected(&repos);
        assert_eq!(shape(&tree), before);
    }

    #[test]
    fn collapse_all_leaves_only_the_repo_rows_and_expand_all_restores_everything() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_all_collapsed(true, &repos);
        assert_eq!(shape(&tree), vec!["0:/w/a", "0:/w/b"]);
        assert!(tree.all_collapsed(&repos));
        tree.set_all_collapsed(false, &repos);
        assert_eq!(shape(&tree).len(), 7);
        assert!(!tree.all_collapsed(&repos));
    }

    // ---- cursor -------------------------------------------------------------------------

    #[test]
    fn the_cursor_clamps_at_both_ends_instead_of_wrapping() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.move_cursor(-5);
        assert_eq!(tree.cursor(), 0);
        tree.move_cursor(100);
        assert_eq!(tree.cursor(), 6);
    }

    #[test]
    fn setting_the_cursor_past_the_end_clamps_to_the_last_row() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(999);
        assert_eq!(tree.cursor(), 6);
    }

    // ---- identity-based state preservation (spec §5.5) ---------------------------------

    #[test]
    fn a_background_refresh_that_shifts_rows_keeps_the_cursor_on_the_same_file() {
        // spec §5.5: restore by row IDENTITY, not index — otherwise every poll round moves the
        // user's cursor onto a different file.
        let mut repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(6); // z.rs in repo b
        assert!(
            matches!(tree.selected().map(|r| &r.id), Some(RowId::File { path, .. }) if path == "z.rs")
        );

        // A new file appears in repo a, pushing everything below it down by one.
        repos[0].groups[0].files.insert(
            0,
            FileEntry {
                path: "aaa.rs".to_string(),
                status: 'M',
                orig_path: None,
            },
        );
        tree.rebuild(&repos);

        assert_eq!(tree.cursor(), 7);
        assert!(
            matches!(tree.selected().map(|r| &r.id), Some(RowId::File { path, .. }) if path == "z.rs")
        );
    }

    #[test]
    fn a_refresh_preserves_which_repos_and_groups_were_collapsed() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        tree.toggle_selected(&repos); // collapse repo a
        tree.rebuild(&repos);
        assert_eq!(
            shape(&tree),
            vec!["0:/w/a", "0:/w/b", "1:Untracked", "2:z.rs"]
        );
    }

    #[test]
    fn when_the_selected_row_disappears_the_cursor_falls_back_to_the_nearest_index() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(6); // the last row, z.rs
        tree.rebuild(&[repos[0].clone()]); // repo b vanishes entirely
        assert_eq!(tree.cursor(), 3); // clamped to the new last row
        assert!(tree.selected().is_some());
    }

    #[test]
    fn collapsed_state_for_a_repo_that_vanished_does_not_leak_onto_a_later_repo() {
        // Collapse is keyed by the repo's own path, so a different repo at the same index is
        // unaffected.
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        tree.toggle_selected(&repos); // collapse /w/a
        let others = vec![repo("c", &[(GroupKind::Changes, &["q.rs"])])];
        tree.rebuild(&others);
        assert_eq!(shape(&tree), vec!["0:/w/c", "1:Changes", "2:q.rs"]);
    }

    // ---- cross-repo jumping (spec §5.3) -------------------------------------------------

    #[test]
    fn next_change_walks_every_file_across_every_repo_and_wraps() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        let mut visited = Vec::new();
        for _ in 0..4 {
            tree.next_file(&repos);
            if let Some(RowId::File { path, .. }) = tree.selected().map(|r| r.id.clone()) {
                visited.push(path);
            }
        }
        assert_eq!(visited, vec!["x.rs", "y.rs", "z.rs", "x.rs"]);
    }

    #[test]
    fn prev_change_walks_backwards_across_repos_and_wraps() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        let mut visited = Vec::new();
        for _ in 0..3 {
            tree.prev_file(&repos);
            if let Some(RowId::File { path, .. }) = tree.selected().map(|r| r.id.clone()) {
                visited.push(path);
            }
        }
        assert_eq!(visited, vec!["z.rs", "y.rs", "x.rs"]);
    }

    #[test]
    fn jumping_to_a_change_expands_whatever_was_hiding_it() {
        // Otherwise `]` is a dead key on a fully collapsed tree, while spec §5.3 defines it as
        // "jump to the next changed FILE" — not "the next visible one".
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_all_collapsed(true, &repos);
        tree.next_file(&repos);
        assert!(
            matches!(tree.selected().map(|r| &r.id), Some(RowId::File { path, .. }) if path == "x.rs"),
            "{:?}",
            shape(&tree)
        );
    }

    #[test]
    fn jumping_in_a_tree_with_no_files_at_all_is_a_no_op() {
        let repos = vec![repo("clean", &[])];
        let mut tree = built(&repos);
        tree.next_file(&repos);
        tree.prev_file(&repos);
        assert_eq!(tree.cursor(), 0);
    }

    // ---- the diff target ------------------------------------------------------------------

    #[test]
    fn the_selected_file_reports_its_repo_group_and_path() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(6);
        assert_eq!(
            tree.selected_file(),
            Some((
                PathBuf::from("/w/b"),
                GroupKind::Untracked,
                "z.rs".to_string()
            ))
        );
    }

    #[test]
    fn a_repo_or_group_row_is_not_a_diff_target() {
        let repos = fixture();
        let mut tree = built(&repos);
        tree.set_cursor(0);
        assert!(tree.selected_file().is_none());
        tree.set_cursor(1);
        assert!(tree.selected_file().is_none());
    }

    #[test]
    fn the_same_path_in_two_groups_is_two_distinct_rows_with_distinct_diff_targets() {
        // spec §4.1: a path that is both staged and modified appears in BOTH groups, each with
        // its own baseline.
        let repos = vec![repo(
            "a",
            &[
                (GroupKind::Staged, &["dual.rs"]),
                (GroupKind::Changes, &["dual.rs"]),
            ],
        )];
        let mut tree = built(&repos);
        tree.set_cursor(2);
        let staged = tree.selected_file();
        tree.set_cursor(4);
        let changes = tree.selected_file();
        assert_eq!(staged.as_ref().map(|t| t.1), Some(GroupKind::Staged));
        assert_eq!(changes.as_ref().map(|t| t.1), Some(GroupKind::Changes));
        assert_ne!(staged, changes);
    }
}

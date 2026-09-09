//! The data model of spec §4 — plain data, no I/O and no behavior beyond derived counts.

use std::path::{Path, PathBuf};

/// How a repo was found, per spec §3.3. Decided purely from the shape of `.git` plus the
/// parent repo's `.gitmodules` — see [`crate::repo_kind::classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    /// The scan start itself.
    Root,
    /// A `.git` FILE whose path is listed in the parent repo's `.gitmodules`.
    Submodule,
    /// A `.git` FILE whose `gitdir:` points into `…/worktrees/…`.
    Worktree,
    /// A `.git` DIRECTORY: an independent clone living inside the tree.
    Nested,
}

impl RepoKind {
    /// The lowercase marker drawn in the repo row (spec §5.2).
    pub fn label(self) -> &'static str {
        match self {
            RepoKind::Root => "root",
            RepoKind::Submodule => "submodule",
            RepoKind::Worktree => "worktree",
            RepoKind::Nested => "nested",
        }
    }
}

/// The three status groups, in the order they are drawn under a repo (spec §5.2).
///
/// `Ord` is derived from this declaration order, so sorting groups yields Staged → Changes →
/// Untracked without a separate sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKind {
    Staged,
    Changes,
    Untracked,
}

impl GroupKind {
    /// The group row's title.
    pub fn title(self) -> &'static str {
        match self {
            GroupKind::Staged => "Staged",
            GroupKind::Changes => "Changes",
            GroupKind::Untracked => "Untracked",
        }
    }
}

/// One changed file inside one group. `path` is repo-relative, forward-slashed (git's own
/// convention). `status` is the group's own column of the porcelain XY pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub status: char,
    /// The pre-rename/copy path, for `R`/`C` entries only.
    pub orig_path: Option<String>,
}

/// A non-empty group of changed files. Empty groups are never constructed — spec §5.2 says an
/// empty group's row is not drawn at all, and the simplest way to guarantee that is to not
/// carry it in the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusGroup {
    pub kind: GroupKind,
    pub files: Vec<FileEntry>,
}

/// One repo's whole row-tree plus its per-repo health flags (spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoEntry {
    /// Absolute path to the repo's working tree root.
    pub path: PathBuf,
    /// The directory name, drawn as the repo's name.
    pub display_name: String,
    /// Path relative to the scan start; empty for the scan start itself.
    pub rel_path: String,
    pub kind: RepoKind,
    /// The branch name, or a short SHA when HEAD is detached. `None` when git could not say.
    pub branch: Option<String>,
    /// Commits ahead of / behind the upstream. Both `None` when there is NO upstream — which is
    /// deliberately distinct from `Some(0)` (an upstream that happens to be level).
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub groups: Vec<StatusGroup>,
    /// This repo's query failed; the row is marked `!` and carries this message (spec §9).
    pub error: Option<String>,
    /// The previous round timed out; the row is marked stale and retried next round (spec §9).
    pub stale: bool,
}

impl RepoEntry {
    /// A clean, unknown-branch entry at an empty path. The base every constructor and test
    /// starts from, so adding a field later does not touch every call site.
    pub fn blank() -> Self {
        RepoEntry {
            path: PathBuf::new(),
            display_name: String::new(),
            rel_path: String::new(),
            kind: RepoKind::Root,
            branch: None,
            ahead: None,
            behind: None,
            groups: Vec::new(),
            error: None,
            stale: false,
        }
    }

    /// The clean, not-yet-queried entry for a discovered repo: identity fields filled in from
    /// the discovery result, every git-derived field still empty.
    pub fn from_root(root: &RepoRoot) -> Self {
        RepoEntry {
            display_name: dir_name(&root.path),
            rel_path: rel_to_slash(&root.path, &root.scan_root),
            kind: root.kind,
            path: root.path.clone(),
            ..RepoEntry::blank()
        }
    }

    /// Total file rows under this repo. A path staged AND modified counts once per group,
    /// because that is how many rows the tree draws for it (spec §4.1).
    pub fn dirty_count(&self) -> usize {
        self.groups.iter().map(|g| g.files.len()).sum()
    }
}

/// A repo as the discovery walk found it, before any git query (spec §6, `discover` output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot {
    /// Absolute path to the repo's working tree root.
    pub path: PathBuf,
    /// The scan start this repo was found under; `rel_path` is measured from here.
    pub scan_root: PathBuf,
    pub kind: RepoKind,
}

/// One poll round's result. `generation` increases by one per round so a late snapshot can be
/// recognized as stale by the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub repos: Vec<RepoEntry>,
    pub generation: u64,
}

/// The final path component as a `String`, or the whole path when it has no file name (`/`).
fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `path` relative to `base`, forward-slashed. Empty when they are the same path; the absolute
/// path (lossy) when `path` is not under `base`, so a mis-scoped entry is visibly odd in the UI
/// rather than silently blank.
fn rel_to_slash(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: char) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            status,
            orig_path: None,
        }
    }

    #[test]
    fn dirty_count_sums_every_group() {
        // The same path can legitimately appear in BOTH Staged and Changes (XY both non-'.'),
        // and it counts twice — the count is "rows under this repo", which is what the header
        // number in the spec's mockup means.
        let repo = RepoEntry {
            groups: vec![
                StatusGroup {
                    kind: GroupKind::Staged,
                    files: vec![file("a.rs", 'M')],
                },
                StatusGroup {
                    kind: GroupKind::Changes,
                    files: vec![file("a.rs", 'M'), file("b.rs", 'D')],
                },
                StatusGroup {
                    kind: GroupKind::Untracked,
                    files: vec![file("c.rs", '?')],
                },
            ],
            ..RepoEntry::blank()
        };
        assert_eq!(repo.dirty_count(), 4);
    }

    #[test]
    fn dirty_count_of_a_clean_repo_is_zero() {
        assert_eq!(RepoEntry::blank().dirty_count(), 0);
    }

    #[test]
    fn from_root_takes_the_directory_name_and_the_relative_path() {
        let root = RepoRoot {
            path: PathBuf::from("/w/teleagent/pencil"),
            scan_root: PathBuf::from("/w/teleagent"),
            kind: RepoKind::Nested,
        };
        let entry = RepoEntry::from_root(&root);
        assert_eq!(entry.display_name, "pencil");
        assert_eq!(entry.rel_path, "pencil");
        assert_eq!(entry.kind, RepoKind::Nested);
        assert_eq!(entry.path, PathBuf::from("/w/teleagent/pencil"));
    }

    #[test]
    fn from_root_leaves_the_scan_root_itself_with_an_empty_relative_path() {
        // spec §4: "rel_path: 相對掃描起點；root 顯示為空".
        let root = RepoRoot {
            path: PathBuf::from("/w/teleagent"),
            scan_root: PathBuf::from("/w/teleagent"),
            kind: RepoKind::Root,
        };
        assert_eq!(RepoEntry::from_root(&root).rel_path, "");
    }

    #[test]
    fn group_titles_match_the_spec_labels() {
        assert_eq!(GroupKind::Staged.title(), "Staged");
        assert_eq!(GroupKind::Changes.title(), "Changes");
        assert_eq!(GroupKind::Untracked.title(), "Untracked");
    }

    #[test]
    fn repo_kind_labels_are_the_lowercase_spec_markers() {
        assert_eq!(RepoKind::Root.label(), "root");
        assert_eq!(RepoKind::Submodule.label(), "submodule");
        assert_eq!(RepoKind::Worktree.label(), "worktree");
        assert_eq!(RepoKind::Nested.label(), "nested");
    }
}

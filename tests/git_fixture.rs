//! `git.rs` against a REAL git tree. The only place snapshot/diff meet actual git output.

mod common;

use common::{Fixture, git, write};
use herdr_scm::git::{GitError, GitService, LiveGit};
use herdr_scm::model::{GroupKind, RepoKind, RepoRoot};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root_of(path: &Path) -> RepoRoot {
    RepoRoot {
        path: path.to_path_buf(),
        scan_root: path.to_path_buf(),
        kind: RepoKind::Root,
    }
}

/// Snapshot one repo and index its groups by kind.
fn groups_of(path: &Path) -> BTreeMap<GroupKind, Vec<(String, char, Option<String>)>> {
    let entries = LiveGit::default().snapshot(&[root_of(path)]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].error, None, "unexpected error: {entries:?}");
    assert!(!entries[0].stale, "unexpected stale: {entries:?}");
    entries[0]
        .groups
        .iter()
        .map(|g| {
            (
                g.kind,
                g.files
                    .iter()
                    .map(|f| (f.path.clone(), f.status, f.orig_path.clone()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn the_root_repos_branch_is_reported() {
    let fx = Fixture::build();
    let entries = LiveGit::default().snapshot(&[root_of(&fx.root())]);
    assert_eq!(entries[0].branch.as_deref(), Some("main"));
}

#[test]
fn a_worktree_modification_lands_in_changes() {
    let fx = Fixture::build();
    let groups = groups_of(&fx.root());
    let changes = &groups[&GroupKind::Changes];
    assert!(
        changes
            .iter()
            .any(|(p, s, _)| p == "src/lib.rs" && *s == 'M'),
        "{changes:?}"
    );
}

#[test]
fn an_index_only_addition_lands_in_staged() {
    let fx = Fixture::build();
    let staged = &groups_of(&fx.root())[&GroupKind::Staged];
    assert!(
        staged
            .iter()
            .any(|(p, s, _)| p == "staged.txt" && *s == 'A'),
        "{staged:?}"
    );
}

#[test]
fn a_staged_rename_carries_its_original_path() {
    let fx = Fixture::build();
    let staged = &groups_of(&fx.root())[&GroupKind::Staged];
    let renamed = staged
        .iter()
        .find(|(p, _, _)| p == "renamed-new.txt")
        .unwrap_or_else(|| panic!("rename missing: {staged:?}"));
    assert_eq!(renamed.1, 'R');
    assert_eq!(renamed.2.as_deref(), Some("renamed-old.txt"));
}

#[test]
fn an_untracked_file_lands_in_untracked() {
    let fx = Fixture::build();
    let untracked = &groups_of(&fx.root())[&GroupKind::Untracked];
    assert!(
        untracked
            .iter()
            .any(|(p, s, _)| p == "untracked.txt" && *s == '?'),
        "{untracked:?}"
    );
}

#[test]
fn a_gitignored_directory_never_appears_as_untracked() {
    let fx = Fixture::build();
    let untracked = &groups_of(&fx.root())[&GroupKind::Untracked];
    assert!(
        !untracked.iter().any(|(p, _, _)| p.starts_with("pencil")),
        "{untracked:?}"
    );
}

#[test]
fn a_conflicted_file_lands_in_changes_marked_u() {
    let fx = Fixture::build();
    let repo = fx.dir.path().join("conflict");
    common::init_repo(&repo);
    write(&repo.join("c.txt"), "base\n");
    git(&repo, &["add", "c.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "other"]);
    write(&repo.join("c.txt"), "other\n");
    git(&repo, &["commit", "-q", "-am", "other"]);
    git(&repo, &["checkout", "-q", "main"]);
    write(&repo.join("c.txt"), "mine\n");
    git(&repo, &["commit", "-q", "-am", "mine"]);
    // The merge is EXPECTED to fail; run it directly rather than through the asserting helper.
    // Identity envs are set (as `common::git` does) so the merge fails on the CONTENT conflict
    // rather than on a missing committer identity in a hermetic/CI environment.
    let _ = std::process::Command::new("git")
        .args(["merge", "other"])
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output();

    let changes = &groups_of(&repo)[&GroupKind::Changes];
    assert!(
        changes.iter().any(|(p, s, _)| p == "c.txt" && *s == 'U'),
        "{changes:?}"
    );
}

#[test]
fn a_repo_with_no_upstream_reports_neither_ahead_nor_behind() {
    let fx = Fixture::build();
    let entries = LiveGit::default().snapshot(&[root_of(&fx.root())]);
    assert_eq!(entries[0].ahead, None);
    assert_eq!(entries[0].behind, None);
}

#[test]
fn ahead_and_behind_are_reported_once_an_upstream_exists() {
    let fx = Fixture::build();
    let bare = fx.dir.path().join("bare.git");
    git(fx.dir.path(), &["init", "-q", "--bare", "bare.git"]);
    let root = fx.root();
    git(
        &root,
        &["remote", "add", "origin", bare.to_str().expect("utf-8")],
    );
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    write(&root.join("ahead.txt"), "x\n");
    git(&root, &["add", "ahead.txt"]);
    git(&root, &["commit", "-q", "-m", "ahead"]);

    let entries = LiveGit::default().snapshot(&[root_of(&root)]);
    assert_eq!(entries[0].ahead, Some(1));
    assert_eq!(entries[0].behind, Some(0));
}

#[test]
fn a_detached_head_reports_a_short_sha_instead_of_a_branch() {
    let fx = Fixture::build();
    let wt = fx.root().join("wt/feature");
    git(&wt, &["checkout", "-q", "--detach"]);
    let branch = LiveGit::default().snapshot(&[root_of(&wt)])[0]
        .branch
        .clone()
        .expect("a detached HEAD still reports a short sha");
    assert_eq!(branch.len(), 7, "{branch}");
    assert!(branch.chars().all(|c| c.is_ascii_hexdigit()), "{branch}");
}

#[test]
fn a_vanished_repo_yields_an_error_row_and_does_not_stop_its_neighbours() {
    // spec §9's headline rule: one repo's failure must not cost the user the panel.
    let fx = Fixture::build();
    let entries = LiveGit::default().snapshot(&[
        root_of(&PathBuf::from("/definitely/not/a/repo/anywhere")),
        root_of(&fx.root()),
    ]);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].error.is_some(), "{:?}", entries[0]);
    assert!(entries[1].error.is_none(), "{:?}", entries[1]);
    assert_eq!(entries[1].branch.as_deref(), Some("main"));
}

#[test]
fn the_changes_diff_shows_the_worktree_edit() {
    let fx = Fixture::build();
    let bytes = LiveGit::default()
        .diff(&fx.root(), GroupKind::Changes, "src/lib.rs")
        .expect("diff");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("@@"), "{text}");
    assert!(text.contains("+fn main() { /* edited */ }"), "{text}");
}

#[test]
fn the_staged_diff_shows_the_index_addition_and_not_the_worktree_edit() {
    let fx = Fixture::build();
    let text = String::from_utf8_lossy(
        &LiveGit::default()
            .diff(&fx.root(), GroupKind::Staged, "staged.txt")
            .expect("diff"),
    )
    .into_owned();
    assert!(text.contains("+staged"), "{text}");
    assert!(!text.contains("edited"), "{text}");
}

#[test]
fn an_untracked_entry_has_no_git_diff() {
    let fx = Fixture::build();
    assert_eq!(
        LiveGit::default().diff(&fx.root(), GroupKind::Untracked, "untracked.txt"),
        Err(GitError::NotApplicable)
    );
}

#[test]
fn a_diff_of_a_path_that_does_not_exist_is_empty_rather_than_an_error() {
    // `git diff -- nosuch` exits 0 with no output; the UI shows an empty diff, not an error row.
    let fx = Fixture::build();
    assert_eq!(
        LiveGit::default()
            .diff(&fx.root(), GroupKind::Changes, "nosuch.rs")
            .expect("empty diff is Ok"),
        Vec::<u8>::new()
    );
}

#[test]
fn resolving_a_pane_cwd_to_its_repo_root_walks_up() {
    use herdr_scm::discover::TopLevelResolver;
    let fx = Fixture::build();
    let resolved = herdr_scm::git::RealTopLevel
        .toplevel(&fx.root().join("src"))
        .expect("toplevel");
    // Compare canonicalized: macOS/WSL tempdirs can be symlinked.
    assert_eq!(
        resolved.canonicalize().expect("canonicalize"),
        fx.root().canonicalize().expect("canonicalize")
    );
}

#[test]
fn resolving_a_directory_outside_any_repo_yields_nothing() {
    use herdr_scm::discover::TopLevelResolver;
    let outside = tempfile::TempDir::new().expect("tempdir");
    assert_eq!(herdr_scm::git::RealTopLevel.toplevel(outside.path()), None);
}

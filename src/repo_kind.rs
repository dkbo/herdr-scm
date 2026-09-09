//! Pure repo-kind classification (spec §3.3) — no I/O whatsoever.
//!
//! The caller does the filesystem work (is `.git` a file or a directory? what does it contain?
//! what does the parent's `.gitmodules` list?) and passes the answers in, so every decision in
//! this module is a unit test away.

use crate::model::RepoKind;
use std::collections::BTreeSet;

/// The shape of a repo's `.git`, as the caller observed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DotGit {
    /// A real `.git` directory: an independent clone.
    Dir,
    /// A `.git` file, carrying the `gitdir:` pointer's value (already parsed by
    /// [`parse_gitdir`]).
    File { gitdir: String },
}

/// The `…/worktrees/…` marker that distinguishes a linked worktree's gitdir pointer.
const WORKTREE_MARKER: &str = "/worktrees/";

/// Classify one discovered repo (spec §3.3).
///
/// - `is_scan_root` — this repo IS the scan start. Wins outright.
/// - `dot_git` — the shape of its `.git`.
/// - `rel_from_parent` — its path relative to the nearest enclosing repo, forward-slashed;
///   `None` when there is no enclosing repo (then it cannot be a submodule).
/// - `parent_submodules` — the enclosing repo's `.gitmodules` paths, from [`submodule_paths`].
///
/// A `.git` file matching neither rule falls back to `Nested`: it is still a repo, and showing
/// it with a slightly wrong marker beats dropping it (spec §9).
pub fn classify(
    is_scan_root: bool,
    dot_git: &DotGit,
    rel_from_parent: Option<&str>,
    parent_submodules: &BTreeSet<String>,
) -> RepoKind {
    if is_scan_root {
        return RepoKind::Root;
    }
    let DotGit::File { gitdir } = dot_git else {
        return RepoKind::Nested;
    };
    if rel_from_parent.is_some_and(|rel| parent_submodules.contains(rel)) {
        return RepoKind::Submodule;
    }
    if gitdir.contains(WORKTREE_MARKER) {
        return RepoKind::Worktree;
    }
    RepoKind::Nested
}

/// Read a `.git` FILE's `gitdir:` pointer. `None` for anything that is not one, or whose value
/// is blank.
pub fn parse_gitdir(content: &str) -> Option<String> {
    let value = content.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Every `path = …` value in a `.gitmodules` file, normalized to compare against a
/// walk-produced relative path: no leading `./`, no trailing `/`.
///
/// Deliberately a line scanner rather than an INI parser: `.gitmodules` is hand-editable and we
/// only ever need the `path` keys. Unknown keys, comments and blank lines are ignored, and a
/// malformed file simply yields fewer paths — never an error.
pub fn submodule_paths(gitmodules: &str) -> BTreeSet<String> {
    gitmodules
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with(';') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            (key.trim() == "path").then_some(value)
        })
        .map(|value| {
            value
                .trim()
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subs(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    // ---- classify: the four kinds of spec §3.3 -------------------------------------------

    #[test]
    fn the_scan_start_itself_is_root_whatever_its_dot_git_looks_like() {
        // Root wins outright: a scan start that is itself a worktree checkout or a submodule
        // is still shown as the root of its own scan (spec §3.3, first row of the table).
        assert_eq!(
            classify(true, &DotGit::Dir, None, &subs(&[])),
            RepoKind::Root
        );
        assert_eq!(
            classify(
                true,
                &DotGit::File {
                    gitdir: "/w/.git/worktrees/wt".to_string()
                },
                Some("wt"),
                &subs(&["wt"])
            ),
            RepoKind::Root
        );
    }

    #[test]
    fn a_dot_git_directory_is_a_nested_clone() {
        assert_eq!(
            classify(false, &DotGit::Dir, Some("pencil"), &subs(&[])),
            RepoKind::Nested
        );
    }

    #[test]
    fn a_dot_git_file_listed_in_gitmodules_is_a_submodule() {
        // A submodule's gitdir points at ../.git/modules/<name> — it contains neither
        // "/worktrees/" nor anything else distinguishing, so .gitmodules membership is what
        // decides. Checked BEFORE the worktree rule, matching the spec table's order.
        assert_eq!(
            classify(
                false,
                &DotGit::File {
                    gitdir: "../.git/modules/vendorlib".to_string()
                },
                Some("vendorlib"),
                &subs(&["vendorlib"])
            ),
            RepoKind::Submodule
        );
    }

    #[test]
    fn a_dot_git_file_pointing_into_worktrees_is_a_worktree() {
        assert_eq!(
            classify(
                false,
                &DotGit::File {
                    gitdir: "/w/root/.git/worktrees/feature-x".to_string()
                },
                Some("feature-x"),
                &subs(&[])
            ),
            RepoKind::Worktree
        );
    }

    #[test]
    fn gitmodules_membership_beats_a_worktree_shaped_gitdir() {
        // Pathological but decidable: a path that is BOTH listed in .gitmodules and has a
        // worktree-shaped gitdir resolves to Submodule, because the spec table lists the
        // submodule row first.
        assert_eq!(
            classify(
                false,
                &DotGit::File {
                    gitdir: "/w/.git/worktrees/dup".to_string()
                },
                Some("dup"),
                &subs(&["dup"])
            ),
            RepoKind::Submodule
        );
    }

    #[test]
    fn an_unrecognizable_dot_git_file_falls_back_to_nested() {
        // Neither in .gitmodules nor worktree-shaped. Not in the spec's four rows; classified
        // as Nested so the repo is still SHOWN rather than dropped — spec §9's principle that
        // one odd repo must not cost the user the panel.
        assert_eq!(
            classify(
                false,
                &DotGit::File {
                    gitdir: "/somewhere/else".to_string()
                },
                Some("odd"),
                &subs(&[])
            ),
            RepoKind::Nested
        );
    }

    #[test]
    fn a_dot_git_file_with_no_known_relative_path_cannot_be_a_submodule() {
        assert_eq!(
            classify(
                false,
                &DotGit::File {
                    gitdir: "../.git/modules/x".to_string()
                },
                None,
                &subs(&["x"])
            ),
            RepoKind::Nested
        );
    }

    // ---- parse_gitdir --------------------------------------------------------------------

    #[test]
    fn parse_gitdir_reads_the_pointer_and_trims_whitespace() {
        assert_eq!(
            parse_gitdir("gitdir: /w/.git/worktrees/x\n"),
            Some("/w/.git/worktrees/x".to_string())
        );
        assert_eq!(
            parse_gitdir("gitdir:../.git/modules/lib"),
            Some("../.git/modules/lib".to_string())
        );
    }

    #[test]
    fn parse_gitdir_rejects_anything_that_is_not_a_gitdir_pointer() {
        assert_eq!(parse_gitdir(""), None);
        assert_eq!(parse_gitdir("ref: refs/heads/main\n"), None);
        assert_eq!(parse_gitdir("gitdir:   \n"), None);
    }

    // ---- submodule_paths -----------------------------------------------------------------

    #[test]
    fn submodule_paths_collects_every_path_key() {
        let text = "\
[submodule \"lib\"]
\tpath = vendor/lib
\turl = https://example.invalid/lib.git
[submodule \"tools\"]
\tpath = tools
\turl = https://example.invalid/tools.git
";
        assert_eq!(submodule_paths(text), subs(&["vendor/lib", "tools"]));
    }

    #[test]
    fn submodule_paths_ignores_comments_blank_lines_and_other_keys() {
        let text = "; a comment\n\n# another\n\tbranch = main\n\tpath=  spaced  \n";
        assert_eq!(submodule_paths(text), subs(&["spaced"]));
    }

    #[test]
    fn submodule_paths_of_an_absent_or_empty_gitmodules_is_empty() {
        assert!(submodule_paths("").is_empty());
    }

    #[test]
    fn submodule_paths_normalizes_trailing_and_leading_slashes() {
        // .gitmodules is hand-editable; the classifier compares against a walk-produced
        // relative path, which never has a leading "./" or a trailing "/".
        assert_eq!(
            submodule_paths("path = ./vendor/lib/\n"),
            subs(&["vendor/lib"])
        );
    }
}

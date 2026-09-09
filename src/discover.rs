//! Repo discovery: the traversal and its three-tier pruning (spec §3.1, §3.2).
//!
//! Every filesystem question goes through the [`Fs`] and [`IgnoreOracle`] seams, so the pruning
//! DECISIONS — the load-bearing part — are unit-tested against an in-memory tree with no disk
//! involved. [`RealFs`] and [`GitignoreOracle`] are the production implementations.

use crate::model::{RepoKind, RepoRoot, rel_slash};
use crate::repo_kind::{self, DotGit};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

/// The walk's bounds (spec §3.2). `depth` counts directory levels below the scan start;
/// `excludes` are directory NAMES pruned unconditionally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanConfig {
    pub depth: usize,
    pub excludes: Vec<String>,
}

/// One directory entry, reduced to what the walk actually needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
    /// Symlinked directories are never followed: a loop would hang the walk regardless of the
    /// depth limit, and a symlinked repo is reachable by its real path anyway.
    pub is_symlink: bool,
}

/// The filesystem seam.
pub trait Fs {
    /// The entries of `dir`. An `Err` means "skip this directory and keep going" (spec §9).
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<DirEntryInfo>>;
    /// The shape of `dir/.git`, or `None` when `dir` is not a repo working tree.
    fn dot_git(&self, dir: &Path) -> Option<DotGit>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
}

/// The gitignore seam. `enclosing_repo` is the nearest ancestor repo whose rules apply; `None`
/// means there is none, so nothing is ignored.
pub trait IgnoreOracle {
    fn is_ignored_dir(&self, enclosing_repo: Option<&Path>, dir: &Path) -> bool;
}

/// `git rev-parse --show-toplevel`, behind a seam so `scan_roots` is testable without git.
pub trait TopLevelResolver {
    fn toplevel(&self, dir: &Path) -> Option<PathBuf>;
}

/// The scan starts (spec §3.1): each pane cwd walked up to its repo root, or used as-is when it
/// is not inside a repo. De-duplicated, and a start nested inside another start is dropped so
/// no subtree is walked twice.
///
/// With no pane cwds at all — the herdr-CLI-unavailable degradation — the `fallback`
/// (`workspace_cwd`) becomes the only scan start.
pub fn scan_roots(
    pane_cwds: &[PathBuf],
    fallback: Option<&Path>,
    tl: &dyn TopLevelResolver,
) -> Vec<PathBuf> {
    let candidates: Vec<PathBuf> = if pane_cwds.is_empty() {
        fallback.map(Path::to_path_buf).into_iter().collect()
    } else {
        pane_cwds.to_vec()
    };
    let mut resolved: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for cwd in candidates {
        let root = tl.toplevel(&cwd).unwrap_or(cwd);
        if seen.insert(root.clone()) {
            resolved.push(root);
        }
    }
    // Drop any start that lives inside another start.
    resolved
        .iter()
        .filter(|a| !resolved.iter().any(|b| *b != **a && a.starts_with(b)))
        .cloned()
        .collect()
}

/// Walk every scan start and return the repos found, in discovery order, each reported once.
pub fn scan(
    roots: &[PathBuf],
    cfg: &ScanConfig,
    fs: &dyn Fs,
    ig: &dyn IgnoreOracle,
) -> Vec<RepoRoot> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let mut walk = Walk {
            cfg,
            fs,
            ig,
            scan_root: root,
            submodules: HashMap::new(),
            out: &mut out,
            seen: &mut seen,
        };
        walk.visit(root, 0, false, None);
    }
    out
}

/// One scan start's traversal state.
struct Walk<'a> {
    cfg: &'a ScanConfig,
    fs: &'a dyn Fs,
    ig: &'a dyn IgnoreOracle,
    scan_root: &'a Path,
    /// `.gitmodules` paths per repo, parsed at most once each.
    submodules: HashMap<PathBuf, BTreeSet<String>>,
    out: &'a mut Vec<RepoRoot>,
    seen: &'a mut HashSet<PathBuf>,
}

impl Walk<'_> {
    /// Visit one directory.
    ///
    /// `ignored_mode` is the spec §3.2 rule-2 state: we are somewhere under a gitignored
    /// directory, so we still descend looking for `.git`, but the FIRST repo we find here ends
    /// the drilling down this branch. Everywhere else the walk continues past a found repo
    /// until the depth limit or a hard exclude stops it.
    fn visit(&mut self, dir: &Path, depth: usize, ignored_mode: bool, enclosing: Option<&Path>) {
        let mut enclosing_here = enclosing;
        let owned_dir;
        if let Some(dot_git) = self.fs.dot_git(dir) {
            let rel = enclosing.and_then(|e| rel_slash(dir, e));
            let submodules = enclosing.map(|e| self.submodules_of(e)).unwrap_or_default();
            let kind =
                repo_kind::classify(dir == self.scan_root, &dot_git, rel.as_deref(), &submodules);
            self.push(dir, kind);
            self.push_declared_submodules(dir);
            if ignored_mode {
                return;
            }
            owned_dir = dir.to_path_buf();
            enclosing_here = Some(&owned_dir);
        }
        if depth >= self.cfg.depth {
            return;
        }
        // A read error (permission, vanished directory) skips this directory only.
        let Ok(entries) = self.fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            if !entry.is_dir || entry.is_symlink || entry.name == ".git" {
                continue;
            }
            // Rule 1: hard excludes prune unconditionally, one level in included.
            if self.cfg.excludes.contains(&entry.name) {
                continue;
            }
            let child = dir.join(&entry.name);
            // Rule 2: entering an ignored directory switches on the stop-at-first-repo mode
            // but does NOT prune. Once on, it stays on for the whole subtree.
            let child_ignored = ignored_mode || self.ig.is_ignored_dir(enclosing_here, &child);
            self.visit(&child, depth + 1, child_ignored, enclosing_here);
        }
    }

    /// Record a repo unless an earlier scan start already did.
    fn push(&mut self, path: &Path, kind: RepoKind) {
        if self.seen.insert(path.to_path_buf()) {
            self.out.push(RepoRoot {
                path: path.to_path_buf(),
                scan_root: self.scan_root.to_path_buf(),
                kind,
            });
        }
    }

    /// List `repo`'s initialized submodules straight from its `.gitmodules` (spec §3.2), so a
    /// submodule is found regardless of the depth limit or a hard-excluded directory name. A
    /// declared submodule with no `.git` is uninitialized and skipped — listing it would give a
    /// permanently erroring row.
    fn push_declared_submodules(&mut self, repo: &Path) {
        for rel in self.submodules_of(repo) {
            let path = repo.join(&rel);
            if self.fs.dot_git(&path).is_some() {
                // Deliberately `RepoKind::Submodule`, not `repo_kind::classify(path)`: classify
                // returns `Nested` for any `.git` DIRECTORY, and an old-style, non-absorbed
                // submodule has exactly that shape. For a path declared in `.gitmodules`,
                // `Submodule` is the truthful answer regardless of what `.git` looks like here —
                // do not "fix" this into calling classify().
                self.push(&path, RepoKind::Submodule);
            }
        }
    }

    /// `repo`'s `.gitmodules` paths, parsed at most once per repo. An absent or unreadable file
    /// yields an empty set.
    fn submodules_of(&mut self, repo: &Path) -> BTreeSet<String> {
        if let Some(cached) = self.submodules.get(repo) {
            return cached.clone();
        }
        let text = self
            .fs
            .read_to_string(&repo.join(".gitmodules"))
            .unwrap_or_default();
        let parsed = repo_kind::submodule_paths(&text);
        self.submodules.insert(repo.to_path_buf(), parsed.clone());
        parsed
    }
}

// ---------------------------------------------------------------------------
// Production implementations
// ---------------------------------------------------------------------------

/// The real filesystem. Read-only: `read_dir`, `symlink_metadata`, `read_to_string`.
pub struct RealFs;

impl Fs for RealFs {
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<DirEntryInfo>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            // An unreadable individual entry skips that entry, not the whole directory.
            let Ok(entry) = entry else { continue };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            entries.push(DirEntryInfo {
                name,
                is_dir: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
            });
        }
        // Stable order so the repo list does not reshuffle between rescans.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn dot_git(&self, dir: &Path) -> Option<DotGit> {
        let path = dir.join(".git");
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if meta.is_dir() {
            return Some(DotGit::Dir);
        }
        if meta.is_file() {
            let content = std::fs::read_to_string(&path).ok()?;
            return Some(DotGit::File {
                gitdir: repo_kind::parse_gitdir(&content)?,
            });
        }
        None
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// The real ignore oracle: one `ignore::gitignore::Gitignore` per repo, built from that repo's
/// `.gitignore` and `.git/info/exclude`, cached for the walk's lifetime.
///
/// The user's GLOBAL gitignore is deliberately not consulted — the panel's repo list should not
/// depend on a machine-level setting the workspace never sees.
#[derive(Default)]
pub struct GitignoreOracle {
    cache: RefCell<HashMap<PathBuf, ignore::gitignore::Gitignore>>,
}

impl GitignoreOracle {
    pub fn new() -> Self {
        GitignoreOracle::default()
    }
}

impl IgnoreOracle for GitignoreOracle {
    fn is_ignored_dir(&self, enclosing_repo: Option<&Path>, dir: &Path) -> bool {
        let Some(repo) = enclosing_repo else {
            return false;
        };
        let mut cache = self.cache.borrow_mut();
        let matcher = cache.entry(repo.to_path_buf()).or_insert_with(|| {
            let mut builder = ignore::gitignore::GitignoreBuilder::new(repo);
            // `add` returns Option<Error>; a missing file is simply no rules.
            let _ = builder.add(repo.join(".gitignore"));
            let _ = builder.add(repo.join(".git").join("info").join("exclude"));
            builder
                .build()
                .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
        });
        matcher.matched(dir, true).is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ---- an in-memory filesystem, so pruning decisions are tested without touching disk ----

    #[derive(Default)]
    struct MemFs {
        /// dir path -> child entries
        dirs: BTreeMap<PathBuf, Vec<DirEntryInfo>>,
        /// repo working-tree root -> the shape of its `.git`
        repos: BTreeMap<PathBuf, DotGit>,
        /// file path -> contents
        files: BTreeMap<PathBuf, String>,
        /// dirs whose `read_dir` fails, simulating a permission error
        unreadable: Vec<PathBuf>,
    }

    impl MemFs {
        /// Declare a directory and every ancestor, wiring up the child lists as it goes.
        fn dir(mut self, path: &str) -> Self {
            let path = PathBuf::from(path);
            let mut current = path.clone();
            while let Some(parent) = current.parent().map(Path::to_path_buf) {
                let name = match current.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => break,
                };
                let children = self.dirs.entry(parent.clone()).or_default();
                if !children.iter().any(|c| c.name == name) {
                    children.push(DirEntryInfo {
                        name,
                        is_dir: true,
                        is_symlink: false,
                    });
                }
                current = parent;
            }
            self.dirs.entry(path).or_default();
            self
        }

        /// Declare `path` a repo with a `.git` directory (an independent clone).
        fn repo(mut self, path: &str) -> Self {
            self = self.dir(path);
            self.repos.insert(PathBuf::from(path), DotGit::Dir);
            self
        }

        /// Declare `path` a repo with a `.git` FILE carrying `gitdir`.
        fn repo_file(mut self, path: &str, gitdir: &str) -> Self {
            self = self.dir(path);
            self.repos.insert(
                PathBuf::from(path),
                DotGit::File {
                    gitdir: gitdir.to_string(),
                },
            );
            self
        }

        fn file(mut self, path: &str, contents: &str) -> Self {
            self.files.insert(PathBuf::from(path), contents.to_string());
            self
        }

        fn symlink(mut self, parent: &str, name: &str) -> Self {
            self.dirs
                .entry(PathBuf::from(parent))
                .or_default()
                .push(DirEntryInfo {
                    name: name.to_string(),
                    is_dir: true,
                    is_symlink: true,
                });
            self
        }

        fn unreadable(mut self, path: &str) -> Self {
            self.unreadable.push(PathBuf::from(path));
            self
        }
    }

    impl Fs for MemFs {
        fn read_dir(&self, dir: &Path) -> io::Result<Vec<DirEntryInfo>> {
            if self.unreadable.iter().any(|p| p == dir) {
                return Err(io::Error::other("permission denied"));
            }
            Ok(self.dirs.get(dir).cloned().unwrap_or_default())
        }

        fn dot_git(&self, dir: &Path) -> Option<DotGit> {
            self.repos.get(dir).cloned()
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::other("no such file"))
        }
    }

    /// An ignore oracle driven by an explicit list of ignored directories.
    struct MemIgnore(Vec<PathBuf>);

    impl MemIgnore {
        fn of(paths: &[&str]) -> Self {
            MemIgnore(paths.iter().map(PathBuf::from).collect())
        }

        fn none() -> Self {
            MemIgnore(Vec::new())
        }
    }

    impl IgnoreOracle for MemIgnore {
        fn is_ignored_dir(&self, _enclosing_repo: Option<&Path>, dir: &Path) -> bool {
            self.0.iter().any(|p| p == dir)
        }
    }

    fn cfg(depth: usize) -> ScanConfig {
        ScanConfig {
            depth,
            excludes: crate::config::DEFAULT_SCAN_EXCLUDES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn found(roots: &[&str], cfg: &ScanConfig, fs: &MemFs, ig: &MemIgnore) -> Vec<String> {
        scan(
            &roots.iter().map(PathBuf::from).collect::<Vec<_>>(),
            cfg,
            fs,
            ig,
        )
        .into_iter()
        .map(|r| r.path.to_string_lossy().into_owned())
        .collect()
    }

    // ---- the load-bearing rule: an ignored directory is DESCENDED, not pruned ----------------

    #[test]
    fn a_repo_inside_a_gitignored_directory_is_found() {
        // spec §3.2 rule 2 and its rationale: every target repo of the motivating workspace
        // lives under a gitignored directory. Pruning on ignore would take the panel from
        // 6 repos to 1 — this test is the whole feature.
        let fs = MemFs::default().repo("/w/root").repo("/w/root/pencil");
        let ig = MemIgnore::of(&["/w/root/pencil"]);
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &ig),
            vec!["/w/root", "/w/root/pencil"]
        );
    }

    #[test]
    fn all_five_ignored_sibling_repos_are_found_not_just_the_first() {
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/tenant-platform")
            .repo("/w/root/teleagent-platform")
            .repo("/w/root/pencil")
            .repo("/w/root/tenant-web")
            .repo("/w/root/tenant-worker");
        let ig = MemIgnore::of(&[
            "/w/root/tenant-platform",
            "/w/root/teleagent-platform",
            "/w/root/pencil",
            "/w/root/tenant-web",
            "/w/root/tenant-worker",
        ]);
        assert_eq!(found(&["/w/root"], &cfg(4), &fs, &ig).len(), 6);
    }

    #[test]
    fn a_repo_nested_inside_an_ignored_repo_is_not_drilled_into() {
        // spec §3.2: "找到就收下該 repo 並停止再往下鑽" — the ONLY stop-drilling point.
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/pencil")
            .repo("/w/root/pencil/deep/inner");
        let ig = MemIgnore::of(&["/w/root/pencil"]);
        assert_eq!(
            found(&["/w/root"], &cfg(6), &fs, &ig),
            vec!["/w/root", "/w/root/pencil"]
        );
    }

    #[test]
    fn a_repo_reached_through_an_ignored_directory_that_is_not_itself_a_repo_is_found() {
        // The ignored directory is a plain folder; the repo is one level down. Descending is
        // required to find it, and finding it stops the drilling there.
        let fs = MemFs::default()
            .repo("/w/root")
            .dir("/w/root/build")
            .repo("/w/root/build/app");
        let ig = MemIgnore::of(&["/w/root/build"]);
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &ig),
            vec!["/w/root", "/w/root/build/app"]
        );
    }

    // ---- rule 1: hard excludes prune unconditionally ------------------------------------------

    #[test]
    fn a_repo_under_a_hard_excluded_directory_is_never_found() {
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/node_modules/pkg")
            .repo("/w/root/target/vendored");
        assert_eq!(
            found(&["/w/root"], &cfg(6), &fs, &MemIgnore::none()),
            vec!["/w/root"]
        );
    }

    #[test]
    fn the_hard_exclude_list_is_configurable_and_replaces_the_defaults() {
        let fs = MemFs::default().repo("/w/root").repo("/w/root/skipme");
        let cfg = ScanConfig {
            depth: 4,
            excludes: vec!["skipme".to_string()],
        };
        assert_eq!(
            found(&["/w/root"], &cfg, &fs, &MemIgnore::none()),
            vec!["/w/root"]
        );
    }

    #[test]
    fn an_empty_exclude_list_prunes_nothing() {
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/node_modules/pkg");
        let cfg = ScanConfig {
            depth: 6,
            excludes: Vec::new(),
        };
        assert_eq!(found(&["/w/root"], &cfg, &fs, &MemIgnore::none()).len(), 2);
    }

    // ---- rule 3: normal traversal keeps going past a found repo ---------------------------------

    #[test]
    fn a_non_ignored_subtree_keeps_drilling_past_a_repo_it_already_found() {
        // spec §3.2: "走訪不因為找到 repo 而整體停止". Only the ignored-subtree case stops.
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/a")
            .repo("/w/root/a/b");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()),
            vec!["/w/root", "/w/root/a", "/w/root/a/b"]
        );
    }

    // ---- depth ------------------------------------------------------------------------------------

    #[test]
    fn the_depth_limit_is_measured_from_the_scan_start_and_is_inclusive() {
        let fs = MemFs::default().repo("/w/root").repo("/w/root/a/b/c/d");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()).len(),
            2
        );
        assert_eq!(
            found(&["/w/root"], &cfg(3), &fs, &MemIgnore::none()).len(),
            1
        );
    }

    // ---- robustness -------------------------------------------------------------------------------

    #[test]
    fn an_unreadable_directory_is_skipped_and_its_siblings_still_scanned() {
        // spec §9: "走訪遇權限錯誤 → 略過該目錄，繼續".
        let fs = MemFs::default()
            .repo("/w/root")
            .dir("/w/root/locked")
            .repo("/w/root/locked/hidden")
            .repo("/w/root/ok")
            .unreadable("/w/root/locked");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()),
            vec!["/w/root", "/w/root/ok"]
        );
    }

    #[test]
    fn symlinked_directories_are_not_followed() {
        // A symlink loop would otherwise hang the walk regardless of the depth limit.
        let fs = MemFs::default().repo("/w/root").symlink("/w/root", "loop");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()),
            vec!["/w/root"]
        );
    }

    #[test]
    fn the_dot_git_directory_itself_is_never_descended() {
        let fs = MemFs::default()
            .repo("/w/root")
            .repo("/w/root/.git/modules/x");
        assert_eq!(
            found(&["/w/root"], &cfg(6), &fs, &MemIgnore::none()),
            vec!["/w/root"]
        );
    }

    #[test]
    fn a_repo_reachable_from_two_scan_roots_is_reported_once() {
        let fs = MemFs::default().repo("/w/a").repo("/w/b");
        assert_eq!(
            found(&["/w/a", "/w/b", "/w/a"], &cfg(4), &fs, &MemIgnore::none()),
            vec!["/w/a", "/w/b"]
        );
    }

    #[test]
    fn scanning_a_directory_that_is_not_a_repo_and_holds_none_finds_nothing() {
        let fs = MemFs::default().dir("/w/empty");
        assert!(found(&["/w/empty"], &cfg(4), &fs, &MemIgnore::none()).is_empty());
    }

    // ---- kinds through the walk ----------------------------------------------------------------------

    #[test]
    fn the_walk_assigns_every_kind_from_the_spec_table() {
        let fs = MemFs::default()
            .repo("/w/root")
            .file(
                "/w/root/.gitmodules",
                "[submodule \"lib\"]\n\tpath = sublib\n",
            )
            .repo_file("/w/root/sublib", "/w/root/.git/modules/lib")
            .repo_file("/w/root/wt/feature", "/w/root/.git/worktrees/feature")
            .repo("/w/root/clone");
        let kinds: BTreeMap<String, RepoKind> = scan(
            &[PathBuf::from("/w/root")],
            &cfg(4),
            &fs,
            &MemIgnore::none(),
        )
        .into_iter()
        .map(|r| (r.path.to_string_lossy().into_owned(), r.kind))
        .collect();

        assert_eq!(kinds["/w/root"], RepoKind::Root);
        assert_eq!(kinds["/w/root/sublib"], RepoKind::Submodule);
        assert_eq!(kinds["/w/root/wt/feature"], RepoKind::Worktree);
        assert_eq!(kinds["/w/root/clone"], RepoKind::Nested);
    }

    #[test]
    fn every_found_repo_carries_the_scan_root_it_was_found_under() {
        let fs = MemFs::default().repo("/w/root").repo("/w/root/a");
        let repos = scan(
            &[PathBuf::from("/w/root")],
            &cfg(4),
            &fs,
            &MemIgnore::none(),
        );
        assert!(repos.iter().all(|r| r.scan_root == Path::new("/w/root")));
    }

    // ---- declared submodules bypass the walk -------------------------------------------------------

    #[test]
    fn a_submodule_declared_in_gitmodules_is_found_even_beyond_the_depth_limit() {
        // spec §3.2: "submodule 由父 repo 的 .gitmodules 另行列舉，不依賴遞迴走訪找到".
        let fs = MemFs::default()
            .repo("/w/root")
            .file("/w/root/.gitmodules", "path = deep/a/b/c/lib\n")
            .repo_file("/w/root/deep/a/b/c/lib", "/w/root/.git/modules/lib");
        let repos = found(&["/w/root"], &cfg(2), &fs, &MemIgnore::none());
        assert!(
            repos.contains(&"/w/root/deep/a/b/c/lib".to_string()),
            "{repos:?}"
        );
    }

    #[test]
    fn a_submodule_declared_but_not_initialized_is_skipped() {
        // An uninitialized submodule directory has no `.git` at all; listing it would produce a
        // permanently erroring row.
        let fs = MemFs::default()
            .repo("/w/root")
            .file("/w/root/.gitmodules", "path = sublib\n")
            .dir("/w/root/sublib");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()),
            vec!["/w/root"]
        );
    }

    #[test]
    fn a_submodule_under_a_hard_excluded_name_is_still_listed_from_gitmodules() {
        // The exclude list bounds the WALK's cost; a declared submodule costs one stat, so it
        // is listed regardless — dropping a real submodule because it is named `vendor` would
        // silently hide a repo the user tracks.
        let fs = MemFs::default()
            .repo("/w/root")
            .file("/w/root/.gitmodules", "path = vendor\n")
            .repo_file("/w/root/vendor", "/w/root/.git/modules/vendor");
        assert_eq!(
            found(&["/w/root"], &cfg(4), &fs, &MemIgnore::none()).len(),
            2
        );
    }

    // ---- scan_roots -----------------------------------------------------------------------------------

    /// A toplevel resolver driven by an explicit dir -> repo-root map.
    struct MemTopLevel(BTreeMap<PathBuf, PathBuf>);

    impl MemTopLevel {
        fn of(pairs: &[(&str, &str)]) -> Self {
            MemTopLevel(
                pairs
                    .iter()
                    .map(|(d, r)| (PathBuf::from(d), PathBuf::from(r)))
                    .collect(),
            )
        }
    }

    impl TopLevelResolver for MemTopLevel {
        fn toplevel(&self, dir: &Path) -> Option<PathBuf> {
            self.0.get(dir).cloned()
        }
    }

    fn roots_of(cwds: &[&str], fallback: Option<&str>, tl: &MemTopLevel) -> Vec<String> {
        scan_roots(
            &cwds.iter().map(PathBuf::from).collect::<Vec<_>>(),
            fallback.map(Path::new),
            tl,
        )
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn a_pane_cwd_inside_a_repo_walks_up_to_that_repos_root() {
        let tl = MemTopLevel::of(&[("/w/root/src", "/w/root")]);
        assert_eq!(roots_of(&["/w/root/src"], None, &tl), vec!["/w/root"]);
    }

    #[test]
    fn a_pane_cwd_outside_any_repo_is_used_as_the_scan_start_itself() {
        assert_eq!(
            roots_of(&["/w/plain"], None, &MemTopLevel::of(&[])),
            vec!["/w/plain"]
        );
    }

    #[test]
    fn two_pane_cwds_in_the_same_repo_collapse_to_one_scan_start() {
        let tl = MemTopLevel::of(&[("/w/root/a", "/w/root"), ("/w/root/b", "/w/root")]);
        assert_eq!(
            roots_of(&["/w/root/a", "/w/root/b"], None, &tl),
            vec!["/w/root"]
        );
    }

    #[test]
    fn a_scan_start_inside_another_scan_start_is_dropped() {
        // Keeping both would walk the inner tree twice and give its repos two different
        // rel_paths depending on which walk reached them first.
        assert_eq!(
            roots_of(&["/w/root", "/w/root/inner"], None, &MemTopLevel::of(&[])),
            vec!["/w/root"]
        );
    }

    #[test]
    fn with_no_pane_cwds_the_fallback_workspace_cwd_becomes_the_only_scan_start() {
        // spec §3.1: "herdr CLI 不可用時，降級為只用 workspace_cwd 當唯一掃描起點".
        assert_eq!(roots_of(&[], Some("/w"), &MemTopLevel::of(&[])), vec!["/w"]);
    }

    #[test]
    fn the_fallback_is_also_walked_up_to_its_repo_root() {
        let tl = MemTopLevel::of(&[("/w/sub", "/w")]);
        assert_eq!(roots_of(&[], Some("/w/sub"), &tl), vec!["/w"]);
    }

    #[test]
    fn with_neither_pane_cwds_nor_a_fallback_there_are_no_scan_starts() {
        assert!(roots_of(&[], None, &MemTopLevel::of(&[])).is_empty());
    }

    #[test]
    fn the_fallback_is_ignored_when_pane_cwds_were_available() {
        assert_eq!(
            roots_of(&["/w/a"], Some("/w/unused"), &MemTopLevel::of(&[])),
            vec!["/w/a"]
        );
    }
}

//! The one place that builds a real git tree. Every integration test that needs real git
//! reaches for `Fixture::build()`; nothing else in the suite shells out to git.
//!
//! The tree deliberately reproduces the workspace shape spec §3.2 was written for: five repos
//! sitting inside gitignored directories, plus a submodule, a linked worktree, and a decoy
//! repo behind a hard-excluded directory name.
//!
//! RULING R11: every COMMITTING step runs first; the dirty working-tree state (the staged
//! addition, the staged rename, the working-tree edit, the untracked file) is created LAST.
//! `git submodule add` followed by `git commit` takes the whole INDEX, so building the dirty
//! state before that commit would sweep the staged addition and the staged rename into the
//! "add submodule" commit and leave the root repo's Staged group empty.

#![allow(dead_code)] // each test file uses a different subset of the helpers

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Run a git command in `cwd`, panicking with its stderr on failure — a broken fixture must
/// fail loudly rather than produce a silently wrong tree.
pub fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // Hermetic: no user identity, hooks, templates or global config leaking in.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .unwrap_or_else(|e| panic!("spawn git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} in {cwd:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Write `contents` to `path`, creating parent directories.
pub fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Initialize a repo at `path` with one commit, so `git status` and `git diff` have a HEAD.
pub fn init_repo(path: &Path) {
    std::fs::create_dir_all(path).expect("create repo dir");
    git(path, &["init", "-q", "-b", "main"]);
    write(&path.join("seed.txt"), "seed\n");
    git(path, &["add", "seed.txt"]);
    git(path, &["commit", "-q", "-m", "seed"]);
}

/// The five gitignored directories that hold nested clones — the shape spec §3.2's rationale
/// describes.
pub const IGNORED_REPOS: [&str; 5] = [
    "tenant-platform",
    "teleagent-platform",
    "pencil",
    "tenant-web",
    "tenant-worker",
];

/// A built git fixture. Dropping it removes the whole tree.
pub struct Fixture {
    pub dir: TempDir,
}

impl Fixture {
    /// The root repo's working tree — the scan start every test passes in.
    pub fn root(&self) -> PathBuf {
        self.dir.path().join("teleagent")
    }

    /// Build the tree:
    ///
    /// ```text
    /// <tmp>/origin/sublib        a repo to add as a submodule
    /// <tmp>/teleagent            root repo
    ///   .gitignore               ignores the five dirs below, plus node_modules/ and wt/
    ///   src/lib.rs               committed, then modified   -> Changes
    ///   staged.txt               added to the index         -> Staged
    ///   untracked.txt            never added                -> Untracked
    ///   renamed-new.txt          committed as -old, then renamed in the index -> Staged R
    ///   sublib/                  real submodule             -> .git FILE
    ///   wt/feature/              linked worktree            -> .git FILE
    ///   node_modules/pkg/        a repo behind a hard exclude -> must NOT be found
    ///   deep/a/b/c/d/            a repo past depth 4        -> must NOT be found at depth 4
    ///   pencil/deep/inner/       a repo inside an ignored repo -> must NOT be found
    ///   <the five ignored dirs>/ nested clones              -> MUST all be found
    /// ```
    ///
    /// Build order (RULING R11): every committing step first, dirty state last — see the
    /// module doc comment for why.
    pub fn build() -> Fixture {
        let dir = TempDir::new().expect("create tempdir");
        let base = dir.path().to_path_buf();

        // Step 1: the submodule's source repo, and the root repo's base commit.
        let origin = base.join("origin/sublib");
        init_repo(&origin);

        let root = base.join("teleagent");
        init_repo(&root);

        write(
            &root.join(".gitignore"),
            "/tenant-platform/\n\
             /teleagent-platform/\n\
             /pencil/\n\
             /tenant-web/\n\
             /tenant-worker/\n\
             /node_modules/\n\
             /wt/\n",
        );
        write(&root.join("src/lib.rs"), "fn main() {}\n");
        write(&root.join("renamed-old.txt"), "content\n");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "base"]);

        // Step 2: the five nested clones inside gitignored directories, plus the decoys.
        for name in IGNORED_REPOS {
            init_repo(&root.join(name));
        }
        // A repo INSIDE one of those: must not be found (stop-drilling rule).
        init_repo(&root.join("pencil/deep/inner"));
        // A decoy behind a hard-excluded directory name.
        init_repo(&root.join("node_modules/pkg"));
        // A repo deeper than the default scan depth of 4.
        init_repo(&root.join("deep/a/b/c/d"));

        // Step 3: a real submodule, added and committed. `protocol.file.allow` is required for
        // a local-path submodule on modern git; it is scoped to this one command.
        git(
            &root,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                origin.to_str().expect("utf-8 tempdir path"),
                "sublib",
            ],
        );
        git(&root, &["commit", "-q", "-m", "add submodule"]);

        // Step 4: a linked worktree, inside the (gitignored) wt/ directory.
        git(
            &root,
            &["worktree", "add", "-q", "-b", "feature", "wt/feature"],
        );

        // Step 5: ONLY NOW create the dirty state, so it lands in the working tree / index
        // rather than being swept into the "add submodule" commit above.
        // Working-tree modification -> Changes.
        write(&root.join("src/lib.rs"), "fn main() { /* edited */ }\n");
        // Index-only addition -> Staged.
        write(&root.join("staged.txt"), "staged\n");
        git(&root, &["add", "staged.txt"]);
        // A rename staged in the index -> Staged with an orig_path.
        git(&root, &["mv", "renamed-old.txt", "renamed-new.txt"]);
        // Never added -> Untracked.
        write(&root.join("untracked.txt"), "untracked\n");

        Fixture { dir }
    }
}

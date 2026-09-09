//! The ONLY module that shells out to `git`, and only with read-only subcommands (spec §10).
//!
//! Two invariants hold for every invocation:
//!  * `GIT_OPTIONAL_LOCKS=0` — otherwise `git status` refreshes and REWRITES the index, taking
//!    `index.lock` on the way. That is a write, which the read-only contract forbids.
//!  * `--no-ext-diff --no-textconv` on every diff — a user's gitconfig can point these at
//!    arbitrary programs, and running them on the user's behalf is exactly the kind of
//!    execution the read-only boundary exists to prevent.

use crate::model::{GroupKind, RepoEntry, RepoRoot};
use crate::{porcelain, proc};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The per-invocation wall-clock budget. Generous enough for a cold `git status` on a large
/// repo, short enough that a wedged repo costs one poll round rather than the panel.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for the stdout reader thread after the child is gone. The pipe is already
/// at EOF by then, so this only covers scheduling.
const READ_GRACE: Duration = Duration::from_millis(250);

/// Cap on bytes captured from one git invocation, bounding memory against a pathological repo.
const MAX_OUTPUT: u64 = 64 * 1024 * 1024;

/// Why a git query did not produce output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The invocation overran its budget and was killed. The row goes `stale` and retries.
    Timeout,
    /// git ran and exited non-zero; carries the full stderr (up to 64 KB). `Display` renders
    /// only its first line — the full text is untrusted and must not reach the screen as-is.
    Failed(String),
    /// git could not be started at all.
    Spawn(String),
    /// There is no git query for this request (an Untracked file's "diff").
    NotApplicable,
}

impl fmt::Display for GitError {
    /// One short line: this lands in a repo row that has exactly one line of space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::Timeout => write!(f, "git timed out"),
            GitError::Failed(msg) => write!(f, "{}", first_line(msg)),
            GitError::Spawn(msg) => write!(f, "could not run git: {}", first_line(msg)),
            GitError::NotApplicable => write!(f, "no diff for this entry"),
        }
    }
}

/// The first non-empty line of `s`, or a generic label when there is none.
fn first_line(s: &str) -> &str {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("git failed")
}

/// The git queries the app needs, behind a trait so the controller and poller can be tested
/// against a stub. `Send + Sync` because the poller thread holds it through an `Arc`.
pub trait GitService: Send + Sync {
    /// One status snapshot per repo. A repo that fails or times out yields an entry carrying
    /// that fact — never a missing entry, and never an error for the whole batch (spec §9).
    fn snapshot(&self, repos: &[RepoRoot]) -> Vec<RepoEntry>;

    /// The raw patch bytes for one file, with the baseline chosen by its group (spec §4.1).
    fn diff(&self, repo: &Path, group: GroupKind, path: &str) -> Result<Vec<u8>, GitError>;
}

/// The real [`GitService`].
pub struct LiveGit {
    timeout: Duration,
}

impl LiveGit {
    pub fn new(timeout: Duration) -> Self {
        LiveGit { timeout }
    }
}

impl Default for LiveGit {
    fn default() -> Self {
        LiveGit::new(DEFAULT_TIMEOUT)
    }
}

impl GitService for LiveGit {
    fn snapshot(&self, repos: &[RepoRoot]) -> Vec<RepoEntry> {
        repos.iter().map(|r| self.entry(r)).collect()
    }

    fn diff(&self, repo: &Path, group: GroupKind, path: &str) -> Result<Vec<u8>, GitError> {
        let args = diff_args(group, path).ok_or(GitError::NotApplicable)?;
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_bounded(repo, &borrowed, self.timeout)
    }
}

impl LiveGit {
    /// One repo's entry. Every failure mode lands in the entry rather than propagating.
    fn entry(&self, root: &RepoRoot) -> RepoEntry {
        let mut entry = RepoEntry::from_root(root);
        match run_bounded(&root.path, status_args(), self.timeout) {
            Ok(bytes) => {
                let status = porcelain::parse(&bytes);
                // A detached HEAD has no branch name; the short oid is what the user can act on.
                entry.branch = status
                    .head
                    .clone()
                    .or_else(|| status.oid.as_deref().map(short_sha));
                entry.ahead = status.ahead;
                entry.behind = status.behind;
                entry.groups = status.groups();
            }
            Err(GitError::Timeout) => entry.stale = true,
            Err(e) => entry.error = Some(e.to_string()),
        }
        entry
    }
}

/// The status query. `--untracked-files=all` lists individual files rather than collapsing an
/// untracked directory to one row: the spec's tree shows files, and every untracked row must be
/// selectable and renderable. The cost of a huge untracked tree is bounded by the timeout, which
/// marks the row stale rather than stalling the panel.
pub fn status_args() -> &'static [&'static str] {
    &[
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
        "--untracked-files=all",
    ]
}

/// The diff argv for one group (spec §4.1), or `None` for Untracked, which has no git diff.
///
/// The path always follows `--`, so a file literally named like an option cannot inject one.
pub fn diff_args(group: GroupKind, path: &str) -> Option<Vec<String>> {
    let mut args: Vec<String> = vec!["diff".to_string()];
    match group {
        GroupKind::Staged => args.push("--cached".to_string()),
        GroupKind::Changes => {}
        GroupKind::Untracked => return None,
    }
    args.extend(
        ["--no-color", "--no-ext-diff", "--no-textconv", "--"]
            .iter()
            .map(|s| s.to_string()),
    );
    args.push(path.to_string());
    Some(args)
}

/// The first seven characters of an object id — git's own abbreviation length.
pub fn short_sha(oid: &str) -> String {
    oid.chars().take(7).collect()
}

/// Build a git [`Command`] with the read-only environment every invocation needs.
fn git_command(repo: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo)
        // See the module docs: without this, `git status` writes the index.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // Never block the poller on a credential prompt.
        .env("GIT_TERMINAL_PROMPT", "0")
        // A pager would never exit; git disables it for non-tty output, but be explicit.
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Run a read-only git query with a wall-clock budget, returning its stdout bytes.
///
/// stdout is drained by a reader thread so a large output can never deadlock against the pipe
/// buffer while we wait. stderr is read only after the child is gone: it is small in practice,
/// and if a pathological repo did fill that buffer the child would stall and the timeout would
/// kill it — which is the correct outcome either way.
fn run_bounded(repo: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, GitError> {
    let mut child = git_command(repo, args)
        .spawn()
        .map_err(|e| GitError::Spawn(e.to_string()))?;

    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    if let Some(stdout) = stdout {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }

    let status = proc::wait_until(&mut child, Instant::now() + timeout);
    let out = rx.recv_timeout(READ_GRACE).unwrap_or_default();

    match status {
        None => Err(GitError::Timeout),
        Some(s) if s.success() => Ok(out),
        Some(_) => {
            let mut stderr = String::new();
            if let Some(pipe) = child.stderr.take() {
                let _ = pipe.take(64 * 1024).read_to_string(&mut stderr);
            }
            Err(GitError::Failed(stderr))
        }
    }
}

/// `git rev-parse --show-toplevel`, for turning a pane cwd into its repo root (spec §3.1).
pub struct RealTopLevel;

impl crate::discover::TopLevelResolver for RealTopLevel {
    fn toplevel(&self, dir: &Path) -> Option<PathBuf> {
        let out = run_bounded(dir, &["rev-parse", "--show-toplevel"], DEFAULT_TIMEOUT).ok()?;
        let text = String::from_utf8_lossy(&out);
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(cmd: &Command) -> Vec<(String, Option<String>)> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn every_git_invocation_disables_optional_locks_so_status_never_writes_the_index() {
        // Without GIT_OPTIONAL_LOCKS=0, `git status` takes index.lock and rewrites the index —
        // a WRITE, which spec §10 forbids outright.
        let cmd = git_command(Path::new("/w"), &["status"]);
        assert!(
            env_of(&cmd).contains(&("GIT_OPTIONAL_LOCKS".to_string(), Some("0".to_string()))),
            "envs: {:?}",
            env_of(&cmd)
        );
    }

    #[test]
    fn every_git_invocation_disables_the_terminal_prompt() {
        // A repo with a credential-requiring remote must never block the poller on a prompt.
        let cmd = git_command(Path::new("/w"), &["status"]);
        assert!(env_of(&cmd).contains(&("GIT_TERMINAL_PROMPT".to_string(), Some("0".to_string()))));
    }

    #[test]
    fn a_git_invocation_runs_in_the_repo_directory() {
        let cmd = git_command(Path::new("/w/repo"), &["status"]);
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/w/repo")));
    }

    #[test]
    fn the_status_query_asks_for_porcelain_v2_nul_separated_with_branch_and_all_untracked() {
        assert_eq!(
            status_args(),
            [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ]
        );
    }

    #[test]
    fn the_staged_diff_is_index_versus_head() {
        assert_eq!(
            diff_args(GroupKind::Staged, "src/app.rs").unwrap(),
            [
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "src/app.rs"
            ]
        );
    }

    #[test]
    fn the_changes_diff_is_worktree_versus_index() {
        assert_eq!(
            diff_args(GroupKind::Changes, "src/app.rs").unwrap(),
            [
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "src/app.rs"
            ]
        );
    }

    #[test]
    fn an_untracked_file_has_no_git_diff_at_all() {
        // spec §4.1: the Untracked row is rendered from the file's own contents, not from git.
        assert!(diff_args(GroupKind::Untracked, "new.rs").is_none());
    }

    #[test]
    fn a_path_that_looks_like_an_option_is_still_safe_because_it_follows_the_double_dash() {
        let args = diff_args(GroupKind::Changes, "--upload-pack=evil").unwrap();
        let dashdash = args.iter().position(|a| a == "--").expect("-- present");
        assert_eq!(args[dashdash + 1], "--upload-pack=evil");
    }

    #[test]
    fn external_diff_and_textconv_are_disabled_on_every_diff() {
        // A user's gitconfig can point these at arbitrary programs; running them would break
        // spec §10's read-only boundary.
        for group in [GroupKind::Staged, GroupKind::Changes] {
            let args = diff_args(group, "f").unwrap();
            assert!(args.iter().any(|a| a == "--no-ext-diff"), "{group:?}");
            assert!(args.iter().any(|a| a == "--no-textconv"), "{group:?}");
        }
    }

    #[test]
    fn a_short_sha_is_seven_characters_and_shorter_input_is_returned_whole() {
        assert_eq!(short_sha("1234567890abcdef"), "1234567");
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(short_sha(""), "");
    }

    #[test]
    fn git_errors_render_as_short_single_line_messages() {
        assert_eq!(GitError::Timeout.to_string(), "git timed out");
        assert!(
            GitError::Failed("fatal: bad".to_string())
                .to_string()
                .contains("fatal: bad")
        );
        assert!(
            !GitError::Spawn("no such file".to_string())
                .to_string()
                .contains('\n')
        );
    }

    #[test]
    fn a_multi_line_git_error_is_collapsed_to_its_first_line_for_the_repo_row() {
        // The row has one line of space; the whole stderr would push everything off screen.
        let e = GitError::Failed("fatal: not a git repository\nsecond line\nthird".to_string());
        assert_eq!(e.to_string(), "fatal: not a git repository");
    }
}

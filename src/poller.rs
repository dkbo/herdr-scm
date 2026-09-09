//! The two background threads of spec §7: the status poller and the diff render worker.
//!
//! Both are plain `std::thread` + `mpsc` — no async runtime. The input thread never blocks on
//! git or on an external renderer; it drains whatever has arrived and draws.

use crate::git::GitService;
use crate::model::{GroupKind, RepoRoot, Snapshot};
use crate::render::{self, Caps, DiffRenderer, Rendered};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Where the repo list comes from. Behind a trait so the poller is tested without a filesystem.
pub trait RepoSource: Send {
    fn rescan(&self) -> Vec<RepoRoot>;
}

/// What the poller publishes.
#[derive(Debug)]
pub enum PollMsg {
    /// A fresh repo list, after a filesystem re-walk.
    Roots(Vec<RepoRoot>),
    /// A status snapshot for the current repo list.
    Snapshot(Snapshot),
}

/// Commands to the poller thread.
enum PollCmd {
    RefreshNow,
    Stop,
}

/// The background status poller (spec §3.4).
pub struct Poller {
    rx: Receiver<PollMsg>,
    tx: Sender<PollCmd>,
    handle: Option<JoinHandle<()>>,
}

impl Poller {
    /// Start polling. A round is published immediately so the panel is never blank at launch;
    /// after that a round runs every `interval`, and the repo list is re-walked every
    /// `rescan_every` rounds. `interval == 0` disables automatic rounds entirely (spec §8)
    /// while leaving [`refresh_now`](Self::refresh_now) working.
    pub fn spawn(
        git: Arc<dyn GitService>,
        source: Box<dyn RepoSource>,
        interval: Duration,
        rescan_every: u32,
    ) -> Poller {
        let (msg_tx, rx) = mpsc::channel();
        let (tx, cmd_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut roots = source.rescan();
            let mut generation = 0u64;
            let mut round = 0u32;
            if msg_tx.send(PollMsg::Roots(roots.clone())).is_err() {
                return;
            }
            loop {
                generation += 1;
                let repos = git.snapshot(&roots);
                if msg_tx
                    .send(PollMsg::Snapshot(Snapshot { repos, generation }))
                    .is_err()
                {
                    return;
                }
                // Wait for the next round, or a command. A zero interval means "no automatic
                // rounds": block until commanded.
                let cmd = if interval.is_zero() {
                    cmd_rx.recv().ok()
                } else {
                    match cmd_rx.recv_timeout(interval) {
                        Ok(cmd) => Some(cmd),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                };
                let forced = match cmd {
                    Some(PollCmd::Stop) => return,
                    Some(PollCmd::RefreshNow) => true,
                    None => false,
                };
                round = round.wrapping_add(1);
                let due = rescan_every > 0 && round.is_multiple_of(rescan_every);
                if forced || due {
                    roots = source.rescan();
                    if msg_tx.send(PollMsg::Roots(roots.clone())).is_err() {
                        return;
                    }
                }
            }
        });
        Poller {
            rx,
            tx,
            handle: Some(handle),
        }
    }

    /// Everything that has arrived since the last call. Never blocks.
    pub fn drain(&self) -> Vec<PollMsg> {
        drain_channel(&self.rx)
    }

    /// Run a round now, including a repo-list re-walk (the `r` key). Best-effort: a stopped
    /// poller simply ignores it.
    pub fn refresh_now(&self) {
        let _ = self.tx.send(PollCmd::RefreshNow);
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        let _ = self.tx.send(PollCmd::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// One diff to produce. `seq` is monotonic; a result whose seq is not the current one is
/// discarded by the controller (spec §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffJob {
    pub seq: u64,
    pub repo: PathBuf,
    pub group: GroupKind,
    pub path: String,
}

/// A finished diff.
pub struct DiffResult {
    pub seq: u64,
    pub rendered: Rendered,
}

/// Where the controller posts diff jobs. Behind a trait so the controller is tested without a
/// thread.
pub trait JobSink {
    fn submit(&self, job: DiffJob);
    /// Whatever has finished since the last call. Defaults to nothing, so a test stub does not
    /// have to implement it.
    fn drain(&self) -> Vec<DiffResult> {
        Vec::new()
    }
}

/// The background diff renderer.
pub struct RenderWorker {
    tx: Sender<DiffJob>,
    rx: Receiver<DiffResult>,
    handle: Option<JoinHandle<()>>,
}

impl RenderWorker {
    pub fn spawn(
        git: Arc<dyn GitService>,
        renderer: Arc<dyn DiffRenderer>,
        caps: Caps,
    ) -> RenderWorker {
        let (tx, job_rx) = mpsc::channel::<DiffJob>();
        let (result_tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let seq = job.seq;
                // spec §7: a renderer panic must not take the worker with it.
                let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_job(&job, git.as_ref(), renderer.as_ref(), caps)
                }))
                .unwrap_or_else(|_| Rendered {
                    text: ratatui::text::Text::raw("the diff renderer failed"),
                    notice: Some("the diff renderer panicked".to_string()),
                });
                if result_tx.send(DiffResult { seq, rendered }).is_err() {
                    return;
                }
            }
        });
        RenderWorker {
            tx,
            rx,
            handle: Some(handle),
        }
    }

    /// Everything finished since the last call. Never blocks.
    pub fn drain(&self) -> Vec<DiffResult> {
        drain_channel(&self.rx)
    }
}

impl JobSink for RenderWorker {
    fn submit(&self, job: DiffJob) {
        let _ = self.tx.send(job);
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        // Dropping the sender ends the worker's `recv` loop.
        let (dead, _) = mpsc::channel();
        let _ = std::mem::replace(&mut self.tx, dead);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Produce one diff: pick the baseline by group (spec §4.1), cap it, then delegate styling.
fn run_job(
    job: &DiffJob,
    git: &dyn GitService,
    renderer: &dyn DiffRenderer,
    caps: Caps,
) -> Rendered {
    let prepared = match job.group {
        GroupKind::Untracked => {
            render::prepare_untracked(&job.repo.join(&job.path), &job.path, caps)
        }
        group => match git.diff(&job.repo, group, &job.path) {
            Ok(bytes) => render::prepare_patch(&bytes, caps),
            Err(e) => {
                return Rendered {
                    text: ratatui::text::Text::raw(e.to_string()),
                    notice: Some(e.to_string()),
                };
            }
        },
    };
    render::render(prepared, renderer)
}

/// Take everything currently queued on `rx` without blocking.
fn drain_channel<T>(rx: &Receiver<T>) -> Vec<T> {
    let mut out = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(item) => out.push(item),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RepoEntry, RepoKind};
    use crate::render::NoRenderer;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// A git stub that records what it was asked and answers instantly.
    #[derive(Default)]
    struct StubGit {
        snapshots: AtomicUsize,
        diffs: Mutex<Vec<(PathBuf, GroupKind, String)>>,
        diff_bytes: Vec<u8>,
    }

    impl GitService for StubGit {
        fn snapshot(&self, repos: &[RepoRoot]) -> Vec<RepoEntry> {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            repos.iter().map(RepoEntry::from_root).collect()
        }
        fn diff(
            &self,
            repo: &Path,
            group: GroupKind,
            path: &str,
        ) -> Result<Vec<u8>, crate::git::GitError> {
            self.diffs
                .lock()
                .expect("lock")
                .push((repo.to_path_buf(), group, path.to_string()));
            Ok(self.diff_bytes.clone())
        }
    }

    #[derive(Default)]
    struct StubSource {
        calls: Arc<AtomicUsize>,
    }

    impl RepoSource for StubSource {
        fn rescan(&self) -> Vec<RepoRoot> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            vec![RepoRoot {
                path: PathBuf::from("/w/a"),
                scan_root: PathBuf::from("/w/a"),
                kind: RepoKind::Root,
            }]
        }
    }

    /// Poll `drain` until `want` messages have arrived or the deadline passes.
    fn collect(poller: &Poller, want: usize) -> Vec<PollMsg> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < want && Instant::now() < deadline {
            got.extend(poller.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    #[test]
    fn the_poller_publishes_a_repo_list_and_a_snapshot_immediately_on_start() {
        // The panel must not be blank for a whole poll interval at launch.
        let poller = Poller::spawn(
            Arc::new(StubGit::default()),
            Box::new(StubSource::default()),
            Duration::from_secs(3600),
            10,
        );
        let msgs = collect(&poller, 2);
        assert!(msgs.iter().any(|m| matches!(m, PollMsg::Roots(_))));
        assert!(msgs.iter().any(|m| matches!(m, PollMsg::Snapshot(_))));
    }

    #[test]
    fn snapshot_generations_increase_by_one_each_round() {
        let poller = Poller::spawn(
            Arc::new(StubGit::default()),
            Box::new(StubSource::default()),
            Duration::from_millis(20),
            10,
        );
        let generations: Vec<u64> = collect(&poller, 6)
            .into_iter()
            .filter_map(|m| match m {
                PollMsg::Snapshot(s) => Some(s.generation),
                _ => None,
            })
            .collect();
        assert!(generations.len() >= 2, "{generations:?}");
        assert_eq!(generations[0], 1);
        for pair in generations.windows(2) {
            assert_eq!(pair[1], pair[0] + 1, "{generations:?}");
        }
    }

    #[test]
    fn the_repo_list_is_re_walked_only_every_rescan_every_rounds() {
        // Walking the filesystem is the expensive half; it must not run every 3 seconds.
        let calls = Arc::new(AtomicUsize::new(0));
        let poller = Poller::spawn(
            Arc::new(StubGit::default()),
            Box::new(StubSource {
                calls: calls.clone(),
            }),
            Duration::from_millis(10),
            5,
        );
        let _ = collect(&poller, 8);
        let rescans = calls.load(Ordering::SeqCst);
        assert!(rescans >= 1, "the startup rescan must happen");
        assert!(
            rescans < 8,
            "rescans={rescans}: too many for rescan_every=5"
        );
    }

    #[test]
    fn a_manual_refresh_re_walks_the_repo_list_without_waiting_for_the_interval() {
        let calls = Arc::new(AtomicUsize::new(0));
        let poller = Poller::spawn(
            Arc::new(StubGit::default()),
            Box::new(StubSource {
                calls: calls.clone(),
            }),
            Duration::from_secs(3600),
            1000,
        );
        let _ = collect(&poller, 2);
        let before = calls.load(Ordering::SeqCst);
        poller.refresh_now();
        let deadline = Instant::now() + Duration::from_secs(5);
        while calls.load(Ordering::SeqCst) == before && Instant::now() < deadline {
            let _ = poller.drain();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(calls.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn an_interval_of_zero_turns_automatic_polling_off_but_keeps_manual_refresh_working() {
        // spec §8: "poll_interval_secs = 0 → 關閉輪詢，退化成純手動 r".
        let git = Arc::new(StubGit::default());
        let poller = Poller::spawn(
            git.clone(),
            Box::new(StubSource::default()),
            Duration::ZERO,
            10,
        );
        let _ = collect(&poller, 2);
        let after_start = git.snapshots.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            git.snapshots.load(Ordering::SeqCst),
            after_start,
            "polling must be off"
        );
        poller.refresh_now();
        let _ = collect(&poller, 1);
        assert!(git.snapshots.load(Ordering::SeqCst) > after_start);
    }

    #[test]
    fn dropping_the_poller_stops_its_thread() {
        let git = Arc::new(StubGit::default());
        {
            let poller = Poller::spawn(
                git.clone(),
                Box::new(StubSource::default()),
                Duration::from_millis(10),
                10,
            );
            let _ = collect(&poller, 2);
        }
        let after_drop = git.snapshots.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(git.snapshots.load(Ordering::SeqCst), after_drop);
    }

    // ---- the render worker -------------------------------------------------------------------

    fn collect_diffs(worker: &RenderWorker, want: usize) -> Vec<DiffResult> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < want && Instant::now() < deadline {
            got.extend(worker.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    #[test]
    fn a_staged_job_asks_git_for_the_cached_diff_and_returns_it_with_its_sequence() {
        let git = Arc::new(StubGit {
            diff_bytes: b"@@ -1 +1 @@\n+x\n".to_vec(),
            ..StubGit::default()
        });
        let worker = RenderWorker::spawn(git.clone(), Arc::new(NoRenderer), Caps::default());
        worker.submit(DiffJob {
            seq: 7,
            repo: PathBuf::from("/w/a"),
            group: GroupKind::Staged,
            path: "f.rs".to_string(),
        });
        let results = collect_diffs(&worker, 1);
        assert_eq!(results[0].seq, 7);
        assert_eq!(
            git.diffs.lock().expect("lock")[0],
            (PathBuf::from("/w/a"), GroupKind::Staged, "f.rs".to_string())
        );
    }

    #[test]
    fn an_untracked_job_never_asks_git_for_a_diff() {
        // spec §4.1: an untracked entry is rendered from the file itself.
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("n.txt"), "hi\n").expect("write");
        let git = Arc::new(StubGit::default());
        let worker = RenderWorker::spawn(git.clone(), Arc::new(NoRenderer), Caps::default());
        worker.submit(DiffJob {
            seq: 1,
            repo: dir.path().to_path_buf(),
            group: GroupKind::Untracked,
            path: "n.txt".to_string(),
        });
        let _ = collect_diffs(&worker, 1);
        assert!(git.diffs.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_git_failure_becomes_a_visible_message_rather_than_a_lost_job() {
        struct FailingGit;
        impl GitService for FailingGit {
            fn snapshot(&self, _: &[RepoRoot]) -> Vec<RepoEntry> {
                Vec::new()
            }
            fn diff(
                &self,
                _: &Path,
                _: GroupKind,
                _: &str,
            ) -> Result<Vec<u8>, crate::git::GitError> {
                Err(crate::git::GitError::Failed(
                    "fatal: bad object".to_string(),
                ))
            }
        }
        let worker =
            RenderWorker::spawn(Arc::new(FailingGit), Arc::new(NoRenderer), Caps::default());
        worker.submit(DiffJob {
            seq: 1,
            repo: PathBuf::from("/w/a"),
            group: GroupKind::Changes,
            path: "f.rs".to_string(),
        });
        let results = collect_diffs(&worker, 1);
        assert_eq!(results[0].seq, 1);
        assert!(results[0].rendered.notice.is_some());
    }

    #[test]
    fn a_renderer_that_panics_does_not_kill_the_worker() {
        // spec §7: the render worker is wrapped in catch_unwind.
        struct PanickingRenderer;
        impl crate::render::DiffRenderer for PanickingRenderer {
            fn render(&self, _patch: &str) -> Option<String> {
                panic!("renderer exploded");
            }
            fn name(&self) -> &str {
                "boom"
            }
        }
        // Keep the expected panic output out of the test run's stderr.
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let worker = RenderWorker::spawn(
            Arc::new(StubGit::default()),
            Arc::new(PanickingRenderer),
            Caps::default(),
        );
        for seq in 1..=2 {
            worker.submit(DiffJob {
                seq,
                repo: PathBuf::from("/w/a"),
                group: GroupKind::Changes,
                path: "f.rs".to_string(),
            });
        }
        // Both jobs must come back: the second proves the thread survived the first panic.
        let results = collect_diffs(&worker, 2);
        std::panic::set_hook(prev_hook);
        assert_eq!(results.len(), 2, "{:?}", results.len());
    }
}

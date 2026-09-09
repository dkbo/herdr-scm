//! Shared subprocess reaping: the one place that knows how to wait for a child within a
//! bounded wall-clock budget and kill/reap it if it overruns. Used by `git.rs` (status/diff)
//! and `render.rs` (the external diff renderer), so the timeout-kill semantics live once.

use std::process::Child;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Wait for `child` for at most `grace`; on overrun kill and reap it and return `None`.
pub fn wait_bounded(child: &mut Child, grace: Duration) -> Option<std::process::ExitStatus> {
    wait_until(child, Instant::now() + grace)
}

/// Wait for `child` through an absolute deadline.
///
/// The deadline bounds only the wait for USEFUL WORK. On overrun the child is killed and reaped
/// unconditionally, so the call may briefly outlive the deadline — that is deliberate: bounding
/// the reap too returns early under load and leaks the killed child as a zombie.
pub fn wait_until(child: &mut Child, deadline: Instant) -> Option<std::process::ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => sleep_until(deadline),
            Ok(None) | Err(_) => {
                let _ = terminate_and_reap(child);
                return None;
            }
        }
    }
}

/// Kill and reap `child` unconditionally. SIGKILL cannot be ignored, so the post-kill `wait()`
/// returns promptly outside pathological OS stalls.
pub fn terminate_and_reap(child: &mut Child) -> Option<std::process::ExitStatus> {
    if let Ok(Some(status)) = child.try_wait() {
        return Some(status);
    }
    let _ = child.kill();
    child.wait().ok()
}

fn sleep_until(deadline: Instant) {
    std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// v1 is Linux-only, so `/bin/sh` is a safe, dependency-free child to spawn.
    fn sh(script: &str) -> std::process::Child {
        Command::new("/bin/sh")
            .args(["-c", script])
            .spawn()
            .expect("spawn /bin/sh")
    }

    #[test]
    fn a_child_that_exits_in_time_keeps_its_status() {
        let mut child = sh("exit 0");
        assert!(wait_bounded(&mut child, Duration::from_secs(5)).is_some_and(|s| s.success()));
    }

    #[test]
    fn a_non_zero_exit_is_still_a_status_not_a_timeout() {
        let mut child = sh("exit 3");
        let status = wait_bounded(&mut child, Duration::from_secs(5));
        assert!(status.is_some());
        assert!(!status.unwrap().success());
    }

    #[test]
    fn an_overrunning_child_is_killed_reaped_and_reported_as_none() {
        let mut child = sh("sleep 60");
        let started = Instant::now();

        assert_eq!(wait_bounded(&mut child, Duration::from_millis(100)), None);
        // The fixture sleeps 60s. A 3s bound is 30x the requested timeout — ample slack for a
        // loaded runner while still proving we returned instead of waiting the child out.
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "timeout must return promptly, took {:?}",
            started.elapsed()
        );
        assert!(
            child.try_wait().expect("try_wait").is_some(),
            "the killed child must be reaped, not left a zombie"
        );
    }

    #[test]
    fn a_zero_grace_still_kills_and_reaps_rather_than_leaking() {
        let mut child = sh("sleep 60");
        assert_eq!(wait_bounded(&mut child, Duration::ZERO), None);
        assert!(child.try_wait().expect("try_wait").is_some());
    }

    #[test]
    fn terminate_and_reap_is_idempotent_on_an_already_exited_child() {
        let mut child = sh("exit 0");
        let _ = child.wait();
        // Must not error or hang when there is nothing left to kill.
        let _ = terminate_and_reap(&mut child);
    }
}

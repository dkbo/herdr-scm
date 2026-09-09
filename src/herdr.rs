//! herdr CLI seam (spec §6). All herdr interaction goes through [`HerdrCli`]; tests inject a
//! fake [`CommandRunner`] so nothing is ever really spawned.
//!
//! Read-only with respect to files and git: this runs herdr *queries* plus the occasional
//! host **layout** command (`pane zoom`), which touches neither the filesystem nor any repo.

use serde::Deserialize;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::PathBuf;
use std::process::Output;

/// Run read-only herdr subcommands.
pub trait HerdrCli {
    /// Run a subcommand expected to emit JSON on stdout. A non-zero exit is an `Err` so the
    /// caller can degrade (spec §9).
    fn run_json(&self, args: &[&str]) -> io::Result<String>;

    /// Run a subcommand for its side effect (a host layout op), discarding stdout.
    fn run(&self, args: &[&str]) -> io::Result<()> {
        self.run_json(args).map(|_| ())
    }
}

/// The inner execution seam, so tests can assert argv without spawning.
pub trait CommandRunner {
    fn run(&self, program: &OsStr, args: &[&str]) -> io::Result<Output>;
}

/// The real runner.
pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, program: &OsStr, args: &[&str]) -> io::Result<Output> {
        std::process::Command::new(program).args(args).output()
    }
}

/// The real [`HerdrCli`], invoking the resolved herdr binary through an injected runner.
pub struct LiveHerdr<R: CommandRunner = RealRunner> {
    program: OsString,
    runner: R,
}

impl LiveHerdr<RealRunner> {
    /// Resolve the binary from `$HERDR_BIN_PATH`, else `herdr` on `$PATH`.
    pub fn from_env() -> Self {
        LiveHerdr {
            program: resolve_program(std::env::var("HERDR_BIN_PATH").ok()),
            runner: RealRunner,
        }
    }
}

impl<R: CommandRunner> LiveHerdr<R> {
    /// Construct with an explicit program and runner (tests).
    pub fn with_runner(program: impl Into<OsString>, runner: R) -> Self {
        LiveHerdr {
            program: program.into(),
            runner,
        }
    }

    /// The injected runner, so a test can read what it recorded.
    pub fn runner(&self) -> &R {
        &self.runner
    }
}

impl<R: CommandRunner> HerdrCli for LiveHerdr<R> {
    fn run_json(&self, args: &[&str]) -> io::Result<String> {
        let out = self.runner.run(&self.program, args)?;
        if !out.status.success() {
            return Err(io::Error::other("herdr exited non-zero"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// `$HERDR_BIN_PATH` when set and non-empty, else `herdr`.
pub fn resolve_program(var: Option<String>) -> OsString {
    match var {
        Some(v) if !v.is_empty() => OsString::from(v),
        _ => OsString::from("herdr"),
    }
}

/// Whether a host-supplied id is safe to pass as a CLI argument: non-empty, no leading `-`
/// (which would option-inject), no whitespace or control characters.
pub fn is_flag_safe(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ---------------------------------------------------------------------------
// pane list
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PaneList {
    result: PaneListResult,
}

#[derive(Deserialize)]
struct PaneListResult {
    #[serde(default)]
    panes: Vec<Pane>,
}

#[derive(Deserialize)]
struct Pane {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    foreground_cwd: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    focused: bool,
}

/// The scan-start candidates from a `herdr pane list` payload (spec §3.1): the cwd of every
/// pane in our workspace, de-duplicated, first-seen order preserved.
///
/// When `workspace_id` is `None` the focused pane's workspace is used instead — a launch that
/// did not inject the id still gets the right scope. With neither, the result is EMPTY rather
/// than every pane: aggregating across workspaces is explicitly out of scope (spec §2), and an
/// empty result is what makes the caller fall back to `workspace_cwd`.
pub fn pane_cwds(pane_list_json: &str, workspace_id: Option<&str>) -> Vec<PathBuf> {
    let Ok(list) = serde_json::from_str::<PaneList>(pane_list_json) else {
        return Vec::new();
    };
    let panes = &list.result.panes;
    let scope = workspace_id.map(str::to_string).or_else(|| {
        panes
            .iter()
            .find(|p| p.focused)
            .and_then(|p| p.workspace_id.clone())
    });
    let Some(scope) = scope else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    panes
        .iter()
        .filter(|p| p.workspace_id.as_deref() == Some(scope.as_str()))
        // spec §3.1 says "取其 cwd"; `foreground_cwd` is the fallback for a pane that reports
        // only the latter.
        .filter_map(|p| {
            p.cwd
                .as_deref()
                .or(p.foreground_cwd.as_deref())
                .filter(|s| !s.is_empty())
        })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// Query herdr for this workspace's pane cwds. Any failure yields an empty vec so the caller
/// degrades to `workspace_cwd` (spec §9).
pub fn panes_in_workspace(cli: &dyn HerdrCli, workspace_id: Option<&str>) -> Vec<PathBuf> {
    // `herdr pane list` has NO `--json` flag on 0.9.0 (it errors) — it already emits JSON.
    // Scope server-side when the id is safe to pass; otherwise list everything and filter here.
    let scoped = workspace_id.filter(|id| is_flag_safe(id));
    let args: Vec<&str> = match scoped {
        Some(id) => vec!["pane", "list", "--workspace", id],
        None => vec!["pane", "list"],
    };
    match cli.run_json(&args) {
        Ok(json) => pane_cwds(&json, workspace_id),
        Err(_) => Vec::new(),
    }
}

/// Toggle this pane's herdr zoom (the `Z` key). Best-effort: a failure is silently ignored,
/// because zoom is a convenience and the panel stays usable without it.
pub fn zoom_current(cli: &dyn HerdrCli) {
    let _ = cli.run(&["pane", "zoom", "--current", "--toggle"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    /// A recording runner: asserts argv without ever spawning anything.
    #[derive(Default)]
    struct Recorder {
        calls: RefCell<Vec<Vec<String>>>,
        stdout: String,
        fail: bool,
    }

    impl CommandRunner for Recorder {
        fn run(&self, program: &OsStr, args: &[&str]) -> io::Result<Output> {
            let mut call = vec![program.to_string_lossy().into_owned()];
            call.extend(args.iter().map(|a| a.to_string()));
            self.calls.borrow_mut().push(call);
            Ok(Output {
                status: ExitStatus::from_raw(if self.fail { 256 } else { 0 }),
                stdout: self.stdout.clone().into_bytes(),
                stderr: Vec::new(),
            })
        }
    }

    /// A pane-list payload in the exact shape herdr 0.9.0 emits (verified live).
    fn pane_list(entries: &[(&str, &str, &str, bool)]) -> String {
        let panes: Vec<String> = entries
            .iter()
            .map(|(pane_id, cwd, ws, focused)| {
                format!(
                    r#"{{"pane_id":"{pane_id}","cwd":"{cwd}","foreground_cwd":"{cwd}","workspace_id":"{ws}","tab_id":"{ws}:t1","focused":{focused}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"id":"cli:pane:list","result":{{"panes":[{}]}}}}"#,
            panes.join(",")
        )
    }

    // ---- resolve_program -------------------------------------------------------------------

    #[test]
    fn the_herdr_binary_comes_from_the_env_var_when_set() {
        assert_eq!(
            resolve_program(Some("/opt/herdr".to_string())),
            OsString::from("/opt/herdr")
        );
    }

    #[test]
    fn an_absent_or_empty_env_var_falls_back_to_the_name_on_path() {
        assert_eq!(resolve_program(None), OsString::from("herdr"));
        assert_eq!(
            resolve_program(Some(String::new())),
            OsString::from("herdr")
        );
    }

    // ---- run_json --------------------------------------------------------------------------

    #[test]
    fn run_json_passes_the_args_through_verbatim_and_returns_stdout() {
        let recorder = Recorder {
            stdout: "{\"ok\":true}".to_string(),
            ..Recorder::default()
        };
        let cli = LiveHerdr::with_runner("herdr", recorder);
        assert_eq!(cli.run_json(&["pane", "list"]).unwrap(), "{\"ok\":true}");
        assert_eq!(
            cli.runner().calls.borrow()[0],
            vec!["herdr", "pane", "list"]
        );
    }

    #[test]
    fn a_non_zero_exit_is_an_error_so_the_caller_can_degrade() {
        let cli = LiveHerdr::with_runner(
            "herdr",
            Recorder {
                fail: true,
                ..Recorder::default()
            },
        );
        assert!(cli.run_json(&["pane", "list"]).is_err());
    }

    // ---- pane_cwds -------------------------------------------------------------------------

    #[test]
    fn pane_cwds_keeps_only_the_panes_of_the_named_workspace() {
        let json = pane_list(&[
            ("wA:p1", "/w/a", "wA", true),
            ("wB:p1", "/w/b", "wB", false),
            ("wA:p2", "/w/a/sub", "wA", false),
        ]);
        assert_eq!(
            pane_cwds(&json, Some("wA")),
            vec![PathBuf::from("/w/a"), PathBuf::from("/w/a/sub")]
        );
    }

    #[test]
    fn pane_cwds_deduplicates_while_preserving_first_seen_order() {
        let json = pane_list(&[
            ("wA:p1", "/w/a", "wA", true),
            ("wA:p2", "/w/a", "wA", false),
            ("wA:p3", "/w/z", "wA", false),
        ]);
        assert_eq!(
            pane_cwds(&json, Some("wA")),
            vec![PathBuf::from("/w/a"), PathBuf::from("/w/z")]
        );
    }

    #[test]
    fn without_a_known_workspace_id_the_focused_panes_workspace_is_used() {
        // A launch that did not inject workspace_id still gets the right scope: the focused
        // pane is by definition in the user's current workspace.
        let json = pane_list(&[
            ("wB:p1", "/w/b", "wB", false),
            ("wA:p1", "/w/a", "wA", true),
        ]);
        assert_eq!(pane_cwds(&json, None), vec![PathBuf::from("/w/a")]);
    }

    #[test]
    fn with_neither_a_workspace_id_nor_a_focused_pane_nothing_is_returned() {
        // Returning every pane would aggregate across workspaces, which spec §2 excludes.
        // Empty makes the caller degrade to workspace_cwd — the documented fallback.
        let json = pane_list(&[("wB:p1", "/w/b", "wB", false)]);
        assert!(pane_cwds(&json, None).is_empty());
    }

    #[test]
    fn relative_and_empty_pane_cwds_are_dropped() {
        let json = pane_list(&[
            ("wA:p1", "", "wA", true),
            ("wA:p2", "relative/dir", "wA", false),
            ("wA:p3", "/w/ok", "wA", false),
        ]);
        assert_eq!(pane_cwds(&json, Some("wA")), vec![PathBuf::from("/w/ok")]);
    }

    #[test]
    fn unparseable_pane_list_output_yields_no_cwds_rather_than_an_error() {
        assert!(pane_cwds("", Some("wA")).is_empty());
        assert!(pane_cwds("not json", Some("wA")).is_empty());
        assert!(pane_cwds(r#"{"result":{}}"#, Some("wA")).is_empty());
    }

    // ---- panes_in_workspace ------------------------------------------------------------------

    #[test]
    fn panes_in_workspace_scopes_the_query_server_side_when_the_id_is_flag_safe() {
        let recorder = Recorder {
            stdout: pane_list(&[("wA:p1", "/w/a", "wA", true)]),
            ..Recorder::default()
        };
        let cli = LiveHerdr::with_runner("herdr", recorder);
        assert_eq!(
            panes_in_workspace(&cli, Some("wA")),
            vec![PathBuf::from("/w/a")]
        );
        // NOTE: `--json` is NOT passed — herdr 0.9.0 rejects it and already emits JSON.
        assert_eq!(
            cli.runner().calls.borrow()[0],
            vec!["herdr", "pane", "list", "--workspace", "wA"]
        );
    }

    #[test]
    fn a_workspace_id_that_could_option_inject_is_never_passed_as_a_flag_value() {
        let recorder = Recorder {
            stdout: pane_list(&[("wA:p1", "/w/a", "wA", true)]),
            ..Recorder::default()
        };
        let cli = LiveHerdr::with_runner("herdr", recorder);
        let _ = panes_in_workspace(&cli, Some("--exec=rm"));
        assert_eq!(
            cli.runner().calls.borrow()[0],
            vec!["herdr", "pane", "list"]
        );
    }

    #[test]
    fn a_failing_herdr_cli_yields_no_cwds_so_the_caller_degrades() {
        let cli = LiveHerdr::with_runner(
            "herdr",
            Recorder {
                fail: true,
                ..Recorder::default()
            },
        );
        assert!(panes_in_workspace(&cli, Some("wA")).is_empty());
    }

    // ---- is_flag_safe --------------------------------------------------------------------------

    #[test]
    fn flag_safety_rejects_leading_dashes_and_whitespace() {
        assert!(is_flag_safe("wA"));
        assert!(is_flag_safe("wA:p1"));
        assert!(!is_flag_safe(""));
        assert!(!is_flag_safe("-x"));
        assert!(!is_flag_safe("--workspace"));
        assert!(!is_flag_safe("a b"));
        assert!(!is_flag_safe("a\nb"));
    }

    // ---- zoom_current ---------------------------------------------------------------------------

    #[test]
    fn zoom_current_toggles_this_pane_and_ignores_failure() {
        let cli = LiveHerdr::with_runner(
            "herdr",
            Recorder {
                fail: true,
                ..Recorder::default()
            },
        );
        zoom_current(&cli); // must not panic despite the non-zero exit
        assert_eq!(
            cli.runner().calls.borrow()[0],
            vec!["herdr", "pane", "zoom", "--current", "--toggle"]
        );
    }
}

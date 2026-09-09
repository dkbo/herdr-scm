//! The run loop and the wiring to the outside world: terminal setup, the clipboard, the editor
//! hand-off, and assembling every seam into a running panel (spec §6, §7).

use crate::config::Settings;
use crate::controller::{Clipboard, Controller, Deps, EditorHandoff, Effects};
use crate::discover::{self, GitignoreOracle, RealFs, ScanConfig};
use crate::git::{LiveGit, RealTopLevel};
use crate::herdr::{self, LiveHerdr};
use crate::input::{self, Bindings};
use crate::model::RepoRoot;
use crate::poller::{JobSink, Poller, RenderWorker, RepoSource};
use crate::render::{Caps, DeltaRenderer, DiffRenderer, NoRenderer};
use crate::{host, presenter};
use crossterm::event::{self, Event};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::time::Duration;

/// How long the input poll waits before looping, so background messages are drained promptly
/// even when the user is not typing.
const TICK: Duration = Duration::from_millis(100);
/// The external renderer's per-diff budget.
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// Which of the binary's four behaviors a given argv selects. Kept separate from [`run`] so
/// this routing decision — unlike any of its destinations' actual work — is unit-testable
/// without a terminal, a child process, or stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dispatch {
    Version,
    LaunchDecision,
    LaunchDecisionTab,
    Tui,
}

fn classify(args: &[String]) -> Dispatch {
    match args.get(1).map(String::as_str) {
        Some("--version") => Dispatch::Version,
        Some("--launch-decision") => Dispatch::LaunchDecision,
        Some("--launch-decision-tab") => Dispatch::LaunchDecisionTab,
        _ => Dispatch::Tui,
    }
}

/// The binary's entry point.
pub fn run(args: Vec<String>) -> ExitCode {
    match classify(&args) {
        Dispatch::Version => {
            println!("herdr-scm {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Dispatch::LaunchDecision => decide(crate::launch::launch_decision),
        Dispatch::LaunchDecisionTab => decide(crate::launch::launch_decision_tab),
        Dispatch::Tui => match run_tui() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("herdr-scm: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Read a `pane list` payload on stdin and print the launcher's decision. Any failure prints
/// `OPEN`, the safe default.
fn decide(f: fn(&str) -> String) -> ExitCode {
    let mut input = String::new();
    let decision = match io::stdin().read_to_string(&mut input) {
        Ok(_) => f(&input),
        Err(_) => "OPEN".to_string(),
    };
    println!("{decision}");
    ExitCode::SUCCESS
}

/// Build every component and run the loop.
fn run_tui() -> io::Result<()> {
    let context = host::from_env();
    let settings = crate::config::load(&|k| std::env::var(k).ok());
    let bindings = input::resolve_bindings(&settings.keys);

    let herdr_cli = LiveHerdr::from_env();
    let pane_cwds = herdr::panes_in_workspace(&herdr_cli, context.workspace_id.as_deref());
    let fallback = context
        .workspace_cwd
        .clone()
        .unwrap_or_else(|| context.cwd.clone());
    let roots = discover::scan_roots(&pane_cwds, Some(&fallback), &RealTopLevel);

    let git = Arc::new(LiveGit::default());
    let renderer = build_renderer(&settings);
    let worker = RenderWorker::spawn(git.clone(), renderer, Caps::default());
    let poller = Poller::spawn(
        git,
        Box::new(FsRepoSource {
            roots: roots.clone(),
            config: ScanConfig {
                depth: settings.scan_depth,
                excludes: settings.scan_excludes.clone(),
            },
        }),
        Duration::from_secs(settings.poll_interval_secs),
        settings.rescan_every,
    );

    let mut controller = Controller::new(
        settings,
        Deps {
            sink: Box::new(WorkerSink(worker)),
            clipboard: Box::new(Osc52Clipboard),
            herdr: Box::new(LiveHerdr::from_env()),
            editor: Box::new(ProcessEditor),
        },
    );
    controller.set_scan_roots(roots);

    let mut terminal = enter_terminal()?;
    let result = event_loop(&mut terminal, &mut controller, &bindings, &poller);
    leave_terminal(&mut terminal)?;
    result
}

/// The event loop: draw, poll input, route, drain (spec §6).
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    controller: &mut Controller,
    bindings: &Bindings,
    poller: &Poller,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            presenter::draw(f, controller, bindings);
        })?;

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && let Some(intent) = input::decode(key, bindings)
        {
            let Effects {
                quit,
                refresh,
                editor,
                ..
            } = controller.handle(intent);
            if quit {
                return Ok(());
            }
            if refresh {
                poller.refresh_now();
            }
            if let Some(path) = editor {
                // The editor takes the terminal over, so leave and re-enter around it.
                leave_terminal(terminal)?;
                controller.editor_finished(&path);
                *terminal = enter_terminal()?;
                terminal.clear()?;
            }
        }

        for msg in poller.drain() {
            controller.apply(msg);
        }
        let results = controller.drain_diffs();
        for result in results {
            controller.apply_diff(result);
        }
    }
}

/// The repo source used in production: re-walk the scan starts.
struct FsRepoSource {
    roots: Vec<PathBuf>,
    config: ScanConfig,
}

impl RepoSource for FsRepoSource {
    fn rescan(&self) -> Vec<RepoRoot> {
        discover::scan(&self.roots, &self.config, &RealFs, &GitignoreOracle::new())
    }
}

/// Adapts the render worker to the controller's job sink, and lets the loop drain its results.
struct WorkerSink(RenderWorker);

impl JobSink for WorkerSink {
    fn submit(&self, job: crate::poller::DiffJob) {
        self.0.submit(job);
    }
    fn drain(&self) -> Vec<crate::poller::DiffResult> {
        self.0.drain()
    }
}

/// Choose the external renderer from the config: a program name, or none at all.
fn build_renderer(settings: &Settings) -> Arc<dyn DiffRenderer> {
    if settings.diff_tool.trim().is_empty() {
        Arc::new(NoRenderer)
    } else {
        Arc::new(DeltaRenderer::new(
            settings.diff_tool.clone(),
            RENDER_TIMEOUT,
        ))
    }
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    // From here on, any failure must undo the raw mode just enabled: its termios settings are
    // a property of the TTY, not the process, so propagating without cleanup would leave the
    // user's shell echo-less and line-editing-less after we exit.
    let mut stdout = io::stdout();
    if let Err(e) = crossterm::execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    Terminal::new(CrosstermBackend::new(stdout)).inspect_err(|_| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
    })
}

fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// The live clipboard: OSC 52, the dependency-free way for a TUI in a pane to set the HOST
/// terminal's clipboard. It travels through multiplexers and SSH, unlike a display-bound API,
/// and produces no visible output, so writing it mid-loop never disturbs the screen.
pub struct Osc52Clipboard;

impl Clipboard for Osc52Clipboard {
    fn copy(&mut self, text: &str) -> io::Result<()> {
        let mut out = io::stdout();
        out.write_all(osc52_sequence(text).as_bytes())?;
        out.flush()
    }
}

/// `ESC ] 52 ; c ; <base64> BEL`. The payload is base64 precisely so an attacker-chosen file
/// name cannot carry an ESC or BEL that terminates the sequence early.
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()))
}

/// Standard base64 (RFC 4648) — a few lines, so OSC 52 needs no extra dependency.
pub fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Editor hand-off
// ---------------------------------------------------------------------------

/// Hand the terminal to `$EDITOR` and wait for it (spec §10: a pure hand-off — this program
/// neither reads nor writes the file).
pub struct ProcessEditor;

impl EditorHandoff for ProcessEditor {
    fn open(&mut self, path: &Path) -> Result<(), String> {
        let argv = editor_argv(std::env::var("EDITOR").ok(), path);
        let (program, args) = argv.split_first().ok_or("no editor configured")?;
        Command::new(program)
            .args(args)
            .status()
            .map(|_| ())
            .map_err(|e| format!("could not open editor: {e}"))
    }
}

/// Split `$EDITOR` into program plus arguments and append the path.
///
/// `vi` is the fallback: POSIX guarantees it, and a silent dead `e` key would be worse.
pub fn editor_argv(editor: Option<String>, path: &Path) -> Vec<OsString> {
    let spec = editor.unwrap_or_default();
    let mut argv: Vec<OsString> = spec.split_whitespace().map(OsString::from).collect();
    if argv.is_empty() {
        argv.push(OsString::from("vi"));
    }
    argv.push(path.as_os_str().to_os_string());
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_including_padding() {
        // Vectors from RFC 4648 §10; the partial-group cases are the easy ones to get wrong.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"a:src/app.rs"), "YTpzcmMvYXBwLnJz");
    }

    #[test]
    fn the_clipboard_sequence_is_osc_52_with_a_base64_payload() {
        assert_eq!(osc52_sequence("foo"), "\x1b]52;c;Zm9v\x07");
    }

    #[test]
    fn a_hostile_payload_cannot_break_out_of_the_clipboard_sequence() {
        // base64 confines every byte to [A-Za-z0-9+/=], so an ESC or BEL inside a file name
        // cannot terminate the sequence early and start driving the terminal.
        let seq = osc52_sequence("evil\x1b]52;c;x\x07name");
        assert_eq!(seq.matches('\x1b').count(), 1, "{seq:?}");
        assert_eq!(seq.matches('\x07').count(), 1, "{seq:?}");
    }

    #[test]
    fn the_editor_argv_uses_the_configured_editor() {
        assert_eq!(
            editor_argv(Some("nvim".to_string()), Path::new("/w/a.rs")),
            [OsString::from("nvim"), OsString::from("/w/a.rs")]
        );
    }

    #[test]
    fn an_editor_with_arguments_is_split_into_program_and_arguments() {
        assert_eq!(
            editor_argv(Some("code --wait".to_string()), Path::new("/w/a.rs")),
            [
                OsString::from("code"),
                OsString::from("--wait"),
                OsString::from("/w/a.rs")
            ]
        );
    }

    #[test]
    fn with_no_editor_configured_vi_is_the_fallback() {
        // POSIX guarantees `vi`; falling back to nothing would make `e` a silent dead key.
        assert_eq!(
            editor_argv(None, Path::new("/w/a.rs"))[0],
            OsString::from("vi")
        );
    }

    #[test]
    fn an_empty_or_whitespace_editor_setting_falls_back_rather_than_spawning_nothing() {
        assert_eq!(
            editor_argv(Some("   ".to_string()), Path::new("/w/a.rs"))[0],
            OsString::from("vi")
        );
    }

    #[test]
    fn the_version_flag_prints_the_crate_version_and_succeeds() {
        assert_eq!(
            run(vec!["herdr-scm".to_string(), "--version".to_string()]),
            std::process::ExitCode::SUCCESS
        );
    }

    // The two tests below moved from `lib.rs`'s Task 1 stub, which this task replaces: that
    // module had only a version flag and an error stub for everything else. Now that the TUI
    // is real, "everything else" dispatches to it instead of erroring — the run loop itself
    // needs a real terminal and background threads, so it is exercised by the manual smoke
    // run (task report), not here. This asserts the *dispatch decision* changed accordingly,
    // without spinning up a terminal or blocking on stdin.

    #[test]
    fn the_version_flag_is_recognized() {
        assert_eq!(
            classify(&["herdr-scm".to_string(), "--version".to_string()]),
            Dispatch::Version
        );
    }

    #[test]
    fn an_unrecognized_invocation_now_dispatches_to_the_tui_instead_of_erroring() {
        assert_eq!(classify(&["herdr-scm".to_string()]), Dispatch::Tui);
    }
}

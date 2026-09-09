//! herdr-scm — a read-only, multi-repo source-control overview panel for herdr.
//!
//! Read-only by construction: no module writes files or mutates git state. `git` is
//! shelled out from `git.rs` alone, and only with read-only subcommands.

pub mod config;
pub mod context;
pub mod discover;
pub mod git;
pub mod herdr;
pub mod host;
pub mod input;
pub mod intent;
pub mod layout;
pub mod model;
pub mod porcelain;
pub mod proc;
pub mod render;
pub mod repo_kind;
pub mod tree;

use std::process::ExitCode;

/// The testable core: everything the binary does, with failure as a message rather than an
/// exit code. `ExitCode` implements no `PartialEq`, so the logic is asserted here and the
/// exit-code mapping stays a one-liner at the boundary.
pub fn run_result(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("herdr-scm {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("the TUI is not implemented yet".to_string()),
    }
}

/// The library entry point the thin binary calls. `args` is the full argv including argv[0].
pub fn run(args: Vec<String>) -> ExitCode {
    match run_result(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("herdr-scm: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_flag_is_recognized() {
        assert!(run_result(&["herdr-scm".to_string(), "--version".to_string()]).is_ok());
    }

    #[test]
    fn an_unrecognized_invocation_reports_that_the_tui_is_not_implemented_yet() {
        assert!(run_result(&["herdr-scm".to_string()]).is_err());
    }
}

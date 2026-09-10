//! herdr-scm — a read-only, multi-repo source-control overview panel for herdr.
//!
//! Read-only by construction: no module writes files or mutates git state. `git` is
//! shelled out from `git.rs` alone, and only with read-only subcommands.

pub mod app;
pub mod config;
pub mod context;
pub mod controller;
pub mod discover;
pub mod git;
pub mod herdr;
pub mod host;
pub mod input;
pub mod intent;
pub mod launch;
pub mod layout;
pub mod model;
pub mod poller;
pub mod porcelain;
pub mod presenter;
pub mod proc;
pub mod render;
pub mod repo_kind;
pub mod theme;
pub mod tree;

/// The library entry point the thin binary calls.
pub fn run(args: Vec<String>) -> std::process::ExitCode {
    app::run(args)
}

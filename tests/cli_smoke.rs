//! The binary's argv surface, exercised as a real process.

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_herdr-scm")
}

#[test]
fn the_version_flag_prints_the_crate_version() {
    let out = Command::new(bin()).arg("--version").output().expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}

/// Feed `stdin` to the binary with `flag` and return its trimmed stdout.
fn decision(flag: &str, stdin: &str) -> String {
    let mut child = Command::new(bin())
        .arg(flag)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn the_launch_decision_reads_the_pane_list_from_stdin() {
    let json = r#"{"result":{"panes":[
        {"pane_id":"wA:p1","tab_id":"wA:t1","focused":true},
        {"pane_id":"wA:p2","tab_id":"wA:t1","focused":false,"label":"SCM"}
    ]}}"#;
    assert_eq!(decision("--launch-decision", json), "FOCUS wA:p2");
}

#[test]
fn the_tab_launch_decision_switches_to_a_panel_in_another_tab() {
    let json = r#"{"result":{"panes":[
        {"pane_id":"wA:p1","tab_id":"wA:t1","focused":true},
        {"pane_id":"wA:p9","tab_id":"wA:t2","focused":false,"label":"SCM"}
    ]}}"#;
    assert_eq!(decision("--launch-decision-tab", json), "SWITCHTAB wA:t2");
}

#[test]
fn garbage_on_stdin_degrades_to_open_rather_than_failing_the_launcher() {
    assert_eq!(decision("--launch-decision", "not json"), "OPEN");
    assert_eq!(decision("--launch-decision-tab", ""), "OPEN");
}

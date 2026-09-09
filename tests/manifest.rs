//! The manifest is the contract with herdr; these pin the values other parts of the code and
//! the launcher scripts depend on.

use std::path::Path;

fn manifest() -> toml::Value {
    let text = std::fs::read_to_string("herdr-plugin.toml").expect("read herdr-plugin.toml");
    // NOTE (ruling R9): `str::parse::<toml::Value>()` fails on this manifest against the
    // declared `toml = { version = "1.1", default-features = false, features = ["parse",
    // "serde"] }` dependency, with "unexpected content, expected nothing" — even though the
    // manifest is valid TOML. `toml::from_str` works with the same dependency features.
    toml::from_str::<toml::Value>(&text).expect("parse herdr-plugin.toml")
}

fn array<'a>(manifest: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    manifest[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
}

#[test]
fn the_manifest_declares_the_ids_the_spec_fixes() {
    let m = manifest();
    assert_eq!(m["id"].as_str(), Some("herdr-scm"));
    assert_eq!(m["min_herdr_version"].as_str(), Some("0.9.0"));
    assert_eq!(
        m["platforms"].as_array().map(Vec::len),
        Some(1),
        "v1 is Linux-only"
    );
    assert_eq!(m["platforms"][0].as_str(), Some("linux"));
}

#[test]
fn the_manifest_version_matches_the_crate_version() {
    // A drifting version makes the installed plugin report a lie.
    assert_eq!(
        manifest()["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn the_pane_entry_matches_what_the_launcher_scripts_ask_for() {
    let m = manifest();
    let pane = &array(&m, "panes")[0];
    assert_eq!(pane["id"].as_str(), Some("scm"));
    assert_eq!(pane["placement"].as_str(), Some("split"));
    assert_eq!(
        pane["command"][0].as_str(),
        Some("./target/release/herdr-scm")
    );
}

#[test]
fn the_pane_title_is_the_label_the_launch_decision_matches_on() {
    // herdr reports the pane's `title` as its `label` in `pane list`; if these drift, the
    // launcher stops recognizing its own panel and opens a second one every keypress.
    assert_eq!(
        manifest()["panes"][0]["title"].as_str(),
        Some(herdr_scm::launch::PANE_LABEL)
    );
}

#[test]
fn both_actions_exist_with_the_ids_the_readme_tells_users_to_bind() {
    let m = manifest();
    let ids: Vec<&str> = array(&m, "actions")
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert_eq!(ids, ["open-scm", "open-scm-tab"]);
}

#[test]
fn every_command_the_manifest_names_exists_on_disk_and_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let m = manifest();
    let mut scripts: Vec<String> = Vec::new();
    for key in ["build", "actions"] {
        for entry in array(&m, key) {
            if let Some(command) = entry["command"].as_array() {
                for arg in command {
                    if let Some(s) = arg.as_str()
                        && s.starts_with("scripts/")
                    {
                        scripts.push(s.to_string());
                    }
                }
            }
        }
    }
    assert!(!scripts.is_empty(), "no scripts referenced");
    for script in scripts {
        let path = Path::new(&script);
        assert!(path.exists(), "{script} is referenced but missing");
        let mode = std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "{script} is not executable (mode {mode:o})"
        );
    }
}

#[test]
fn the_launcher_scripts_never_pass_the_json_flag_that_herdr_rejects() {
    // `herdr pane list --json` fails with "unknown option" on 0.9.0; the flag would silently
    // reduce every launch to OPEN, duplicating the panel on every keypress.
    for script in ["scripts/open-scm.sh", "scripts/open-scm-tab.sh"] {
        let text = std::fs::read_to_string(script).expect(script);
        assert!(
            !text.contains("pane list --json"),
            "{script} passes --json to pane list"
        );
        assert!(text.contains("pane list"), "{script} never lists panes");
    }
}

#[test]
fn the_example_config_documents_every_setting_the_loader_understands() {
    let text = std::fs::read_to_string("config.example.toml").expect("config.example.toml");
    for key in [
        "poll_interval_secs",
        "rescan_every",
        "scan_depth",
        "scan_excludes",
        "diff_tool",
        "split_threshold_cols",
        "[keys]",
    ] {
        assert!(text.contains(key), "{key} is undocumented");
    }
}

#[test]
fn the_example_config_still_parses_to_the_built_in_defaults() {
    // Every value in the example is written as the default; if one drifts, users copying the
    // file silently get different behavior from a fresh install.
    let text = std::fs::read_to_string("config.example.toml").expect("config.example.toml");
    let settings = herdr_scm::config::resolve(herdr_scm::config::parse(&text), &|_| None);
    let defaults = herdr_scm::config::Settings::default();
    assert_eq!(settings.poll_interval_secs, defaults.poll_interval_secs);
    assert_eq!(settings.rescan_every, defaults.rescan_every);
    assert_eq!(settings.scan_depth, defaults.scan_depth);
    assert_eq!(settings.scan_excludes, defaults.scan_excludes);
    assert_eq!(settings.diff_tool, defaults.diff_tool);
    assert_eq!(settings.split_threshold_cols, defaults.split_threshold_cols);
}

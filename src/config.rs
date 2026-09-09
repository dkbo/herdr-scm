//! Config Loader — read-only TOML, degrading the WHOLE file to defaults on any parse error
//! (spec §8). Never writes the file back, never panics.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The plugin id, used to namespace the XDG fallback config directory.
const PLUGIN_ID: &str = "herdr-scm";

pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 3;
/// A ceiling so a typo cannot turn the panel into a manual-only tool by accident. One hour.
pub const MAX_POLL_INTERVAL_SECS: u64 = 3600;
pub const DEFAULT_RESCAN_EVERY: u32 = 10;
pub const DEFAULT_SCAN_DEPTH: usize = 4;
/// Depth 1 still finds a repo directly under a scan start, so it is the meaningful floor.
pub const MIN_SCAN_DEPTH: usize = 1;
/// Past this the walk cost stops being bounded in any useful sense on a real tree.
pub const MAX_SCAN_DEPTH: usize = 12;
pub const DEFAULT_SCAN_EXCLUDES: &[&str] = &["node_modules", "target", "vendor", ".venv", "dist"];
pub const DEFAULT_DIFF_TOOL: &str = "delta";
pub const DEFAULT_SPLIT_THRESHOLD_COLS: u16 = 120;
/// Below this no two-column layout is readable, so the threshold itself has a floor.
pub const MIN_SPLIT_THRESHOLD_COLS: u16 = 40;
/// Above this the threshold would never be met on a real terminal — i.e. "always stacked".
pub const MAX_SPLIT_THRESHOLD_COLS: u16 = 400;

/// The env var that lets a user switch or disable the external diff renderer without touching
/// the config file. The only env var in the `config > env > default` chain (spec §8).
const ENV_DIFF_TOOL: &str = "HERDR_SCM_DIFF_TOOL";

/// A `[keys]` value: one key (`refresh = "r"`) or several (`nav_up = ["w", "Up"]`).
///
/// `#[serde(untagged)]` tries variants in declaration order, so `One` MUST stay first.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum KeySpec {
    One(String),
    Many(Vec<String>),
}

/// The file's shape. Every field optional, unknown keys ignored, so a partial or
/// forward-looking config still loads what it can.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RawConfig {
    pub poll_interval_secs: Option<u64>,
    pub rescan_every: Option<u32>,
    pub scan_depth: Option<usize>,
    pub scan_excludes: Option<Vec<String>>,
    pub diff_tool: Option<String>,
    pub split_threshold_cols: Option<u16>,
    #[serde(default)]
    pub keys: BTreeMap<String, KeySpec>,
}

/// The resolved, clamped settings the rest of the app reads. No `Option`s: every question has
/// an answer by the time anything else sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Seconds between snapshot rounds. `0` means polling is OFF (manual `r` only) — a
    /// meaning, not an out-of-range value, so it is never clamped up.
    pub poll_interval_secs: u64,
    /// Re-walk the filesystem for the repo list every N poll rounds. Never 0.
    pub rescan_every: u32,
    pub scan_depth: usize,
    pub scan_excludes: Vec<String>,
    /// The external diff renderer's program name; empty means plain text.
    pub diff_tool: String,
    pub split_threshold_cols: u16,
    pub keys: BTreeMap<String, KeySpec>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            rescan_every: DEFAULT_RESCAN_EVERY,
            scan_depth: DEFAULT_SCAN_DEPTH,
            scan_excludes: DEFAULT_SCAN_EXCLUDES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            diff_tool: DEFAULT_DIFF_TOOL.to_string(),
            split_threshold_cols: DEFAULT_SPLIT_THRESHOLD_COLS,
            keys: BTreeMap::new(),
        }
    }
}

/// Parse config text. ANY error — syntax or wrong type — degrades the whole document to
/// `RawConfig::default()` (spec §8).
pub fn parse(text: &str) -> RawConfig {
    toml::from_str(text).unwrap_or_default()
}

/// Apply the `config > env > default` precedence and clamp everything into range.
pub fn resolve(raw: RawConfig, env: &dyn Fn(&str) -> Option<String>) -> Settings {
    let d = Settings::default();
    Settings {
        poll_interval_secs: raw
            .poll_interval_secs
            .map(|v| v.min(MAX_POLL_INTERVAL_SECS))
            .unwrap_or(d.poll_interval_secs),
        // Clamped up to 1: this is a modulo divisor on the poll counter.
        rescan_every: raw.rescan_every.map(|v| v.max(1)).unwrap_or(d.rescan_every),
        scan_depth: raw
            .scan_depth
            .map(|v| v.clamp(MIN_SCAN_DEPTH, MAX_SCAN_DEPTH))
            .unwrap_or(d.scan_depth),
        // An explicitly empty list means "no hard exclusions" and is honored; only an ABSENT
        // key falls back to the defaults.
        scan_excludes: raw
            .scan_excludes
            .map(|list| {
                list.into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or(d.scan_excludes),
        // config > env > default. An empty string at either level is a real value ("plain
        // text"), so `is_some()` — not "is non-empty" — is what decides.
        diff_tool: raw
            .diff_tool
            .or_else(|| env(ENV_DIFF_TOOL))
            .unwrap_or(d.diff_tool),
        split_threshold_cols: raw
            .split_threshold_cols
            .map(|v| v.clamp(MIN_SPLIT_THRESHOLD_COLS, MAX_SPLIT_THRESHOLD_COLS))
            .unwrap_or(d.split_threshold_cols),
        keys: raw.keys,
    }
}

/// Where the config file lives: `$HERDR_PLUGIN_CONFIG_DIR/config.toml` outright, else
/// `$XDG_CONFIG_HOME/herdr-scm/config.toml`, else `$HOME/.config/herdr-scm/config.toml`.
///
/// `None` when no base resolves, or when the resolved base is relative. The pane process starts
/// in the PLUGIN root, so a cwd-relative fallback would read a file the user never wrote —
/// reading nothing is the correct answer.
pub fn config_path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let non_empty = |k: &str| env(k).filter(|s| !s.is_empty());
    let path = if let Some(dir) = non_empty("HERDR_PLUGIN_CONFIG_DIR") {
        PathBuf::from(dir).join("config.toml")
    } else if let Some(xdg) = non_empty("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join(PLUGIN_ID).join("config.toml")
    } else {
        PathBuf::from(non_empty("HOME")?)
            .join(".config")
            .join(PLUGIN_ID)
            .join("config.toml")
    };
    path.is_absolute().then_some(path)
}

/// Read and resolve the config. An unreadable or absent file is not an error — it yields the
/// defaults, exactly like an empty file.
pub fn load(env: &dyn Fn(&str) -> Option<String>) -> Settings {
    let text = config_path(env)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    resolve(parse(&text), env)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An env lookup that knows nothing — the "no environment at all" baseline.
    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn resolve_text(text: &str) -> Settings {
        resolve(parse(text), &no_env)
    }

    // ---- defaults ------------------------------------------------------------------------

    #[test]
    fn an_empty_config_yields_every_documented_default() {
        let s = resolve_text("");
        assert_eq!(s.poll_interval_secs, 3);
        assert_eq!(s.rescan_every, 10);
        assert_eq!(s.scan_depth, 4);
        assert_eq!(
            s.scan_excludes,
            vec!["node_modules", "target", "vendor", ".venv", "dist"]
        );
        assert_eq!(s.diff_tool, "delta");
        assert_eq!(s.split_threshold_cols, 120);
        assert!(s.keys.is_empty());
    }

    #[test]
    fn the_spec_example_config_parses_to_its_stated_values() {
        let s = resolve_text(
            r#"
poll_interval_secs   = 3
rescan_every         = 10
scan_depth           = 4
scan_excludes        = ["node_modules", "target", "vendor", ".venv", "dist"]
diff_tool            = "delta"
split_threshold_cols = 120

[keys]
refresh = "r"
"#,
        );
        assert_eq!(s.poll_interval_secs, 3);
        assert_eq!(s.keys.get("refresh"), Some(&KeySpec::One("r".to_string())));
    }

    // ---- degradation ---------------------------------------------------------------------

    #[test]
    fn malformed_toml_degrades_the_whole_file_to_defaults_without_panicking() {
        // spec §8: "解析失敗整份退化成預設值，不 panic" — the WHOLE file, not field-by-field.
        let s = resolve_text("poll_interval_secs = = = 3\n[unclosed");
        assert_eq!(s.poll_interval_secs, 3);
        assert_eq!(s.diff_tool, "delta");
    }

    #[test]
    fn a_wrong_typed_value_degrades_the_whole_file_to_defaults() {
        // A String where a u64 belongs makes serde fail the whole document — which is exactly
        // the spec's "整份退化" rule, so this is asserted rather than worked around.
        let s = resolve_text("poll_interval_secs = \"soon\"\ndiff_tool = \"bat\"\n");
        assert_eq!(s.poll_interval_secs, 3);
        assert_eq!(s.diff_tool, "delta");
    }

    #[test]
    fn unknown_keys_are_ignored_so_a_forward_looking_config_still_loads() {
        let s = resolve_text("diff_tool = \"bat\"\nfuture_feature = true\n");
        assert_eq!(s.diff_tool, "bat");
    }

    // ---- clamping ------------------------------------------------------------------------

    #[test]
    fn poll_interval_zero_is_kept_because_it_means_polling_off() {
        // spec §8: "0 = 關閉輪詢，退化成純手動 r". Zero is a MEANING, not an out-of-range value.
        assert_eq!(resolve_text("poll_interval_secs = 0").poll_interval_secs, 0);
    }

    #[test]
    fn an_absurd_poll_interval_clamps_down() {
        assert_eq!(
            resolve_text("poll_interval_secs = 100000").poll_interval_secs,
            MAX_POLL_INTERVAL_SECS
        );
    }

    #[test]
    fn rescan_every_zero_clamps_up_to_one_so_the_modulo_is_never_by_zero() {
        assert_eq!(resolve_text("rescan_every = 0").rescan_every, 1);
    }

    #[test]
    fn scan_depth_clamps_into_range() {
        assert_eq!(resolve_text("scan_depth = 0").scan_depth, MIN_SCAN_DEPTH);
        assert_eq!(resolve_text("scan_depth = 9999").scan_depth, MAX_SCAN_DEPTH);
    }

    #[test]
    fn split_threshold_clamps_into_range() {
        assert_eq!(
            resolve_text("split_threshold_cols = 3").split_threshold_cols,
            MIN_SPLIT_THRESHOLD_COLS
        );
        assert_eq!(
            resolve_text("split_threshold_cols = 5000").split_threshold_cols,
            MAX_SPLIT_THRESHOLD_COLS
        );
    }

    #[test]
    fn an_explicitly_empty_excludes_list_is_honored_not_replaced_by_the_defaults() {
        // "I want no hard exclusions" must be expressible; only an ABSENT key takes the default.
        assert!(resolve_text("scan_excludes = []").scan_excludes.is_empty());
    }

    #[test]
    fn blank_exclude_entries_are_dropped() {
        assert_eq!(
            resolve_text("scan_excludes = [\"target\", \"\", \"  \"]").scan_excludes,
            vec!["target"]
        );
    }

    #[test]
    fn an_empty_diff_tool_means_plain_text_and_is_preserved() {
        // spec §8: `diff_tool = ""` → plain text. Empty must NOT fall back to "delta".
        assert_eq!(resolve_text("diff_tool = \"\"").diff_tool, "");
    }

    // ---- precedence: config > env > default ------------------------------------------------

    #[test]
    fn the_env_diff_tool_beats_the_default_but_loses_to_the_config_file() {
        let env = |k: &str| (k == "HERDR_SCM_DIFF_TOOL").then(|| "bat".to_string());
        assert_eq!(resolve(parse(""), &env).diff_tool, "bat");
        assert_eq!(
            resolve(parse("diff_tool = \"riff\""), &env).diff_tool,
            "riff"
        );
    }

    #[test]
    fn an_env_diff_tool_set_to_empty_still_beats_the_default() {
        // Explicitly turning the renderer off from the environment must work.
        let env = |k: &str| (k == "HERDR_SCM_DIFF_TOOL").then(String::new);
        assert_eq!(resolve(parse(""), &env).diff_tool, "");
    }

    // ---- config_path ------------------------------------------------------------------------

    #[test]
    fn the_plugin_config_dir_wins_outright() {
        let env = |k: &str| (k == "HERDR_PLUGIN_CONFIG_DIR").then(|| "/x/cfg".to_string());
        assert_eq!(config_path(&env), Some(PathBuf::from("/x/cfg/config.toml")));
    }

    #[test]
    fn xdg_config_home_is_the_first_fallback_and_is_namespaced_by_the_plugin_id() {
        let env = |k: &str| (k == "XDG_CONFIG_HOME").then(|| "/x/xdg".to_string());
        assert_eq!(
            config_path(&env),
            Some(PathBuf::from("/x/xdg/herdr-scm/config.toml"))
        );
    }

    #[test]
    fn home_dot_config_is_the_second_fallback() {
        let env = |k: &str| (k == "HOME").then(|| "/home/u".to_string());
        assert_eq!(
            config_path(&env),
            Some(PathBuf::from("/home/u/.config/herdr-scm/config.toml"))
        );
    }

    #[test]
    fn an_empty_env_value_falls_through_to_the_next_candidate() {
        let env = |k: &str| match k {
            "HERDR_PLUGIN_CONFIG_DIR" => Some(String::new()),
            "HOME" => Some("/home/u".to_string()),
            _ => None,
        };
        assert_eq!(
            config_path(&env),
            Some(PathBuf::from("/home/u/.config/herdr-scm/config.toml"))
        );
    }

    #[test]
    fn with_no_resolvable_base_there_is_no_config_path_at_all() {
        // Never fall back to a cwd-relative path: the pane process starts in the PLUGIN root,
        // so a relative "config.toml" would read a file the user never wrote.
        assert_eq!(config_path(&no_env), None);
    }

    #[test]
    fn a_relative_config_dir_is_rejected() {
        let env = |k: &str| (k == "HERDR_PLUGIN_CONFIG_DIR").then(|| "cfg".to_string());
        assert_eq!(config_path(&env), None);
    }

    // ---- load ------------------------------------------------------------------------------

    #[test]
    fn load_with_no_readable_config_file_yields_the_defaults() {
        let env = |k: &str| {
            (k == "HERDR_PLUGIN_CONFIG_DIR").then(|| "/nonexistent-herdr-scm-dir".to_string())
        };
        assert_eq!(load(&env).poll_interval_secs, 3);
    }

    // ---- KeySpec shape ----------------------------------------------------------------------

    #[test]
    fn a_keys_entry_accepts_both_a_bare_string_and_an_array() {
        let s = resolve_text("[keys]\nrefresh = \"g\"\nnav_up = [\"w\", \"Up\"]\n");
        assert_eq!(s.keys.get("refresh"), Some(&KeySpec::One("g".to_string())));
        assert_eq!(
            s.keys.get("nav_up"),
            Some(&KeySpec::Many(vec!["w".to_string(), "Up".to_string()]))
        );
    }
}

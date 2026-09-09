//! Host Adapter — parse `HERDR_PLUGIN_CONTEXT_JSON` (spec §3.1, §6).
//!
//! Defensive by contract: missing, malformed, or wrong-typed input degrades to a minimal
//! `{ cwd }` context. Never panics.

use crate::context::LaunchContext;
use serde::Deserialize;
use std::path::PathBuf;

/// The shape of the injected JSON. Every field optional so a partial object still parses;
/// unknown fields are ignored so a newer herdr does not break us.
#[derive(Deserialize, Default)]
struct RawContext {
    focused_pane_cwd: Option<String>,
    workspace_cwd: Option<String>,
    cwd: Option<String>,
    workspace_id: Option<String>,
}

/// Build the launch context from the process environment. Never panics.
pub fn from_env() -> LaunchContext {
    let json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok();
    let cwd = std::env::current_dir().unwrap_or_default();
    parse_context(json.as_deref(), cwd)
}

/// The pure parser behind [`from_env`], testable without touching the process environment.
pub fn parse_context(json: Option<&str>, fallback_cwd: PathBuf) -> LaunchContext {
    let raw: RawContext = json
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    // An empty string is a malformed host value, not a path: drop it so it falls through to
    // the next candidate rather than rooting the whole scan at "".
    let non_empty = |v: Option<String>| v.filter(|s| !s.is_empty());
    let workspace_cwd = non_empty(raw.workspace_cwd).map(PathBuf::from);
    let cwd = non_empty(raw.focused_pane_cwd)
        .map(PathBuf::from)
        .or_else(|| workspace_cwd.clone())
        .or_else(|| non_empty(raw.cwd).map(PathBuf::from))
        .unwrap_or(fallback_cwd);
    LaunchContext {
        cwd,
        workspace_cwd,
        workspace_id: non_empty(raw.workspace_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback() -> PathBuf {
        PathBuf::from("/proc/cwd")
    }

    #[test]
    fn a_full_context_uses_the_focused_pane_cwd_and_keeps_the_workspace_fields() {
        let json = r#"{"workspace_id":"wA","workspace_cwd":"/w","focused_pane_cwd":"/w/sub"}"#;
        let ctx = parse_context(Some(json), fallback());
        assert_eq!(ctx.cwd, PathBuf::from("/w/sub"));
        assert_eq!(ctx.workspace_cwd, Some(PathBuf::from("/w")));
        assert_eq!(ctx.workspace_id, Some("wA".to_string()));
    }

    #[test]
    fn without_a_focused_pane_cwd_the_workspace_cwd_is_used() {
        let ctx = parse_context(Some(r#"{"workspace_cwd":"/w"}"#), fallback());
        assert_eq!(ctx.cwd, PathBuf::from("/w"));
        assert_eq!(ctx.workspace_cwd, Some(PathBuf::from("/w")));
    }

    #[test]
    fn a_plain_cwd_field_is_accepted_as_the_last_json_candidate() {
        let ctx = parse_context(Some(r#"{"cwd":"/c"}"#), fallback());
        assert_eq!(ctx.cwd, PathBuf::from("/c"));
        assert_eq!(ctx.workspace_cwd, None);
    }

    #[test]
    fn absent_json_degrades_to_the_process_cwd_with_no_workspace_information() {
        let ctx = parse_context(None, fallback());
        assert_eq!(ctx.cwd, fallback());
        assert_eq!(ctx.workspace_cwd, None);
        assert_eq!(ctx.workspace_id, None);
    }

    #[test]
    fn malformed_json_degrades_rather_than_panicking() {
        for bad in ["", "{", "not json at all", "[]", "null", "42"] {
            let ctx = parse_context(Some(bad), fallback());
            assert_eq!(ctx.cwd, fallback(), "input {bad:?}");
            assert_eq!(ctx.workspace_id, None, "input {bad:?}");
        }
    }

    #[test]
    fn empty_string_fields_fall_through_instead_of_rooting_at_an_empty_path() {
        let json = r#"{"focused_pane_cwd":"","workspace_cwd":"","cwd":"","workspace_id":""}"#;
        let ctx = parse_context(Some(json), fallback());
        assert_eq!(ctx.cwd, fallback());
        assert_eq!(ctx.workspace_cwd, None);
        assert_eq!(ctx.workspace_id, None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"workspace_cwd":"/w","future_field":{"nested":true}}"#;
        assert_eq!(
            parse_context(Some(json), fallback()).cwd,
            PathBuf::from("/w")
        );
    }

    #[test]
    fn a_wrong_typed_field_degrades_the_whole_context_without_panicking() {
        let ctx = parse_context(Some(r#"{"workspace_cwd":123}"#), fallback());
        assert_eq!(ctx.cwd, fallback());
    }
}

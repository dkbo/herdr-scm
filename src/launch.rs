//! The launcher's OPEN / FOCUS / CLOSE / SWITCHTAB decision, computed in-process so it is unit
//! tested and the ids it emits are validated flag-safe before a shell script passes them to
//! `herdr pane zoom|close` / `herdr tab focus`.

use crate::herdr::is_flag_safe;
use serde::Deserialize;

/// The pane label herdr reports for our pane — it is the manifest's `title`, so the two are
/// pinned together by `tests/manifest.rs`.
pub const PANE_LABEL: &str = "SCM";

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
    pane_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    focused: bool,
}

/// The split launcher's decision, scoped to the CURRENT TAB.
///
/// Unparseable input, or no focused pane (we cannot tell which tab is current), degrades to
/// `OPEN`: spawning a fresh panel is always safe, acting on a pane in an unknown tab is not.
pub fn launch_decision(pane_list_json: &str) -> String {
    let Some((panes, focused_index)) = parse(pane_list_json) else {
        return open();
    };
    let focused = &panes[focused_index];
    let tab = focused.tab_id.as_deref();
    let Some(ours) = panes
        .iter()
        .find(|p| is_ours(p) && p.tab_id.as_deref() == tab)
    else {
        return open();
    };
    act_on(ours, focused)
}

/// The tab launcher's decision, scoped to the CURRENT WORKSPACE: a panel in another tab here is
/// switched to rather than duplicated; one in another workspace is left alone.
pub fn launch_decision_tab(pane_list_json: &str) -> String {
    let Some((panes, focused_index)) = parse(pane_list_json) else {
        return open();
    };
    let focused = &panes[focused_index];
    if let Some(here) = panes
        .iter()
        .find(|p| is_ours(p) && p.tab_id.as_deref() == focused.tab_id.as_deref())
    {
        return act_on(here, focused);
    }
    let workspace = workspace_of(focused);
    if workspace.is_some()
        && let Some(elsewhere) = panes
            .iter()
            .find(|p| is_ours(p) && workspace_of(p) == workspace)
        && let Some(tab) = elsewhere.tab_id.as_deref().filter(|t| is_flag_safe(t))
    {
        return format!("SWITCHTAB {tab}");
    }
    open()
}

/// Parse the payload and locate the focused pane's index.
fn parse(json: &str) -> Option<(Vec<Pane>, usize)> {
    let list: PaneList = serde_json::from_str(json).ok()?;
    let panes = list.result.panes;
    let index = panes.iter().position(|p| p.focused)?;
    Some((panes, index))
}

fn is_ours(pane: &Pane) -> bool {
    pane.label.as_deref() == Some(PANE_LABEL)
}

/// Toggle off when it IS the focused pane, focus it otherwise.
fn act_on(ours: &Pane, focused: &Pane) -> String {
    let Some(id) = ours.pane_id.as_deref().filter(|id| is_flag_safe(id)) else {
        return open();
    };
    if Some(id) == focused.pane_id.as_deref() {
        format!("CLOSE {id}")
    } else {
        format!("FOCUS {id}")
    }
}

/// The workspace a pane belongs to, taken from the prefix of the id we would act on rather than
/// a separate field a malformed payload could disagree with.
fn workspace_of(pane: &Pane) -> Option<&str> {
    pane.tab_id.as_deref()?.split_once(':').map(|(ws, _)| ws)
}

fn open() -> String {
    "OPEN".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane entry in the shape herdr 0.9.0 emits (verified live).
    fn pane(pane_id: &str, label: Option<&str>, tab: &str, focused: bool) -> String {
        let label = match label {
            Some(l) => format!(r#","label":"{l}""#),
            None => String::new(),
        };
        format!(r#"{{"pane_id":"{pane_id}","tab_id":"{tab}","focused":{focused}{label}}}"#)
    }

    fn list(panes: &[String]) -> String {
        format!(
            r#"{{"id":"cli:pane:list","result":{{"panes":[{}]}}}}"#,
            panes.join(",")
        )
    }

    // ---- the split launcher ---------------------------------------------------------------

    #[test]
    fn with_no_scm_pane_in_this_tab_the_launcher_opens_one() {
        let json = list(&[pane("wA:p1", None, "wA:t1", true)]);
        assert_eq!(launch_decision(&json), "OPEN");
    }

    #[test]
    fn an_unfocused_scm_pane_in_this_tab_is_focused() {
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("wA:p2", Some(PANE_LABEL), "wA:t1", false),
        ]);
        assert_eq!(launch_decision(&json), "FOCUS wA:p2");
    }

    #[test]
    fn pressing_the_key_while_the_scm_pane_is_focused_closes_it() {
        let json = list(&[pane("wA:p2", Some(PANE_LABEL), "wA:t1", true)]);
        assert_eq!(launch_decision(&json), "CLOSE wA:p2");
    }

    #[test]
    fn an_scm_pane_in_another_tab_is_ignored_by_the_split_launcher() {
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("wA:p9", Some(PANE_LABEL), "wA:t2", false),
        ]);
        assert_eq!(launch_decision(&json), "OPEN");
    }

    #[test]
    fn with_no_focused_pane_the_launcher_opens_rather_than_acting_on_an_unknown_tab() {
        let json = list(&[pane("wA:p2", Some(PANE_LABEL), "wA:t1", false)]);
        assert_eq!(launch_decision(&json), "OPEN");
    }

    #[test]
    fn unparseable_input_degrades_to_open() {
        for bad in ["", "not json", "{}", r#"{"result":{}}"#] {
            assert_eq!(launch_decision(bad), "OPEN", "{bad:?}");
        }
    }

    #[test]
    fn a_pane_id_that_could_option_inject_is_never_emitted() {
        // The launcher passes the id to `herdr pane zoom|close <id>`; a leading dash would
        // become a flag.
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("--force", Some(PANE_LABEL), "wA:t1", false),
        ]);
        assert_eq!(launch_decision(&json), "OPEN");
    }

    // ---- the tab launcher -------------------------------------------------------------------

    #[test]
    fn an_scm_pane_in_another_tab_of_this_workspace_is_switched_to_not_duplicated() {
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("wA:p9", Some(PANE_LABEL), "wA:t2", false),
        ]);
        assert_eq!(launch_decision_tab(&json), "SWITCHTAB wA:t2");
    }

    #[test]
    fn a_pane_in_the_focused_tab_is_preferred_over_one_elsewhere() {
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("wA:p2", Some(PANE_LABEL), "wA:t1", false),
            pane("wA:p9", Some(PANE_LABEL), "wA:t2", false),
        ]);
        assert_eq!(launch_decision_tab(&json), "FOCUS wA:p2");
    }

    #[test]
    fn an_scm_pane_in_a_different_workspace_is_left_alone() {
        // Switching would yank the user out of their current workspace; opening here is right.
        let json = list(&[
            pane("wA:p1", None, "wA:t1", true),
            pane("wB:p9", Some(PANE_LABEL), "wB:t1", false),
        ]);
        assert_eq!(launch_decision_tab(&json), "OPEN");
    }

    #[test]
    fn a_tab_id_that_could_option_inject_is_never_emitted() {
        // The workspace gate above only ever admits a SWITCHTAB candidate whose tab id shares
        // the focused pane's workspace prefix (the text before the first `:`). That means the
        // full token handed to `herdr tab focus <token>` can start with `-` ONLY IF the shared
        // workspace prefix itself does. A colon-less id (like the previous version of this
        // test used) has workspace `None`, which never equals the focused pane's `Some(_)`
        // workspace, so it is rejected by the workspace gate BEFORE `is_flag_safe` is ever
        // consulted — proving nothing about the flag-safety guard it claims to cover. Giving
        // both panes the dash-prefixed workspace `-w` is what actually clears the gate and
        // reaches the filter below, which then rejects it.
        let json = list(&[
            pane("-w:p1", None, "-w:t1", true),
            pane("-w:p9", Some(PANE_LABEL), "-w:t2", false),
        ]);
        assert_eq!(launch_decision_tab(&json), "OPEN");
        // Positive twin, pinning the other side of the guard (a same-workspace, flag-SAFE tab
        // id elsewhere DOES switch): `an_scm_pane_in_another_tab_of_this_workspace_is_switched_to_not_duplicated`
        // above. Without that test, a filter that rejected everything would also pass this one.
    }
}

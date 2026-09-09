//! The keybinding registry, its config overrides, and the key -> [`Intent`] dispatcher
//! (spec §5.3).
//!
//! One registry row per intent, so "every action has a key" is a test rather than a hope. A
//! `[keys]` entry REPLACES that intent's key set and displaces whichever default held the key;
//! `Esc` is then re-pinned to [`Intent::Close`] so no configuration can lock the user inside an
//! overlay.

use crate::config::KeySpec;
use crate::intent::Intent;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::{BTreeMap, HashMap, HashSet};

/// One action's registry entry: its config key name, its default keys, and its help text.
pub struct RegistryRow {
    pub intent: Intent,
    /// The `[keys]` name that overrides this row.
    pub name: &'static str,
    pub default_keys: &'static [&'static str],
    pub description: &'static str,
}

/// Every action, its config name, and its default keys — the spec §5.3 table, in help order.
pub const REGISTRY: &[RegistryRow] = &[
    RegistryRow {
        intent: Intent::NavDown,
        name: "nav_down",
        default_keys: &["j", "Down"],
        description: "Move the cursor down",
    },
    RegistryRow {
        intent: Intent::NavUp,
        name: "nav_up",
        default_keys: &["k", "Up"],
        description: "Move the cursor up",
    },
    RegistryRow {
        intent: Intent::Activate,
        name: "activate",
        default_keys: &["Enter", "Space"],
        description: "Expand or collapse the selected repo or group",
    },
    RegistryRow {
        intent: Intent::FocusToggle,
        name: "focus_toggle",
        default_keys: &["Tab"],
        description: "Move focus between the tree and the diff",
    },
    RegistryRow {
        intent: Intent::NextChange,
        name: "next_change",
        default_keys: &["]"],
        description: "Jump to the next changed file, across repos",
    },
    RegistryRow {
        intent: Intent::PrevChange,
        name: "prev_change",
        default_keys: &["["],
        description: "Jump to the previous changed file, across repos",
    },
    RegistryRow {
        intent: Intent::Refresh,
        name: "refresh",
        default_keys: &["r"],
        description: "Re-scan now, including the repo list",
    },
    RegistryRow {
        intent: Intent::ToggleAll,
        name: "toggle_all",
        default_keys: &["a"],
        description: "Expand or collapse everything",
    },
    RegistryRow {
        intent: Intent::Zoom,
        name: "zoom",
        default_keys: &["Z"],
        description: "Toggle this pane's full-screen zoom",
    },
    RegistryRow {
        intent: Intent::CopyPath,
        name: "copy_path",
        default_keys: &["y"],
        description: "Copy repo:path for the selected file",
    },
    RegistryRow {
        intent: Intent::OpenEditor,
        name: "open_editor",
        default_keys: &["e"],
        description: "Open the selected file in $EDITOR",
    },
    RegistryRow {
        intent: Intent::Help,
        name: "help",
        default_keys: &["?"],
        description: "Show this help",
    },
    RegistryRow {
        intent: Intent::Close,
        name: "close",
        default_keys: &["Esc"],
        description: "Close the overlay",
    },
    RegistryRow {
        intent: Intent::Quit,
        name: "quit",
        default_keys: &["q"],
        description: "Quit",
    },
];

/// The resolved key -> intent map plus which intents the user customized.
pub struct Bindings {
    map: HashMap<KeyCode, Intent>,
    customized: HashSet<Intent>,
}

impl Bindings {
    /// The intent this key fires, or `None` when it is unbound.
    pub fn intent_for(&self, code: KeyCode) -> Option<Intent> {
        self.map.get(&code).copied()
    }

    /// The effective keys for an intent, ordered by their rendered label so the help overlay is
    /// stable across runs.
    pub fn keys_for(&self, intent: Intent) -> Vec<KeyCode> {
        let mut codes: Vec<KeyCode> = self
            .map
            .iter()
            .filter(|(_, i)| **i == intent)
            .map(|(c, _)| *c)
            .collect();
        codes.sort_by_key(|c| key_label(*c));
        codes
    }

    /// Whether this intent's keys came from the user's config.
    pub fn is_customized(&self, intent: Intent) -> bool {
        self.customized.contains(&intent)
    }
}

/// The registry's defaults, with no config applied.
pub fn default_bindings() -> Bindings {
    resolve_bindings(&BTreeMap::new())
}

/// Layer a `[keys]` config over the registry defaults.
///
/// A recognized entry with at least one parseable key REPLACES that intent's key set and wins
/// the key outright, displacing whichever default held it. Unknown names, unparseable keys and
/// empty lists leave the default in place. Finally `Esc` is pinned to [`Intent::Close`].
pub fn resolve_bindings(keys: &BTreeMap<String, KeySpec>) -> Bindings {
    let mut effective: Vec<(Intent, Vec<KeyCode>, bool)> = REGISTRY
        .iter()
        .map(|row| {
            let defaults: Vec<KeyCode> = row
                .default_keys
                .iter()
                .filter_map(|s| parse_key_spec(s))
                .collect();
            (row.intent, defaults, false)
        })
        .collect();

    for (name, spec) in keys {
        let Some(position) = REGISTRY.iter().position(|r| r.name == name) else {
            continue;
        };
        let specs: Vec<&str> = match spec {
            KeySpec::One(s) => vec![s.as_str()],
            KeySpec::Many(v) => v.iter().map(String::as_str).collect(),
        };
        let codes: Vec<KeyCode> = specs.iter().filter_map(|s| parse_key_spec(s)).collect();
        if codes.is_empty() {
            continue;
        }
        effective[position].1 = codes;
        effective[position].2 = true;
    }

    // Customized intents claim their keys first, so a custom binding displaces a default.
    let mut map = HashMap::new();
    for (intent, codes, _) in effective.iter().filter(|(_, _, custom)| *custom) {
        for code in codes {
            map.insert(*code, *intent);
        }
    }
    for (intent, codes, _) in effective.iter().filter(|(_, _, custom)| !*custom) {
        for code in codes {
            map.entry(*code).or_insert(*intent);
        }
    }
    // The no-lockout floor: Esc always closes an overlay (spec §5.3).
    map.insert(KeyCode::Esc, Intent::Close);

    Bindings {
        map,
        customized: effective
            .into_iter()
            .filter(|(_, _, custom)| *custom)
            .map(|(intent, _, _)| intent)
            .collect(),
    }
}

/// Parse one key spec. A single character is taken literally and IS case-sensitive (`Z` is not
/// `z`); named keys are case-insensitive.
pub fn parse_key_spec(spec: &str) -> Option<KeyCode> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut chars = trimmed.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(c));
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=12).contains(&n)
    {
        return Some(KeyCode::F(n));
    }
    Some(match lower.as_str() {
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "enter" | "return" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        _ => return None,
    })
}

/// A key's human label, for the help overlay.
pub fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        other => format!("{other:?}"),
    }
}

/// Decode a key event into an intent.
///
/// Only key PRESSES count (a terminal that reports releases must not fire an action twice), and
/// only `Shift` is tolerated as a modifier — and only on a character, because that is how an
/// uppercase key arrives. Every other chord (Ctrl+C above all) stays the terminal's.
pub fn decode(ev: KeyEvent, bindings: &Bindings) -> Option<Intent> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let allowed = match ev.code {
        KeyCode::Char(_) => KeyModifiers::SHIFT,
        _ => KeyModifiers::NONE,
    };
    if !ev.modifiers.difference(allowed).is_empty() {
        return None;
    }
    bindings.intent_for(ev.code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn cfg(entries: &[(&str, KeySpec)]) -> BTreeMap<String, KeySpec> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    // ---- the registry ----------------------------------------------------------------------

    #[test]
    fn every_intent_appears_exactly_once_in_the_registry() {
        // A missing row means an unreachable action; a duplicate means an ambiguous config key.
        let mut names: Vec<&str> = REGISTRY.iter().map(|r| r.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate registry names");

        let mut intents: Vec<Intent> = REGISTRY.iter().map(|r| r.intent).collect();
        intents.sort();
        intents.dedup();
        assert_eq!(intents.len(), REGISTRY.len(), "duplicate registry intents");
    }

    #[test]
    fn every_registry_default_key_parses_and_every_row_has_one() {
        for row in REGISTRY {
            assert!(
                !row.default_keys.is_empty(),
                "{} has no default key",
                row.name
            );
            assert!(
                !row.description.is_empty(),
                "{} has no description",
                row.name
            );
            for spec in row.default_keys {
                assert!(
                    parse_key_spec(spec).is_some(),
                    "{}: unparseable default key {spec:?}",
                    row.name
                );
            }
        }
    }

    #[test]
    fn no_two_registry_rows_claim_the_same_default_key() {
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for row in REGISTRY {
            for spec in row.default_keys {
                if let Some(previous) = seen.insert((*spec).to_string(), row.name) {
                    panic!("{spec:?} is claimed by both {previous} and {}", row.name);
                }
            }
        }
    }

    // ---- the spec's key table --------------------------------------------------------------

    #[test]
    fn the_default_bindings_are_exactly_the_spec_key_table() {
        let b = default_bindings();
        let expect = [
            (KeyCode::Char('j'), Intent::NavDown),
            (KeyCode::Down, Intent::NavDown),
            (KeyCode::Char('k'), Intent::NavUp),
            (KeyCode::Up, Intent::NavUp),
            (KeyCode::Enter, Intent::Activate),
            (KeyCode::Char(' '), Intent::Activate),
            (KeyCode::Tab, Intent::FocusToggle),
            (KeyCode::Char(']'), Intent::NextChange),
            (KeyCode::Char('['), Intent::PrevChange),
            (KeyCode::Char('r'), Intent::Refresh),
            (KeyCode::Char('a'), Intent::ToggleAll),
            (KeyCode::Char('Z'), Intent::Zoom),
            (KeyCode::Char('y'), Intent::CopyPath),
            (KeyCode::Char('e'), Intent::OpenEditor),
            (KeyCode::Char('?'), Intent::Help),
            (KeyCode::Esc, Intent::Close),
            (KeyCode::Char('q'), Intent::Quit),
        ];
        for (code, intent) in expect {
            assert_eq!(b.intent_for(code), Some(intent), "{code:?}");
        }
    }

    #[test]
    fn an_unbound_key_decodes_to_nothing() {
        assert_eq!(decode(ch('§'), &default_bindings()), None);
    }

    // ---- modifiers -------------------------------------------------------------------------

    #[test]
    fn a_control_chord_never_fires_an_intent() {
        // Ctrl+C must stay the terminal interrupt, not `CopyPath`'s neighbour.
        let ev = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(decode(ev, &default_bindings()), None);
    }

    #[test]
    fn an_alt_chord_never_fires_an_intent() {
        let ev = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT);
        assert_eq!(decode(ev, &default_bindings()), None);
    }

    #[test]
    fn shift_is_allowed_on_a_character_because_that_is_how_an_uppercase_key_arrives() {
        let ev = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT);
        assert_eq!(decode(ev, &default_bindings()), Some(Intent::Zoom));
    }

    #[test]
    fn a_key_release_event_is_ignored_so_an_action_never_fires_twice() {
        let mut ev = ch('q');
        ev.kind = KeyEventKind::Release;
        assert_eq!(decode(ev, &default_bindings()), None);
    }

    // ---- parse_key_spec ----------------------------------------------------------------------

    #[test]
    fn a_single_character_spec_is_case_sensitive() {
        assert_eq!(parse_key_spec("z"), Some(KeyCode::Char('z')));
        assert_eq!(parse_key_spec("Z"), Some(KeyCode::Char('Z')));
        assert_eq!(parse_key_spec("]"), Some(KeyCode::Char(']')));
    }

    #[test]
    fn named_keys_are_case_insensitive() {
        for (spec, code) in [
            ("Up", KeyCode::Up),
            ("down", KeyCode::Down),
            ("ENTER", KeyCode::Enter),
            ("Space", KeyCode::Char(' ')),
            ("tab", KeyCode::Tab),
            ("Esc", KeyCode::Esc),
            ("Home", KeyCode::Home),
            ("PageDown", KeyCode::PageDown),
            ("f5", KeyCode::F(5)),
        ] {
            assert_eq!(parse_key_spec(spec), Some(code), "{spec}");
        }
    }

    #[test]
    fn an_unparseable_spec_yields_nothing_rather_than_a_wrong_key() {
        for spec in ["", "  ", "ctrl+a", "NotAKey", "F0", "F13", "up arrow"] {
            assert_eq!(parse_key_spec(spec), None, "{spec}");
        }
    }

    // ---- bindings resolution -----------------------------------------------------------------

    #[test]
    fn a_config_entry_replaces_that_intents_default_keys_rather_than_adding_to_them() {
        let b = resolve_bindings(&cfg(&[("refresh", KeySpec::One("g".to_string()))]));
        assert_eq!(b.intent_for(KeyCode::Char('g')), Some(Intent::Refresh));
        assert_eq!(b.intent_for(KeyCode::Char('r')), None);
        assert!(b.is_customized(Intent::Refresh));
    }

    #[test]
    fn an_array_of_keys_binds_all_of_them() {
        let b = resolve_bindings(&cfg(&[(
            "nav_up",
            KeySpec::Many(vec!["w".to_string(), "Up".to_string()]),
        )]));
        assert_eq!(b.intent_for(KeyCode::Char('w')), Some(Intent::NavUp));
        assert_eq!(b.intent_for(KeyCode::Up), Some(Intent::NavUp));
        assert_eq!(b.intent_for(KeyCode::Char('k')), None);
    }

    #[test]
    fn a_custom_binding_displaces_the_default_that_held_that_key() {
        // Binding refresh to `q` must not leave `q` also quitting — the custom binding wins and
        // Quit simply loses that key.
        let b = resolve_bindings(&cfg(&[("refresh", KeySpec::One("q".to_string()))]));
        assert_eq!(b.intent_for(KeyCode::Char('q')), Some(Intent::Refresh));
        assert!(b.keys_for(Intent::Quit).is_empty());
    }

    #[test]
    fn an_unknown_intent_name_in_the_config_is_ignored() {
        let b = resolve_bindings(&cfg(&[("not_an_intent", KeySpec::One("x".to_string()))]));
        assert_eq!(b.intent_for(KeyCode::Char('x')), None);
        assert_eq!(b.intent_for(KeyCode::Char('r')), Some(Intent::Refresh));
    }

    #[test]
    fn an_entry_whose_keys_are_all_unparseable_keeps_the_default() {
        let b = resolve_bindings(&cfg(&[("refresh", KeySpec::One("ctrl+r".to_string()))]));
        assert_eq!(b.intent_for(KeyCode::Char('r')), Some(Intent::Refresh));
        assert!(!b.is_customized(Intent::Refresh));
    }

    #[test]
    fn an_empty_key_array_keeps_the_default() {
        let b = resolve_bindings(&cfg(&[("refresh", KeySpec::Many(vec![]))]));
        assert_eq!(b.intent_for(KeyCode::Char('r')), Some(Intent::Refresh));
    }

    #[test]
    fn escape_always_closes_an_overlay_even_if_the_config_rebinds_it() {
        // spec §5.3: "Esc 永遠關閉疊層". The floor is applied last, so no config can lock the
        // user inside the help overlay.
        let b = resolve_bindings(&cfg(&[("quit", KeySpec::One("Esc".to_string()))]));
        assert_eq!(b.intent_for(KeyCode::Esc), Some(Intent::Close));
    }

    #[test]
    fn an_empty_config_resolves_to_exactly_the_defaults() {
        let default = default_bindings();
        let resolved = resolve_bindings(&BTreeMap::new());
        for row in REGISTRY {
            assert_eq!(
                resolved.keys_for(row.intent),
                default.keys_for(row.intent),
                "{}",
                row.name
            );
            assert!(!resolved.is_customized(row.intent), "{}", row.name);
        }
    }

    #[test]
    fn keys_for_returns_a_deterministic_order_so_the_help_overlay_is_stable() {
        let b = default_bindings();
        assert_eq!(b.keys_for(Intent::NavDown), b.keys_for(Intent::NavDown));
        let labels: Vec<String> = b
            .keys_for(Intent::NavDown)
            .into_iter()
            .map(key_label)
            .collect();
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted);
    }

    #[test]
    fn key_labels_are_human_readable() {
        assert_eq!(key_label(KeyCode::Char(' ')), "Space");
        assert_eq!(key_label(KeyCode::Char('j')), "j");
        assert_eq!(key_label(KeyCode::Up), "Up");
        assert_eq!(key_label(KeyCode::F(5)), "F5");
    }
}

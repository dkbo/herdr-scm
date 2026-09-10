//! The panel's palette (spec §4).
//!
//! ANSI-16 only, by design. The right half of the pane is coloured by `delta` plus the user's
//! terminal theme, which this crate cannot see; choosing the terminal's own semantic colours —
//! rather than picking pretty hex values — is the only way the two halves stay in step when the
//! user switches themes. It also keeps the panel readable on a light background and over a
//! 16-colour SSH session.
//!
//! Pure presentation: no I/O, and deliberately no dependency on `model.rs`.

use ratatui::style::{Color, Modifier, Style};

/// What a run of text MEANS. Line builders in `presenter.rs` emit these, never a `Style`, so the
/// whole palette lives in this one file and nothing else knows what colour anything is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A repo's directory name — the highest-ranking text in the tree.
    RepoName,
    /// A repo's path relative to the scan start.
    RelPath,
    /// The `root` / `submodule` / `worktree` / `nested` marker.
    Kind,
    /// The `▸` / `▾` expansion marker.
    Marker,
    /// Borders, dividers, indentation, and the punctuation between fields.
    Chrome,
    /// A branch name or short SHA.
    Branch,
    /// `↑N` or `↓N` with N > 0: there is something to act on.
    Sync,
    /// `↑0` or `↓0`: level with the upstream, so nothing to act on.
    SyncIdle,
    /// A repo's changed-file count.
    DirtyCount,
    /// `Staged` / `Changes` / `Untracked`.
    GroupTitle,
    /// How many files a group holds.
    GroupCount,
    /// A changed file's repo-relative path.
    Path,
    /// The `← old/path` half of a rename or copy.
    OrigPath,
    /// The file name above the diff.
    DiffTitle,
    /// A repo whose git query failed.
    Error,
    /// A repo whose previous round timed out.
    Stale,
    /// A porcelain status letter. `char` keeps `Role` `Copy`.
    Status(char),
}

impl Role {
    /// Every role the palette answers for.
    ///
    /// A forgotten variant is caught by the compiler, not by this list: `style` has no `_` arm
    /// for the non-`Status` variants, so adding one without assigning it fails to compile. What
    /// `ALL` is for is the palette-wide invariants — no `White`, no `Rgb` — which have to be
    /// checked over every role rather than proven per arm.
    pub const ALL: &[Role] = &[
        Role::RepoName,
        Role::RelPath,
        Role::Kind,
        Role::Marker,
        Role::Chrome,
        Role::Branch,
        Role::Sync,
        Role::SyncIdle,
        Role::DirtyCount,
        Role::GroupTitle,
        Role::GroupCount,
        Role::Path,
        Role::OrigPath,
        Role::DiffTitle,
        Role::Error,
        Role::Stale,
        Role::Status('A'),
        Role::Status('M'),
        Role::Status('D'),
        Role::Status('R'),
        Role::Status('C'),
        Role::Status('U'),
        Role::Status('?'),
        Role::Status('T'),
    ];
}

/// The palette (spec §4.3).
///
/// The rule the assignments follow: green/red/yellow are file-change semantics (aligned with
/// `delta`'s `+` and `-`), cyan/magenta are git refs and sync state, dark grey is structure and
/// zeroed-out information, and the terminal's OWN foreground is identity.
pub fn style(role: Role) -> Style {
    match role {
        // Identity: no foreground at all, so the terminal's own decides.
        Role::RepoName | Role::DiffTitle => Style::default().add_modifier(Modifier::BOLD),
        Role::GroupTitle | Role::Path => Style::default(),
        // A magnitude, not a kind — colouring it yellow would steal "modified"'s meaning.
        Role::DirtyCount => Style::default().add_modifier(Modifier::BOLD),
        // Structure, and information that has gone to zero.
        Role::RelPath
        | Role::Kind
        | Role::Marker
        | Role::Chrome
        | Role::GroupCount
        | Role::OrigPath
        | Role::SyncIdle => Style::default().fg(Color::DarkGray),
        // Refs and sync.
        Role::Branch => Style::default().fg(Color::Cyan),
        Role::Sync => Style::default().fg(Color::Magenta),
        // Health. `Stale` borrowing yellow is the one deliberate exception to the rule above:
        // "the numbers on this row may be wrong" outranks palette purity, and it only ever
        // appears on a repo row, never beside a file row's yellow `M`.
        Role::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Role::Stale => Style::default().fg(Color::Yellow),
        Role::Status(code) => status_style(code),
    }
}

/// One porcelain status letter's colour.
///
/// The `_` arm is load-bearing: porcelain v2 can grow codes, and an unassigned one must render
/// as ordinary text rather than disappear.
fn status_style(code: char) -> Style {
    match code {
        'A' => Style::default().fg(Color::Green),
        'M' => Style::default().fg(Color::Yellow),
        'D' => Style::default().fg(Color::Red),
        'R' | 'C' => Style::default().fg(Color::Cyan),
        'U' => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        // Untracked is noise most of the time — that is why its group is drawn last.
        '?' => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

/// The cursor row's style, which is also the tree's only focus cue (it has no border to brighten).
///
/// Neither variant sets a colour: `REVERSED` is readable by construction and `UNDERLINED` leaves
/// the row's own semantic colours alone. A `bg(DarkGray)` "inactive selection" would be dark grey
/// behind dark text on a light-background terminal.
pub fn selection(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::UNDERLINED)
    }
}

/// A pane's border glyphs. Applied via `Block::border_style`, never `Block::style`, so it cannot
/// bleed into the block's title.
pub fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};

    #[test]
    fn the_palette_never_uses_white_so_a_light_background_terminal_stays_readable() {
        for &role in Role::ALL {
            let s = style(role);
            assert_ne!(s.fg, Some(Color::White), "{role:?}");
            assert_ne!(s.bg, Some(Color::White), "{role:?}");
        }
    }

    #[test]
    fn the_palette_is_ansi_only_so_it_follows_the_terminal_theme() {
        for &role in Role::ALL {
            let s = style(role);
            for color in [s.fg, s.bg].into_iter().flatten() {
                assert!(
                    !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                    "{role:?} uses {color:?}, which does not follow the terminal theme"
                );
            }
        }
    }

    #[test]
    fn identity_text_leaves_the_foreground_to_the_terminal() {
        // A hard-coded foreground here is what makes a light-background terminal unreadable.
        for role in [
            Role::RepoName,
            Role::GroupTitle,
            Role::Path,
            Role::DirtyCount,
        ] {
            assert_eq!(style(role).fg, None, "{role:?}");
        }
    }

    #[test]
    fn the_status_letters_follow_deltas_plus_and_minus() {
        assert_eq!(style(Role::Status('A')).fg, Some(Color::Green));
        assert_eq!(style(Role::Status('D')).fg, Some(Color::Red));
        assert_eq!(style(Role::Status('M')).fg, Some(Color::Yellow));
        assert_eq!(style(Role::Status('R')).fg, Some(Color::Cyan));
        assert_eq!(style(Role::Status('C')).fg, Some(Color::Cyan));
        assert_eq!(style(Role::Status('?')).fg, Some(Color::DarkGray));
    }

    #[test]
    fn a_conflict_is_the_loudest_thing_on_the_screen() {
        let s = style(Role::Status('U'));
        assert_eq!(s.fg, Some(Color::Red));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_unknown_status_letter_is_still_visible() {
        // porcelain v2 can grow codes; an unassigned one must not render as invisible.
        assert_eq!(style(Role::Status('T')), Style::default());
    }

    #[test]
    fn a_level_upstream_recedes_while_a_pending_one_stays_lit() {
        assert_eq!(style(Role::Sync).fg, Some(Color::Magenta));
        assert_eq!(style(Role::SyncIdle).fg, Some(Color::DarkGray));
    }

    #[test]
    fn structure_is_dimmer_than_content() {
        for role in [
            Role::Marker,
            Role::Chrome,
            Role::RelPath,
            Role::Kind,
            Role::GroupCount,
            Role::OrigPath,
        ] {
            assert_eq!(style(role).fg, Some(Color::DarkGray), "{role:?}");
        }
    }

    #[test]
    fn a_repo_name_outranks_a_group_title() {
        assert!(style(Role::RepoName).add_modifier.contains(Modifier::BOLD));
        assert!(
            !style(Role::GroupTitle)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn the_selection_bar_says_which_pane_has_focus_without_relying_on_a_background_color() {
        let (on, off) = (selection(true), selection(false));
        assert_ne!(on, off);
        // A bg color would be dark-grey-on-dark-text in a light terminal. REVERSED and
        // UNDERLINED are both readable whatever the background is.
        assert_eq!(on.bg, None);
        assert_eq!(off.bg, None);
        assert_eq!(on.fg, None, "the row keeps its own semantic colors");
        assert_eq!(off.fg, None);
        assert!(on.add_modifier.contains(Modifier::REVERSED));
        assert!(off.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn a_focused_pane_border_is_distinguishable_from_an_unfocused_one() {
        assert_ne!(pane_border(true), pane_border(false));
        assert_eq!(pane_border(false).fg, Some(Color::DarkGray));
    }
}

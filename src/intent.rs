//! Every action the panel can perform, decoupled from the keys that trigger it (spec §5.3).
//!
//! Read-only by construction: no variant writes a file or mutates git. `Zoom` drives herdr's own
//! layout and `OpenEditor` hands the terminal to `$EDITOR`; neither touches repo state here.

/// One user action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Intent {
    /// Move the cursor up one row.
    NavUp,
    /// Move the cursor down one row.
    NavDown,
    /// Expand or collapse the selected repo or group.
    Activate,
    /// Move focus between the tree and the diff.
    FocusToggle,
    /// Jump to the next changed file, across repos.
    NextChange,
    /// Jump to the previous changed file, across repos.
    PrevChange,
    /// Re-scan now, including the repo list.
    Refresh,
    /// Expand or collapse everything.
    ToggleAll,
    /// Toggle this herdr pane's full-screen zoom.
    Zoom,
    /// Copy `repo:path` for the selected file to the terminal clipboard.
    CopyPath,
    /// Hand the selected file to `$EDITOR`.
    OpenEditor,
    /// Show the help overlay.
    Help,
    /// Close whatever overlay is open.
    Close,
    /// Quit the panel.
    Quit,
}

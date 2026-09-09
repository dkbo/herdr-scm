//! The launch context herdr injects, after parsing.

use std::path::PathBuf;

/// What the plugin knows about how it was launched (spec §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The most specific directory herdr told us about: `focused_pane_cwd`, else
    /// `workspace_cwd`, else `cwd`, else the process working directory.
    pub cwd: PathBuf,
    /// The workspace root, when herdr reported one. Kept SEPARATELY from `cwd` because the
    /// spec §3.1 degradation path ("herdr CLI unavailable → use workspace_cwd as the only scan
    /// start") specifically needs the workspace root, not the focused pane's subdirectory.
    pub workspace_cwd: Option<PathBuf>,
    /// The workspace this pane belongs to; the filter for `herdr pane list`.
    pub workspace_id: Option<String>,
}

//! Pure parser for `git status --porcelain=v2 -z` (spec §4). No I/O; `git.rs` feeds it bytes.
//!
//! Records are NUL-terminated, including the `# …` headers. A rename/copy record (`2 …`) is
//! followed by its original path as a SEPARATE NUL-delimited field — getting that wrong shifts
//! every subsequent record, so it has its own tests.

use crate::model::{FileEntry, GroupKind, StatusGroup};

/// What `git status --porcelain=v2 -z` said about one repo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PorcelainStatus {
    /// The branch name. `None` when HEAD is detached (git says `(detached)`) or unreported —
    /// the caller falls back to a short `oid`.
    pub head: Option<String>,
    /// HEAD's object id. `None` in a repo with no commits (git says `(initial)`).
    pub oid: Option<String>,
    pub upstream: Option<String>,
    /// Commits ahead of / behind upstream. Both `None` when there is no upstream at all, which
    /// is deliberately distinct from `Some(0)`.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub staged: Vec<FileEntry>,
    pub changes: Vec<FileEntry>,
    pub untracked: Vec<FileEntry>,
}

impl PorcelainStatus {
    /// The non-empty groups, in the order the tree draws them (spec §5.2). An empty group is
    /// omitted entirely, because the spec says its row is not drawn.
    pub fn groups(&self) -> Vec<StatusGroup> {
        [
            (GroupKind::Staged, &self.staged),
            (GroupKind::Changes, &self.changes),
            (GroupKind::Untracked, &self.untracked),
        ]
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(|(kind, files)| StatusGroup {
            kind,
            files: files.clone(),
        })
        .collect()
    }
}

/// Parse a porcelain v2 `-z` byte stream. Malformed or truncated records are skipped; the
/// parse never fails and never panics.
pub fn parse(bytes: &[u8]) -> PorcelainStatus {
    let mut status = PorcelainStatus::default();
    // Trailing NUL yields a final empty slice; `filter(non-empty)` inside the loop drops it.
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut i = 0;
    while i < fields.len() {
        let record = String::from_utf8_lossy(fields[i]).into_owned();
        i += 1;
        if record.is_empty() {
            continue;
        }
        match record.as_bytes()[0] {
            b'#' => read_header(&record, &mut status),
            b'1' => {
                if let Some((xy, path)) = ordinary(&record) {
                    push_xy(&mut status, xy, &path, None);
                }
            }
            b'2' => {
                // The original path is the NEXT NUL field; consume it whether or not we use it.
                let orig = fields
                    .get(i)
                    .map(|f| String::from_utf8_lossy(f).into_owned())
                    .filter(|s| !s.is_empty());
                if orig.is_some() {
                    i += 1;
                }
                if let Some((xy, path)) = rename(&record) {
                    push_xy(&mut status, xy, &path, orig.as_deref());
                }
            }
            b'u' => {
                if let Some(path) = unmerged(&record) {
                    status.changes.push(FileEntry {
                        path,
                        status: 'U',
                        orig_path: None,
                    });
                }
            }
            b'?' => {
                if let Some(path) = tail_after(&record, 1) {
                    status.untracked.push(FileEntry {
                        path,
                        status: '?',
                        orig_path: None,
                    });
                }
            }
            // `!` is an ignored entry — dropped; anything else is unknown and skipped.
            _ => {}
        }
    }
    for group in [
        &mut status.staged,
        &mut status.changes,
        &mut status.untracked,
    ] {
        group.sort_by(|a, b| a.path.cmp(&b.path));
    }
    status
}

/// Split off the first `n` whitespace-separated tokens and return the REST verbatim as the
/// path — paths may contain spaces, so the tail must never be re-split.
fn split_head_tail(record: &str, n: usize) -> Option<(Vec<&str>, String)> {
    let mut rest = record;
    let mut tokens = Vec::with_capacity(n);
    for _ in 0..n {
        let rest_trimmed = rest.trim_start();
        let end = rest_trimmed.find(char::is_whitespace)?;
        tokens.push(&rest_trimmed[..end]);
        rest = &rest_trimmed[end..];
    }
    let path = rest.trim_start().to_string();
    (!path.is_empty()).then_some((tokens, path))
}

/// The path of a record whose first `n` whitespace tokens are fixed fields.
fn tail_after(record: &str, n: usize) -> Option<String> {
    split_head_tail(record, n).map(|(_, path)| path)
}

/// `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>` — 8 fixed fields, then the path.
fn ordinary(record: &str) -> Option<(&str, String)> {
    let (tokens, path) = split_head_tail(record, 8)?;
    Some((xy_of(&tokens)?, path))
}

/// `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>` — 9 fixed fields, then the path.
fn rename(record: &str) -> Option<(&str, String)> {
    let (tokens, path) = split_head_tail(record, 9)?;
    Some((xy_of(&tokens)?, path))
}

/// `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>` — 10 fixed fields, then the path.
fn unmerged(record: &str) -> Option<String> {
    tail_after(record, 10)
}

/// The XY field (token index 1) when it is exactly two characters.
fn xy_of<'a>(tokens: &[&'a str]) -> Option<&'a str> {
    tokens.get(1).copied().filter(|xy| xy.chars().count() == 2)
}

/// Route one record's XY pair into the groups: `X` (index) → Staged, `Y` (worktree) → Changes.
/// A `.` in a column contributes nothing. `orig` is attached only to the rename/copy row.
fn push_xy(status: &mut PorcelainStatus, xy: &str, path: &str, orig: Option<&str>) {
    let mut chars = xy.chars();
    let (Some(x), Some(y)) = (chars.next(), chars.next()) else {
        return;
    };
    let entry = |code: char| FileEntry {
        path: path.to_string(),
        status: code,
        orig_path: matches!(code, 'R' | 'C')
            .then(|| orig.map(str::to_string))
            .flatten(),
    };
    if x != '.' {
        status.staged.push(entry(x));
    }
    if y != '.' {
        status.changes.push(entry(y));
    }
}

/// Read one `# branch.…` header into the status. Unknown headers and malformed values are
/// ignored.
fn read_header(record: &str, status: &mut PorcelainStatus) {
    let Some((key, value)) = record.trim_start_matches('#').trim().split_once(' ') else {
        return;
    };
    let value = value.trim();
    match key {
        "branch.oid" => status.oid = (value != "(initial)").then(|| value.to_string()),
        "branch.head" => status.head = (value != "(detached)").then(|| value.to_string()),
        "branch.upstream" => status.upstream = Some(value.to_string()),
        "branch.ab" => {
            // "+<ahead> -<behind>". Both must parse, or neither is recorded.
            let mut parts = value.split_whitespace();
            let ahead = parts
                .next()
                .and_then(|s| s.strip_prefix('+'))
                .and_then(|s| s.parse().ok());
            let behind = parts
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.parse().ok());
            if let (Some(a), Some(b)) = (ahead, behind) {
                status.ahead = Some(a);
                status.behind = Some(b);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `-z` porcelain v2 stream: every record NUL-terminated, exactly as git emits it.
    fn z(records: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for r in records {
            out.extend_from_slice(r.as_bytes());
            out.push(0);
        }
        out
    }

    fn paths(files: &[FileEntry]) -> Vec<&str> {
        files.iter().map(|f| f.path.as_str()).collect()
    }

    // ---- headers ---------------------------------------------------------------------------

    #[test]
    fn branch_headers_are_read() {
        let s = parse(&z(&[
            "# branch.oid 1234567890abcdef1234567890abcdef12345678",
            "# branch.head master",
            "# branch.upstream origin/master",
            "# branch.ab +6 -1",
        ]));
        assert_eq!(s.head.as_deref(), Some("master"));
        assert_eq!(
            s.oid.as_deref(),
            Some("1234567890abcdef1234567890abcdef12345678")
        );
        assert_eq!(s.upstream.as_deref(), Some("origin/master"));
        assert_eq!(s.ahead, Some(6));
        assert_eq!(s.behind, Some(1));
    }

    #[test]
    fn a_detached_head_reports_no_branch_but_keeps_the_oid() {
        let s = parse(&z(&[
            "# branch.oid deadbeefdeadbeefdeadbeef",
            "# branch.head (detached)",
        ]));
        assert_eq!(s.head, None);
        assert_eq!(s.oid.as_deref(), Some("deadbeefdeadbeefdeadbeef"));
    }

    #[test]
    fn an_initial_commit_less_repo_reports_no_oid() {
        let s = parse(&z(&["# branch.oid (initial)", "# branch.head main"]));
        assert_eq!(s.oid, None);
        assert_eq!(s.head.as_deref(), Some("main"));
    }

    #[test]
    fn with_no_upstream_there_is_no_ab_header_and_both_counts_stay_none() {
        // This is why ahead/behind are Option: "no upstream" must be distinguishable from
        // "level with upstream" (spec §4).
        let s = parse(&z(&["# branch.head main"]));
        assert_eq!(s.ahead, None);
        assert_eq!(s.behind, None);
        assert_eq!(s.upstream, None);
    }

    #[test]
    fn a_level_upstream_reports_zeroes_not_none() {
        let s = parse(&z(&["# branch.upstream origin/main", "# branch.ab +0 -0"]));
        assert_eq!(s.ahead, Some(0));
        assert_eq!(s.behind, Some(0));
    }

    #[test]
    fn a_malformed_ab_header_is_ignored_rather_than_panicking() {
        let s = parse(&z(&["# branch.ab garbage"]));
        assert_eq!(s.ahead, None);
        assert_eq!(s.behind, None);
    }

    // ---- ordinary entries ---------------------------------------------------------------------

    #[test]
    fn the_x_column_goes_to_staged_and_the_y_column_to_changes() {
        // "MM": staged modification AND a further worktree modification — the same path
        // appears in BOTH groups, each with its own column's letter (spec §4.1).
        let s = parse(&z(&["1 MM N... 100644 100644 100644 aaa bbb src/app.rs"]));
        assert_eq!(paths(&s.staged), vec!["src/app.rs"]);
        assert_eq!(s.staged[0].status, 'M');
        assert_eq!(paths(&s.changes), vec!["src/app.rs"]);
        assert_eq!(s.changes[0].status, 'M');
    }

    #[test]
    fn a_dot_column_contributes_nothing() {
        let s = parse(&z(&[
            "1 .M N... 100644 100644 100644 aaa bbb only-worktree.rs",
            "1 A. N... 000000 100644 100644 000 bbb only-index.rs",
        ]));
        assert_eq!(paths(&s.staged), vec!["only-index.rs"]);
        assert_eq!(s.staged[0].status, 'A');
        assert_eq!(paths(&s.changes), vec!["only-worktree.rs"]);
        assert_eq!(s.changes[0].status, 'M');
    }

    #[test]
    fn a_path_containing_spaces_survives_intact() {
        let s = parse(&z(&[
            "1 .M N... 100644 100644 100644 aaa bbb dir with spaces/a b.txt",
        ]));
        assert_eq!(paths(&s.changes), vec!["dir with spaces/a b.txt"]);
    }

    // ---- renames and copies ----------------------------------------------------------------

    #[test]
    fn a_rename_records_the_original_path_from_the_following_nul_field() {
        // In -z mode the original path is its OWN NUL-delimited field after the record.
        let s = parse(&z(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 new/name.rs",
            "old/name.rs",
        ]));
        assert_eq!(paths(&s.staged), vec!["new/name.rs"]);
        assert_eq!(s.staged[0].status, 'R');
        assert_eq!(s.staged[0].orig_path.as_deref(), Some("old/name.rs"));
    }

    #[test]
    fn a_rename_does_not_swallow_the_record_that_follows_it() {
        // The most fragile parse case: consuming the wrong number of fields shifts everything.
        let s = parse(&z(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.rs",
            "old.rs",
            "? untracked.rs",
        ]));
        assert_eq!(paths(&s.staged), vec!["new.rs"]);
        assert_eq!(paths(&s.untracked), vec!["untracked.rs"]);
    }

    #[test]
    fn a_rename_with_a_worktree_modification_puts_orig_path_only_on_the_rename_row() {
        let s = parse(&z(&[
            "2 RM N... 100644 100644 100644 aaa bbb R090 new.rs",
            "old.rs",
        ]));
        assert_eq!(s.staged[0].orig_path.as_deref(), Some("old.rs"));
        assert_eq!(s.changes[0].status, 'M');
        assert_eq!(s.changes[0].orig_path, None);
    }

    #[test]
    fn a_truncated_rename_record_missing_its_orig_field_does_not_panic() {
        let s = parse(&z(&["2 R. N... 100644 100644 100644 aaa bbb R100 new.rs"]));
        assert_eq!(paths(&s.staged), vec!["new.rs"]);
        assert_eq!(s.staged[0].orig_path, None);
    }

    // ---- unmerged and untracked --------------------------------------------------------------

    #[test]
    fn an_unmerged_entry_lands_in_changes_marked_u() {
        let s = parse(&z(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflicted.rs",
        ]));
        assert_eq!(paths(&s.changes), vec!["conflicted.rs"]);
        assert_eq!(s.changes[0].status, 'U');
        assert!(s.staged.is_empty());
    }

    #[test]
    fn untracked_entries_are_their_own_group() {
        let s = parse(&z(&["? new-file.rs", "? dir/other.rs"]));
        assert_eq!(paths(&s.untracked), vec!["dir/other.rs", "new-file.rs"]);
        assert!(s.untracked.iter().all(|f| f.status == '?'));
    }

    #[test]
    fn ignored_entries_are_dropped_entirely() {
        let s = parse(&z(&["! target/debug/x", "? kept.rs"]));
        assert_eq!(paths(&s.untracked), vec!["kept.rs"]);
    }

    // ---- robustness ----------------------------------------------------------------------------

    #[test]
    fn empty_and_garbage_input_yield_an_empty_status_without_panicking() {
        for input in [b"".to_vec(), z(&[""]), z(&["x", "1", "1 M", "?"])] {
            let s = parse(&input);
            assert!(s.staged.is_empty() && s.changes.is_empty() && s.untracked.is_empty());
        }
    }

    #[test]
    fn invalid_utf8_in_a_path_is_shown_lossily_rather_than_dropping_the_row() {
        // Losing the row would silently hide a real change; a lossy name is visible and its
        // diff failure surfaces as that repo's error row (spec §9).
        let mut bytes = b"1 .M N... 100644 100644 100644 aaa bbb bad".to_vec();
        bytes.push(0xff);
        bytes.push(0);
        assert_eq!(parse(&bytes).changes.len(), 1);
    }

    // ---- groups() -------------------------------------------------------------------------------

    #[test]
    fn groups_are_emitted_in_spec_order_and_empty_ones_are_omitted() {
        let s = parse(&z(&[
            "? u.rs",
            "1 M. N... 100644 100644 100644 aaa bbb s.rs",
        ]));
        let groups = s.groups();
        assert_eq!(
            groups.iter().map(|g| g.kind).collect::<Vec<_>>(),
            vec![GroupKind::Staged, GroupKind::Untracked]
        );
    }

    #[test]
    fn files_within_a_group_are_sorted_by_path_so_the_tree_is_stable() {
        let s = parse(&z(&["? z.rs", "? a.rs", "? m.rs"]));
        assert_eq!(paths(&s.untracked), vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn a_clean_repo_has_no_groups_at_all() {
        assert!(parse(&z(&["# branch.head main"])).groups().is_empty());
    }
}

//! Discovery against a REAL git tree (spec §11's "git fixture 整合測試" layer).

mod common;

use common::{Fixture, IGNORED_REPOS};
use herdr_scm::config::DEFAULT_SCAN_EXCLUDES;
use herdr_scm::discover::{GitignoreOracle, RealFs, ScanConfig, scan};
use herdr_scm::model::{RepoKind, rel_slash};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn scan_fixture(fx: &Fixture, depth: usize) -> BTreeMap<String, RepoKind> {
    let root = fx.root();
    let cfg = ScanConfig {
        depth,
        excludes: DEFAULT_SCAN_EXCLUDES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    scan(
        std::slice::from_ref(&root),
        &cfg,
        &RealFs,
        &GitignoreOracle::new(),
    )
    .into_iter()
    .map(|r| {
        (
            rel_slash(&r.path, &root).unwrap_or_else(|| r.path.to_string_lossy().into_owned()),
            r.kind,
        )
    })
    .collect()
}

#[test]
fn every_repo_behind_a_gitignored_directory_is_found() {
    // The load-bearing rule of spec §3.2: without it these five vanish and the panel is
    // reduced to the root repo alone.
    let fx = Fixture::build();
    let found = scan_fixture(&fx, 4);
    for name in IGNORED_REPOS {
        assert!(
            found.contains_key(name),
            "{name} missing; found: {:?}",
            found.keys().collect::<Vec<_>>()
        );
        assert_eq!(found[name], RepoKind::Nested, "{name} kind");
    }
}

#[test]
fn the_scan_start_itself_is_reported_as_the_root() {
    let fx = Fixture::build();
    assert_eq!(scan_fixture(&fx, 4)[""], RepoKind::Root);
}

#[test]
fn a_real_submodule_is_classified_as_a_submodule() {
    let fx = Fixture::build();
    assert_eq!(scan_fixture(&fx, 4)["sublib"], RepoKind::Submodule);
}

#[test]
fn a_real_linked_worktree_is_classified_as_a_worktree() {
    let fx = Fixture::build();
    assert_eq!(scan_fixture(&fx, 4)["wt/feature"], RepoKind::Worktree);
}

#[test]
fn a_repo_inside_an_ignored_repo_is_not_drilled_into() {
    let fx = Fixture::build();
    let found = scan_fixture(&fx, 8);
    assert!(found.contains_key("pencil"));
    assert!(
        !found.contains_key("pencil/deep/inner"),
        "drilling must stop at the first repo inside an ignored subtree"
    );
}

#[test]
fn a_repo_behind_a_hard_excluded_directory_is_never_found() {
    let fx = Fixture::build();
    assert!(!scan_fixture(&fx, 8).contains_key("node_modules/pkg"));
}

#[test]
fn the_depth_limit_keeps_a_deep_repo_out_at_the_default_depth() {
    let fx = Fixture::build();
    assert!(!scan_fixture(&fx, 4).contains_key("deep/a/b/c/d"));
    assert!(scan_fixture(&fx, 5).contains_key("deep/a/b/c/d"));
}

#[test]
fn the_whole_fixture_yields_exactly_the_expected_repo_set_at_the_default_depth() {
    let fx = Fixture::build();
    let mut names: Vec<String> = scan_fixture(&fx, 4).into_keys().collect();
    names.sort();
    let mut expected: Vec<String> = IGNORED_REPOS.iter().map(|s| s.to_string()).collect();
    expected.extend([
        "".to_string(),
        "sublib".to_string(),
        "wt/feature".to_string(),
    ]);
    expected.sort();
    assert_eq!(names, expected);
}

#[test]
fn scanning_an_empty_directory_finds_nothing_without_erroring() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    let cfg = ScanConfig {
        depth: 4,
        excludes: Vec::new(),
    };
    assert!(
        scan(
            &[PathBuf::from(empty.path())],
            &cfg,
            &RealFs,
            &GitignoreOracle::new()
        )
        .is_empty()
    );
}

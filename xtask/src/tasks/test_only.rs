//! `just test-only <filterset>` is `just test`'s own invocation plus `-E <filterset>`, by construction.
//!
//! `test-only` is a dependency-only header over `test`, so it cannot carry a cargo line of its own
//! that drifts: the filter is the only difference. `test`'s doctest leg runs only with no filter,
//! because nextest runs no doctests and a filterset cannot select them.
//! `test`'s whole command list is compared, shebang included and comments excluded, so a
//! same-meaning rewrite of one of its lines is red too.
//! Limit: this reads the justfile's text, so it holds the wiring and the commands, not what
//! `nix/with-tier.sh` or nextest then do with them.

const NEXTEST: &str = r#"cargo nextest run --workspace --all-features ${filter:+-E "$filter"}"#;
const DOCTEST: &str = r#"[ -n "$filter" ] || cargo test --doc --workspace --all-features"#;

fn justfile() -> String {
    let root = crate::repo::root().expect("the repo root");
    std::fs::read_to_string(root.join("justfile")).expect("the justfile")
}

fn commands(name: &str) -> Vec<String> {
    let root = crate::repo::root().expect("the repo root");
    crate::tasks::recipe_body(&root, name)
        .unwrap_or_else(|| panic!("no `{name}` recipe in the justfile"))
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && (line.starts_with("#!") || !line.starts_with('#')))
        .map(String::from)
        .collect()
}

#[test]
fn the_filtered_task_is_test_handed_a_filterset_and_nothing_else() {
    let text = justfile();
    let headers: Vec<&str> = text.lines().collect();
    assert!(
        headers.contains(&"test $filter='':"),
        "`just test` no longer takes the filterset as an optional exported parameter"
    );
    assert!(
        headers.contains(&"test-only filter: (test filter)"),
        "`just test-only` no longer hands its filterset to `just test`"
    );
    assert_eq!(
        commands("test-only"),
        Vec::<String>::new(),
        "`just test-only` carries a command of its own"
    );
}

#[test]
fn the_test_task_adds_the_filter_to_one_nextest_line_and_skips_doctests_under_it() {
    assert_eq!(
        commands("test"),
        [
            "#!/usr/bin/env bash",
            "set -euo pipefail",
            "source nix/with-tier.sh",
            "sutura_tier_up",
            "export SUTURA_DEV_RELAXED_TOLERANCE=1",
            NEXTEST,
            DOCTEST,
        ],
    );
}

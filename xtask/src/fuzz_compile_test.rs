//! `fuzz_compile`'s own test, kept out of `changes.rs`: a test beside the change it proves
//! cannot be reverted independently of it, so causality could not redden it on base. See
//! `github.com/telekom/sutura#2095`.

use super::classify;

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| String::from(*s)).collect()
}

#[test]
fn a_path_in_the_fuzzed_tree_surface_sets_fuzz_compile() {
    // `crates/sutura-domain/src/catalog.rs` is listed verbatim in the "fuzzed tree" surface
    // (`xtask/src/hook_coverage/surfaces.rs`, `reached_by: "fuzz-smoke"`) AND matches the "rust"
    // area (`crates/**`), which keeps `run_all` false here and so isolates the new flag from
    // `classify`'s unrelated fail-open path.
    let r = classify(&paths(&["crates/sutura-domain/src/catalog.rs"]));
    assert!(!r.run_all, "{:?}", r.reasons);
    assert!(r.fuzz_compile);
}

#[test]
fn a_rust_path_outside_the_fuzzed_tree_surface_leaves_fuzz_compile_false() {
    // Matches "rust" (`crates/**`) but no pattern of the "fuzzed tree" surface names
    // `sutura-cli` at all, so the compile-only leg must not select on this path.
    let r = classify(&paths(&["crates/sutura-cli/src/main.rs"]));
    assert!(!r.run_all, "{:?}", r.reasons);
    assert!(!r.fuzz_compile);
}

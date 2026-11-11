use super::base::BaseOutcome;
use super::fixtures::{Mutation, inseparable_claim_case};
use super::{BaseState, retry_with_held_back};
use crate::Verdict;

#[test]
fn a_vendored_implementation_is_reverted_without_collecting_its_tests() {
    use super::fixtures::{changed, manifest, tree};
    use super::plan::{Plan, partition};

    for source in ["vendor/dep/src/lib.rs", "vendor/dep/src/ffi.RS"] {
        let test = "crates/x/tests/t.rs";
        let files = vec![
            changed(test, 1, &["#[test]", "fn uses_the_dependency() {}"]),
            changed(source, 1, &["fn fixed() {}", "#[test]", "fn a_vendor_test() {}"]),
            changed("vendor/dep/Cargo.toml", 1, &["[package]"]),
            changed("vendor/dep/Cargo.lock", 1, &["version = 4"]),
            changed("fuzz/fuzz_targets/t.rs", 1, &["#[test]", "fn a_fuzz_test() {}"]),
            changed("fuzz/Cargo.lock", 1, &["version = 4"]),
        ];
        let read = tree(&[
            (test, "#[test]\nfn uses_the_dependency() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
            (source, "fn fixed() {}\n#[test]\nfn a_vendor_test() {}\n"),
        ]);
        let Plan::Separable(baseline) = partition(&files, &read) else {
            panic!("a workspace test over a vendor dependency is separable");
        };
        assert_eq!(baseline.revert, [String::from(source)]);
        assert_eq!(baseline.test_files, [String::from(test)]);
        assert_eq!(
            baseline.build_inputs,
            [String::from("vendor/dep/Cargo.toml"), String::from("vendor/dep/Cargo.lock")]
        );
    }
}

/// `github.com/telekom/sutura#414`: `sources_under`, the step between the census and the
/// feature scan, carries a refusal through rather than an empty listing that would let
/// *nothing was enabled* pass unread. The scan itself reads the real repository and has no cell.
#[test]
fn sources_under_carries_a_census_refusal_through_and_keeps_only_that_directory() {
    let refused: super::features::Listing = Err(String::from("the census refused"));
    assert_eq!(
        super::sources_under(&refused, "crates/x"),
        Err(String::from("the census refused"))
    );
    let listed: super::features::Listing = Ok(vec![
        String::from("crates/x/src/lib.rs"),
        String::from("crates/xy/src/lib.rs"),
    ]);
    assert_eq!(
        super::sources_under(&listed, "crates/x"),
        Ok(vec![String::from("crates/x/src/lib.rs")])
    );
}

/// `github.com/telekom/sutura#837` direction 2, half one: a complete declaration on an
/// inseparable diff is EVALUATED, and a mutation that kills by the cell's own assertion is
/// accepted.
///
/// NOT DISCRIMINATING ALONE, stated rather than hidden: the old `Plan::NotSeparable`
/// fallback (`report_not_separable`) also answers `Verdict::Pass`, unconditionally, so this
/// assertion by itself cannot be shown red-on-base - confirmed by hand: neutralising the
/// dispatch guard below with `&& false` leaves this test green. The causal evidence that the
/// NEW dispatch, not the old fallback, is what answers here is the CONTRAST with the two
/// tests after it: the same fixture shape with an invalid declaration
/// (`Mutation::DoesNotKill`, `Mutation::Missing`) answers `Verdict::Fail` under the same
/// neutralising mutation - reversed to green - which an unconditional fallback could never
/// do. A single arm sensitive to the patch's validity is not reachable through one that is
/// not.
#[test]
fn a_declared_claim_cell_is_evaluated_from_an_inseparable_plan() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::Kills, None),
        Verdict::Pass,
        "a complete declaration on an inseparable diff must be consulted, and its killing \
             mutation accepted"
    );
}

/// The arm is reached, not merely declared: a mutation that applies but does not kill is
/// refused rather than passing by the old unconsulted route (`Verdict::Pass`, unconditionally,
/// before this decision). CONFIRMED BY HAND: neutralising the dispatch guard with `&& false`
/// turns this back into `Verdict::Pass` - this is one of the two tests that discriminate the
/// new dispatch from the old fallback.
#[test]
fn an_inseparable_claim_cell_whose_mutation_does_not_kill_is_refused() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::DoesNotKill, None),
        Verdict::Fail,
        "reaching the arm and finding the mutation does not kill must refuse - the old route \
             answered `Verdict::Pass` unconditionally here"
    );
}

/// `github.com/telekom/sutura#837` direction 2's own guard: a declaration with no committed
/// patch is `Cause::MissingPatch`, not treated as though nothing were declared. CONFIRMED BY
/// HAND: neutralising the dispatch guard with `&& false` turns this back into
/// `Verdict::Pass` too - the second of the two discriminating tests.
#[test]
fn an_inseparable_claim_cell_with_no_committed_patch_is_refused_as_missing() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::Missing, None),
        Verdict::Fail,
        "a declared cell with no patch must refuse as missing - the old route answered \
             `Verdict::Pass` unconditionally here"
    );
}

#[test]
fn an_inseparable_claim_does_not_hide_an_ordinary_test() {
    assert_eq!(
        inseparable_claim_case(
            true,
            Mutation::Kills,
            Some("    #[test]\n    fn the_ordinary_one() { assert_eq!(super::g(), 1); }"),
        ),
        Verdict::Fail,
        "the claimed mutation cannot prove a neighbouring ordinary test in the same inseparable file"
    );
}

/// `github.com/telekom/sutura#970`: the patch is committed and no trailer declares it, so no
/// run would ever apply it - refused, where the base answered the NOT MECHANICALLY SEPARABLE pass.
#[test]
fn a_committed_mutation_no_trailer_declares_is_refused() {
    assert_eq!(inseparable_claim_case(false, Mutation::Kills, None), Verdict::Fail);
}

/// Half two, and UNCHANGED behaviour: with no `Claim-Cell:` trailer at all, an inseparable
/// plan still answers exactly the non-verdict it did before this decision. Not provable by
/// causality - the base tree already gives this same answer, by construction - so this pins
/// by inspection rather than by a mutation: `causality::run`'s `if let Some(..) = claim` guard
/// is `None` here and the new code path is never entered at all.
#[test]
fn an_inseparable_plan_with_no_declaration_stays_the_same_non_verdict() {
    assert_eq!(
        inseparable_claim_case(false, Mutation::Missing, None),
        Verdict::Pass,
        "no `Claim-Cell:` trailer at all must still be the loud NOT MECHANICALLY SEPARABLE pass"
    );
}

#[test]
fn only_a_tree_that_did_not_compile_is_asked_twice() {
    let one = String::from("crates/x/src/pinned.rs");
    let held = BaseState {
        restore: vec![&one],
        remove: Vec::new(),
        stub: &[],
    };
    let nothing_held = BaseState {
        restore: Vec::new(),
        remove: Vec::new(),
        stub: &[],
    };
    // The case the second attempt exists for.
    assert!(retry_with_held_back(&BaseOutcome::DidNotCompile, &held));
    // Nothing is held at HEAD, so a second attempt would reconstruct the same tree and fail
    // the same way. One build, not two.
    assert!(!retry_with_held_back(&BaseOutcome::DidNotCompile, &nothing_held));
    // A retry whose only change is a stubbed entry - the crate's `src/lib.rs` held at HEAD the
    // first time, kept the second only by a test file - still builds a different tree.
    let entry = [String::from("crates/x/src/lib.rs")];
    let only_stubbed = BaseState {
        restore: Vec::new(),
        remove: Vec::new(),
        stub: &entry,
    };
    assert!(retry_with_held_back(&BaseOutcome::DidNotCompile, &only_stubbed));
    // A run that reached an assertion has ANSWERED. Retrying would revert the implementation
    // whose absence that assertion just measured, turning evidence into a different question.
    assert!(!retry_with_held_back(
        &BaseOutcome::RedByAssertion { failed: Vec::new() },
        &held
    ));
    assert!(!retry_with_held_back(&BaseOutcome::Green, &held));
}

/// A claim-only exhaustive match follows its reverted enum without dropping ordinary proof.
#[test]
fn an_accepted_claim_body_can_follow_the_existing_held_retry() {
    use std::process::Command;

    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this fixture changes cwd; run under `just test`"
    );
    let original = std::env::current_dir().unwrap();
    for (case, before_extra, after_extra, visibility, observed, expected) in [
        ("body", "", "", "", 2, Verdict::Pass),
        ("module", "mod child;\n", "mod child;\n", "", 2, Verdict::Fail),
        (
            "helper",
            "fn helper() { every_failure_is_unauthorized(); }\n",
            "fn helper() { every_failure_is_unauthorized(); }\n",
            "",
            2,
            Verdict::Fail,
        ),
        ("import", "", "use super::observed;\n", "", 2, Verdict::Fail),
        ("public", "", "", "pub ", 2, Verdict::Fail),
        ("green", "", "", "", 1, Verdict::Fail),
        ("unkilled", "", "", "", 2, Verdict::Fail),
        (
            "samefile",
            "",
            "#[test]\nfn ordinary_beside_claim() { assert_eq!(super::observed(), 2); }\n",
            "",
            2,
            Verdict::Fail,
        ),
    ] {
        let dir = std::env::temp_dir().join(format!("sutura-claim-held-{}-{case}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(dir.join("src/invariant")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        let output = |args: &[&str]| {
            let mut command = Command::new("git");
            crate::repo::strip_git_env(&mut command);
            command.current_dir(&dir).args(args).output().expect("git runs")
        };
        let git = |args: &[&str]| {
            let out = output(args);
            assert!(
                out.status.success(),
                "{case}: git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "test"]);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[profile.ci]\ninherits = \"dev\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
        let production = concat!(
            "pub enum Failure { Unauthorized }\n",
            "pub fn code(failure: &Failure) -> &'static str {\n",
            "    match failure { Failure::Unauthorized => \"unauthorized\" }\n",
            "}\n",
            "pub fn observed() -> u8 { 1 }\n",
            "#[cfg(test)]\nmod invariant;\n",
        );
        let invariant = format!(
            "use super::Failure;\n#[test]\n{visibility}fn every_failure_is_unauthorized() {{\n    let every = [Failure::Unauthorized];\n    for failure in every {{\n        match failure {{ Failure::Unauthorized => {{}} }}\n        assert_eq!(super::code(&failure), \"unauthorized\");\n    }}\n}}\n"
        );
        std::fs::write(dir.join("src/lib.rs"), production).unwrap();
        std::fs::write(dir.join("src/invariant.rs"), format!("{invariant}{before_extra}")).unwrap();
        std::fs::write(dir.join("src/invariant/child.rs"), "").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "feat: establish failure vocabulary"]);
        let base = String::from_utf8(output(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_owned();
        let head = production
            .replace("Unauthorized }", "Unauthorized, TokenTooLong }")
            .replace("Failure::Unauthorized =>", "Failure::Unauthorized | Failure::TokenTooLong =>")
            .replace("{ 1 }", &format!("{{ {observed} }}"));
        let changed = invariant
            .replace("[Failure::Unauthorized]", "[Failure::Unauthorized, Failure::TokenTooLong]")
            .replace("Failure::Unauthorized =>", "Failure::Unauthorized | Failure::TokenTooLong =>");
        std::fs::write(dir.join("src/lib.rs"), &head).unwrap();
        std::fs::write(dir.join("src/invariant.rs"), format!("{changed}{after_extra}")).unwrap();
        std::fs::write(
            dir.join("tests/ordinary.rs"),
            format!("#[test]\nfn observes_new_behavior() {{ assert_eq!(wired::observed(), {observed}); }}\n"),
        )
        .unwrap();
        // A production-only mutation must kill the claim's own assertion before any retry.
        std::fs::write(dir.join(".old.rs"), &head).unwrap();
        let replacement = if case == "unkilled" {
            "\"unauthorized\" "
        } else {
            "\"broken\""
        };
        std::fs::write(dir.join(".new.rs"), head.replace("\"unauthorized\"", replacement)).unwrap();
        let patch = String::from_utf8(output(&["diff", "--no-index", "--", ".old.rs", ".new.rs"]).stdout)
            .unwrap()
            .replace(".old.rs", "src/lib.rs")
            .replace(".new.rs", "src/lib.rs");
        std::fs::remove_file(dir.join(".old.rs")).unwrap();
        std::fs::remove_file(dir.join(".new.rs")).unwrap();
        std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
        std::fs::write(dir.join("devco/claim-mutations/every_failure_is_unauthorized.patch"), patch).unwrap();
        git(&["add", "-A"]);
        git(&[
            "commit",
            "-q",
            "-m",
            "feat: extend failure vocabulary\n\nClaim-Cell: every_failure_is_unauthorized",
        ]);
        std::env::set_current_dir(&dir).unwrap();
        let verdict = super::run(&[String::from("--since"), base]);
        std::env::set_current_dir(&original).unwrap();
        drop(std::fs::remove_dir_all(&dir));
        assert_eq!(verdict, expected, "{case}");
    }
}

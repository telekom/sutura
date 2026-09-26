use super::{Caller, Cause, Claim, MANIFEST, Repo, Scan, Verdict};
use crate::causality::place::AddedTest;

#[test]
fn a_claim_cell_in_a_mixed_file_reaches_the_kill_step() {
    let manifest = format!("{MANIFEST}\n[profile.ci]\ninherits = \"dev\"\n");
    let repo = Repo::with("Cargo.toml", &manifest);
    repo.write("flake.nix", "{ }\n");
    repo.commit("mark the miniature repo root");
    repo.write(
        "src/lib.rs",
        "pub fn answer() -> u8 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn mixed_cell() { assert_eq!(super::answer(), 1); }\n}\n",
    );
    repo.write(
        "devco/claim-mutations/mixed_cell.patch",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,4 +1,4 @@\n-pub fn answer() -> u8 { 1 }\n+pub fn answer() -> u8 { 2 }\n #[cfg(test)]\n mod tests {\n     #[test]\n",
    );
    repo.commit("add a mixed claim cell");
    let declaring = repo.commit_hash();
    repo.write("tests/pure.rs", "#[test]\nfn pure_cell() {}\n");
    repo.commit("add a pure test beside it");
    let files = crate::causality::diff::commit_additions(&repo.dir, &repo.commit_hash()).expect("commit diff");
    let read = |path: &str| std::fs::read_to_string(repo.dir.join(path)).ok();
    let scope = match Scan::of(&files, &[String::from("tests/pure.rs")], &read) {
        Scan::Runnable(scoped) => scoped,
        other => panic!("expected a pure test scope, got {other:?}"),
    };
    assert_eq!(scope.tests().iter().map(AddedTest::name).collect::<Vec<_>>(), ["pure_cell"]);
    let claim = Claim {
        cells: vec![String::from("mixed_cell")],
        by_commit: vec![(declaring, vec![String::from("mixed_cell")])],
    };
    assert_eq!(
        super::super::validate(&repo.dir, &claim, &[String::from("tests/pure.rs")]),
        Vec::<Cause>::new()
    );
    assert_eq!(
        super::super::run(
            &repo.dir,
            &scope,
            &[String::from("tests/pure.rs")],
            &claim,
            Caller::TEST_CAUSALITY
        ),
        Verdict::Pass,
    );
}

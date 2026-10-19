//! The falsifier seed reaches the rule it names, not an earlier refusal.
//!
//! `check-refusal-coverage`'s seed declares one enum per enrolled subject. A seed whose variant
//! count differs from the registry's is refused by [`Declared::read`] before the coverage rule
//! runs, so the sweep's `Fail` would hold for the wrong reason.

use super::declared::Declared;
use super::registry::ENROLLED;
use super::{excuses, name_evidence, problems};

#[test]
fn the_falsifier_seed_leaves_t6_as_the_one_uncovered_variant() {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "the evidence scan reads the working directory: run under just test for one process per test"
    );
    let task = crate::tasks()
        .find(|task| task.name == "check-refusal-coverage")
        .expect("the gate is registered");
    let tree = crate::falsifier::falsifier_tree();
    crate::falsifier::apply_seeds(&tree, &task.falsifier);

    let original = std::env::current_dir().expect("a current directory");
    std::env::set_current_dir(&tree).expect("enter the seeded tree");
    let found: Vec<_> = ENROLLED
        .iter()
        .map(|subject| {
            let outcome = Declared::read(subject, &tree).and_then(|declared| {
                let excused = excuses(subject, &tree)?;
                let evidence = name_evidence(subject, &tree, &declared).map_err(|why| why.describe())?;
                Ok(problems(subject, &declared, &excused, &evidence))
            });
            (subject.name, outcome)
        })
        .collect();
    std::env::set_current_dir(original).expect("restore the working directory");
    drop(std::fs::remove_dir_all(&tree));

    let uncovered = "T6: no test names it and no snapshot records it";
    let floor = "6 variant(s) with evidence plus 0 excused does not account for the 7 TlsNotUsable declares";
    for (name, outcome) in &found {
        let problems = outcome
            .as_ref()
            .unwrap_or_else(|why| panic!("{name}: the seed is refused before the coverage rule runs: {why}"));
        if *name == "TlsNotUsable" {
            assert!(
                matches!(problems.as_slice(), [first, second] if first.starts_with(uncovered) && second.starts_with(floor)),
                "{name}: expected T6 uncovered and the floor that follows from it, got {problems:?}"
            );
        } else {
            assert!(problems.is_empty(), "{name}: the seed leaves problems: {problems:?}");
        }
    }
}

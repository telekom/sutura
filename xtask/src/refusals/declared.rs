//! The variant list this gate may act on, and the two derivations that have to agree on its length.
//!
//! A gate over an enum's variants has one interesting failure: it narrows its own subject and keeps
//! printing `ok`. `xtask/src/repo/census.rs` records the shape - a floor counted off the same walk
//! as the loop moves with it - and the remedy here is the same one: the count is **minted by the
//! walk and compared against a number written somewhere else**. The enrolment table declares how
//! many variants each subject has; this module walks the declaration and refuses unless the two
//! agree. A `.take(n)` on the walk is therefore a failure and not a smaller `ok`.
//!
//! [`Declared`]'s field is private and [`Declared::read`] is its only production constructor, so no
//! caller can mint a variant list, and [`Declared::count`] is the length of what the walk read
//! rather than a number a caller states. **It does NOT stop a caller shortening the sequence it
//! iterates**: [`Declared::names`] hands out a slice, exactly the return shape
//! `check-newtype-leaks` refuses for a sealed witness type, so this type is deliberately not one.
//! What catches a narrowed adjudication loop is the reconciliation floor in `super::problems`,
//! which is what `names`' own doc says. **And this is closed over PRODUCTION code only**: the
//! `#[cfg(test)]` fixture below builds a list from names, which is what lets the decision function
//! be driven over shapes a real tree has not got.
//!
//! An enrolment of ZERO variants is a compile error rather than a runtime refusal: [`variants`] is a
//! `const fn` whose zero arm panics, and a `const` item that evaluates it does not build.
//!
//! # The set, and reaching every member of it
//!
//! [`Enrolled`] is the other half, and it exists because the count `super::over` used to keep was
//! **minted before the work it attested to**: the loop incremented a counter and then called the
//! per-subject check, so one `continue` between the two removed a subject from the gate's work
//! while the floor still reconciled - measured `just hygiene` exit 0 with `NotValidated` absent
//! from the output entirely, whole suite green. `xtask/src/repo/accounting.rs` records the class.
//! So the loop lives in [`Enrolled::each`], the witness is one verdict PUSHED PER RETURN, and the
//! closure's return type is [`crate::Verdict`] rather than `()` - there is no arm in which a
//! visitor can decline. [`Enrolled::declared`] names `super::ENROLLED` itself and the field is
//! private to this module, so `super::run` has no argument to narrow either.

use std::num::NonZeroUsize;
use std::path::Path;

use crate::Verdict;

/// One enrolled enum: where it is declared, how many variants it declares, and its own exception
/// namespace.
pub(super) struct Subject {
    /// The exact enum identifier used in declarations and test-code evidence.
    pub(super) name: &'static str,
    /// A missing declaration fails rather than silently narrowing the scope.
    pub(super) declared_in: &'static str,
    /// Exceptions belong only to this enum, even when variant names overlap.
    pub(super) allow_file: &'static str,
    /// The variant count, enrolled by hand.
    ///
    /// **This is the second derivation and the whole reason the count is a witness.** It also makes
    /// the enrolment unable to go stale in one direction: a variant added to or removed from the
    /// enum is a failure until this number moves with it, which is a visible diff in the enrolment
    /// table. The OTHER direction - a refusal-shaped enum nobody enrolled - is held by nothing;
    /// `crate::refusals`' own module doc is where that is argued.
    pub(super) variants: NonZeroUsize,
}

/// The enrolled variant count, refused at COMPILE time when zero.
///
/// A subject enrolled with no variants would be a gate checking a list nobody declared - the state
/// `report` cannot detect, because every variant of an empty list is named. `panic!` inside a
/// `const fn` reached from a `const` item is a compile error, so the bad state is unrepresentable
/// rather than reported.
#[expect(
    clippy::panic,
    reason = "const-evaluated: a zero enrolment fails the build, and this is the only shape that refuses one without unwrap"
)]
pub(super) const fn variants(count: usize) -> NonZeroUsize {
    match NonZeroUsize::new(count) {
        Some(nonzero) => nonzero,
        None => panic!("a subject is enrolled over at least one variant"),
    }
}

/// The variants one enrolled enum declares, minted by the walk that read them.
#[derive(Debug)]
pub(super) struct Declared {
    /// Private, so nothing outside this module can shorten the list a verdict is counted off.
    names: Vec<String>,
}

impl Declared {
    /// Walk `subject`'s declaration and require the walk and the enrolment to agree.
    ///
    /// Four refusals, each naming what is wrong: the file cannot be read, it no longer declares the
    /// enum, the body has unbalanced braces, and the walk found a different number of variants than
    /// the enrolment declares - which covers an empty walk, since an enrolment cannot be zero.
    pub(super) fn read(subject: &Subject, root: &Path) -> Result<Self, String> {
        let Subject {
            name,
            declared_in,
            variants: enrolled,
            ..
        } = subject;
        let declaration = format!("pub enum {name} {{");
        let path = root.join(declared_in);
        let text = std::fs::read_to_string(&path).map_err(|cause| format!("{declared_in} could not be read: {cause}"))?;
        let at = text
            .find(&declaration)
            .ok_or_else(|| format!("{declared_in} no longer declares `{declaration}` - this gate would check nothing"))?;
        let body = enum_body(&text, at.saturating_add(declaration.len()))
            .ok_or_else(|| format!("{declared_in}: the {name} body has unbalanced braces"))?;
        let names = variant_names(body);
        if names.len() != enrolled.get() {
            return Err(format!(
                "{declared_in}: {name} declares {} variant(s) and the enrolment in xtask/src/refusals.rs \
                 declares {} - update the enrolled count in the same diff, so this gate cannot check a \
                 shorter list than the enum has",
                names.len(),
                enrolled.get()
            ));
        }
        Ok(Self { names })
    }

    /// How many variants the walk read. Nothing else can state this number.
    pub(super) const fn count(&self) -> usize {
        self.names.len()
    }

    /// The variant names, for the evidence scan. Narrowing this loses evidence and fails.
    pub(super) fn names(&self) -> &[String] {
        &self.names
    }

    /// Is `variant` one of this enum's variants? The other direction of the allow file's ratchet.
    pub(super) fn declares(&self, variant: &str) -> bool {
        self.names.iter().any(|name| name == variant)
    }

    /// A fixture list, so the decision function can be driven over shapes a real tree has not got.
    #[cfg(test)]
    pub(super) fn for_tests(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| String::from(*name)).collect(),
        }
    }
}

/// The enrolled subjects, and the reach of the loop over them - as a witness, not a slice.
///
/// The field is private to this module and there is no accessor, so `super::run` cannot hand
/// `super::over` a shorter set and `super::over` cannot write the loop. That is
/// `crate::repo::accounting::Offered`'s argument at the scale of this gate's own subject list;
/// this type is separate only because a subject here is a `&Subject` rather than a path.
pub(super) struct Enrolled<'a> {
    /// The subjects, in the order the gate will check them.
    subjects: &'a [&'a Subject],
}

impl Enrolled<'static> {
    /// The production set. It NAMES the enrolment rather than accepting one, which is the whole
    /// point: a narrowed argument has nowhere to be written at the call site.
    pub(super) const fn declared() -> Self {
        Self {
            subjects: &super::ENROLLED,
        }
    }
}

/// A fixture set, so the gate can be driven over subjects a real tree has not got.
#[cfg(test)]
impl<'a> Enrolled<'a> {
    pub(super) const fn of(subjects: &'a [&'a Subject]) -> Self {
        Self { subjects }
    }
}

impl Enrolled<'_> {
    /// Check every enrolled subject, and refuse unless every one of them was reached.
    ///
    /// **The witness is the length of what the loop PUSHED**, and a verdict is pushed only when
    /// `check` has returned one - so a `continue` written in this loop is a refusal rather than a
    /// shorter `ok`. The visitor returns a [`Verdict`] rather than `()`, so it has no arm in which
    /// to decline a subject and let the push happen anyway.
    ///
    /// The caller never receives the sequence: this folds and returns ONE verdict, so a
    /// `.take(n)` on the fold has nowhere to be written either.
    ///
    /// ONE refusal with two disjuncts, for the reason `super::over`'s call site records: an empty
    /// enrolment, which every other check here reads as trivially covered, and a loop that did not
    /// reach every subject. The first has a provocation (`of(&[])`); the second is arrangeable only
    /// by a diff, and the gate refuses when that diff exists - which is what changed. They are one
    /// condition so that neutralising the refusal reddens the empty-set test rather than nothing.
    pub(super) fn each(self, mut check: impl FnMut(&Subject) -> Verdict) -> Result<Verdict, String> {
        let mut verdicts: Vec<Verdict> = Vec::with_capacity(self.subjects.len());
        for subject in self.subjects {
            verdicts.push(check(subject));
        }
        if verdicts.is_empty() || verdicts.len() != self.subjects.len() {
            return Err(format!(
                "{} of {} enrolled enum(s) were checked - an empty enrolment, or a loop that did \
                 not reach every subject",
                verdicts.len(),
                self.subjects.len()
            ));
        }
        Ok(if verdicts.iter().all(|verdict| *verdict == Verdict::Pass) {
            Verdict::Pass
        } else {
            Verdict::Fail
        })
    }
}

/// The text of the enum body that starts just after its opening brace at `from`.
fn enum_body(text: &str, from: usize) -> Option<&str> {
    let rest = text.get(from..)?;
    let mut depth = 1_usize;
    for (at, character) in rest.char_indices() {
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return rest.get(..at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The top-level variant names in an enum body.
///
/// Depth-counted, so a field named `Something` inside a struct variant is not a variant, and
/// indentation is not what decides. A doc comment or an attribute line has no identifier followed
/// by `{`, `(` or `,` at depth zero, so neither reads as one.
fn variant_names(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0_usize;
    for line in body.lines() {
        let trimmed = line.trim();
        if depth == 0
            && let Some(name) = variant_at(trimmed)
        {
            names.push(String::from(name));
        }
        depth = depth
            .saturating_add(trimmed.matches('{').count())
            .saturating_sub(trimmed.matches('}').count());
    }
    names
}

/// The variant name this line declares, or `None`.
fn variant_at(trimmed: &str) -> Option<&str> {
    let mut characters = trimmed.char_indices();
    let (_, first) = characters.next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let end = trimmed
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(trimmed.len());
    let name = trimmed.get(..end)?;
    let rest = trimmed.get(end..)?.trim_start();
    // `Name {`, `Name(`, `Name,` and a trailing `Name` are the four shapes a variant is written in.
    (rest.is_empty() || rest.starts_with('{') || rest.starts_with('(') || rest.starts_with(',')).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{Declared, Subject, enum_body, variant_names, variants};

    /// A fixture root of this module's own.
    ///
    /// `falsifier_tree` keys on the process id and removes the directory first, so two tests
    /// sharing a process would delete each other's tree. One process per test is what `just test`
    /// gives, and this says so rather than failing mysteriously under a bare `cargo test`.
    fn fixture_tree() -> std::path::PathBuf {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this fixture owns a pid-keyed tree: run under just test for one process per test"
        );
        crate::falsifier::falsifier_tree()
    }

    /// A subject over a fixture file, so these tests do not depend on the real tree's enums.
    fn subject(declared_in: &'static str, count: usize) -> Subject {
        Subject {
            name: "Fixture",
            declared_in,
            allow_file: "devco/does-not-exist",
            variants: variants(count),
        }
    }

    #[test]
    fn the_walk_and_the_enrolment_have_to_agree_on_the_count() {
        let tree = fixture_tree();
        let path = "crates/example/src/fixture.rs";
        std::fs::create_dir_all(tree.join("crates/example/src")).expect("fixture directories");
        std::fs::write(tree.join(path), "pub enum Fixture {\n    Alpha,\n    Beta,\n    Gamma,\n}\n").expect("fixture content");
        let agreed = Declared::read(&subject(path, 3), &tree).expect("three enrolled, three declared");
        assert_eq!(agreed.count(), 3);
        // The narrowing this pair exists to catch: a walk that read fewer than the enum has, and an
        // enrolment left behind by a new variant, are the same disagreement from opposite sides.
        let short = Declared::read(&subject(path, 2), &tree).expect_err("two enrolled, three declared");
        assert!(
            short.contains("declares 3 variant(s)") && short.contains("declares 2"),
            "{short}"
        );
        let long = Declared::read(&subject(path, 4), &tree).expect_err("four enrolled, three declared");
        assert!(
            long.contains("declares 3 variant(s)") && long.contains("declares 4"),
            "{long}"
        );
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
    }

    /// A named declaration edit and whether it left a readable variant list.
    type Edit<'a> = (&'a str, Option<&'a str>);

    #[test]
    fn a_declaration_that_moved_vanished_or_emptied_is_a_refusal() {
        let tree = fixture_tree();
        let path = "crates/example/src/fixture.rs";
        std::fs::create_dir_all(tree.join("crates/example/src")).expect("fixture directories");
        let cases: &[Edit<'_>] = &[
            ("absent", None),
            ("renamed", Some("pub enum Renamed {\n    Alpha,\n}\n")),
            ("empty", Some("pub enum Fixture {}\n")),
            ("unbalanced", Some("pub enum Fixture {\n    Alpha,\n")),
        ];
        let passed: Vec<&str> = cases
            .iter()
            .filter(|(_, text)| {
                match *text {
                    Some(text) => std::fs::write(tree.join(path), text).expect("fixture content"),
                    None => drop(std::fs::remove_file(tree.join(path))),
                }
                Declared::read(&subject(path, 1), &tree).is_ok()
            })
            .map(|(name, _)| *name)
            .collect();
        assert!(passed.is_empty(), "unreadable declarations passed: {passed:?}");
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
    }

    #[test]
    fn the_variant_walk_reads_the_top_level_only() {
        let body = "\n    /// A doc comment.\n    MetricUnknown { metric: MetricName },\n    Bare,\n    Tuple(u8),\n    Nested {\n        Inner: u8,\n    },\n";
        assert_eq!(
            variant_names(body),
            vec![
                String::from("MetricUnknown"),
                String::from("Bare"),
                String::from("Tuple"),
                String::from("Nested")
            ]
        );
    }

    #[test]
    fn an_attribute_or_a_comment_is_not_a_variant() {
        let body = "\n    #[error(\"x\")]\n    // A note.\n    /// Doc.\n    Real,\n";
        assert_eq!(variant_names(body), vec![String::from("Real")]);
    }

    #[test]
    fn the_enum_body_ends_at_its_own_brace() {
        let text = "pub enum E {\n    A { b: u8 },\n}\nfn after() {}\n";
        let body = enum_body(text, "pub enum E {".len()).expect("balanced");
        assert!(body.contains('A'), "{body}");
        assert!(!body.contains("after"), "{body}");
    }
}

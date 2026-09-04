//! Which tests the diff added, and the nextest filter that runs exactly those.
//!
//! WHY THE RUN IS SCOPED AT ALL. The base run was `--workspace` unfiltered, so its verdict was a
//! property of the WHOLE suite rather than of the tests the diff added. With nextest's fail-fast,
//! one unrelated failing cell early in the run is enough to answer "red on base" - and the tests
//! actually under test never run. Measured on a real branch: the base run died on two cells after
//! 86 of 1810 tests, and the gate answered *red on base, green on head*, which reads as *this
//! change is causal* about tests nothing had executed. Scoping is the half that stops the
//! unrelated failure from happening; `super::base` is the half that stops one being read as
//! evidence if it happens anyway.
//!
//! FAIL CLOSED ON AN EMPTY SCAN, and that is what [`Scoped`] is for. A scan naming no test may
//! not fall back to "no filter", because the unfiltered run IS the defect above. So the
//! non-empty set is a TYPE rather than a check somebody remembers to write at the call site:
//! [`Scoped::of`] answers `None`, and the gate refuses instead of measuring the suite.
//!
//! WHAT IT DOES NOT REACH. A name is read from an ADDED test attribute and the function under
//! it, so a body-only edit inside an existing `#[test]` names nothing here. [`adds_test`] also
//! accepts an added `#[cfg(test)]` or `mod tests {` marker, which names no function - so a diff
//! adding only a marker is a file the plan calls a test file and this cannot name. That is the
//! empty scan, and it is a refusal rather than a wider run. Both attribute lists live in this one
//! module because a name this cannot extract from a marker `adds_test` accepts is exactly the
//! disagreement that would reopen the unfiltered run.

use crate::causality::diff::ChangedFile;
use crate::causality::regions::{AddedLine, PostImage};

/// Attributes that mark the function below them as a test.
///
/// Deliberately syntactic and deliberately generous: a false positive costs a slower gate, a
/// false negative lets a vacuous test through, so the bias goes one way on purpose. Written
/// without the closing `]` where an argument list is legal, so `#[tokio::test(flavor = "..")]`
/// is recognised too.
const DECLARES_A_TEST: &[&str] = &["#[test]", "#[tokio::test", "#[rstest", "#[test_case"];

/// One test the diff added, by function name.
///
/// A newtype that PARSES, and [`Scoped::filterset`] is the reason: a name reaches nextest inside
/// a regular expression, so one carrying a metacharacter would widen the filter or break it
/// rather than fail visibly. A Rust function name cannot carry one; anything that is not one does
/// not get through this constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestName(String);

impl TestName {
    /// The name, if `raw` is an ASCII Rust function name.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut chars = raw.chars();
        let leading = chars.next()?;
        if !leading.is_ascii_alphabetic() && leading != '_' {
            return None;
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        Some(Self(String::from(raw)))
    }

    /// The name as written.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The tests a diff added: at least one, by construction.
#[derive(Debug)]
pub(crate) struct Scoped(Vec<TestName>);

impl Scoped {
    /// The tests the `provable` files added, or `None` when the scan names none.
    ///
    /// `provable` is the plan's own list - a file carrying both an implementation change and its
    /// tests is excluded there, and its tests are not part of the proof, so they are not part of
    /// the run either.
    pub(crate) fn of(files: &[ChangedFile], provable: &[String], read: &PostImage<'_>) -> Option<Self> {
        let mut names: Vec<TestName> = Vec::new();
        for file in files.iter().filter(|file| provable.contains(&file.path)) {
            let Some(text) = read(&file.path) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            for name in file.added.iter().filter_map(|added| declared_under(&lines, added)) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        (!names.is_empty()).then_some(Self(names))
    }

    /// The names, for comparing a failure against the set under test.
    pub(crate) fn names(&self) -> &[TestName] {
        &self.0
    }

    /// The nextest filter expression that runs exactly these tests.
    ///
    /// A test's nextest name is its module path (`tests::postgres::sums`), and a parametrised one
    /// carries a case suffix one segment BELOW the function (`tests::sums::case_1`) - so each
    /// pattern anchors the name to a `::` boundary at both ends. A plain substring match would
    /// pull in every neighbour whose name merely contains this one, which is how a scoped run
    /// quietly becomes a wide one.
    pub(crate) fn filterset(&self) -> String {
        self.0
            .iter()
            .map(|name| format!("test(/(?:^|::){}(?:::|$)/)", name.as_str()))
            .collect::<Vec<String>>()
            .join(" + ")
    }
}

/// Does this diff hunk add a test?
///
/// The marker forms - `#[cfg(test)]` and a new `mod tests` - answer yes and name nothing, which
/// is deliberate: they decide whether a file is a test file, and the module header says what an
/// unnamed one costs.
pub(crate) fn adds_test(added: &[AddedLine]) -> bool {
    added.iter().any(|line| {
        let trimmed = line.text.trim();
        declares_a_test(trimmed)
            || trimmed.starts_with("#[cfg(test)]")
            || (trimmed.starts_with("mod tests") && trimmed.contains('{'))
    })
}

/// Is this line one of the attributes that names the test below it?
fn declares_a_test(trimmed: &str) -> bool {
    DECLARES_A_TEST.iter().any(|attribute| trimmed.starts_with(attribute))
}

/// The test an added attribute line declares, read out of the post-image below it.
///
/// The post-image rather than the added set, because the attribute and its function are two
/// lines and only one of them has to be new: appending `#[test]` above an existing helper, or
/// adding the attribute and the signature in one hunk, both have to name the same test.
fn declared_under(lines: &[&str], added: &AddedLine) -> Option<TestName> {
    if !declares_a_test(added.text.trim()) {
        return None;
    }
    // `number` is 1-based, so skipping that many lands on the line AFTER the attribute.
    let declaration = lines
        .iter()
        .skip(added.number)
        .map(|line| line.trim())
        .find(|trimmed| !sits_between(trimmed))?;
    function_name(declaration)
}

/// Blank, a comment or another attribute: the things that legitimately sit between an attribute
/// and the function it applies to. Anything else ends the search, so a stray attribute does not
/// reach down the file and name an unrelated test.
fn sits_between(trimmed: &str) -> bool {
    trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[")
}

/// The name in `fn NAME(`, if this line declares a function.
///
/// One line rather than a parser: a test's signature is written on one line in this tree, and the
/// shapes that occur are `fn`, `async fn` and a visibility in front of either.
fn function_name(line: &str) -> Option<TestName> {
    let declared = line.split_whitespace().skip_while(|word| *word != "fn").nth(1)?;
    TestName::parse(declared.split(['(', '<', ':']).next()?)
}

#[cfg(test)]
mod tests {
    use super::{Scoped, TestName, adds_test};
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, tree};
    use crate::causality::regions::{AddedLine, PostImage};

    /// The names `Scoped::of` extracts, as plain strings.
    fn scoped_names(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> Option<Vec<String>> {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        Scoped::of(files, &owned, read).map(|scoped| scoped.names().iter().map(|name| String::from(name.as_str())).collect())
    }

    #[test]
    fn recognises_added_tests() {
        let added = |text: &str| vec![AddedLine::new(1, text)];
        assert!(adds_test(&added("    #[test]")));
        assert!(adds_test(&added("#[tokio::test]")));
        assert!(adds_test(&added("#[cfg(test)]")));
        assert!(adds_test(&added("mod tests {")));
        assert!(!adds_test(&added("fn thing() {}")));
    }

    #[test]
    fn the_base_run_is_scoped_to_the_tests_the_diff_added() {
        // The property the whole module exists for: the run names the ADDED test and nothing
        // else. `existing` is unchanged context in the same module and must not be scoped in,
        // because a verdict about it is a verdict about the suite rather than the change.
        let file = concat!(
            "pub fn open() -> u8 {\n",       // 1
            "    1\n",                       // 2
            "}\n",                           // 3
            "#[cfg(test)]\n",                // 4
            "mod tests {\n",                 // 5
            "    #[test]\n",                 // 6
            "    fn existing() {}\n",        // 7
            "    #[test]\n",                 // 8
            "    fn added_one() {}\n",       // 9
            "    #[tokio::test]\n",          // 10
            "    async fn added_two() {}\n", // 11
            "}\n",                           // 12
        );
        let files = vec![changed(
            "crates/x/src/a.rs",
            8,
            &[
                "    #[test]",
                "    fn added_one() {}",
                "    #[tokio::test]",
                "    async fn added_two() {}",
            ],
        )];
        let read = tree(&[("crates/x/src/a.rs", file)]);
        assert_eq!(
            scoped_names(&files, &["crates/x/src/a.rs"], &read),
            Some(vec![String::from("added_one"), String::from("added_two")])
        );
    }

    #[test]
    fn a_marker_with_no_test_function_names_nothing() {
        // The fail-closed shape. `#[cfg(test)]` makes `adds_test` answer yes and names no
        // function, so the scan comes back empty - and empty has to be unrepresentable rather
        // than "run everything", which is the unfiltered run this module replaced.
        let files = vec![changed("crates/x/src/a.rs", 2, &["#[cfg(test)]"])];
        let read = tree(&[("crates/x/src/a.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n")]);
        assert_eq!(scoped_names(&files, &["crates/x/src/a.rs"], &read), None);
    }

    #[test]
    fn a_file_not_in_the_proof_is_not_scanned() {
        // A file carrying both an implementation change and its tests is excluded from the proof
        // by the plan. Its tests must not reach the run either: they cannot be red on base,
        // because their own implementation is never reverted.
        let held = concat!(
            "fn fixed() -> u8 { 2 }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn held() {}\n",
            "}\n"
        );
        let files = vec![changed("crates/x/src/held.rs", 4, &["    #[test]", "    fn held() {}"])];
        let read = tree(&[("crates/x/src/held.rs", held)]);
        assert_eq!(scoped_names(&files, &[], &read), None);
    }

    #[test]
    fn the_filterset_anchors_each_name_at_both_ends() {
        // A substring match is how a scoped run becomes a wide one: `sums` would pull in
        // `sums_by_month`. The boundary at the END has to allow `::`, because a parametrised
        // case sits one segment below the function.
        let files = vec![changed("crates/x/tests/t.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[("crates/x/tests/t.rs", "#[test]\nfn sums() {}\n")]);
        let scoped = Scoped::of(&files, &[String::from("crates/x/tests/t.rs")], &read).expect("one test");
        assert_eq!(scoped.filterset(), "test(/(?:^|::)sums(?:::|$)/)");
    }

    #[test]
    fn two_tests_are_one_expression() {
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &["#[test]", "fn one() {}", "#[test]", "fn two() {}"],
        )];
        let read = tree(&[("crates/x/tests/t.rs", "#[test]\nfn one() {}\n#[test]\nfn two() {}\n")]);
        let scoped = Scoped::of(&files, &[String::from("crates/x/tests/t.rs")], &read).expect("two tests");
        assert_eq!(
            scoped.filterset(),
            "test(/(?:^|::)one(?:::|$)/) + test(/(?:^|::)two(?:::|$)/)"
        );
    }

    #[test]
    fn a_name_that_is_not_a_function_name_is_refused() {
        // What keeps a regular expression out of the filter expression.
        assert!(TestName::parse("sums_by_month").is_some());
        assert!(TestName::parse("_private").is_some());
        assert!(TestName::parse("").is_none());
        assert!(TestName::parse("9lives").is_none());
        assert!(TestName::parse("sums|.*").is_none());
        assert!(TestName::parse("two words").is_none());
    }

    #[test]
    fn a_stray_attribute_does_not_reach_down_the_file() {
        // The attribute is the last line of its module, so there is no function under it. Naming
        // the next test in the file would scope in something the diff did not add.
        let file = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "}\n",
            "#[test]\n",
            "fn elsewhere() {}\n"
        );
        let files = vec![changed("crates/x/src/a.rs", 3, &["    #[test]"])];
        let read = tree(&[("crates/x/src/a.rs", file)]);
        assert_eq!(scoped_names(&files, &["crates/x/src/a.rs"], &read), None);
    }
}

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
//! A BARE FUNCTION NAME IS NOT A KEY IN THIS TREE, and believing it was is the defect this module
//! last carried. Measured on nextest 0.9.143 over a synthetic workspace: `test(/(?:^|::)sums/)`
//! matched six tests in three packages, including a MODULE called `sums` in a package the diff
//! never touched. In this workspace 23 of 1782 test-function names are duplicated -
//! `deserialization_goes_through_the_constructor` occurs four times in `sutura-domain` alone. So a
//! test is keyed by three things, all of which its FILE settles: the binary or package it compiles
//! into, the module path the file contributes, and the function name. `package(=..) & test(/../)`
//! is the filter that follows, and [`AddedTest::claims`] is the same key applied to a failure the
//! run reported.
//!
//! WHAT THE KEY STILL DOES NOT SEPARATE, measured rather than guessed. Every one of the 1849
//! tests `just test` lists satisfies the key derived from its own declaring file - so the
//! derivation never produces a filter that matches nothing. Separation is the other direction and
//! is only partial: of 41 duplicated names, 16 are told apart and **25 are not**, and every one of
//! the 25 is a `macro_rules!` body expanded into several modules of ONE file
//! (`crates/sutura-app/tests/golden/`, where `mod $name {` is generated four times per dialect).
//! No prefix read from a path or a declaration can separate those; only running nextest and asking
//! could, which is the same filter. So the pattern allows any nesting below the file
//! (`(?:.*::)?`), and a name shared between a module and its own DESCENDANT in one package is not
//! discriminated either: a test added in `src/model.rs` also accepts
//! `model::qualified::tests::<same name>`.
//!
//! What that residual can and cannot do: a collided failure only manufactures a false green if it
//! is red on base AND green on head, which means the diff changed ITS behaviour - so the change is
//! causal and the misattribution is to the wrong test name. The vacuous added test riding along is
//! the defect, and it is now confined to one file's macro-generated modules rather than the whole
//! workspace.
//!
//! FAIL CLOSED ON AN EMPTY SCAN, and that is what [`Scoped`] is for. A scan naming no test may
//! not fall back to "no filter", because the unfiltered run IS the defect above. So the
//! non-empty set is a TYPE rather than a check somebody remembers to write at the call site:
//! [`Scan::of`] answers [`Scan::Unnamed`], and the gate refuses instead of measuring the suite.
//!
//! AN `#[ignore]`d TEST IS NAMED AND DROPPED, because a filterset naming only ignored tests
//! matches nothing and nextest exits 4 with *error: no tests to run* - a false RED on legitimate
//! work, and this tree holds 12 such attributes in 3 files
//! (`git grep -c -E '^[[:space:]]*#\[ignore' -- '*.rs'`, 2026-09-05; the unanchored form answers 23
//! in 7, because most `#[ignore` here is a doc comment ABOUT one).
//! Running them is the wrong direction for the reason a tier-backed cell is not required in the
//! reconstructed worktree
//! (see [`super::nextest`]): they are ignored because this venue lacks what they need, so forcing
//! them in a tree nothing provisioned fails CLOSED and reads as red-on-base. So they leave the
//! scope, and a diff whose every added test is ignored gets [`Scan::OnlyIgnored`] - a statement
//! that this gate has not verified the change, not a claim that the extractor is broken.
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
use crate::changes::package_name;

/// Attributes that mark the function below them as a test.
///
/// Deliberately syntactic and deliberately generous: a false positive costs a slower gate, a
/// false negative lets a vacuous test through, so the bias goes one way on purpose. Written
/// without the closing `]` where an argument list is legal, so `#[tokio::test(flavor = "..")]`
/// is recognised too.
const DECLARES_A_TEST: &[&str] = &["#[test]", "#[tokio::test", "#[rstest", "#[test_case"];

/// One Rust identifier: a test function's name, or one segment of a module path.
///
/// A newtype that PARSES, and [`AddedTest::term`] is the reason: this reaches nextest inside a
/// regular expression, so one carrying a metacharacter would widen the filter or break it rather
/// than fail visibly. A Rust identifier cannot carry one; anything that is not one does not get
/// through this constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ident(String);

impl Ident {
    /// The identifier, if `raw` is an ASCII Rust one.
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

    /// The identifier as written.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A cargo package or target name.
///
/// Parses for the same reason [`Ident`] does - it reaches nextest inside a filter expression -
/// but it is a different alphabet: cargo allows `-`, which Rust does not, and `sutura-domain` and
/// `multi_player` are both real names here. It is NOT a superset of `Ident` in intent, so the two
/// stay separate types rather than one lenient one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoName(String);

impl CargoName {
    /// The name, if `raw` is one cargo could have accepted.
    fn parse(raw: &str) -> Option<Self> {
        let mut chars = raw.chars();
        let leading = chars.next()?;
        if !leading.is_ascii_alphanumeric() && leading != '_' {
            return None;
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return None;
        }
        Some(Self(String::from(raw)))
    }

    /// The name as written.
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which test binary a test compiles into - the coarsest half of the key.
///
/// Measured binary-id shapes on nextest 0.9.143: a package's lib unit tests are `<package>`, an
/// integration target is `<package>::<target>`, and a bin's unit tests are `<package>::bin/<name>`.
/// A file under `tests/` at the top level IS a target, so its id is exact. A file under `src/`
/// could be compiled into the lib's binary or a bin's and its path does not say which, so the
/// qualifier there is the package.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Binary {
    /// Any binary of this package.
    Package(CargoName),
    /// Exactly one integration target: `tests/<target>.rs`.
    Target(CargoName, CargoName),
}

impl Binary {
    /// The nextest predicate that selects it.
    fn predicate(&self) -> String {
        match *self {
            Self::Package(ref package) => format!("package(={})", package.as_str()),
            Self::Target(ref package, ref target) => {
                format!("binary_id(={}::{})", package.as_str(), target.as_str())
            }
        }
    }

    /// Could a failure nextest attributes to `id` have come from here?
    fn holds(&self, id: &str) -> bool {
        match *self {
            Self::Package(ref package) => id
                .strip_prefix(package.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with("::")),
            Self::Target(ref package, ref target) => {
                id.strip_prefix(package.as_str()).and_then(|rest| rest.strip_prefix("::")) == Some(target.as_str())
            }
        }
    }
}

/// The module path a test file contributes, from its crate or target root.
///
/// `model::qualified` for `crates/sutura-domain/src/model/qualified/tests.rs`. Empty for a crate
/// root, for a target root, and for any file whose place in the module tree its path does not
/// settle - `tests/golden/catalogs.rs` is reached by a `#[path]` attribute from another file, so
/// the path is not the answer there and this claims nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Module(String);

impl Module {
    /// The module path of a file at `inner`, relative to the `src/` it sits under.
    ///
    /// `None` when any segment is not an identifier, which is the conservative direction: no
    /// prefix rather than a wrong one.
    fn of(inner: &str) -> Option<Self> {
        let stem = inner.strip_suffix(".rs")?;
        // `a/b/mod.rs` IS module `a::b`, declared one level up.
        let stem = stem.strip_suffix("/mod").unwrap_or(stem);
        let segments: Option<Vec<Ident>> = stem.split('/').map(Ident::parse).collect();
        let joined = segments?
            .iter()
            .map(|segment| String::from(segment.as_str()))
            .collect::<Vec<String>>()
            .join("::");
        Some(Self(joined))
    }

    /// The module one segment names.
    fn named(segment: &Ident) -> Self {
        Self(String::from(segment.as_str()))
    }

    /// The prefix a test path under this module begins with: `model::qualified::`, or empty.
    fn prefix(&self) -> String {
        if self.0.is_empty() {
            String::new()
        } else {
            format!("{}::", self.0)
        }
    }

    /// `path` with this module's prefix removed, or `None` when it does not begin with it.
    fn strip<'a>(&self, path: &'a str) -> Option<&'a str> {
        if self.0.is_empty() {
            return Some(path);
        }
        path.strip_prefix(self.0.as_str())?.strip_prefix("::")
    }
}

/// One test the diff added, as a key that identifies it in a run's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddedTest {
    binary: Binary,
    within: Module,
    name: Ident,
}

impl AddedTest {
    /// The filter expression term that runs exactly this test.
    ///
    /// Parenthesised rather than relying on `&` binding tighter than `+`, because the whole
    /// expression is built by joining terms and a precedence surprise here is a silently wider
    /// run rather than an error.
    fn term(&self) -> String {
        format!(
            "({} & test(/^{}(?:.*::)?{}(?:::|$)/))",
            self.binary.predicate(),
            self.within.prefix(),
            self.name.as_str()
        )
    }

    /// Is the failure nextest reported as `binary_id`/`path` this test?
    ///
    /// The SAME key the filter uses, applied by us rather than by nextest - so it catches a
    /// filter that stopped filtering (a version whose expression syntax moved, a predicate this
    /// tree spells differently) and not a key that is wrong. Two enforcers of one key, which is
    /// what defence in depth is here; it is not two independent keys, and claiming otherwise is
    /// how this comparison came to accept a name-collided failure.
    ///
    /// `binary_id` is absent for the `cargo test` wording, which prints no binary id - so the
    /// coarse half of the key cannot be checked there and is not. That path is only reached when
    /// the runner is not nextest at all.
    pub(crate) fn claims(&self, binary_id: Option<&str>, path: &str) -> bool {
        if binary_id.is_some_and(|id| !self.binary.holds(id)) {
            return false;
        }
        self.within
            .strip(path)
            .is_some_and(|rest| rest.split("::").any(|segment| segment == self.name.as_str()))
    }
}

/// The tests a diff added: at least one, by construction.
#[derive(Debug)]
pub(crate) struct Scoped(Vec<AddedTest>);

impl Scoped {
    /// The keys, for comparing a failure against the set under test.
    pub(crate) fn tests(&self) -> &[AddedTest] {
        &self.0
    }

    /// The nextest filter expression that runs exactly these tests.
    pub(crate) fn filterset(&self) -> String {
        self.0.iter().map(AddedTest::term).collect::<Vec<String>>().join(" + ")
    }
}

/// What scanning the diff's test files found.
#[derive(Debug)]
pub(crate) enum Scan {
    /// Tests this venue can run, so there is something to measure.
    Runnable(Scoped),
    /// Every test the diff added is `#[ignore]`d. Named, and unreachable by any run here.
    OnlyIgnored(Vec<Ident>),
    /// No test could be named at all.
    Unnamed,
}

impl Scan {
    /// The tests the `provable` files added.
    ///
    /// `provable` is the plan's own list - a file carrying both an implementation change and its
    /// tests is excluded there, and its tests are not part of the proof, so they are not part of
    /// the run either.
    pub(crate) fn of(files: &[ChangedFile], provable: &[String], read: &PostImage<'_>) -> Self {
        let mut runnable: Vec<AddedTest> = Vec::new();
        let mut ignored: Vec<Ident> = Vec::new();
        for file in files.iter().filter(|file| provable.contains(&file.path)) {
            let Some(text) = read(&file.path) else {
                continue;
            };
            let Some(place) = place(&file.path, read) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            for declared in file.added.iter().filter_map(|added| declared_under(&lines, added)) {
                match declared {
                    Declared::Ignored(name) => {
                        if !ignored.contains(&name) {
                            ignored.push(name);
                        }
                    }
                    Declared::Runs(name) => {
                        let one = AddedTest {
                            binary: place.binary.clone(),
                            within: place.within.clone(),
                            name,
                        };
                        if !runnable.contains(&one) {
                            runnable.push(one);
                        }
                    }
                }
            }
        }
        if !runnable.is_empty() {
            return Self::Runnable(Scoped(runnable));
        }
        if ignored.is_empty() {
            Self::Unnamed
        } else {
            Self::OnlyIgnored(ignored)
        }
    }
}

/// Where a test file's tests land: which binary, and under what module path.
struct Place {
    binary: Binary,
    within: Module,
}

/// Resolve `path` to the binary and module path its tests carry.
///
/// `None` when no ancestor `Cargo.toml` declares a package, which means cargo compiles nothing
/// from this file and it has no tests to run. The package is read rather than derived from the
/// directory name, because the two differ in this workspace: `dev/` is package `sutura-dev`.
fn place(path: &str, read: &PostImage<'_>) -> Option<Place> {
    let (package, dir) = owning_package(path, read)?;
    let rest = path.get(dir.len()..)?.trim_start_matches('/');
    if let Some(inner) = rest.strip_prefix("tests/") {
        // A top-level `tests/<stem>.rs` IS a target, so its binary id is exact.
        if let Some(target) = inner
            .strip_suffix(".rs")
            .filter(|stem| !stem.contains('/'))
            .and_then(CargoName::parse)
        {
            return Some(Place {
                binary: Binary::Target(package, target),
                within: Module::default(),
            });
        }
        // Anything deeper is a submodule of one, and which one under what name is a DECLARATION
        // rather than a path. Falling back to the package alone is what this tree needed reading:
        // 25 of its 41 duplicated test names were conflated by that fallback, nearly all of them
        // in `crates/sutura-app/tests/golden/`.
        return Some(included_by(&package, dir, inner, read).unwrap_or_else(|| Place {
            binary: Binary::Package(package),
            within: Module::default(),
        }));
    }
    // A crate root and a `src/bin/<name>.rs` are both roots: their module path is empty, and
    // deriving one from the path would produce `bin::<name>`, which no test carries.
    let within = rest
        .strip_prefix("src/")
        .filter(|inner| !matches!(*inner, "lib.rs" | "main.rs") && !inner.starts_with("bin/"))
        .and_then(Module::of)
        .unwrap_or_default();
    Some(Place {
        binary: Binary::Package(package),
        within,
    })
}

/// The target that includes a file deeper under `tests/`, and the module name it arrives as.
///
/// cargo compiles only the TOP level of `tests/` as targets, so `tests/golden/dialects.rs` is a
/// submodule of one - and which one, under what name, is settled by the declaration reaching it
/// rather than by its path. `tests/golden.rs` carries `#[path = "golden/dialects.rs"] mod
/// dialects;`, so the binary is `<package>::golden` and the module is `dialects`. Reading the
/// DECLARATION rather than guessing from the path is the pattern `regions::declared_under_cfg_test`
/// already uses, and for the same reason: `golden.rs`'s own comment records that a bare
/// `mod dialects;` there would resolve to `tests/dialects.rs` instead, so the name and the path
/// are genuinely independent and a guess would be a filter matching nothing.
///
/// Only the target named by the FIRST segment is consulted. A file that some OTHER target also
/// pulls in by `#[path]` is therefore keyed to this one - which NARROWS the filter rather than
/// widening it, so the failure direction is a loud `RedOutsideTheDiff` and never a false green.
/// `tests/support/mod.rs`, which two bigquery targets share, has no `tests/support.rs` above it
/// and so falls back to the package.
fn included_by(package: &CargoName, dir: &str, inner: &str, read: &PostImage<'_>) -> Option<Place> {
    let (first, _) = inner.split_once('/')?;
    let target = CargoName::parse(first)?;
    let text = read(&in_dir(dir, &format!("tests/{first}.rs")))?;
    Some(Place {
        binary: Binary::Target(package.clone(), target),
        within: Module::named(&declared_at(&text, inner)?),
    })
}

/// The module name `text` gives the file at `inner` through a `#[path]` declaration.
fn declared_at(text: &str, inner: &str) -> Option<Ident> {
    let attribute = format!("#[path = \"{inner}\"]");
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.iter().position(|line| line.trim() == attribute)?;
    lines
        .iter()
        .skip(at + 1)
        .map(|line| line.trim())
        .find(|trimmed| !sits_between(trimmed))
        .and_then(module_name)
}

/// The name in `mod NAME;`, if this line declares an out-of-line module.
fn module_name(line: &str) -> Option<Ident> {
    let declared = line.split_whitespace().skip_while(|word| *word != "mod").nth(1)?;
    Ident::parse(declared.strip_suffix(';')?)
}

/// `rel` under `dir`, where an empty `dir` is the repo root.
fn in_dir(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        String::from(rel)
    } else {
        format!("{dir}/{rel}")
    }
}

/// The package owning `path`, and that package's directory - empty for one at the repo root.
///
/// The nearest ancestor directory whose `Cargo.toml` declares a `[package]` name, walking up.
/// `changes::package_name` reads the manifest, because the TOML shape is the part that could rot
/// and one reader for it is enough; the WALK differs - this one goes through the post-image reader
/// so the resolution is testable without a checkout.
fn owning_package<'p>(path: &'p str, read: &PostImage<'_>) -> Option<(CargoName, &'p str)> {
    let mut dir = path;
    loop {
        dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        if let Some(text) = read(&in_dir(dir, "Cargo.toml"))
            && let Some(name) = package_name(&text)
            && let Some(package) = CargoName::parse(&name)
        {
            return Some((package, dir));
        }
        if dir.is_empty() {
            return None;
        }
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

/// A test an added attribute declares, and whether a run in this venue reaches it.
enum Declared {
    /// A test that runs here.
    Runs(Ident),
    /// `#[ignore]`d, so no filter can make it run and naming it in one matches nothing.
    Ignored(Ident),
}

/// The test an added attribute line declares, read out of the post-image below it.
///
/// The post-image rather than the added set, because the attribute and its function are two
/// lines and only one of them has to be new: appending `#[test]` above an existing helper, or
/// adding the attribute and the signature in one hunk, both have to name the same test.
fn declared_under(lines: &[&str], added: &AddedLine) -> Option<Declared> {
    if !declares_a_test(added.text.trim()) {
        return None;
    }
    // `number` is 1-based, so skipping that many lands on the line AFTER the attribute.
    let (index, declaration) = lines
        .iter()
        .enumerate()
        .skip(added.number)
        .map(|(index, line)| (index, line.trim()))
        .find(|&(_, trimmed)| !sits_between(trimmed))?;
    let name = function_name(declaration)?;
    Some(if is_ignored(lines, index) {
        Declared::Ignored(name)
    } else {
        Declared::Runs(name)
    })
}

/// Does the attribute block attached to the function at `index` carry an `#[ignore]`?
///
/// Walks UP over the contiguous attributes and comments, because `#[ignore]` is legal on either
/// side of `#[test]` and only the upward walk sees both. A blank line ends the block, so an
/// attribute belonging to an earlier item cannot be borrowed. `#[cfg_attr(.., ignore)]` is not
/// recognised - no such spelling exists in this tree, and the direction of missing one is the
/// `no tests to run` failure this dropping exists to prevent, which is loud.
fn is_ignored(lines: &[&str], index: usize) -> bool {
    lines.get(..index).is_some_and(|above| {
        above
            .iter()
            .rev()
            .map(|line| line.trim())
            .take_while(|trimmed| trimmed.starts_with("#[") || trimmed.starts_with("//"))
            .any(|trimmed| trimmed.starts_with("#[ignore"))
    })
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
fn function_name(line: &str) -> Option<Ident> {
    let declared = line.split_whitespace().skip_while(|word| *word != "fn").nth(1)?;
    Ident::parse(declared.split(['(', '<', ':']).next()?)
}

#[cfg(test)]
mod tests {
    use super::{AddedTest, CargoName, Ident, Scan, adds_test, place};
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, tree};
    use crate::causality::regions::{AddedLine, PostImage};

    /// A manifest declaring one package, as the post-image reader will hand it back.
    fn manifest(name: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion.workspace = true\n")
    }

    /// The names `Scan::of` found runnable, as plain strings.
    fn runnable(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> Option<Vec<String>> {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        match Scan::of(files, &owned, read) {
            Scan::Runnable(scoped) => Some(scoped.tests().iter().map(|one| String::from(one.name.as_str())).collect()),
            _ => None,
        }
    }

    /// The filterset for the tests `provable` added.
    fn filterset(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> String {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        match Scan::of(files, &owned, read) {
            Scan::Runnable(scoped) => scoped.filterset(),
            other => panic!("expected runnable tests, got {other:?}"),
        }
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
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/src/a.rs"], &read),
            Some(vec![String::from("added_one"), String::from("added_two")])
        );
    }

    #[test]
    fn a_marker_with_no_test_function_names_nothing() {
        // The fail-closed shape. `#[cfg(test)]` makes `adds_test` answer yes and names no
        // function, so the scan comes back empty - and empty has to be unrepresentable rather
        // than "run everything", which is the unfiltered run this module replaced.
        let files = vec![changed("crates/x/src/a.rs", 2, &["#[cfg(test)]"])];
        let read = tree(&[
            ("crates/x/src/a.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert!(matches!(
            Scan::of(&files, &[String::from("crates/x/src/a.rs")], &read),
            Scan::Unnamed
        ));
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
        let read = tree(&[("crates/x/src/held.rs", held), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(runnable(&files, &[], &read), None);
    }

    #[test]
    fn the_filterset_qualifies_a_name_by_the_package_and_the_module_it_sits_in() {
        // THE DEFECT. A bare `test(/(?:^|::)sums(?:::|$)/)` matched six tests in three packages
        // on nextest 0.9.143 - including a MODULE called `sums` in another package - so a
        // name-collided failure was accepted as this test's red. This tree has 23 duplicated
        // test-function names, four of one name in `sutura-domain` alone. The package and the
        // file's module path are both in the term, and both come from the file's own path.
        let files = vec![changed(
            "crates/sutura-domain/src/model/qualified/tests.rs",
            1,
            &["#[test]", "fn sums() {}"],
        )];
        let read = tree(&[
            ("crates/sutura-domain/src/model/qualified/tests.rs", "#[test]\nfn sums() {}\n"),
            ("crates/sutura-domain/Cargo.toml", &manifest("sutura-domain")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/sutura-domain/src/model/qualified/tests.rs"], &read),
            "(package(=sutura-domain) & test(/^model::qualified::tests::(?:.*::)?sums(?:::|$)/))"
        );
    }

    #[test]
    fn an_integration_target_is_qualified_by_its_exact_binary_id() {
        // `tests/<stem>.rs` IS a cargo target, so nextest's id for it is exactly
        // `<package>::<stem>` - measured on 0.9.143. That separates it from a same-named unit
        // test in the same package's lib, which `package(=..)` alone would pull in.
        let files = vec![changed("crates/x/tests/served.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[
            ("crates/x/tests/served.rs", "#[test]\nfn sums() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/x/tests/served.rs"], &read),
            "(binary_id(=x::served) & test(/^(?:.*::)?sums(?:::|$)/))"
        );
    }

    #[test]
    fn a_file_deeper_under_tests_is_keyed_by_the_declaration_that_reaches_it() {
        // cargo compiles only the top level of `tests/`, so `tests/golden/catalogs.rs` is a
        // submodule of the `golden` target - and the name it arrives under is a `#[path]`
        // declaration rather than its path. `golden.rs` records why they differ: a bare
        // `mod catalogs;` there would resolve to `tests/catalogs.rs` instead.
        //
        // Worth more than tidiness: the two tier-backed cells that produced #278's false green
        // are `sutura-app::differential`, and the golden suite is `sutura-app::golden`. Keyed by
        // the package alone, a `differential` failure could be read as a golden test's evidence.
        let target = concat!(
            "#[cfg(test)]\n",                     // 1
            "#[path = \"golden/catalogs.rs\"]\n", // 2
            "mod catalogs;\n",                    // 3
        );
        let files = vec![changed("crates/x/tests/golden/catalogs.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[
            ("crates/x/tests/golden/catalogs.rs", "#[test]\nfn sums() {}\n"),
            ("crates/x/tests/golden.rs", target),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/x/tests/golden/catalogs.rs"], &read),
            "(binary_id(=x::golden) & test(/^catalogs::(?:.*::)?sums(?:::|$)/))"
        );
    }

    #[test]
    fn a_shared_helper_no_target_declares_falls_back_to_the_package() {
        // `tests/support/mod.rs` is pulled in by two bigquery targets and has no `tests/support.rs`
        // above it, so the declaration that would name a binary is not there. The package alone is
        // the honest answer; claiming one of the two targets would be a filter matching nothing
        // half the time.
        let files = vec![changed("crates/x/tests/support/mod.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[
            ("crates/x/tests/support/mod.rs", "#[test]\nfn sums() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/x/tests/support/mod.rs"], &read),
            "(package(=x) & test(/^(?:.*::)?sums(?:::|$)/))"
        );
    }

    #[test]
    fn the_package_is_read_from_the_manifest_rather_than_the_directory_name() {
        // They differ here: `dev/` is package `sutura-dev`, so a filter built from the directory
        // name would name a package nextest does not know and match nothing.
        let files = vec![changed("dev/src/scope.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[
            ("dev/src/scope.rs", "#[test]\nfn sums() {}\n"),
            ("dev/Cargo.toml", &manifest("sutura-dev")),
        ]);
        assert_eq!(
            filterset(&files, &["dev/src/scope.rs"], &read),
            "(package(=sutura-dev) & test(/^scope::(?:.*::)?sums(?:::|$)/))"
        );
    }

    #[test]
    fn a_crate_root_and_a_mod_rs_get_the_module_path_cargo_gives_them() {
        // `lib.rs` and `main.rs` ARE the root, so a prefix from their path would be wrong.
        // `a/b/mod.rs` IS module `a::b`, not `a::b::mod`.
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/lib.rs", "\n"),
            ("crates/x/src/deep/mod.rs", "\n"),
            ("crates/x/src/deep/nested.rs", "\n"),
        ]);
        let prefix = |path: &str| place(path, &read).expect("a package").within.prefix();
        assert_eq!(prefix("crates/x/src/lib.rs"), "");
        assert_eq!(prefix("crates/x/src/deep/mod.rs"), "deep::");
        assert_eq!(prefix("crates/x/src/deep/nested.rs"), "deep::nested::");
    }

    #[test]
    fn a_path_no_package_owns_is_not_scanned() {
        // Nothing compiles it, so it has no test to run - and inventing a package name for it
        // would put a name nextest does not know into the filter.
        let files = vec![changed("stray/a.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[("stray/a.rs", "#[test]\nfn sums() {}\n")]);
        assert!(matches!(
            Scan::of(&files, &[String::from("stray/a.rs")], &read),
            Scan::Unnamed
        ));
    }

    #[test]
    fn two_tests_are_one_expression() {
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &["#[test]", "fn one() {}", "#[test]", "fn two() {}"],
        )];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn one() {}\n#[test]\nfn two() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/x/tests/t.rs"], &read),
            concat!(
                "(binary_id(=x::t) & test(/^(?:.*::)?one(?:::|$)/))",
                " + (binary_id(=x::t) & test(/^(?:.*::)?two(?:::|$)/))"
            )
        );
    }

    #[test]
    fn an_ignored_test_leaves_the_scope_rather_than_emptying_the_run() {
        // Measured on nextest 0.9.143: a filterset naming only `#[ignore]`d tests matches
        // nothing and exits 4 with `error: no tests to run`, which the gate read as a failure.
        // This tree has 23 ignored tests. `#[ignore]` is legal on either side of `#[test]`, so
        // both orders are dropped, and the runnable neighbour is still proven.
        let file = concat!(
            "#[test]\n",                      // 1
            "#[ignore = \"needs a tier\"]\n", // 2
            "fn below() {}\n",                // 3
            "#[ignore]\n",                    // 4
            "#[test]\n",                      // 5
            "fn above() {}\n",                // 6
            "#[test]\n",                      // 7
            "fn runs() {}\n",                 // 8
        );
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &[
                "#[test]",
                "#[ignore = \"needs a tier\"]",
                "fn below() {}",
                "#[ignore]",
                "#[test]",
                "fn above() {}",
                "#[test]",
                "fn runs() {}",
            ],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/tests/t.rs"], &read),
            Some(vec![String::from("runs")])
        );
    }

    #[test]
    fn a_diff_whose_every_added_test_is_ignored_is_named_not_refused() {
        // The other half of the same measurement, and the reason it is a third answer rather
        // than the empty scan: an all-`#[ignore]`d diff is not an extractor bug, so it must not
        // print one. The names come back so the report can say what it could not measure.
        let file = "#[test]\n#[ignore]\nfn acceptance() {}\n";
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &["#[test]", "#[ignore]", "fn acceptance() {}"],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
            Scan::OnlyIgnored(ref names) => {
                assert_eq!(names.iter().map(Ident::as_str).collect::<Vec<&str>>(), vec!["acceptance"]);
            }
            other => panic!("an ignored test is named, got {other:?}"),
        }
    }

    #[test]
    fn a_name_that_is_not_an_identifier_is_refused() {
        // What keeps a regular expression out of the filter expression.
        assert!(Ident::parse("sums_by_month").is_some());
        assert!(Ident::parse("_private").is_some());
        assert!(Ident::parse("").is_none());
        assert!(Ident::parse("9lives").is_none());
        assert!(Ident::parse("sums|.*").is_none());
        assert!(Ident::parse("two words").is_none());
        // A cargo name is a different alphabet: `-` is legal there and not in Rust.
        assert!(CargoName::parse("sutura-domain").is_some());
        assert!(CargoName::parse("multi_player").is_some());
        assert!(CargoName::parse("bad)name").is_none());
        assert!(CargoName::parse("has space").is_none());
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
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(runnable(&files, &["crates/x/src/a.rs"], &read), None);
    }

    #[test]
    fn a_collided_name_in_another_package_is_not_this_test() {
        // The comparison side of the same key, and the false green it closes: with the name
        // alone, `pb other::sums` and `pb sums::inner` were both accepted for a scoped `sums`,
        // so a vacuous added test rode a pre-existing failure to `ok - red on base, green on
        // head`. Reproduced on nextest 0.9.143 before the qualifier went in.
        let files = vec![changed("crates/pa/src/lib.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[
            ("crates/pa/src/lib.rs", "#[test]\nfn sums() {}\n"),
            ("crates/pa/Cargo.toml", &manifest("pa")),
        ]);
        let scan = Scan::of(&files, &[String::from("crates/pa/src/lib.rs")], &read);
        let Scan::Runnable(scoped) = scan else {
            panic!("expected one runnable test");
        };
        let one: &AddedTest = &scoped.tests()[0];
        assert!(one.claims(Some("pa"), "tests::sums"), "its own package");
        assert!(!one.claims(Some("pb"), "other::sums"), "another package's module");
        assert!(!one.claims(Some("pb"), "sums::inner"), "another package's module named sums");
        assert!(!one.claims(Some("pc::bin/pc"), "tests::sums"), "another package's bin");
    }

    #[test]
    fn a_sibling_module_in_the_same_package_is_not_this_test() {
        // `deserialization_goes_through_the_constructor` occurs four times in `sutura-domain`,
        // all in the lib's own binary - so the binary id cannot separate them and the module
        // path is what does. A test added in `src/calendar.rs` is not the one in `src/model.rs`.
        let files = vec![changed(
            "crates/sutura-domain/src/calendar.rs",
            1,
            &["#[test]", "fn deserialization_goes_through_the_constructor() {}"],
        )];
        let read = tree(&[
            (
                "crates/sutura-domain/src/calendar.rs",
                "#[test]\nfn deserialization_goes_through_the_constructor() {}\n",
            ),
            ("crates/sutura-domain/Cargo.toml", &manifest("sutura-domain")),
        ]);
        let scan = Scan::of(&files, &[String::from("crates/sutura-domain/src/calendar.rs")], &read);
        let Scan::Runnable(scoped) = scan else {
            panic!("expected one runnable test");
        };
        let one: &AddedTest = &scoped.tests()[0];
        let name = "deserialization_goes_through_the_constructor";
        assert!(one.claims(Some("sutura-domain"), &format!("calendar::tests::{name}")));
        assert!(!one.claims(Some("sutura-domain"), &format!("model::tests::{name}")));
        assert!(!one.claims(Some("sutura-domain"), &format!("definitions::tests::{name}")));
        // An rstest case sits one segment BELOW the function and is still its failure.
        assert!(one.claims(Some("sutura-domain"), &format!("calendar::tests::{name}::case_2")));
        // And a name that merely CONTAINS this one is a different test.
        assert!(!one.claims(Some("sutura-domain"), &format!("calendar::tests::{name}_too")));
    }
}

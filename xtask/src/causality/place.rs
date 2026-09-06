//! Where a test file's tests land, as the key a run's output can be matched against.
//!
//! A BARE FUNCTION NAME IS NOT A KEY IN THIS TREE, and believing it was is the defect this half
//! last carried. Measured on nextest 0.9.143 over a synthetic workspace: `test(/(?:^|::)sums/)`
//! matched six tests in three packages, including a MODULE called `sums` in a package the diff
//! never touched. So a test is keyed by three things, all of which its FILE settles: the binary or
//! package it compiles into, the module path the file contributes, and the function name.
//! `package(=..) & test(/../)` is the filter that follows, and `super::scoped::AddedTest` is where
//! that key is applied - to the filterset it builds, and to a failure the run reported.
//!
//! WHAT THE KEY STILL DOES NOT SEPARATE, measured rather than guessed, and in both directions
//! because only one of them is a false green. CONSISTENCY: every test `just test` lists satisfies
//! the key derived from its own declaring file, so the derivation never produces a filter that
//! matches nothing - that is the false-RED direction and it is clean. SEPARATION is the other one
//! and is partial: **of 41 duplicated test names, 16 are told apart and 25 are not**
//! (2026-09-05, over `just test`'s own listing of 1922). Every one of the 25 is a `macro_rules!`
//! body expanded into several modules of ONE file - `crates/sutura-app/tests/golden/`, where
//! `mod $name {` is generated once per dialect - so no prefix read from a path or a declaration
//! can separate them; only running nextest and asking could, which is the same filter. That is
//! also why the pattern allows any nesting below the file (`(?:.*::)?`), and why a name shared
//! between a module and its own DESCENDANT in one package is not discriminated: a test added in
//! `src/model.rs` also accepts `model::qualified::tests::<same name>`.
//!
//! What that residual can and cannot do: a collided failure only manufactures a false green if it
//! is red on base AND green on head, which means the diff changed ITS behaviour - so the change is
//! causal and the misattribution is to the wrong test name. The vacuous added test riding along is
//! the defect, and it is now confined to one file's macro-generated modules rather than the whole
//! workspace.
//!
//! THIS MODULE READS PATHS AND DECLARATIONS, never attributes. It is the seam `scoped.rs` was
//! split on when that file reached the unexemptable 1000-line cap, and it is a real one: nothing
//! here asks whether a line is a test, and nothing in `scoped` derives a path.

use crate::causality::attributes::{attached, item_below};
use crate::causality::diff::ChangedFile;
use crate::causality::names::{CargoName, Ident};
use crate::causality::regions::{PostImage, item_head, module_name};
use crate::changes::package_name;

/// One test the diff added, as a key that identifies it in a run's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddedTest {
    binary: Binary,
    within: Module,
    name: Ident,
}

impl AddedTest {
    /// The test `name` declares, in the file whose tests land at `at`.
    pub(super) fn at(at: &Place, name: Ident) -> Self {
        Self {
            binary: at.binary.clone(),
            within: at.within.clone(),
            name,
        }
    }

    /// The function name, for a line a PERSON reads.
    ///
    /// Not a key - this module's header carries the measurement that says so - and nothing built
    /// from this reaches nextest. `super::coverage` prints it so a reader can find a test the
    /// proof left out; the filter and the failure comparison both go through the whole key.
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The filter expression term that runs exactly this test.
    ///
    /// Parenthesised rather than relying on `&` binding tighter than `+`, because the whole
    /// expression is built by joining terms and a precedence surprise here is a silently wider
    /// run rather than an error.
    pub(super) fn term(&self) -> String {
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

/// What a file's added lines declare about a test module.
pub(super) enum Declares {
    /// `mod <name> {` - the body is in this same file, so its lines are in this diff by
    /// construction and there is no second file to look for.
    Inline,
    /// `mod <name>;` - the module's source is another file. The candidates cargo would compile,
    /// in the order [`declared_module_files`] produces them.
    OutOfLine(Vec<String>),
}

/// The test module a file that named no test declares, if this can read one.
///
/// The declaration is read from the ADDED lines, and for a bare `#[cfg(test)]` from the item under
/// it in the post-image - the same split `super::attributes::adds` makes, and for the same reason:
/// the attribute and its item are two lines and only the attribute has to be new.
///
/// An INLINE module wins over an out-of-line one when a hunk adds both, because an inline body
/// arrives with the file and is the shape that needs no second file to exist.
///
/// `None` when nothing in the added lines declares a module at all. `super::scoped` reads that as
/// a refusal: something made the file a test file and this cannot account for it.
pub(super) fn accounted_for(file: &ChangedFile, lines: &[&str]) -> Option<Declares> {
    let mut out_of_line: Option<Vec<String>> = None;
    for added in &file.added {
        let trimmed = added.text.trim();
        // `number` is 1-based, so it is the 0-based index of the line after this one.
        let (at, item) = if trimmed
            .strip_prefix("#[cfg(test)]")
            .is_some_and(|rest| rest.trim().is_empty())
        {
            item_below(lines, added.number)?
        } else {
            (added.number.saturating_sub(1), trimmed)
        };
        if !item_head(item).starts_with("mod ") {
            continue;
        }
        if item.contains('{') {
            return Some(Declares::Inline);
        }
        if let Some(name) = module_name(item) {
            out_of_line = Some(declared_module_files(&file.path, name, relocated(lines, at).as_deref()));
        }
    }
    out_of_line.map(Declares::OutOfLine)
}

/// The `#[path = ".."]` value on the declaration at 0-based `at`, if it carries one.
///
/// Read out of the attribute block `attributes::attached` resolves, so a wrapped attribute in the
/// block does not hide it. `crates/sutura-app/tests/golden.rs` writes this, and ignoring it would
/// resolve the declaration to a file that is not there - which is a refusal, so the wrong answer
/// here reddens a correct change.
fn relocated(lines: &[&str], at: usize) -> Option<String> {
    attached(lines, at).into_iter().find_map(|opening| {
        opening
            .strip_prefix("#[path")?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .strip_prefix('"')?
            .split('"')
            .next()
            .map(String::from)
    })
}

/// Which test binary a test compiles into - the coarsest half of the key.
///
/// Measured binary-id shapes on nextest 0.9.143: a package's lib unit tests are `<package>`, an
/// integration target is `<package>::<target>`, and a bin's unit tests are `<package>::bin/<name>`.
/// A file under `tests/` at the top level IS a target, so its id is exact. A file under `src/`
/// could be compiled into the lib's binary or a bin's and its path does not say which, so the
/// qualifier there is the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Binary {
    /// Any binary of this package.
    Package(CargoName),
    /// Exactly one integration target: `tests/<target>.rs`.
    Target(CargoName, CargoName),
}

impl Binary {
    /// The nextest predicate that selects it.
    pub(super) fn predicate(&self) -> String {
        match *self {
            Self::Package(ref package) => format!("package(={})", package.as_str()),
            Self::Target(ref package, ref target) => {
                format!("binary_id(={}::{})", package.as_str(), target.as_str())
            }
        }
    }

    /// Could a failure nextest attributes to `id` have come from here?
    pub(super) fn holds(&self, id: &str) -> bool {
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
pub(super) struct Module(String);

impl Module {
    /// The module path of a file at `inner`, relative to the `src/` it sits under.
    ///
    /// `None` when any segment is not an identifier, which is the conservative direction: no
    /// prefix rather than a wrong one.
    fn of(inner: &str) -> Option<Self> {
        let stem = inner.strip_suffix(".rs")?;
        // `a/b/mod.rs` IS module `a::b`, declared one level up.
        let stem = stem.strip_suffix("/mod").unwrap_or(stem);
        stem.split('/')
            .all(|segment| Ident::parse(segment).is_some())
            .then(|| Self(stem.replace('/', "::")))
    }

    /// The module one segment names.
    fn named(segment: &Ident) -> Self {
        Self(String::from(segment.as_str()))
    }

    /// The prefix a test path under this module begins with: `model::qualified::`, or empty.
    pub(super) fn prefix(&self) -> String {
        if self.0.is_empty() {
            String::new()
        } else {
            format!("{}::", self.0)
        }
    }

    /// `path` with this module's prefix removed, or `None` when it does not begin with it.
    ///
    /// Through [`Module::prefix`] so the separator rule is stated once: an empty prefix strips
    /// nothing, which is what `strip_prefix("")` already answers.
    pub(super) fn strip<'a>(&self, path: &'a str) -> Option<&'a str> {
        path.strip_prefix(self.prefix().as_str())
    }
}

/// Where a test file's tests land: which binary, and under what module path.
pub(super) struct Place {
    pub(super) binary: Binary,
    pub(super) within: Module,
}

/// Resolve `path` to the binary and module path its tests carry.
///
/// `None` when no ancestor `Cargo.toml` declares a package, which means cargo compiles nothing
/// from this file and it has no tests to run. The package is read rather than derived from the
/// directory name, because the two differ in this workspace: `dev/` is package `sutura-dev`.
pub(super) fn place(path: &str, read: &PostImage<'_>) -> Option<Place> {
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
///
/// The attribute is matched as the line rustfmt writes it, spaces included. Any other spelling
/// finds nothing and the caller falls back to the package - generous, and the direction that
/// cannot turn into a false green.
fn declared_at(text: &str, inner: &str) -> Option<Ident> {
    let attribute = format!("#[path = \"{inner}\"]");
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.iter().position(|line| line.trim() == attribute)?;
    item_below(&lines, at + 1)
        .and_then(|(_, item)| module_name(item))
        .and_then(Ident::parse)
}

/// Where the out-of-line module `name`, declared in the file at `path`, keeps its own source.
///
/// Both spellings, because either is legal and only one exists at a time. `lib.rs`, `main.rs` and
/// `<stem>/mod.rs` declare their children BESIDE themselves; any other `<stem>.rs` declares them
/// under `<stem>/`. A `#[path = ".."]` on the declaration overrides all of that, resolved against
/// the declaring file's own directory - `tests/golden.rs` uses it, so not following it would look
/// for a file that is not there.
///
/// The inverse of `regions::declared_under_cfg_test`, which walks child to parent. Written here
/// because it is the same path arithmetic `in_dir` and `owning_package` do, and because
/// `super::scoped` needed to answer a question the sentence it PRINTED had been asserting: is the
/// declared module's own file in this diff at all?
///
/// LATENT, recorded so it is not rediscovered: a `mod child;` nested INSIDE an inline `mod a {}`
/// of `lib.rs` resolves here to `child.rs` rather than to Rust's `a/child.rs`. Unreachable through
/// the current call graph - [`accounted_for`] answers [`Declares::Inline`] on the parent `mod a {`
/// first, so the nested declaration never reaches this function. It is wrong the moment something
/// else calls it.
pub(super) fn declared_module_files(path: &str, name: &str, relocated: Option<&str>) -> Vec<String> {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    if let Some(rel) = relocated {
        return vec![in_dir(dir, rel)];
    }
    let base = match file {
        "lib.rs" | "main.rs" | "mod.rs" => String::from(dir),
        stem => in_dir(dir, stem.strip_suffix(".rs").unwrap_or(stem)),
    };
    vec![in_dir(&base, &format!("{name}.rs")), in_dir(&base, &format!("{name}/mod.rs"))]
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

/// The cargo package that compiles `path`, if one does.
///
/// The same walk [`place`] already does, exposed on its own for [`super::reverted`]: whether a
/// reverted file can reach a test in scope is first a question about which package compiles each
/// of them, and deriving that a second way would be a second thing to keep in step with this
/// workspace's directory-name-is-not-the-package-name shape (`dev/` is `sutura-dev`).
pub(super) fn package(path: &str, read: &PostImage<'_>) -> Option<CargoName> {
    owning_package(path, read).map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::{AddedTest, declared_module_files, place};
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::regions::PostImage;
    use crate::causality::scoped::Scan;

    /// The filterset for the tests `provable` added, through the scan that builds the keys.
    ///
    /// The expression is what a person reads in the gate's own output, so it is asserted whole
    /// rather than field by field: a change to the pattern that still parses is exactly the shape
    /// that widens a run silently.
    fn filterset(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> String {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        match Scan::of(files, &owned, read) {
            Scan::Runnable(scoped) => scoped.filterset(),
            other => panic!("expected runnable tests, got {other:?}"),
        }
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
    fn a_declared_module_is_resolved_to_the_file_cargo_would_compile() {
        // The three layouts, because `super::scoped` compares the answer against the diff's own
        // file list: getting it wrong either refuses a correct change or passes a module of
        // pre-existing tests that just became compiled.
        assert_eq!(
            declared_module_files("crates/x/src/lib.rs", "tests", None),
            vec![
                String::from("crates/x/src/tests.rs"),
                String::from("crates/x/src/tests/mod.rs")
            ]
        );
        // A non-root file declares its children one directory DOWN, under its own stem.
        assert_eq!(
            declared_module_files("crates/x/src/model.rs", "tests", None),
            vec![
                String::from("crates/x/src/model/tests.rs"),
                String::from("crates/x/src/model/tests/mod.rs")
            ]
        );
        // `a/b/mod.rs` IS module `b`, so its children sit beside it rather than under `mod/`.
        assert_eq!(
            declared_module_files("crates/x/src/deep/mod.rs", "tests", None),
            vec![
                String::from("crates/x/src/deep/tests.rs"),
                String::from("crates/x/src/deep/tests/mod.rs")
            ]
        );
    }

    #[test]
    fn a_path_attribute_on_the_declaration_is_the_answer_rather_than_the_layout() {
        // `crates/sutura-app/tests/golden.rs` writes exactly this, so a resolver that ignored the
        // attribute would look for `tests/golden/catalogs/..` and find nothing - and "not in this
        // diff" is a refusal, so the wrong answer here reddens a correct change.
        assert_eq!(
            declared_module_files("crates/x/tests/golden.rs", "catalogs", Some("golden/catalogs.rs")),
            vec![String::from("crates/x/tests/golden/catalogs.rs")]
        );
    }
    #[test]
    fn the_filterset_qualifies_a_name_by_the_package_and_the_module_it_sits_in() {
        // THE DEFECT. A bare `test(/(?:^|::)sums(?:::|$)/)` matched six tests in three packages
        // on nextest 0.9.143 - including a MODULE called `sums` in another package - so a
        // name-collided failure was accepted as this test's red. The package and the file's module
        // path are both in the term, and both come from the file's own path.
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
        // `deserialization_goes_through_the_constructor` occurs in four of `sutura-domain`'s
        // modules, all in the lib's own binary - so the binary id cannot separate them and the
        // module path is what does. A test added in `src/calendar.rs` is not the one in
        // `src/model.rs`.
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

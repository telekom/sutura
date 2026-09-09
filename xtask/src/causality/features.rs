//! The one input this gate did not read at all: the FEATURE TABLE a diff changed.
//!
//! `super::scoped::Scan::Enabled` refuses a `.rs` diff that DECLARES a module of pre-existing
//! tests - a `#[cfg(test)] mod legacy;` added while `legacy.rs` sits untouched compiles a whole
//! module of tests that no added line names and that, because the declaring file is held at HEAD,
//! is in both trees and cannot be red on base either.
//!
//! **ITS INVERSE PASSED IN SILENCE, and the asymmetry was strict.** A pre-existing
//! `#[cfg(feature = "x")] mod tests;` whose `x` a **`Cargo.toml`-only** diff declares compiles the
//! same whole module of tests with ZERO added `.rs` lines: `super::plan` finds no changed test
//! file, answers `Plan::NotRequired`, and the gate printed *no changed tests - nothing to prove*
//! about a diff that put tests into the build. Reproduced through `plan` on
//! `github.com/telekom/sutura#319`, stated in the output by `github.com/telekom/sutura#359`, and
//! held here. `github.com/telekom/sutura#343` is the record.
//!
//! WHY DECLARING IS THE PREDICATE, and it is the only non-obvious step. "Which features are ON" is
//! ordinarily a graph over defaults and dependents - but **both of this gate's runs are
//! `--all-features`** (`super::runner::nextest`, and the test below asks the built command for its
//! arguments rather than trusting this sentence), so a feature that EXISTS in the table is enabled.
//! That collapses the graph to a set difference: under `--all-features` the only way a
//! `#[cfg(feature = ..)]` gate can flip from off to on is for the manifest to declare a name
//! **in `[features]`** that it did not declare before.
//!
//! **THAT SCOPE IS THE CLAIM, and the unqualified version of this sentence was wrong.** Review
//! worked the premise against six shapes; it holds for five - a feature whose definition changes
//! (same-crate features are already on, and adding a `dep:` reference can only turn an implicit
//! feature *off*), a `default = [..]` change (irrelevant under `--all-features`, and `default` is
//! already a key so it is not a false positive either), a feature added in a depended-on member
//! (that member's manifest is necessarily in the diff, so the subtraction sees it), a `not(..)`
//! compound (never compiled under `--all-features` at all), and a workspace-level feature (this
//! workspace's root manifest is virtual). It **fails** for the implicit feature an
//! `optional = true` dependency creates: `--all-features` enables that name, and `[features]` does
//! not carry it - the first row of the table below, and the reason the sentence has to say
//! *in `[features]`* rather than *a name*. Measured at **12** optional dependencies in this tree,
//! so the shape is live for future work rather than structurally absent.
//!
//! **So it is not a diff scan.** Nothing here looks for an added line: it reads the whole table on
//! each side of the base commit and subtracts. A `[features]` key that moved, was reformatted, or
//! arrived through a reordering is not a new feature and does not fire.
//!
//! WHICH DIRECTION EACH LIMIT FAILS IN, because the previous version of this argument was a
//! sentence in a doc comment and the measurement is what makes it safe to act on:
//!
//! | Not read | What happens | Direction |
//! | --- | --- | --- |
//! | an IMPLICIT feature from `optional = true` on a dependency | the table this reads does not carry it, so a module gated on that name reads as not newly enabled | the silent pass this module removes, for one shape of it. No such gate is in this tree - the five feature-gated `mod` declarations under `crates/`, `xtask/` and `dev/` all name a `[features]` key |
//! | an INLINE `#[cfg(feature = ..)] mod tests { .. }` | skipped: its body is in the declaring file, and asking *does this file declare a test* would fire on one declared elsewhere in it | the silent pass, and there is no inline gated module in this tree |
//! | `#[cfg(all(feature = "x", ..))]` and `cfg_attr` | only the exact `#[cfg(feature = "..")]` spelling is read, the same limit `super::attributes` states for `#[cfg(test)]` | the silent pass |
//! | a test in a SUBMODULE of the enabled module | only the module's own file is read for a test declaration | the silent pass |
//! | a nested workspace member under another member's directory | its sources would be read against the outer table | a refusal that is not the author's fault. There is none: every member manifest here is one level under `crates/`, `dev/` or the root |
//!
//! WHAT MAY NOT HAPPEN. Every changed manifest the diff carries lands in exactly one place: a
//! resolved pair of tables, or [`Unread`]. So *nothing was enabled* cannot be said about a manifest
//! whose base table was never read, and it cannot be said about a package whose sources came back
//! empty either - an empty subject set is [`Unread::Sources`] rather than a pass.
//!
//! **What holds that, at the strength it actually has**, because *the types* would be an
//! overstatement: `Activation::one` returns `Result<_, Unread>` and every path in it that has not
//! yet resolved both tables returns `Err`, so an answer of *nothing* is reachable only after both
//! were read. Nothing in the type system stops a future early `Ok(Vec::new())` from being added
//! above them - what does is
//! `tests::a_manifest_whose_base_table_could_not_be_read_is_refused_rather_than_answered`, which
//! reddens on exactly that mutation and names the input that went missing.
//!
//! WHAT IT CANNOT BE ASKED OF, so the widest reading of the scan is not the one a reader takes: a
//! manifest with no `[package]` declares no `[features]` - cargo rejects a feature table on a
//! virtual manifest - so this workspace's root `Cargo.toml` resolves to an empty set on both sides
//! and no source under it is ever listed. That is the reason the package directory can be derived
//! from the manifest path at all.

use std::collections::BTreeSet;

use super::attributes::{declares_a_test, item_below};
use super::diff::ChangedFile;
use super::place::{Declares, accounted_for};
use super::provenance::Reach;
use super::regions::{AddedLine, PostImage};

/// A declaration that puts a module of tests THIS DIFF DOES NOT CONTAIN into the build.
///
/// Lives here rather than in `super::scoped` because there are two causes now and only one of them
/// is a `.rs` diff. One refusal for one input: the earlier shape of this gate had one input with
/// two remedies, and the one that fired was the pass.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Enabled {
    /// The file carrying the declaration.
    pub(crate) path: String,
    /// Where cargo will look for the module's source. Not in this diff, which is the finding.
    pub(crate) module: String,
    /// What made it compile.
    pub(crate) because: Because,
}

/// Why a module of pre-existing tests is being compiled.
///
/// Matched exhaustively where it is printed, with no wildcard: the two causes ask for the same
/// evidence and name different things, and a third would need its own sentence rather than
/// inheriting one of these.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Because {
    /// An added `.rs` line declares the module. `super::scoped::Scan::of` is where this is found.
    Declared,
    /// A pre-existing declaration gated on a feature the changed manifest DECLARES and the base
    /// manifest did not, so the module entered the build with no added `.rs` line at all.
    Feature { manifest: String, name: String },
}

/// A changed manifest's own inputs that this scan could not read.
///
/// A refusal rather than a skip in all three cases, which is the property the header states: an
/// answer of *nothing was enabled* is a claim about tables and sources, so it may not be given
/// when one of those did not come back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unread {
    /// The manifest's post-image. Nothing can be said about a table this cannot read at HEAD.
    Manifest(String),
    /// The base commit HAS this manifest and its content did not come back, so there is no base
    /// table to subtract. A manifest the base does NOT have is a new package and its base table is
    /// legitimately empty - `super::worktree::at_base` and `base_has` are separate calls for that
    /// distinction alone.
    AtBase(String),
    /// The manifest declares a feature the base did not, and no source file under its package came
    /// back. *Nothing was enabled* would then be a statement about files this never saw.
    Sources { manifest: String, dir: String },
}

/// A file's content at the base commit, with *absent* and *unreadable* kept apart.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BaseText {
    /// The base commit does not have the file: a new package, whose base feature table is empty.
    Absent,
    /// The content at base.
    Text(String),
    /// The base has it and the content did not come back. A refusal, never an empty table.
    Unreadable,
}

/// A reader for a path's content at the base commit, beside `super::regions::PostImage` for the
/// head one.
pub(crate) type BaseImage<'reader> = dyn Fn(&str) -> BaseText + 'reader;

/// A reader for the source files under a package directory.
pub(crate) type Sources<'reader> = dyn Fn(&str) -> Vec<String> + 'reader;

/// What this scan needs of the two trees, so the classification is assertable without a repository.
pub(crate) struct Trees<'a> {
    /// The working tree's content for a repo-relative path - the post-image every other scan in
    /// this gate reads.
    pub(crate) head: &'a PostImage<'a>,
    /// The same path at the base commit.
    pub(crate) base: &'a BaseImage<'a>,
    /// Every `.rs` path the working tree has under a package directory.
    pub(crate) sources: &'a Sources<'a>,
}

/// What a diff's manifest changes put into the build.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Activation {
    /// Every changed manifest was resolved on both sides, and no feature it newly declares makes a
    /// declaration active that compiles a module of tests this diff does not contain.
    Nothing,
    /// It does, and here is each one. Refuses: the remedy is stated evidence, the same as for the
    /// `.rs` cause, because no extractor improvement can measure a test that is green in both
    /// trees.
    Enables(Vec<Enabled>),
    /// The scan could not read an input it needs, so it has no answer to give.
    Unread(Vec<Unread>),
}

impl Activation {
    /// What the changed manifests in `files` turn on.
    ///
    /// The order of the two refusals is the order `super::scoped::Scan::of` uses and for the same
    /// reason: an input this could not read makes every other answer over the same diff a verdict
    /// the gate cannot read its own inputs for.
    pub(crate) fn of(files: &[ChangedFile], trees: &Trees<'_>) -> Self {
        let changed: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        let mut enables: Vec<Enabled> = Vec::new();
        let mut unread: Vec<Unread> = Vec::new();
        for manifest in changed.iter().copied().filter(|path| is_manifest(path)) {
            match Self::one(manifest, &changed, trees) {
                Ok(found) => enables.extend(found),
                Err(why) => unread.push(why),
            }
        }
        if !unread.is_empty() {
            return Self::Unread(unread);
        }
        if enables.is_empty() {
            Self::Nothing
        } else {
            Self::Enables(enables)
        }
    }

    /// One changed manifest: the features it newly declares, and what they activate.
    ///
    /// `Err` is the only way to answer nothing without having read both tables, which is what
    /// makes the count in [`Self::of`] a witness rather than a number: a manifest contributes
    /// either declarations or a named refusal.
    fn one(manifest: &str, changed: &[&str], trees: &Trees<'_>) -> Result<Vec<Enabled>, Unread> {
        let Some(head) = (trees.head)(manifest) else {
            return Err(Unread::Manifest(String::from(manifest)));
        };
        let base = match (trees.base)(manifest) {
            BaseText::Absent => String::new(),
            BaseText::Text(text) => text,
            BaseText::Unreadable => return Err(Unread::AtBase(String::from(manifest))),
        };
        let newly: BTreeSet<String> = declared_features(&head)
            .difference(&declared_features(&base))
            .cloned()
            .collect();
        if newly.is_empty() {
            return Ok(Vec::new());
        }
        let dir = manifest.rsplit_once('/').map_or("", |(parent, _)| parent);
        let sources = (trees.sources)(dir);
        if sources.is_empty() {
            return Err(Unread::Sources {
                manifest: String::from(manifest),
                dir: String::from(dir),
            });
        }
        Ok(sources
            .iter()
            // A source the diff touched is the `.rs` route's business: its added lines are what
            // `super::plan` and `super::scoped` classify, and its base text is not this file's.
            .filter(|path| !changed.contains(&path.as_str()))
            .filter_map(|path| (trees.head)(path).map(|text| (path, text)))
            .flat_map(|(path, text)| {
                gated_declarations(path, &text)
                    .into_iter()
                    .filter(|(feature, _)| newly.contains(feature))
                    .filter_map(|(feature, candidates)| {
                        let module = candidates.into_iter().find(|candidate| {
                            !changed.contains(&candidate.as_str())
                                && (trees.head)(candidate).is_some_and(|text| declares_tests(&text))
                        })?;
                        Some(Enabled {
                            path: path.clone(),
                            module,
                            because: Because::Feature {
                                manifest: String::from(manifest),
                                name: feature,
                            },
                        })
                    })
                    .collect::<Vec<Enabled>>()
            })
            .collect())
    }
}

/// Is this changed path a package manifest?
///
/// Through [`Reach`] rather than by extension, so a vendored manifest - which nothing in this
/// workspace resolves, and which carries its own `[features]` and its own `#[test]`s - is not one.
fn is_manifest(path: &str) -> bool {
    Reach::of(path) == Reach::BuildInput && path.rsplit('/').next().unwrap_or(path) == "Cargo.toml"
}

/// The feature names a manifest DECLARES, whatever their values.
///
/// A line scan, because this workspace's `xtask` has no TOML reader and `crate::changes` reads a
/// manifest the same way - one more parser would be one more thing to keep true. What it has to
/// get right is narrower than TOML: only the keys of one table, and only for a diff that changed
/// that file.
///
/// Two shapes it does handle, because both are written here and both would produce a wrong SET:
/// a value spread over several lines (its continuation lines are not keys, so the bracket depth is
/// tracked), and a commented-out key (`# legacy = []` inside `[features]` declares nothing).
fn declared_features(manifest: &str) -> BTreeSet<String> {
    let mut declared = BTreeSet::new();
    let mut in_features = false;
    let mut open = 0_isize;
    for line in manifest.lines() {
        let text = line.trim();
        if text.starts_with('#') {
            continue;
        }
        if open == 0 && text.starts_with('[') {
            in_features = text == "[features]";
            continue;
        }
        let value = if open == 0 && in_features {
            match text.split_once('=') {
                Some((key, value)) => {
                    let name = key.trim().trim_matches('"');
                    if !name.is_empty() {
                        declared.insert(String::from(name));
                    }
                    value
                }
                None => text,
            }
        } else {
            text
        };
        open = (open + brackets(value)).max(0);
    }
    declared
}

/// How many `[` this text opens and does not close.
fn brackets(text: &str) -> isize {
    let count = |wanted: char| isize::try_from(text.chars().filter(|c| *c == wanted).count()).unwrap_or(0);
    count('[') - count(']')
}

/// A feature name, and the files cargo would compile the module it gates from.
type Gated = (String, Vec<String>);

/// The out-of-line `mod` declarations in one file that a named feature gates, each with the files
/// cargo would compile it from.
///
/// The resolution is `super::place::accounted_for`, the same one the `.rs` route uses, reached by
/// handing it the `mod` line as though it were added. One resolver for one question: a second
/// implementation of *where does this declaration's source live* is what would silently stop
/// agreeing with the first, and `#[path = ".."]` is the case where they would differ.
fn gated_declarations(path: &str, text: &str) -> Vec<Gated> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| gated_on(line.trim()).map(|feature| (index, feature)))
        .filter_map(|(index, feature)| {
            let (at, item) = item_below(&lines, index + 1)?;
            let one = ChangedFile {
                path: String::from(path),
                added: vec![AddedLine::new(at + 1, item)],
                // A SYNTHETIC file, not a diff: this stands the declaration up so
                // `super::place` can read it. Nothing was removed because nothing was diffed.
                removed: Vec::new(),
            };
            match accounted_for(&one, &lines)? {
                Declares::OutOfLine(candidates) => Some((feature, candidates)),
                // The body is in this file, so nothing arrived for the module's own file to be.
                // Stated as a limit in this module's header rather than answered here.
                Declares::Inline => None,
            }
        })
        .collect()
}

/// The feature name in `#[cfg(feature = "..")]`, if this line is exactly that.
fn gated_on(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("#[cfg(feature")?.trim_start().strip_prefix('=')?;
    let value = rest.trim_start().strip_prefix('"')?;
    let (name, tail) = value.split_once('"')?;
    (tail.trim() == ")]" && !name.is_empty()).then(|| String::from(name))
}

/// Does this file declare a test?
///
/// The same attribute list `super::attributes` reads, over the whole file rather than over added
/// lines: the file is not in the diff, so every test in it is one that was already written and is
/// only now being compiled. A comment-stripped line scan shares that module's limit - a
/// `#[test]`-shaped line inside a raw string literal would count - and the direction is a refusal
/// on a file that holds no test, which is loud.
fn declares_tests(text: &str) -> bool {
    text.lines().any(|line| declares_a_test(line.trim()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Activation, BaseText, Because, Enabled, Reach, Trees, Unread, declared_features};
    use crate::causality::fixtures::changed;
    use crate::causality::isolation::Isolated;
    use crate::causality::regions::PostImage;
    use crate::causality::runner::{Tree, nextest};

    /// A manifest declaring one package and the given feature keys.
    fn with_features(name: &str, features: &[&str]) -> String {
        let table = features.iter().fold(String::new(), |mut table, one| {
            table.push_str(one);
            table.push_str(" = []\n");
            table
        });
        format!("[package]\nname = \"{name}\"\n\n[features]\n{table}")
    }

    /// The base tree as a fixed map: a path it does not carry is [`BaseText::Absent`].
    fn at_base(files: &[(&str, &str)]) -> impl Fn(&str) -> BaseText + use<> {
        let owned: Vec<(String, String)> = files
            .iter()
            .map(|&(path, text)| (String::from(path), String::from(text)))
            .collect();
        move |wanted: &str| {
            owned
                .iter()
                .find(|(path, _)| path == wanted)
                .map_or(BaseText::Absent, |(_, text)| BaseText::Text(text.clone()))
        }
    }

    /// The `.rs` files a fixed head tree carries under `dir`.
    fn sources_of(paths: &[&str]) -> impl Fn(&str) -> Vec<String> + use<> {
        let owned: Vec<String> = paths.iter().map(|path| String::from(*path)).collect();
        move |dir: &str| {
            owned
                .iter()
                .filter(|path| path.starts_with(dir) && Reach::of(path) == Reach::Compiled)
                .cloned()
                .collect()
        }
    }

    /// The head tree, the base tree and the source listing of one scenario.
    fn scan(files: &[crate::causality::diff::ChangedFile], head: &PostImage<'_>, base: &[(&str, &str)]) -> Activation {
        let sources = sources_of(&["crates/x/src/lib.rs", "crates/x/src/legacy.rs"]);
        let at = at_base(base);
        Activation::of(
            files,
            &Trees {
                head,
                base: &at,
                sources: &sources,
            },
        )
    }

    #[test]
    fn a_manifest_only_diff_that_turns_on_a_test_module_no_longer_passes_in_silence() {
        // THE FINDING, and it is the whole reason this module exists. `legacy.rs` has held two
        // tests all along, gated behind a feature that did not exist; the diff is one line of
        // `Cargo.toml` and no `.rs` line at all. `plan` finds no changed test file, so before this
        // the gate printed *no changed tests - nothing to prove* about a diff that put two tests
        // into the build - and, because a manifest is held at HEAD rather than reverted, they are
        // in both trees and cannot be red on base either.
        let files = vec![changed("crates/x/Cargo.toml", 5, &["legacy = []"])];
        let head = crate::causality::fixtures::tree(&[
            ("crates/x/Cargo.toml", &with_features("x", &["legacy"])),
            (
                "crates/x/src/lib.rs",
                "fn f() {}\n#[cfg(feature = \"legacy\")]\nmod legacy;\n",
            ),
            (
                "crates/x/src/legacy.rs",
                "#[test]\nfn old_one() {}\n#[test]\nfn old_two() {}\n",
            ),
        ]);
        let base = [("crates/x/Cargo.toml", with_features("x", &[]))];
        let base: Vec<(&str, &str)> = base.iter().map(|(p, t)| (*p, t.as_str())).collect();
        match scan(&files, &head, &base) {
            Activation::Enables(ref found) => assert_eq!(
                *found,
                vec![Enabled {
                    path: String::from("crates/x/src/lib.rs"),
                    module: String::from("crates/x/src/legacy.rs"),
                    because: Because::Feature {
                        manifest: String::from("crates/x/Cargo.toml"),
                        name: String::from("legacy"),
                    },
                }]
            ),
            other => panic!("expected the manifest diff to be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_feature_the_base_manifest_already_declared_enables_nothing() {
        // The direction that must NOT fire, and it is the common one: a dependency bump or a
        // reformatting touches `[features]` without declaring a name. Both runs are
        // `--all-features`, so a feature that already existed was already on and the module was
        // already compiled - there is nothing new in the build to be red about. A gate that
        // reddens correct work gets disabled, which is why the predicate is the SET DIFFERENCE
        // rather than "this diff touched a feature table".
        let files = vec![changed("crates/x/Cargo.toml", 6, &["legacy = [\"dep:serde\"]"])];
        let head = crate::causality::fixtures::tree(&[
            ("crates/x/Cargo.toml", &with_features("x", &["legacy"])),
            (
                "crates/x/src/lib.rs",
                "fn f() {}\n#[cfg(feature = \"legacy\")]\nmod legacy;\n",
            ),
            ("crates/x/src/legacy.rs", "#[test]\nfn old_one() {}\n"),
        ]);
        let base = [("crates/x/Cargo.toml", with_features("x", &["legacy"]))];
        let base: Vec<(&str, &str)> = base.iter().map(|(p, t)| (*p, t.as_str())).collect();
        assert_eq!(scan(&files, &head, &base), Activation::Nothing);
    }

    #[test]
    fn a_new_feature_gating_a_module_with_no_test_is_not_this_gates_business() {
        // The narrowing that keeps the refusal honest: an ordinary feature-gated module entering
        // the build is what a feature is FOR. Only a module that declares tests is a claim this
        // gate could otherwise be read as having measured.
        let files = vec![changed("crates/x/Cargo.toml", 5, &["legacy = []"])];
        let head = crate::causality::fixtures::tree(&[
            ("crates/x/Cargo.toml", &with_features("x", &["legacy"])),
            (
                "crates/x/src/lib.rs",
                "fn f() {}\n#[cfg(feature = \"legacy\")]\nmod legacy;\n",
            ),
            ("crates/x/src/legacy.rs", "pub fn helper() {}\n"),
        ]);
        let base = [("crates/x/Cargo.toml", with_features("x", &[]))];
        let base: Vec<(&str, &str)> = base.iter().map(|(p, t)| (*p, t.as_str())).collect();
        assert_eq!(scan(&files, &head, &base), Activation::Nothing);
    }

    #[test]
    fn a_module_file_the_diff_also_carries_is_the_rs_routes_business() {
        // The two routes may not both claim one input. When the module's own file is in the diff
        // its added lines are what `plan` and `scoped` classify, and its tests are measurable the
        // ordinary way - so this scan says nothing about it.
        let files = vec![
            changed("crates/x/Cargo.toml", 5, &["legacy = []"]),
            changed("crates/x/src/legacy.rs", 1, &["#[test]", "fn added() {}"]),
        ];
        let head = crate::causality::fixtures::tree(&[
            ("crates/x/Cargo.toml", &with_features("x", &["legacy"])),
            (
                "crates/x/src/lib.rs",
                "fn f() {}\n#[cfg(feature = \"legacy\")]\nmod legacy;\n",
            ),
            ("crates/x/src/legacy.rs", "#[test]\nfn added() {}\n"),
        ]);
        let base = [("crates/x/Cargo.toml", with_features("x", &[]))];
        let base: Vec<(&str, &str)> = base.iter().map(|(p, t)| (*p, t.as_str())).collect();
        assert_eq!(scan(&files, &head, &base), Activation::Nothing);
    }

    #[test]
    fn a_manifest_whose_base_table_could_not_be_read_is_refused_rather_than_answered() {
        // THE EMPTY SUBJECT SET, at both places it can arise. Without the base table the
        // difference would be *every feature is new*, and without a source listing *nothing was
        // enabled* would be a claim about files this never saw. Both are refusals, so neither can
        // reach the caller as a pass.
        let files = vec![changed("crates/x/Cargo.toml", 5, &["legacy = []"])];
        let head = crate::causality::fixtures::tree(&[("crates/x/Cargo.toml", &with_features("x", &["legacy"]))]);
        let unreadable = |_: &str| BaseText::Unreadable;
        let sources = sources_of(&["crates/x/src/lib.rs"]);
        assert_eq!(
            Activation::of(
                &files,
                &Trees {
                    head: &head,
                    base: &unreadable,
                    sources: &sources,
                }
            ),
            Activation::Unread(vec![Unread::AtBase(String::from("crates/x/Cargo.toml"))])
        );
        // A package whose sources came back empty, with a base table that read fine.
        let base = at_base(&[("crates/x/Cargo.toml", "[package]\nname = \"x\"\n")]);
        let none = sources_of(&[]);
        assert_eq!(
            Activation::of(
                &files,
                &Trees {
                    head: &head,
                    base: &base,
                    sources: &none,
                }
            ),
            Activation::Unread(vec![Unread::Sources {
                manifest: String::from("crates/x/Cargo.toml"),
                dir: String::from("crates/x"),
            }])
        );
        // And the head manifest itself, which is the input every other answer here is derived
        // from.
        let empty = crate::causality::fixtures::tree(&[]);
        assert_eq!(
            Activation::of(
                &files,
                &Trees {
                    head: &empty,
                    base: &base,
                    sources: &sources,
                }
            ),
            Activation::Unread(vec![Unread::Manifest(String::from("crates/x/Cargo.toml"))])
        );
    }

    #[test]
    fn a_vendored_manifest_is_not_one_this_workspace_resolves() {
        // `vendor/` is `exclude`d in the root manifest and carries its own `[features]` and its own
        // `#[test]`s, so reading its table would refuse a vendor bump over tests `--workspace`
        // never builds. `provenance::Reach` owns that rule and this asks it rather than the
        // extension.
        let files = vec![changed("vendor/mimalloc_rust/Cargo.toml", 5, &["legacy = []"])];
        let head =
            crate::causality::fixtures::tree(&[("vendor/mimalloc_rust/Cargo.toml", &with_features("mimalloc", &["legacy"]))]);
        let base = at_base(&[]);
        let sources = sources_of(&[]);
        assert_eq!(
            Activation::of(
                &files,
                &Trees {
                    head: &head,
                    base: &base,
                    sources: &sources,
                }
            ),
            Activation::Nothing
        );
    }

    #[test]
    fn a_feature_table_is_read_as_keys_rather_than_as_lines() {
        // What a set difference over a line scan has to get right, and both shapes are written in
        // this workspace. A value spread over several lines contributes NO keys, so its entries do
        // not read as features - `"dep:x",` would otherwise be one. And a commented-out key
        // declares nothing, which is the difference between a feature that exists and one somebody
        // wrote down.
        let manifest = concat!(
            "[package]\nname = \"x\"\n\n",
            "[features]\n",
            "default = [\"wire\"]\n",
            "wire = [\n",
            "    \"dep:reqwest\",\n",
            "    \"dep:hyper\",\n",
            "]\n",
            "\"mock-issuer\" = []\n",
            "# legacy = []\n",
            "\n[dependencies]\n",
            "reqwest = { version = \"1\", optional = true }\n",
        );
        let declared = declared_features(manifest);
        assert_eq!(
            declared.iter().map(String::as_str).collect::<Vec<&str>>(),
            vec!["default", "mock-issuer", "wire"]
        );
        // No table at all is an empty set, not a failure: the root manifest of this workspace has
        // none, and a new package's absent base text is read the same way.
        assert!(declared_features("[package]\nname = \"x\"\n").is_empty());
        assert!(declared_features("").is_empty());
    }

    #[test]
    fn the_predicate_is_tied_to_the_flag_on_both_runs() {
        // WHY THIS IS A TEST AND NOT A SENTENCE. *A declared feature is an enabled one* holds only
        // because BOTH of this gate's runs pass `--all-features`. Drop it from either and the
        // predicate is wrong in the silent direction: a feature that exists is no longer on, so a
        // `cfg(feature)` gate can flip without the table gaining a name and this scan answers
        // `Nothing`.
        //
        // **THE FIRST VERSION OF THIS READ `runner.rs`'s SOURCE TEXT, and review broke it.** A
        // comment-stripped line scan for `--all-features` is satisfied by the literal appearing
        // anywhere in that file, whatever it is attached to - so moving the flag under
        // `if tree == Tree::Provisioned` takes it off the BASE run, which is exactly the condition
        // this rests on, and all 932 gate tests still passed. Asserting a string appears is not
        // asserting an argument is passed; `super::runner`'s own
        // `both_runs_are_filtered_to_the_tests_the_diff_added` is the house pattern, and this is
        // the same shape asked of both `Tree` values - which is the half that closes that
        // mutation, since one value alone would still have passed it.
        let isolated = Isolated::for_a_wiring_test(Path::new("/tmp/root"), Path::new("/tmp/target"));
        for tree in [Tree::Provisioned, Tree::Reconstructed] {
            let args: Vec<String> = nextest(&isolated, "test(=t)", tree)
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.iter().any(|arg| arg == "--all-features"),
                "{tree:?} must pass --all-features, or `declared_features` is the wrong predicate: {args:?}"
            );
        }
    }
}

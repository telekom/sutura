//! Tests for [`super`].
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape. The reason is
//! mechanical: the module plus these cases is over the thousand-line limit `cargo xtask max-lines`
//! enforces, and the only way past that gate is to split the file. Same arrangement as
//! `sutura_domain::catalog::tests`.

use crate::LocalCatalog;
use std::path::{Path, PathBuf};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

/// The name every test catalog is recorded under in its manifest.
fn test_name() -> SourceName {
    SourceName::parse("test").expect("a test name is a name")
}

fn catalog(root: PathBuf) -> LocalCatalog {
    LocalCatalog::new(
        test_name(),
        root,
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
    )
}

/// An empty directory of this test's own, cleared on the way IN.
///
/// `tempfile` is not a dependency of this workspace and one test is not the argument for adding
/// one; `xtask` builds its scratch directories the same way. Cleared before rather than after so
/// a failing run leaves its evidence on disk and the next run still starts clean.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-catalog-local-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// What one document loads to, given the whole document.
///
/// A catalog of exactly one file, so the error is about that file and nothing else: an empty
/// directory is its own error here, and a second document would let `Definitions::assemble` fail
/// first for a reason these tests are not about.
///
/// The whole outcome rather than the failure, because a test asserting that a document is refused
/// says nothing on its own: the same assertion passes for a document refused two guards earlier,
/// for a reason the test is not about. The twin - the same document with the one field corrected,
/// loading - is what makes it a statement about that field, and it needs the `Ok` side.
fn outcome_for(name: &str, document: &str) -> Result<crate::Content, crate::LocalCatalogError> {
    outcome_of(name, &[("doc.md", document)])
}

/// What a whole small catalog loads to.
///
/// More than one document, for the case where the failure under test is a check over the WHOLE
/// bundle rather than over one file: a note is about the definitions beside it, so the twin that
/// shows the refusal is about the note's own field needs a definition for the note to be about.
#[expect(
    clippy::unwrap_in_result,
    reason = "the panic is the scratch directory being unwritable, which is the harness failing rather than \
              a catalog being unreadable - folding it into LocalCatalogError would give every test a second \
              failure mode indistinguishable from the one it asserts"
)]
fn outcome_of(name: &str, documents: &[(&str, &str)]) -> Result<crate::Content, crate::LocalCatalogError> {
    let root = scratch(name);
    for &(file, document) in documents {
        std::fs::write(root.join(file), document).expect("a document is writable");
    }
    let outcome = catalog(root.clone()).read_all();
    drop(std::fs::remove_dir_all(&root));
    outcome
}

/// What a document failed to be, given the whole document.
fn error_for(name: &str, document: &str) -> crate::LocalCatalogError {
    outcome_for(name, document).expect_err("this document cannot load")
}

#[test]
fn the_authored_sql_example_loads_and_its_fragment_is_under_the_digest() {
    // Red on base by construction: there `MetricDoc` has no `authored_sql` key and
    // `deny_unknown_fields` refuses the example's metric document, so the load fails. What the
    // load does NOT do is compile the fragment - `sutura-catalog-local` may not reach `sutura-sql`
    // (`FORBIDDEN_EDGES` in `xtask/src/boundaries.rs`), so this asserts admission and the digest,
    // and nothing about the SQL being SQL.
    use sutura_domain::model::MetricName;
    use sutura_domain::pinned::SemanticCatalog as _;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/authored-sql/catalog");
    let pinned = catalog(root.clone()).load().expect("the authored SQL example loads");
    let name = MetricName::parse("order_value_spread").expect("the example metric has a name");
    let metric = pinned.definitions().metric(&name).expect("the example declares its metric");
    assert_eq!(metric.computation().kind(), "authored_sql");
    assert!(metric.measure().is_none());

    // The authored text is part of the definition: change one token and the digest moves, which
    // is what makes a re-pin visible when somebody edits the SQL under a certified name.
    let changed_root = scratch("authored-digest");
    let model = std::fs::read_to_string(root.join("models/orders.md")).expect("the example model is readable");
    let authored = std::fs::read_to_string(root.join("metrics/order_value_spread.md")).expect("the example metric is readable");
    std::fs::write(changed_root.join("orders.md"), model).expect("the model copy is writable");
    std::fs::write(
        changed_root.join("order_value_spread.md"),
        authored.replace("MAX(amount_cents)", "SUM(amount_cents)"),
    )
    .expect("the changed metric is writable");
    let changed = catalog(changed_root.clone())
        .load()
        .expect("the changed authored expression still loads");
    assert_ne!(pinned.digest(), changed.digest());
    drop(std::fs::remove_dir_all(changed_root));
}

/// Unix only, and that is a portability statement rather than a gap in the suite.
///
/// The walk is what these tests exercise, and provoking the case that broke it needs a symlink.
/// Creating one on Windows requires either developer mode or an elevated process, so a test that
/// created one there would fail on a plain checkout for a reason that has nothing to do with this
/// code. Everything that gates - the container and CI - is Linux, so the case is covered where the
/// verdict is taken; the fix itself is `DirEntry::file_type`, which does not follow a link on either
/// platform.
#[cfg(unix)]
mod symlinks {
    use super::{catalog, scratch};

    #[test]
    fn a_symlink_pointing_at_an_ancestor_does_not_make_the_walk_descend_into_itself() {
        // The bug: `path.is_dir()` follows the link, so `root/loop` answered "directory" and the
        // walk pushed it, found `root/loop/loop` one level down, pushed that, and kept going. What
        // this asserted before the fix is worth recording, because it is not what it looks like:
        // the walk terminated - the kernel stops resolving after 40 links in one path - and
        // returned `revenue.md` 41 times, once per level. So the failure was not a hang, it was one
        // metric defined 41 times and a digest that depended on the link structure.
        let root = scratch("symlink-loop");
        std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
        std::os::unix::fs::symlink(&root, root.join("loop")).expect("a symlink to the root is creatable");

        let found = catalog(root.clone())
            .documents()
            .expect("the walk terminates and reports the one document");

        assert_eq!(found, vec![root.join("revenue.md")]);
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_symlink_to_a_document_outside_the_root_is_skipped_rather_than_followed() {
        // The same rule seen from the other side, and the reason it is a rule rather than a
        // cycle-detector: a visited set would still have followed this one. A catalog is what is IN
        // the directory, so a link out of it is not a document - and the alternative is a catalog
        // whose digest depends on a file the tree does not contain.
        let root = scratch("symlink-out");
        let outside = scratch("symlink-out-target");
        let target = outside.join("elsewhere.md");
        std::fs::write(&target, "---\nkind: metric\n---\n").expect("a document is writable");
        std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
        std::os::unix::fs::symlink(&target, root.join("linked.md")).expect("a symlink to a file is creatable");

        let found = catalog(root.clone()).documents().expect("the walk reports the real document");

        assert_eq!(found, vec![root.join("revenue.md")]);
        drop(std::fs::remove_dir_all(&root));
        drop(std::fs::remove_dir_all(&outside));
    }
}

/// The two startup bounds on catalog intake: how many documents a root may hold, and how many
/// bytes they may sum to.
///
/// Compile-time RED against a `main` with neither variant: before this change,
/// `LocalCatalogError` had no way to name either failure, so a directory over either bound was
/// read in full. The mutation substitute for `just causality` on each test below is deleting its
/// own early-return check - the walk then runs to completion (or the read proceeds), and the
/// assertion on the specific refusal variant turns RED.
mod bounds {
    use super::{catalog, outcome_of, scratch};
    use crate::{LocalCatalogError, MAX_CATALOG_BYTES, MAX_CATALOG_DOCUMENTS};

    #[test]
    fn a_directory_with_one_document_more_than_the_cap_is_refused_before_the_walk_finishes() {
        // `documents()` directly, not `read_all()`: the count is checked during the walk itself,
        // before a single file is opened for content - this asserts that stage on its own,
        // exactly like the symlink tests above assert the walk without touching `read_all`.
        let root = scratch("too-many-documents");
        for n in 0..=MAX_CATALOG_DOCUMENTS {
            std::fs::write(root.join(format!("doc-{n:05}.md")), "").expect("a document is writable");
        }

        let err = catalog(root.clone())
            .documents()
            .expect_err("one document more than the cap must be refused, not read");

        match err {
            LocalCatalogError::TooManyDocuments { found, limit, .. } => {
                assert_eq!(limit, MAX_CATALOG_DOCUMENTS);
                // Exactly one past the limit: the walk stops at the first document that crosses
                // it rather than counting the rest of the directory.
                assert_eq!(found, MAX_CATALOG_DOCUMENTS + 1);
            }
            other => panic!("expected TooManyDocuments, got {other:?}"),
        }
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_directory_at_exactly_the_document_cap_is_not_refused_for_its_size() {
        // The twin: exactly `MAX_CATALOG_DOCUMENTS` documents is admitted past the count check -
        // the assertion is only that the walk itself reports every one of them, not that the
        // rest of a load would succeed (these are empty files, which fail a later check).
        let root = scratch("at-the-document-cap");
        for n in 0..MAX_CATALOG_DOCUMENTS {
            std::fs::write(root.join(format!("doc-{n:05}.md")), "").expect("a document is writable");
        }
        let found = catalog(root.clone())
            .documents()
            .expect("exactly the cap is admitted by the count check");
        assert_eq!(found.len(), MAX_CATALOG_DOCUMENTS);
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_document_one_byte_over_the_aggregate_cap_is_refused() {
        // One document, sized to cross `MAX_CATALOG_BYTES` on its own - `read_all` checks the
        // running total from `fs::metadata` before `read_to_string`, so this proves the bound
        // without needing many files.
        let oversized_len = usize::try_from(MAX_CATALOG_BYTES).expect("the cap fits a usize") + 1;
        let oversized = "a".repeat(oversized_len);
        let err = outcome_of("too-large-aggregate", &[("huge.md", oversized.as_str())])
            .expect_err("one document past the aggregate cap must be refused");
        match err {
            LocalCatalogError::TooLarge { found, limit, .. } => {
                assert_eq!(limit, MAX_CATALOG_BYTES);
                assert_eq!(found, MAX_CATALOG_BYTES + 1);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn several_documents_summing_to_exactly_the_aggregate_cap_are_not_refused_for_size() {
        // The aggregate is a SUM, not a per-file limit: several files whose total sits EXACTLY
        // at `MAX_CATALOG_BYTES`, none of them individually notable, and the twin to the
        // single-oversized-file case above. `count` is asserted against `MAX_CATALOG_BYTES` by
        // multiplication rather than derived by dividing - `clippy::integer_division` is denied
        // in this workspace, and multiplying is the same check without it.
        let per_file = 1024 * 1024_usize;
        let count = 16_usize;
        assert_eq!(
            per_file.checked_mul(count),
            Some(usize::try_from(MAX_CATALOG_BYTES).expect("the cap fits a usize")),
            "per_file * count must equal MAX_CATALOG_BYTES exactly, or this test proves nothing about the boundary"
        );
        let root = scratch("at-the-aggregate-cap");
        let body = "a".repeat(per_file);
        for n in 0..count {
            std::fs::write(root.join(format!("doc-{n:05}.md")), &body).expect("a document is writable");
        }
        let err = catalog(root.clone())
            .read_all()
            .expect_err("none of these are valid catalog documents");
        // At exactly the cap (not over it), the aggregate check does not fire; the load still
        // fails because none of these are valid catalog documents - `MalformedFrontmatter` is
        // the next check in line, and asserting it is not `TooLarge` is what proves the bound did
        // not fire for a total that is at, not over, the limit.
        assert!(
            !matches!(err, LocalCatalogError::TooLarge { .. }),
            "exactly the cap must not be reported as over it: {err:?}"
        );
        drop(std::fs::remove_dir_all(&root));
    }
}

/// The variant a document's failure lands in, which is the part a caller matches on.
mod kinds {
    use super::error_for;
    use crate::LocalCatalogError;

    #[test]
    fn a_frontmatter_block_that_is_not_yaml_is_not_reported_as_a_missing_kind() {
        // The finding. Every failure of the kind probe used to become one variant whose message
        // read "declares no `kind`", so a syntax error at line 2 was reported as an absent key -
        // a false statement about the file, in the one field a caller can branch on. The source
        // chain carried the truth, which is not the same as the error stating it.
        let broken = error_for("kind-broken-yaml", "---\nkind: [metric\nname: revenue\n---\nProse.\n");
        assert!(
            matches!(broken, LocalCatalogError::MalformedFrontmatter { .. }),
            "a frontmatter block that is not YAML must say so: {broken:?}"
        );
        assert!(
            core::error::Error::source(&broken).is_some(),
            "the parse failure names the line, so it has to stay reachable: {broken:?}"
        );
    }

    #[test]
    fn a_document_that_parses_and_does_not_identify_itself_is_one_neutral_variant() {
        // Three different ways of not saying what a document is, and one variant for all three,
        // because nothing in this workspace branches on the difference and each one sends the
        // author to the same line of the same file. The variant is neutral for that reason: it
        // says the frontmatter does not identify the document rather than claiming which of the
        // three it was.
        for (name, document) in [
            ("kind-unknown", "---\nkind: dashboard\nname: revenue\n---\nProse.\n"),
            ("kind-wrong-type", "---\nkind: 3\nname: revenue\n---\nProse.\n"),
            ("kind-missing", "---\nname: revenue\n---\nProse.\n"),
        ] {
            let err = error_for(name, document);
            assert!(
                matches!(err, LocalCatalogError::IdentifyKind { .. }),
                "{name} must be an IdentifyKind failure: {err:?}"
            );
            assert!(
                core::error::Error::source(&err).is_some(),
                "{name} must keep the parse failure reachable as a source"
            );
        }
    }

    #[test]
    fn malformed_yaml_and_an_unrecognised_kind_are_different_variants() {
        // The assertion the finding actually asks for, made on the discriminant rather than on a
        // message: two failures that a caller has to be able to tell apart must not be one
        // variant, whatever their messages say.
        let broken = error_for("split-broken-yaml", "---\nkind: [metric\n---\nProse.\n");
        let unknown = error_for("split-unknown-kind", "---\nkind: dashboard\n---\nProse.\n");
        assert_ne!(
            core::mem::discriminant(&broken),
            core::mem::discriminant(&unknown),
            "these must not collapse into one variant: {broken:?} / {unknown:?}"
        );
    }
}

/// The three refusals that are about the CONTENT of a catalog rather than about its YAML.
///
/// **What was untested here is not the refusal, it is the wiring to it.** Every refusal
/// underneath these three variants - `InvalidDescription`, `InvalidNoteBody` and all of
/// `InconsistentKnowledge` - is provoked in `sutura_domain` over a value built by hand, and a
/// hand-built value is not evidence that a directory on disk reaches the parse. The parse is on
/// this side of the boundary: `absorb_definition` and `absorb_note` each call it on
/// `Split::body()` before touching the frontmatter, and `assemble` calls
/// `Knowledge::assemble` after both halves are read. Nothing in the domain's suite would notice
/// if one of those calls went away.
///
/// **Every test here carries the twin, and the twin is what makes it a test.** "This document is
/// refused" passes for a document refused two guards earlier over something else, so each case
/// below loads the same document with the one field corrected. Measured, not assumed: deleting
/// the `Description::parse` call and passing `Description::default()` instead leaves the
/// twin green and turns the refusal red, which is the failure this shape is for.
mod content {
    use super::{outcome_for, outcome_of};
    use crate::LocalCatalogError;
    use sutura_domain::catalog::InvalidDescription;
    use sutura_domain::knowledge::{InconsistentKnowledge, InvalidNoteBody};

    /// A model document, which is the smallest DEFINITION document that loads on its own.
    ///
    /// A metric would need its model beside it or `Definitions::assemble` fails first, for a
    /// reason these tests are not about - and a model's prose goes through the same
    /// `absorb_definition` a metric's does, which is the call under test.
    const MODEL: &str =
        "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents, order_date]\n---\n";

    /// A metric over that model. Two documents together are the smallest catalog a note can be
    /// about.
    const METRIC: &str = "---\nkind: metric\nname: revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: sum, column: amount_cents }\ntime_column: order_date\ngrains: [month]\n---\nNet revenue, in minor units.\n";

    /// A knowledge document with no referent of its own, so it loads beside no definitions.
    const NOT_DEFINED: &str = "---\nkind: not_defined\nphrase: revenue forecast\n---\n";

    /// The file a failure has to name, since a catalog is many of them.
    fn names_the_document(err: &LocalCatalogError) -> bool {
        err.to_string().contains("doc.md")
    }

    #[test]
    fn the_prose_of_a_definition_document_is_parsed_as_a_description() {
        // The twin first, so what follows is a statement about the prose and not about the
        // frontmatter above it.
        drop(outcome_for("prose-model-ok", &format!("{MODEL}Net revenue, in minor units.\n")).expect("this model loads"));

        // The reachable case, and the reason this is not a theoretical one: a CRLF working tree
        // gives every line of every description a trailing `\r`. `frontmatter::split` strips one
        // at the fence lines alone and `cargo xtask line-endings` sees tracked files only, so a
        // catalog directory an operator mounted from a Windows editor arrives here like this -
        // and `sutura_app::prompt::quote` would DROP it, which is the alteration at render the
        // description type exists to forbid.
        let crlf = outcome_for("prose-model-crlf", &format!("{MODEL}Net revenue.\r\nIn minor units.\n"))
            .expect_err("a carriage return in the prose is not a description");
        assert!(
            matches!(
                crlf,
                LocalCatalogError::Description {
                    cause: InvalidDescription::ControlCharacter { code: 0x0D },
                    ..
                }
            ),
            "{crlf:?}"
        );
        assert!(names_the_document(&crlf), "{crlf}");

        // And the other half of the same rule, at the other set: a code point the renderer KEEPS
        // and a reader cannot see. This is the one the type was added for.
        let invisible = outcome_for(
            "prose-model-invisible",
            &format!("{MODEL}Revenue where status = 'act\u{202E}ive'.\n"),
        )
        .expect_err("an invisible code point in the prose is not a description");
        assert!(
            matches!(
                invisible,
                LocalCatalogError::Description {
                    cause: InvalidDescription::InvisibleCharacter { code: 0x202E },
                    ..
                }
            ),
            "{invisible:?}"
        );
        assert!(names_the_document(&invisible), "{invisible}");
    }

    #[test]
    fn the_prose_of_a_knowledge_document_is_parsed_as_a_note_body() {
        // The twin: the same frontmatter, with prose under it.
        drop(outcome_for("body-ok", &format!("{NOT_DEFINED}Nothing here forecasts anything.\n")).expect("this note loads"));

        // A document with no prose at all. The frontmatter is complete, so the only thing wrong
        // with it is that the note says nothing - which would render as a heading over blank
        // space in the agent-facing prompt.
        let empty = outcome_for("body-empty", NOT_DEFINED).expect_err("a note with no prose is not a note");
        assert!(
            matches!(
                empty,
                LocalCatalogError::NoteBody {
                    cause: InvalidNoteBody::Empty,
                    ..
                }
            ),
            "{empty:?}"
        );
        assert!(names_the_document(&empty), "{empty}");

        // And a body that is not empty in bytes and is empty on the page, which is the case the
        // emptiness check is decided on what a reader will see for. Same variant, reached from
        // the other direction.
        let blank = outcome_for("body-blank", &format!("{NOT_DEFINED}\u{200B}\u{FEFF}\u{2060}\n"))
            .expect_err("a note that draws nothing is not a note");
        assert!(
            matches!(
                blank,
                LocalCatalogError::NoteBody {
                    cause: InvalidNoteBody::Empty,
                    ..
                }
            ),
            "{blank:?}"
        );

        // One mixed into a sentence is the other refusal, and the order between the two is
        // asserted in the domain. Here it is that the adapter carries whichever fired.
        let hidden = outcome_for(
            "body-invisible",
            &format!("{NOT_DEFINED}Nothing here for\u{200B}ecasts anything.\n"),
        )
        .expect_err("an invisible code point in a note is not a note");
        assert!(
            matches!(
                hidden,
                LocalCatalogError::NoteBody {
                    cause: InvalidNoteBody::InvisibleCharacter { code: 0x200B },
                    ..
                }
            ),
            "{hidden:?}"
        );
    }

    #[test]
    fn a_note_that_does_not_hold_together_with_the_definitions_fails_the_load() {
        // The one variant here whose check is over the bundle rather than over a file, which is
        // why this case needs a catalog rather than a document: `Knowledge::assemble` runs after
        // both halves are read, and the twin has to be a caveat about a metric that exists.
        let scoped = "---\nkind: caveat\nname: revenue_is_in_minor_units\nabout:\n  - { metric: revenue }\n---\nEvery revenue figure here is in minor units.\n";
        drop(
            outcome_of(
                "knowledge-ok",
                &[("model.md", MODEL), ("metric.md", METRIC), ("caveat.md", scoped)],
            )
            .expect("a caveat about a metric that exists loads"),
        );

        // The same document with its scope emptied. An unscoped caveat is the shape that would
        // make the catalog an arbitrary text channel into the prompt's preamble, so it is refused
        // - and this asserts that the refusal survives a real directory rather than only a
        // hand-built bundle.
        let unscoped =
            "---\nkind: caveat\nname: revenue_is_in_minor_units\nabout: []\n---\nEvery revenue figure here is in minor units.\n";
        let err = outcome_of(
            "knowledge-unscoped",
            &[("model.md", MODEL), ("metric.md", METRIC), ("caveat.md", unscoped)],
        )
        .expect_err("a caveat about nothing is not a caveat");
        assert!(
            matches!(
                err,
                LocalCatalogError::UncheckableKnowledge {
                    cause: InconsistentKnowledge::CaveatAboutNothing { .. }
                }
            ),
            "{err:?}"
        );
        // The cause carries no path - that limit is on the variant, and it is stated there - so
        // what a reader gets instead is the note's own name, which is unique across the catalog.
        assert!(
            core::error::Error::source(&err).is_some_and(|cause| cause.to_string().contains("revenue_is_in_minor_units")),
            "{err:?}"
        );
    }
}

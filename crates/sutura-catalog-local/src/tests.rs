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
    const METRIC: &str = "---\nkind: metric\nname: revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: sum, column: amount_cents }\ntime_column: order_date\ngrains: [month]\naudience: open\n---\nNet revenue, in minor units.\n";

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
    fn an_empty_relationship_keys_list_fails_the_load_as_a_relationship_document() {
        // PR #1019 left this unwired: the domain's `InvalidJoinKeys::Empty` was already refused
        // by `JoinKeys::of`, but nothing asserted that a `keys: []` DOCUMENT reaches
        // `LocalCatalogError::Relationship` through a real directory. The refusal fires in
        // `absorb_definition`, before assembly - so the twin below (the same relationship with one
        // key) is what shows the loop reads the model, and the `keys: []` swap is what shows the
        // empty set is the one fault.
        let usage = "---\nkind: model\nname: daily_usage\nsource: local\ntable: fct_usage_daily\ncolumns: [usage_date, subscription_key]\n---\n";
        let subscriptions = "---\nkind: model\nname: subscriptions\nsource: local\ntable: dim_subscriptions\ncolumns: [subscription_key, month]\n---\n";
        let full = "---\nkind: relationship\nname: usage_subscription\norigin: { model: daily_usage }\ntarget: { model: subscriptions }\njoin_type: many_to_one\nkeys:\n  - { origin: subscription_key, target: subscription_key }\n---\n";
        let empty = "---\nkind: relationship\nname: usage_subscription\norigin: { model: daily_usage }\ntarget: { model: subscriptions }\njoin_type: many_to_one\nkeys: []\n---\n";
        drop(
            outcome_of(
                "relationship-empty-keys-twin",
                &[("usage.md", usage), ("subscriptions.md", subscriptions), ("doc.md", full)],
            )
            .expect("the same relationship with one key loads"),
        );

        let err = outcome_of(
            "relationship-empty-keys",
            &[("usage.md", usage), ("subscriptions.md", subscriptions), ("doc.md", empty)],
        )
        .expect_err("`keys: []` joins nothing");
        assert!(
            matches!(
                err,
                LocalCatalogError::Relationship {
                    cause: crate::document::InvalidRelationshipDocument::EmptyKeys(_),
                    ..
                }
            ),
            "an empty keys list must reach the Relationship refusal naming the empty set: {err:?}"
        );
        assert!(names_the_document(&err), "{err}");
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

/// `kind: cube` (`telekom/sutura#1148`), through a real directory, because the expansion and the
/// `absorb` arm that reaches it are both on this side of the boundary. Every case is red on a tree
/// without the kind, where the probe refuses `cube` before any check below exists.
mod cube {
    use super::{catalog, outcome_of, scratch};
    use crate::LocalCatalogError;
    use sutura_domain::catalog::{Audience, InconsistentDefinitions};
    use sutura_domain::model::{DimensionName, MetricName, ModelName};
    use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog as _};

    const MODEL: &str = "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents, order_date, order_key, region]\n---\nOne row per order.\n";

    const CUBE: &str = "---
kind: cube
name: sales
model: orders
time_column: order_date
grains: [month]
dimensions:
  - name: region
    column: region
    values: [east, west]
measures:
  - name: revenue
    measure:
      simple: { aggregate: sum, column: amount_cents }
    required_filters:
      - equals: { column: region, value: east }
    anchor:
      range: { start: 2026-06-01, end: 2026-07-01 }
      value: \"202121\"
    audience: open
    description: Net revenue, in minor units.
  - name: orders
    measure:
      simple: { aggregate: count_distinct, column: order_key }
    audience:
      restricted: [finance]
---
Sales over the order fact.
";

    /// [`CUBE`], written out by hand as the two metric documents it stands for.
    const FLAT_REVENUE: &str = "---\nkind: metric\nname: sales_revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: sum, column: amount_cents }\nrequired_filters:\n  - equals: { column: region, value: east }\nanchor:\n  range: { start: 2026-06-01, end: 2026-07-01 }\n  value: \"202121\"\ntime_column: order_date\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    values: [east, west]\naudience: open\n---\nNet revenue, in minor units.\n";
    const FLAT_ORDERS: &str = "---\nkind: metric\nname: sales_orders\nmodel: orders\nmeasure:\n  simple: { aggregate: count_distinct, column: order_key }\ntime_column: order_date\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    values: [east, west]\naudience:\n  restricted: [finance]\n---\nSales over the order fact.\n";

    fn name(raw: &str) -> MetricName {
        MetricName::parse(raw).expect("a test name is a name")
    }

    fn cube_outcome(case: &str, cube: &str) -> Result<crate::Content, LocalCatalogError> {
        outcome_of(case, &[("orders.md", MODEL), ("doc.md", cube)])
    }

    fn pinned(case: &str, documents: &[(&str, &str)]) -> PinnedDefinitions {
        let root = scratch(case);
        for &(file, document) in documents {
            std::fs::write(root.join(file), document).expect("a document is writable");
        }
        let pinned = catalog(root.clone()).load().expect("this catalog loads");
        drop(std::fs::remove_dir_all(&root));
        pinned
    }

    /// A cube refused after it parsed: the file it names, and the cube's own refusal as its source.
    ///
    /// Messages rather than a `matches!` on `LocalCatalogError::Cube`, so these cells compile on a
    /// tree without the kind and `just causality` can watch them go red there.
    fn cube_refusal(case: &str, cube: &str) -> String {
        let err = cube_outcome(case, cube).expect_err("this cube is refused");
        assert!(err.to_string().contains("doc.md is not a usable cube"), "{err:?}");
        core::error::Error::source(&err)
            .expect("a cube refusal keeps its cause")
            .to_string()
    }

    /// A cube refused while its frontmatter is read, and the serde message that says why.
    fn frontmatter_refusal(case: &str, cube: &str) -> String {
        let err = cube_outcome(case, cube).expect_err("this cube is refused");
        assert!(matches!(err, LocalCatalogError::Frontmatter { kind: "cube", .. }), "{err:?}");
        core::error::Error::source(&err)
            .expect("a frontmatter refusal keeps its serde cause")
            .to_string()
    }

    #[test]
    fn a_cube_expands_to_one_metric_per_measure_sharing_its_model_time_and_dimensions() {
        let (definitions, _) = cube_outcome("cube-expands", CUBE).expect("the cube loads");
        let revenue = definitions
            .metric(&name("sales_revenue"))
            .expect("the first measure is a metric");
        let orders = definitions
            .metric(&name("sales_orders"))
            .expect("the second measure is a metric");
        assert!(
            definitions.metric(&name("sales")).is_none(),
            "the cube itself is not a metric"
        );
        let region = DimensionName::parse("region").expect("a test name is a name");
        for metric in [revenue, orders] {
            assert_eq!(metric.model().as_str(), "orders");
            assert_eq!(metric.time_column().as_str(), "order_date");
            assert_eq!(metric.dimensions().keys().collect::<Vec<_>>(), [&region]);
        }
        assert_eq!(revenue.required_filters().len(), 1, "a measure's required filter is its own");
        assert_eq!(orders.required_filters().len(), 0);
        assert!(
            revenue.anchor().is_some() && orders.anchor().is_none(),
            "a measure's anchor is its own"
        );
        assert_eq!(revenue.audience(), &Audience::Open);
        assert_ne!(orders.audience(), &Audience::Open, "a measure's audience is its own");
        assert_eq!(revenue.description(), "Net revenue, in minor units.");
        assert_eq!(
            orders.description(),
            "Sales over the order fact.",
            "no description: the cube's prose"
        );
    }

    #[test]
    fn a_cube_and_its_hand_flattened_equivalent_pin_the_same_digest() {
        let cube = pinned("cube-digest", &[("orders.md", MODEL), ("sales.md", CUBE)]);
        let flat = pinned(
            "cube-digest-flat",
            &[("orders.md", MODEL), ("revenue.md", FLAT_REVENUE), ("count.md", FLAT_ORDERS)],
        );
        assert_eq!(cube.digest(), flat.digest());
        // The twin: the digest sees the field the cube supplies by fallback, so equality above is
        // not a digest blind to the difference.
        let reworded = pinned(
            "cube-digest-reworded",
            &[
                ("orders.md", MODEL),
                ("revenue.md", FLAT_REVENUE),
                ("count.md", &FLAT_ORDERS.replace("Sales over", "Orders over")),
            ],
        );
        assert_ne!(cube.digest(), reworded.digest());
    }

    /// A second fact and a calendar both facts reach: what a cross-model ratio needs to load.
    const TWO_FACTS: [(&str, &str); 5] = [
        ("orders.md", MODEL),
        (
            "invoices.md",
            "---\nkind: model\nname: invoices\nsource: local\ntable: fct_invoice\ncolumns: [invoice_key, order_date]\n---\nOne row per invoice.\n",
        ),
        (
            "calendar.md",
            "---\nkind: model\nname: calendar\nsource: local\ntable: dim_calendar\ncolumns: [order_date]\nprimary_key: [order_date]\n---\nOne row per day.\n",
        ),
        (
            "order_calendar.md",
            "---\nkind: relationship\nname: order_calendar\norigin: { model: orders, column: order_date }\ntarget: { model: calendar, column: order_date }\njoin_type: many_to_one\n---\nThe first fact's hop.\n",
        ),
        (
            "invoice_calendar.md",
            "---\nkind: relationship\nname: invoice_calendar\norigin: { model: invoices, column: order_date }\ntarget: { model: calendar, column: order_date }\njoin_type: many_to_one\n---\nThe second fact's hop.\n",
        ),
    ];

    const RATIO_CUBE: &str = "---\nkind: cube\nname: sales\nmodel: orders\ntime_column: order_date\ngrains: [month]\nmeasures:\n  - name: per_invoice\n    measure:\n      ratio:\n        numerator: { aggregate: sum, column: amount_cents }\n        denominator: { aggregate: count, column: invoice_key, model: invoices }\n        zero_denominator: yields_null\n    shared_calendar: calendar\n    audience: open\n---\nRevenue per invoice.\n";
    const FLAT_RATIO: &str = "---\nkind: metric\nname: sales_per_invoice\nmodel: orders\nmeasure:\n  ratio:\n    numerator: { aggregate: sum, column: amount_cents }\n    denominator: { aggregate: count, column: invoice_key, model: invoices }\n    zero_denominator: yields_null\ntime_column: order_date\ngrains: [month]\nshared_calendar: calendar\naudience: open\n---\nRevenue per invoice.\n";

    fn two_facts_and(case: &str, file: &str, document: &str) -> PinnedDefinitions {
        let mut documents = TWO_FACTS.to_vec();
        documents.push((file, document));
        pinned(case, &documents)
    }

    #[test]
    fn a_cube_s_cross_model_ratio_pins_its_hand_written_metric_s_digest() {
        let cube = two_facts_and("cube-ratio", "sales.md", RATIO_CUBE);
        let flat = two_facts_and("cube-ratio-flat", "ratio.md", FLAT_RATIO);
        assert_eq!(cube.digest(), flat.digest());
        // The twin: the digest sees the calendar, so a cube dropping it could not pass as equal.
        let uncalendared = two_facts_and(
            "cube-ratio-uncalendared",
            "ratio.md",
            &FLAT_RATIO.replace("shared_calendar: calendar\n", ""),
        );
        assert_ne!(cube.digest(), uncalendared.digest());
    }

    #[test]
    fn a_measure_declaring_dimensions_of_its_own_is_refused() {
        let cube = CUBE.replace(
            "    description: Net",
            "    dimensions:\n      - name: channel\n        column: region\n    description: Net",
        );
        let cause = frontmatter_refusal("cube-measure-dimensions", &cube);
        assert!(cause.contains("unknown field `dimensions`"), "{cause}");
    }

    #[test]
    fn a_cube_without_a_time_column_is_refused() {
        let cause = frontmatter_refusal("cube-no-time", &CUBE.replace("time_column: order_date\n", ""));
        assert!(cause.contains("missing field `time_column`"), "{cause}");
    }

    #[test]
    fn a_cube_declaring_hierarchies_is_refused_by_name() {
        let cube = CUBE.replace(
            "measures:\n",
            "hierarchies:\n  - name: geography\n    levels: [region]\nmeasures:\n",
        );
        let cause = frontmatter_refusal("cube-hierarchies", &cube);
        assert!(cause.contains("unknown field `hierarchies`"), "{cause}");
    }

    #[test]
    fn two_measures_of_one_cube_sharing_a_name_are_refused_naming_the_cube() {
        let cause = cube_refusal("cube-duplicate", &CUBE.replace("  - name: orders\n", "  - name: revenue\n"));
        assert_eq!(cause, "cube sales declares measure revenue more than once");
    }

    #[test]
    fn a_cube_with_no_measure_is_refused() {
        let empty = format!(
            "{}measures: []\n---\nNothing.\n",
            CUBE.split("measures:\n").next().unwrap_or_default()
        );
        assert_eq!(cube_refusal("cube-no-measures", &empty), "cube sales declares no measure");
    }

    #[test]
    fn a_measure_its_metric_refuses_refuses_the_cube_rather_than_vanishing() {
        let cube = CUBE.replace(
            "    measure:\n      simple: { aggregate: count_distinct, column: order_key }\n",
            "",
        );
        assert_eq!(
            cube_refusal("cube-measure-refused", &cube),
            "a metric must say what it computes: write `measure`, or `authored_sql` for SQL the vocabulary cannot express"
        );
    }

    #[test]
    fn a_measure_naming_an_undeclared_model_reaches_the_consistency_refusal() {
        let cube = CUBE.replace(
            "      simple: { aggregate: sum, column: amount_cents }\n",
            "      ratio:\n        numerator: { aggregate: sum, column: amount_cents }\n        denominator: { aggregate: count, column: invoice_key, model: invoices }\n        zero_denominator: yields_null\n",
        );
        let err = cube_outcome("cube-unknown-model", &cube).expect_err("a term on an undeclared model is refused");
        assert!(
            matches!(
                &err,
                LocalCatalogError::Inconsistent {
                    cause: InconsistentDefinitions::UnknownTermModel { metric, model },
                } if *metric == name("sales_revenue") && *model == ModelName::parse("invoices").expect("a test name is a name")
            ),
            "{err:?}"
        );
    }
}

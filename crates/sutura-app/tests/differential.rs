//! One plan, computed locally and pushed down to a data source, compared.
//!
//! The two sides are not two implementations of one thing, and reading them that way overstates what
//! this proves. `DataFusion` is the ENGINE, executing a plan locally over Arrow. `DuckDB` is a DATA
//! SOURCE, which the compiler renders a statement for and pushes down to. Comparing them is
//! comparing "we computed it here" against "we asked a database to compute it", and for a simple
//! aggregate they agree trivially.
//!
//! **So this is a cheap regression net, not a proof of correctness.** It is kept for one class of bug
//! nothing else here catches: a rendered statement that is VALID SQL with different semantics. Every
//! such bug found while building the renderer was of that kind - a truncated date coming back as a
//! timestamp, an integer division silently truncating, a week starting on the wrong day. An anchor
//! check compares one number and would miss most of them; the parse golden proves a statement is well
//! formed and says nothing about what it means. A row-by-row comparison against a real SQL engine is
//! what covers the gap between those two.
//!
//! What it is NOT: a reason to keep two execution paths. When federation moves the engine above the
//! `Warehouse` port, `DataFusion` stops being a peer of a data source and this test's shape changes
//! with it.
//!
//! Two things it caught on first being written are noted on the assertions below. Both were shallow -
//! column labels and row ordering - which is about the yield to expect from it.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_app::{answer, verify_anchors};
    use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog as _, Validated};
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::warehouse::{RowSet, Value};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn source() -> sutura_domain::model::SourceName {
        sutura_domain::model::SourceName::parse("local").expect("a fixture source is a source")
    }

    fn load() -> PinnedDefinitions {
        let version = sutura_domain::pinned::DefinitionVersion::parse("differential-1").expect("a test version is a version");
        sutura_catalog_local::LocalCatalog::new(fixtures().join("catalog"), version)
            .load()
            .expect("the fixture catalog loads")
    }

    fn questions() -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(fixtures().join("questions"))
            .expect("the questions directory is there")
            .map(|e| e.expect("a directory entry is readable").path())
            .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
            .collect();
        found.sort();
        found
    }

    fn read(path: &Path) -> Query {
        let text = std::fs::read_to_string(path).expect("a question file is readable");
        serde_norway::from_str(&text).expect("a question file is a question")
    }

    /// The engine and the data source, each over the same CSVs.
    fn both_sides() -> (
        sutura_exec_duckdb::DuckDbWarehouse,
        sutura_exec_datafusion::DataFusionWarehouse,
    ) {
        let data = fixtures().join("data");
        let duck = sutura_exec_duckdb::DuckDbWarehouse::in_memory(source()).expect("an in-memory database opens");
        let fusion = sutura_exec_datafusion::DataFusionWarehouse::new(source()).expect("an in-process engine starts");
        for model in load().definitions().models().values() {
            let csv = data.join(format!("{}.csv", model.table()));
            duck.attach_csv(model.table(), &csv).expect("duckdb attaches the fixture csv");
            fusion
                .attach_csv(model.table(), &csv)
                .expect("datafusion attaches the fixture csv");
        }
        (duck, fusion)
    }

    /// A result as comparable text.
    ///
    /// Rendered rather than compared as `Value`, because the two sides legitimately return different
    /// Rust types for the same number - one hands back a `DECIMAL` where the other hands back a wide
    /// integer - and `Value::render` is the one canonical form both are already required to agree on.
    /// Comparing the enum would fail on a difference that is not a difference.
    ///
    /// **Floats are cut to twelve significant digits, and that is not a loosening.** Summing the same
    /// rows in a different order changes the last place of an `f64`, so comparing the full binary
    /// expansion asserts that both sides summed in the same order - which is not a property either
    /// one promises, and not what this test is for. It has already fired once for real, on the example
    /// corpus. Twelve digits is far beyond any figure a metric reports and far short of the noise;
    /// integers and dates are untouched, so an exact count stays exactly compared.
    fn rendered(rows: &RowSet) -> Vec<Vec<String>> {
        rows.rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| match *value {
                        Value::Real(v) => format!("{v:.12e}"),
                        ref other => other.render(),
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_engine_and_the_data_source_agree_on_every_question() {
        // The whole point. A wrong number has to be produced twice, the same way, by the engine and
        // by the data source - one building a logical plan over Arrow, one executing rendered SQL.
        //
        // Two things this caught when it was first written, both of which a snapshot would have
        // happily pinned as correct: the column LABELS disagreed, because one engine took them from
        // the driver's result schema and the other built them from the plan; and the two disagreed
        // on row ORDER until both sorted by the grouped expressions.
        let pinned = load();
        let (duck, fusion) = both_sides();
        let report = verify_anchors(&pinned, &duck);
        let validated = Validated::new(pinned, &report).expect("the anchors hold");

        let mut compared = 0_usize;
        for path in questions() {
            let question = read(&path);
            let name = path
                .file_stem()
                .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned());

            let from_duck = answer(&validated, &question, &duck).unwrap_or_else(|e| panic!("{name}: duckdb failed: {e}"));
            let from_fusion = answer(&validated, &question, &fusion).unwrap_or_else(|e| panic!("{name}: datafusion failed: {e}"));

            match (from_duck, from_fusion) {
                (ToolOutcome::Answer { rows: ref a, .. }, ToolOutcome::Answer { rows: ref b, .. }) => {
                    assert_eq!(
                        a.columns(),
                        b.columns(),
                        "{name}: the two sides labelled the result differently"
                    );
                    assert_eq!(rendered(a), rendered(b), "{name}: the two sides returned different rows");
                    compared = compared.saturating_add(1);
                }
                (ToolOutcome::Refusal { reason: ref a }, ToolOutcome::Refusal { reason: ref b }) => {
                    // A refusal is decided by the compiler, above both adapters, so the two must
                    // always agree. If they ever do not, something below the plan is deciding
                    // governance, which is the thing that must never happen.
                    assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "{name}: the two sides refused for different reasons"
                    );
                }
                (duck_outcome, fusion_outcome) => {
                    panic!(
                        "{name}: one side answered and the other refused\n  duckdb: \
                         {duck_outcome:?}\n  datafusion: {fusion_outcome:?}"
                    );
                }
            }
        }
        assert!(
            compared > 0,
            "no question produced an answer from both sides, so this compared nothing"
        );
    }

    #[test]
    fn both_sides_reproduce_every_declared_anchor() {
        // An anchor is the number somebody certified. Checking it both ways is what makes "the
        // definition still means what it claimed" independent of which side computed it, and it is
        // what would catch an arithmetic difference between the two - exactly the class of thing a
        // one-sided anchor check cannot see.
        let pinned = load();
        let (duck, fusion) = both_sides();
        let from_duck = verify_anchors(&pinned, &duck);
        let from_fusion = verify_anchors(&pinned, &fusion);
        assert_eq!(
            from_duck.checks(),
            from_fusion.checks(),
            "the engine and the data source disagree about whether the anchors hold"
        );
        assert!(
            !from_duck.checks().is_empty(),
            "the fixture catalog declares no anchor, so this proved nothing"
        );
        drop(Validated::new(pinned, &from_fusion).expect("the in-process engine agrees they hold"));
    }
}

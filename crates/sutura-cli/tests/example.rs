//! The example under `examples/single-player`, run as a test.
//!
//! **One directory, two purposes, and no second copy of either.** `examples/single-player` is what
//! a reader is told to run, and it is also the corpus this file loads: the catalog, the CSVs and
//! the questions are the same bytes in both roles. A quickstart that stopped working therefore
//! fails the build rather than failing the next person who tried it, which is the only arrangement
//! under which a README and a program cannot drift.
//!
//! It lives on `sutura-cli` because that crate is the composition root, so the pair it names is the
//! pair the example runs on: the local catalog adapter and the ENGINE. **Not the `DuckDB` one** - the
//! binary does not link a data-system driver, and this file executing through
//! `sutura_exec_datafusion` is what makes the quickstart's claim about the shipped artifact rather
//! than about a test. The test drives the LIBRARIES rather than spawning the binary. Spawning would
//! test argument parsing and then assert on stdout, which is a slower way of asserting less.
//!
//! Deliberately smaller than `sutura-app`'s golden suite and not a replacement for it. That one is a
//! matrix over `adapters::registered` - every registered catalog, every dialect the compiler renders
//! for, every registered data system - and exists to pin what the compiler decides. This one exists
//! to prove that one documented directory still answers, on the one adapter the binary ships and in
//! the one dialect its README shows.
//!
//! Every test here executes. There used to be a feature gate, because the only adapter that could
//! run anything was `DuckDB` and there is no musl `libduckdb` for the cross builds to link against.
//! The engine ships now, so there is no build in which this example cannot be run - and no half of
//! this file that CI skips.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` for code inside a `#[cfg(test)]`
// item, and an integration test target is compiled with `--test` so it is true here. Without it
// every `expect` in the fixture helpers below is a lint error.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
    use sutura_domain::query::Query;
    use sutura_semantic::{Compiled, compile};
    use sutura_sql::Dialect;

    /// The one dialect this example is documented for.
    ///
    /// The README shows `DuckDB` output, so pinning a second dialect here would pin something the
    /// example never claims. Nothing executes it: the engine renders no SQL, so this dialect only
    /// decides what the statement snapshots say. `sutura-app`'s suite covers every dialect.
    const DIALECT: Dialect = Dialect::DuckDb;

    /// A result set as text, with floats cut to a precision two engines can agree on.
    ///
    /// **Not cosmetic.** Pinning a raw `f64` pins its full binary expansion, and two engines
    /// computing the same ratio over the same rows legitimately differ in the last place - summing
    /// in a different order is enough to do it. A snapshot of that asserts WHICH ENGINE RAN, which
    /// is not what this test is for, and it goes red on a change that altered no number anybody
    /// cares about. That is exactly what happened when the engine took this example over from the
    /// data-source adapter: seven of eleven digits agreed and the snapshot failed.
    ///
    /// Twelve significant digits - far beyond any figure this example reports, far short of the
    /// noise. Integers and dates are untouched, so an exact count stays exactly asserted.
    fn stable(rows: &sutura_domain::warehouse::RowSet) -> Vec<Vec<String>> {
        rows.rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| match *value {
                        sutura_domain::warehouse::Value::Real(v) => format!("{v:.12e}"),
                        ref other => other.render(),
                    })
                    .collect()
            })
            .collect()
    }

    /// The version the example is stamped with in this suite.
    ///
    /// Fixed rather than taken from the working tree. The digest is over the parsed definitions and
    /// does not include the version, but a version that moved between runs would still churn every
    /// snapshot that carries provenance.
    const VERSION: &str = "example-single-player";

    /// The prefix a question file uses to say it exists to be refused.
    ///
    /// A convention the test reads rather than a list it keeps: a refusal fixture added to the
    /// directory is covered by the assertions below without this file being edited, and one renamed
    /// out of the convention starts being required to answer.
    const REFUSED_PREFIX: &str = "refused-";

    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    fn catalog_root() -> PathBuf {
        example_root().join("catalog")
    }

    /// Every question in the corpus, sorted.
    ///
    /// Sorted so the corpus is a function of the directory rather than of the filesystem: an order
    /// that changed between runs would produce snapshot churn unrelated to the change under review.
    fn questions() -> Vec<PathBuf> {
        let dir = example_root().join("questions");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("the example has a questions directory")
            .map(|entry| entry.expect("a directory entry is readable").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
            .collect();
        found.sort();
        assert!(!found.is_empty(), "no questions under {}", dir.display());
        found
    }

    fn read_question(path: &Path) -> Query {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
        serde_norway::from_str(&text).unwrap_or_else(|e| panic!("{} is not a question: {e}", path.display()))
    }

    /// The example catalog, read the way the `catalog` command reads it.
    fn load() -> PinnedDefinitions {
        let version = DefinitionVersion::parse(VERSION).expect("the fixed version is a version");
        let name = sutura_domain::model::SourceName::parse("local").expect("the example catalog name is a name");
        LocalCatalog::new(name, catalog_root(), version)
            .load()
            .unwrap_or_else(|e| panic!("the example catalog does not load: {e}"))
    }

    /// Settings every snapshot in this file uses.
    ///
    /// The path is set explicitly so the files land in `tests/snapshots/` rather than wherever the
    /// macro would guess from the module path, and the module prefix is dropped so a snapshot is
    /// named after the question it is about.
    fn settings() -> insta::Settings {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings
    }

    /// The stem of a question file, which is what its snapshots are named after.
    fn stem(path: &Path) -> String {
        path.file_stem()
            .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned())
    }

    /// An error and every cause beneath it, outermost first, as one block.
    ///
    /// Snapshotted rather than asserted with `contains`, for the reason the `query` command walks
    /// the chain at all: `Display` on a `thiserror` enum prints the outermost message and stops,
    /// and the outermost message for the case this reaches is "the data system did not answer",
    /// which names nothing. What a reader needs is the column and which of the three non-finite
    /// values it was, and both of those live one and two levels down.
    fn chain(error: &dyn core::error::Error) -> String {
        let mut out = error.to_string();
        let mut cursor = error.source();
        while let Some(cause) = cursor {
            out.push_str("\n  caused by: ");
            out.push_str(&cause.to_string());
            cursor = cause.source();
        }
        out
    }

    /// A statement and the values bound to it, as `sutura compile` prints them.
    ///
    /// One snapshot rather than two. The parameters are here rather than in a file of their own
    /// because the interesting property is the relationship between the two halves: every value the
    /// question carried is in the list below the statement and none of them is in the statement
    /// itself, and that is only readable when both are on the screen at once.
    fn rendered(query: &sutura_sql::GeneratedQuery) -> String {
        use core::fmt::Write as _;
        let mut out = String::from(query.sql());
        out.push('\n');
        for (index, param) in query.params().iter().enumerate() {
            writeln!(out, "-- ${} = {}", index + 1, param.render()).expect("writing to a String cannot fail");
        }
        out
    }

    // ------------------------------------------------------------------------- without a database ---

    #[test]
    fn the_example_catalog_loads_and_its_digest_is_pinned() {
        // The bug this prevents: an edit to a catalog document that changes what a metric means and
        // is reviewed as prose. The digest is over the parsed definitions, so it moves when a
        // measure, a filter, a grain, a dimension or a description changes, and the diff on this
        // snapshot is what makes that visible in review rather than at runtime.
        //
        // It is also the number the example's README prints on its first line, which is why a
        // reader can tell that the table they got came from the definitions they read.
        let pinned = load();
        settings().bind(|| {
            insta::assert_snapshot!("example_digest", pinned.digest().as_str());
            insta::assert_yaml_snapshot!("example_definitions", pinned.definitions());
        });
    }

    #[test]
    fn every_model_in_the_example_catalog_has_the_csv_the_quickstart_needs() {
        // The bug this prevents: a model added to the catalog with no data file beside it. The
        // quickstart then fails on the first command with a message about a missing table, which is
        // the worst possible first impression and is invisible to a reviewer reading the catalog
        // diff alone. Named after `table` rather than after `name`, because that is the file the
        // adapter looks for.
        let pinned = load();
        let data = example_root().join("data");
        for model in pinned.definitions().models().values() {
            let csv = data.join(format!("{}.csv", model.table()));
            assert!(
                csv.is_file(),
                "model {} declares table {} and there is no {}",
                model.name(),
                model.table(),
                csv.display()
            );
        }
    }

    #[test]
    fn the_example_exercises_every_shape_and_every_term_in_the_measure_vocabulary() {
        // The bug this prevents: the example quietly stops being the thing it is for. Its whole
        // purpose is to show every measure shape, every term and a definitional filter in one
        // catalog, and a metric deleted or rewritten during a refactor would leave the README
        // claiming coverage that no longer exists. Written against the vocabulary rather than
        // against metric names, so renaming a metric does not fail it and dropping a shape does.
        //
        // Terms are asserted as well as shapes, and that is the half this test used to be missing.
        // A conditional count was a shape when it could only be a whole measure; now it is a term,
        // so a catalog holding one `simple` and one `ratio` would satisfy a shape-only check while
        // never once compiling a `count_if` - which is precisely the coverage this example exists
        // to carry.
        let pinned = load();
        let mut shapes: BTreeSet<&str> = BTreeSet::new();
        let mut terms: BTreeSet<&str> = BTreeSet::new();
        let mut ratio_terms: BTreeSet<&str> = BTreeSet::new();
        let mut aggregates: BTreeSet<&str> = BTreeSet::new();
        let mut zero_denominators: BTreeSet<&str> = BTreeSet::new();
        let mut with_required_filter = 0_usize;
        for metric in pinned.definitions().metrics().values() {
            let measure = metric.measure();
            shapes.insert(measure.shape());
            if let sutura_domain::measure::Measure::Ratio { zero_denominator, .. } = *measure {
                zero_denominators.insert(zero_denominator.as_str());
            }
            for term in measure.terms() {
                terms.insert(term.kind());
                if let sutura_domain::measure::Term::Aggregate(ref aggregated) = *term {
                    aggregates.insert(aggregated.aggregate().as_str());
                }
                if measure.shape() == "ratio" {
                    ratio_terms.insert(term.kind());
                }
            }
            if !metric.required_filters().is_empty() {
                with_required_filter += 1;
            }
        }
        assert_eq!(
            shapes,
            BTreeSet::from(["ratio", "simple"]),
            "the example is supposed to demonstrate every measure shape"
        );
        assert_eq!(
            terms,
            BTreeSet::from(["aggregate", "count_if"]),
            "the example is supposed to demonstrate every term"
        );
        // The point of the two-level vocabulary, asserted rather than described: a conditional count
        // is usable where an aggregate is. A catalog that used `count_if` only as a whole measure
        // would pass every other assertion here and demonstrate nothing that the previous
        // vocabulary could not already do.
        assert_eq!(
            ratio_terms,
            BTreeSet::from(["aggregate", "count_if"]),
            "no ratio in the example holds a conditional count, which is the metric the vocabulary was changed for"
        );
        // The third axis, and the one this test used to leave open. A shape and a term say how a
        // measure is assembled; the AGGREGATE is what it computes, and each one is a separate arm in
        // each generator. `avg` and a plain `count` are the two that reach nothing else here: every
        // other count in the catalog is distinct or conditional, and every other mean is a ratio. So
        // a refactor that dropped `mean_subscription_mrr` or rewrote `subscription_months_billed` to
        // `count_distinct` would leave two generator arms rendered by no document in this repository,
        // with the shape and term assertions above still green.
        //
        // `min` and `max` are deliberately absent from the expectation rather than missing from it. A
        // metric here would have to mean something - a smallest monthly bill is a real figure and a
        // definition nobody asked for - and the set is written as an equality so adding one is a
        // decision that shows up in this diff.
        assert_eq!(
            aggregates,
            BTreeSet::from(["avg", "count", "count_distinct", "sum"]),
            "the example no longer writes every aggregate it is supposed to demonstrate"
        );
        // And the other closed set a ratio carries. Both words have to be reachable or the enum is
        // half-covered: `yields_null` guards the division, `fails` emits it unguarded and is caught
        // where the value crosses back into the domain, and the second one answered the string `inf`
        // under a certified metric name for exactly as long as no document chose it.
        assert_eq!(
            zero_denominators,
            BTreeSet::from(["fails", "yields_null"]),
            "the example no longer declares both meanings of a zero denominator"
        );
        assert!(
            with_required_filter > 0,
            "no metric in the example carries a required filter, which is the one feature that \
             changes what a metric means"
        );
        // A metric with a filter and one without, over the same model, is what makes the difference
        // legible as two numbers rather than as a sentence in a README.
        assert!(
            with_required_filter < pinned.definitions().metrics().len(),
            "every metric carries a required filter, so the example no longer contrasts one that does \
             with one that does not"
        );
    }

    #[test]
    fn every_question_in_the_example_compiles_and_its_statement_is_pinned() {
        // The bug this prevents: a generator change that is valid SQL, plans without complaint and
        // asks the data system for something else. The statement and its bound values are pinned
        // together, so a value that moved out of the parameter list and into the statement text is
        // a diff here rather than a discovery in production.
        //
        // It also asserts the naming convention in both directions. A `refused-` question that
        // started answering, or a plain question that started being refused, are both silent
        // failures: the corpus would still pass as "every question produced an outcome", and the
        // example would be demonstrating the opposite of what its name says.
        let pinned = load();
        let mut refusals = 0_usize;
        let mut statements = 0_usize;
        for path in questions() {
            let name = stem(&path);
            let question = read_question(&path);
            let compiled = compile(&question, &pinned).unwrap_or_else(|e| panic!("{name} would not compile: {e}"));
            let expected_refusal = name.starts_with(REFUSED_PREFIX);
            settings().bind(|| match compiled {
                Compiled::Refused { ref reason } => {
                    assert!(
                        expected_refusal,
                        "{name} was refused as {reason:?}, and is not named as a refusal"
                    );
                    refusals += 1;
                    insta::assert_yaml_snapshot!(format!("{name}__refusal"), reason);
                }
                Compiled::Planned { ref plan } => {
                    assert!(!expected_refusal, "{name} is named as a refusal and was answered");
                    statements += 1;
                    let query = sutura_sql::generate(plan, DIALECT).expect("a planned question renders");
                    insta::assert_snapshot!(format!("{name}__statement"), rendered(&query));
                }
                // Not reached by the example corpus, which is single-source; a federated plan has no
                // single statement (the CLI renders it leg by leg), so assert the split instead.
                Compiled::Federated { ref plan } => {
                    assert!(!expected_refusal, "{name} is named as a refusal and was federated");
                    statements += 1;
                    assert_eq!(plan.legs().len(), 2, "{name} federated into something other than two legs");
                }
            });
        }
        // Both halves have to be non-empty or the assertions above are vacuous: a corpus of only
        // refusals proves the compiler refuses everything, and one with none proves nothing about
        // refusals at all.
        assert!(statements > 0, "no question in the example produced a statement");
        assert!(
            refusals > 1,
            "the example is supposed to show a reader more than one kind of refusal"
        );
    }

    // ------------------------------------------------------------------------- against a database ---

    /// The example catalog, executed by the engine over the committed CSVs.
    ///
    /// No database and nothing to load: the engine reads the files, for the reason the `query`
    /// command gives - a database file in a repository is a binary nobody reviews, and a fixture
    /// read from the CSV every time cannot drift from it.
    /// It returns a registry, because that is what `verify_and_validate` and `answer` take: a plan
    /// selects the data system it names, and this example configures one.
    fn engine(pinned: &PinnedDefinitions) -> sutura_app::Warehouses<sutura_exec_datafusion::DataFusionWarehouse> {
        let sources = sutura_app::sources(pinned);
        let [source] = sources.as_slice() else {
            panic!(
                "the example catalog is single-source, and this one names {} systems",
                sources.len()
            );
        };
        // The same ceiling the `query` command uses, read through the same parse, so this exercises
        // the bound a person running the example actually gets. `available_memory_bytes` is asked
        // for the same reason it is there: a machine smaller than the default should say so here
        // rather than inside a join.
        let ceiling = sutura_config::WorkingSetCeiling::parse(
            sutura_config::WorkingSetCeiling::DEFAULT_BYTES,
            sutura_config::available_memory_bytes(),
        )
        .expect("the embedded default is a ceiling on this machine");
        // The posture the `sutura` command itself declares: this tool reads the files of whoever ran
        // it, as that person's own identity. Stated here rather than left to a default for the reason
        // `commands::single_user_posture` gives.
        let posture = sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse(
                    "the example is read by one person, as that person's own operating-system identity",
                )
                .expect("the fixture reason is a reason"),
            ),
        };
        let warehouse = sutura_exec_datafusion::DataFusionWarehouse::new(
            (*source).clone(),
            posture,
            sutura_exec_datafusion::WorkingSet::of_bytes(ceiling.bytes()),
        )
        .expect("the engine starts");
        let data = example_root().join("data");
        for model in pinned.definitions().models().values() {
            let csv = data.join(format!("{}.csv", model.table()));
            warehouse
                .attach_csv(model.table_name(), &csv)
                .unwrap_or_else(|e| panic!("could not attach {}: {e}", csv.display()));
        }
        sutura_app::Warehouses::of(warehouse)
    }

    /// The credential the example is answered with: the static broker, over the one source this build
    /// opens, under the same declaration the engine above was opened with.
    ///
    /// The REAL implementor rather than a fake, which is what `commands::query` uses too - so this
    /// test exercises the broker a person running `sutura` actually gets.
    fn single_user_broker() -> sutura_config::StaticCredentialBroker {
        sutura_config::StaticCredentialBroker::for_one_shared_source(
            sutura_domain::model::SourceName::parse("local").expect("the example source name is a name"),
            sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse(
                    "the example is read by one person, as that person's own operating-system identity",
                )
                .expect("the fixture reason is a reason"),
            ),
        )
    }

    /// Who the example's questions are asked by: nobody, truthfully.
    ///
    /// This command has no transport, so nothing establishes a caller identity, and
    /// `Subject::TheDeploymentItself` is the honest value rather than an invented one.
    fn a_caller() -> sutura_domain::identity::RequestContext {
        sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
            sutura_domain::identity::Subject::TheDeploymentItself,
        ))
    }

    #[test]
    fn every_declared_anchor_in_the_example_reproduces_its_number() {
        // The bug this prevents: an example whose numbers are aspirational. An anchor is a figure
        // somebody wrote into a catalog document by hand, and the only thing separating that from a
        // guess is this test re-executing it against the committed CSVs.
        //
        // It fails in both directions on purpose. Edit the data and the anchor stops matching; edit
        // the anchor and it stops matching the data. Either way the bundle is unservable rather
        // than answering, which is the behaviour the example exists to demonstrate.
        let pinned = load();
        let warehouse = engine(&pinned);
        let report = sutura_app::verify_anchors(&pinned, &warehouse);
        settings().bind(|| insta::assert_yaml_snapshot!("example_anchor_report", &report));
        let mut checked = 0_usize;
        for (metric, check) in report.checks() {
            assert_eq!(
                *check,
                sutura_domain::pinned::AnchorCheck::Matched,
                "{metric} did not reproduce its declared number"
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "no metric in the example declares an anchor, so this test proved nothing"
        );
        // The same anchors again, through the one operation that mints a servable bundle. The report
        // above is what an operator reads; this is what `sutura_app::answer` will accept, and it
        // cannot be obtained from a report at all.
        drop(sutura_app::verify_and_validate(pinned, &warehouse).expect("a bundle whose anchors all matched is fit to serve"));
    }

    #[test]
    fn every_question_in_the_example_answers_is_refused_or_fails_and_the_rows_are_pinned() {
        // The bug this prevents: a change that compiles to the same statement and returns different
        // rows. Nothing upstream of the data system can catch that, which is why the rows are
        // pinned here and not only the SQL - a CSV edited in the same commit is exactly the change
        // that would otherwise look reviewed.
        //
        // The refusals are asserted rather than snapshotted: the compile test already pins the
        // reason, and what matters at this end is that the question did not reach the data system.
        //
        // THREE outcomes, not two, and the third is not an escape hatch for a flaky corpus. A
        // question may be one the data system answered and the adapter will not carry:
        // `revenue_per_churned_subscription` declares `zero_denominator: fails`, so over a month
        // with no terminations the division is emitted unguarded, IEEE float division answers `inf`
        // rather than raising, and `Value::Real` refuses to hold it. That is the whole point of the
        // word, so the corpus has to be able to express it - and the failure is pinned as its error
        // chain, which is what would go red if the guard were removed and the string `inf` came back
        // under a certified metric name instead.
        let pinned = load();
        let warehouse = engine(&pinned);
        let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
        let mut failures = 0_usize;
        for path in questions() {
            let name = stem(&path);
            let question = read_question(&path);
            let answered = sutura_app::answer(&validated, &question, &a_caller(), &single_user_broker(), &warehouse, 1 << 30);
            let expected_refusal = name.starts_with(REFUSED_PREFIX);
            settings().bind(|| match answered.map(sutura_app::Answered::into_outcome) {
                Ok(sutura_domain::query::ToolOutcome::Refusal { ref reason }) => {
                    assert!(
                        expected_refusal,
                        "{name} was refused as {reason:?}, and is not named as a refusal"
                    );
                }
                Ok(sutura_domain::query::ToolOutcome::Answer { ref rows, .. }) => {
                    assert!(!expected_refusal, "{name} is named as a refusal and was answered");
                    insta::assert_yaml_snapshot!(format!("{name}__rows"), stable(rows));
                }
                Err(ref error) => {
                    // A refusal is decided before anything runs, so a `refused-` question that
                    // reached the data system at all is a hole in the governance rather than a
                    // failing fixture, whatever the error says.
                    assert!(
                        !expected_refusal,
                        "{name} is named as a refusal and instead reached the data system: {}",
                        chain(error)
                    );
                    failures += 1;
                    insta::assert_snapshot!(format!("{name}__error"), chain(error));
                }
            });
        }
        // Or the arm above is decoration. `fails` was a wish for as long as nothing executed it.
        assert!(
            failures > 0,
            "no question in the example fails, so `zero_denominator: fails` is once again a word \
             nothing in this corpus reaches"
        );
    }

    // ------------------------------------------------------------------- the agent prompt ---

    /// The prompt for the example, at one prose setting.
    ///
    /// Every operation, because the HTTP surface mounts every operation. `sutura-app`'s own tests
    /// are where a hidden operation is exercised; here the point is the document an operator would
    /// actually hand out.
    fn prompt(pinned: &PinnedDefinitions, prose: sutura_app::prompt::CatalogProse) -> String {
        sutura_app::prompt::render(
            pinned,
            &sutura_app::prompt::PromptInputs::new(sutura_app::prompt::Tool::ALL, prose, None),
        )
    }

    #[test]
    fn the_prompt_for_the_example_catalog_is_pinned() {
        // Generated text, so it is pinned as a snapshot rather than asserted against by substring -
        // the same rule the statements above follow, and for the same reason: a hand-written
        // expectation asserts what somebody wished the renderer produced.
        //
        // This is the whole document an operator would pipe into an agent's configuration for the
        // documented example, so a change to any part of it - a refusal remedy, a bound, a metric's
        // own prose - arrives as a reviewable diff. Both prose settings are pinned, because the
        // difference between them is a governance decision rather than a formatting one.
        let pinned = load();
        settings().bind(|| {
            insta::assert_snapshot!("example_prompt", prompt(&pinned, sutura_app::prompt::CatalogProse::Quoted));
            insta::assert_snapshot!(
                "example_prompt_without_prose",
                prompt(&pinned, sutura_app::prompt::CatalogProse::Omitted)
            );
        });
    }

    #[test]
    fn every_metric_and_every_permitted_value_in_the_example_reaches_the_prompt() {
        // The property the snapshot cannot state: the prompt is DERIVED from the bundle rather than
        // written beside it. A metric added to the example catalog with no line in the prompt would
        // be a metric an agent never asks about, and a snapshot diff alone would not say that is
        // what happened.
        //
        // Asserted over the permitted values as well as the names, because a value list that stopped
        // being rendered is the failure that turns every filter an agent writes into a refusal.
        let pinned = load();
        let text = prompt(&pinned, sutura_app::prompt::CatalogProse::Quoted);
        let mut values = 0_usize;
        for (name, metric) in pinned.definitions().metrics() {
            assert!(text.contains(&format!("### {name}")), "{name} is missing from the prompt");
            for (dimension, declared) in metric.dimensions() {
                assert!(
                    text.contains(&format!("`{dimension}`")),
                    "dimension {dimension} of {name} is missing from the prompt"
                );
                for value in declared.allowed_values().into_iter().flatten() {
                    assert!(
                        text.contains(value.as_str()),
                        "value {value} of {dimension} on {name} is missing from the prompt"
                    );
                    values += 1;
                }
            }
        }
        assert!(values > 0, "the example declares no permitted value, so this proved nothing");
        // And the other direction: no table name from the catalog reaches the document. The prompt
        // renders what `GET /v1/catalog` renders and not one field more, because a table name in an
        // agent's context is a name it will eventually try to use and the surface has no field for
        // one. `sutura-app`'s own tests cover column and model names against a fixture whose prose
        // does not mention them; the example's prose does mention columns, so the table is what is
        // checkable over this corpus.
        for model in pinned.definitions().models().values() {
            assert!(
                !text.contains(model.table_name().as_str()),
                "table {} reached the prompt",
                model.table()
            );
        }
    }
}

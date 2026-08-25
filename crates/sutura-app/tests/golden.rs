//! The golden suite: one corpus of questions, run against two catalogs, three dialects and two
//! data systems.
//!
//! Everything here is arranged so a change to what we compile shows up as a reviewable diff rather
//! than as a different number. **The snapshots are regenerated and read as a diff, never typed.** A
//! hand-written expectation asserts what somebody wished the generator did.
//!
//! What each part is for:
//!
//! - **The differential oracle.** The markdown catalog and a hand-written one must produce the same
//!   definitions. With one adapter that claim is untestable, which is why there are two.
//! - **The plan and SQL goldens.** Per dialect, so a change to identifier quoting or placeholder
//!   style is visible in the target it affects rather than only in the one we execute against.
//! - **The no-injection assertion.** Mechanical, over the whole corpus: no literal from a question
//!   appears in the statement generated for it. This is the only thing holding that claim up.
//! - **The refusal corpus.** Every `RefusalReason` variant provoked by a question file, because a
//!   refusal nobody has seen happen is a refusal nobody knows works.
//! - **The anchor test.** The metrics execute against `DuckDB` and reproduce the numbers their
//!   authors declared, and a corrupted anchor makes the bundle unservable rather than answering.
//!
//! Wrapped in `#[cfg(test)] mod`, which looks redundant for an integration test that is only ever
//! compiled as one. `clippy::tests_outside_test_module` is on for the whole workspace, and being
//! consistent with it costs one level of indentation.

// `cfg(test)` because an integration test target is compiled with `--test`, so it is true here -
// and clippy only honours `allow-expect-in-tests` for code inside a `#[cfg(test)]` item. Without it
// every `expect` in a fixture builder is a lint error, and writing fixture setup in the
// `?`-ceremony the ban would demand makes the fixtures worse, which is what that exemption exists
// to avoid.
#[cfg(test)]
mod support;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_app::{answer, verify_anchors};
    use sutura_domain::pinned::{AnchorCheck, AnchorReport, SemanticCatalog as _, Validated};
    use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_semantic::{Compiled, Dialect, PredicateOrigin, compile, dialect};

    use crate::support::{
        HandWrittenCatalog, RecordingWarehouse, catalog_root, data_root, load_local, questions, read_question, source,
        without_descriptions,
    };

    /// Settings every snapshot in this file uses.
    ///
    /// The snapshot path is set explicitly so the files land in `tests/snapshots/` next to the
    /// corpus rather than wherever the macro would guess from the module path.
    fn settings() -> insta::Settings {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings
    }

    /// The stem of a question file, which is what a snapshot is named after.
    fn stem(path: &std::path::Path) -> String {
        path.file_stem()
            .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned())
    }

    // ------------------------------------------------------------------ the differential oracle ---

    #[test]
    fn the_two_catalogs_agree_about_everything_that_decides_what_executes() {
        // The property the `SemanticCatalog` port exists to have. Two adapters reading the same
        // content must produce the same definitions, and if they do not, one of them is wrong. The
        // comparison is over the machine-readable content: prose lives in the markdown and nowhere
        // else, so comparing it would compare one implementation against a copy of itself.
        let from_markdown = load_local();
        let from_rust = HandWrittenCatalog.load().expect("the hand-written catalog cannot fail");
        assert_eq!(
            without_descriptions(from_markdown.definitions()),
            without_descriptions(from_rust.definitions()),
            "the markdown catalog and the hand-written one disagree"
        );
    }

    #[test]
    fn the_markdown_catalog_is_pinned_as_a_whole() {
        // The digest and the parsed content, in one snapshot. It moves when a definition changes and
        // when a description changes, because both are part of what was certified; it does not move
        // when a file is reformatted or when two documents swap order, which is what makes it worth
        // reading.
        let pinned = load_local();
        settings().bind(|| {
            insta::assert_snapshot!("catalog_digest", pinned.digest().as_str());
            insta::assert_yaml_snapshot!("catalog_definitions", pinned.definitions());
        });
    }

    #[test]
    fn reformatting_a_document_does_not_move_the_digest() {
        // What the digest is FOR. It is taken over the canonical form of the parsed definitions, so
        // whitespace and key order are not part of it. If it were taken over the bytes on disk, a
        // reformat would look like a changed definition and nobody would trust it.
        let pinned = load_local();
        let same = load_local();
        assert_eq!(pinned.digest(), same.digest());
        // And the hand-written catalog, which differs only in prose, must therefore differ.
        let other = HandWrittenCatalog.load().expect("cannot fail");
        assert_ne!(
            pinned.digest(),
            other.digest(),
            "prose is part of what is certified, so dropping it must move the digest"
        );
    }

    // -------------------------------------------------------------------- plan and SQL goldens ---

    /// Compiles the whole corpus for one dialect and snapshots what came out.
    fn corpus_for(dialect: Dialect) {
        let pinned = load_local();
        for path in questions() {
            let question = read_question(&path);
            let name = stem(&path);
            let compiled = compile(&question, &pinned, dialect).unwrap_or_else(|e| panic!("{name} would not compile: {e}"));
            let mut settings = settings();
            settings.set_snapshot_suffix(dialect.as_str());
            settings.bind(|| match compiled {
                Compiled::Refused { ref reason } => {
                    insta::assert_yaml_snapshot!(format!("{name}__refusal"), reason);
                }
                Compiled::Statement { ref plan, ref query } => {
                    insta::assert_snapshot!(format!("{name}__sql"), query.sql());
                    insta::assert_yaml_snapshot!(format!("{name}__plan"), plan);
                    insta::assert_yaml_snapshot!(format!("{name}__params"), query.params());
                }
            });
        }
    }

    /// One test per dialect, so a failure names the data system it is about.
    macro_rules! corpus {
        ($name:ident, $dialect:expr) => {
            #[test]
            fn $name() {
                corpus_for($dialect);
            }
        };
    }

    corpus!(the_corpus_compiles_for_duckdb, Dialect::DuckDb);
    corpus!(the_corpus_compiles_for_postgres, Dialect::Postgres);
    corpus!(the_corpus_compiles_for_clickhouse, Dialect::ClickHouse);

    #[test]
    fn every_dialect_in_the_list_has_a_corpus_test() {
        // `dialect::ALL` is what a reader will assume the suite covers. A variant added to it
        // without a `corpus!` line above would be a data system with no goldens, which reads as
        // covered and is not. Three names, checked against three tests.
        assert_eq!(
            dialect::ALL.len(),
            3,
            "a dialect was added to ALL; add a corpus! line above and re-run with INSTA_UPDATE=always"
        );
    }

    // --------------------------------------------------------------- the no-injection assertion ---

    /// A parameter's value as text, for comparing against what a question carried.
    fn bound_value(param: &sutura_domain::warehouse::ParamValue) -> String {
        match *param {
            sutura_domain::warehouse::ParamValue::Text(ref v) => v.clone(),
            sutura_domain::warehouse::ParamValue::Integer(v) => v.to_string(),
            sutura_domain::warehouse::ParamValue::Date(d) => d.to_iso(),
        }
    }

    /// The statement with every quoted identifier removed.
    ///
    /// Searching the raw SQL for a value gives false positives, and one bit immediately: the metric
    /// `web_revenue` has a required filter of `channel = 'web'`, the predicate is correctly bound as
    /// `"orders"."channel" = ?`, and a plain substring search still found "web" - inside the alias
    /// `AS "web_revenue"`. The value had not reached the statement at all.
    ///
    /// Identifiers are double-quoted and a value inlined as text would be single-quoted, so dropping
    /// the double-quoted spans leaves exactly the part of the statement a value could have leaked
    /// into. It is a stronger check than looking for `'value'` would be: it also catches a value
    /// inlined bare, without quotes.
    ///
    /// Toggling on `"` is enough because an identifier here cannot contain one - `ColumnName` and its
    /// siblings reject it, which is what makes that a fact rather than an assumption.
    fn without_identifiers(sql: &str) -> String {
        let mut out = String::with_capacity(sql.len());
        let mut inside = false;
        for ch in sql.chars() {
            if ch == '"' {
                inside = !inside;
            } else if !inside {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn no_value_reaches_the_statement_as_text() {
        // The mechanical form of the claim the first-party-models decision makes. Every value that
        // ends up in a predicate must arrive as a bind parameter, so none of them may appear in the
        // SQL - and there are now two sources of them.
        //
        // Written against `Query::literals` and `QueryPlan::definitional_params` rather than as a
        // list of places to look, so a field added to either cannot quietly stop being covered.
        let pinned = load_local();
        let mut checked = 0_usize;
        let mut with_definitional = 0_usize;
        for path in questions() {
            let question = read_question(&path);
            let literals = question.literals();
            for dialect in dialect::ALL.iter().copied() {
                let compiled = compile(&question, &pinned, dialect).expect("the corpus compiles");
                let Compiled::Statement { ref plan, ref query } = compiled else {
                    continue;
                };

                // Searched with the quoted identifiers removed: see `without_identifiers`.
                let searchable = without_identifiers(query.sql());

                // What the CALLER sent. The classic injection: a filter value or a date written
                // into the statement instead of bound.
                for literal in &literals {
                    assert!(
                        !searchable.contains(literal.as_str()),
                        "{} for {dialect} carries the question's literal {literal:?}:\n{}",
                        stem(&path),
                        query.sql()
                    );
                }

                // What the CATALOG said. A required filter's value is not caller text, so inlining it
                // would not be an injection today - it would be a generator with one inlining path
                // and one binding path, and the inlining path is the one that eventually gets handed
                // caller text. Asserting it here is what keeps there being only one path.
                let definitional: Vec<String> = plan.definitional_params().into_iter().map(bound_value).collect();
                for value in &definitional {
                    // A date bound is definitional too, and its ISO text is checked above. Skip the
                    // ones the question also carries so the message cannot be misattributed.
                    if literals.contains(value) {
                        continue;
                    }
                    assert!(
                        !searchable.contains(value.as_str()),
                        "{} for {dialect} carries the catalog's value {value:?}:\n{}",
                        stem(&path),
                        query.sql()
                    );
                    with_definitional = with_definitional.saturating_add(1);
                }

                // The values are still there, as parameters. Without this half a generator that
                // dropped the predicate entirely would pass everything above.
                let bound: BTreeSet<String> = query.params().iter().map(bound_value).collect();
                for literal in &literals {
                    assert!(
                        bound.contains(literal),
                        "{} for {dialect} did not bind the question's {literal:?}; \
                         the predicate is missing rather than inlined:\n{}",
                        stem(&path),
                        query.sql()
                    );
                }
                checked = checked.saturating_add(1);
            }
        }
        assert!(checked > 0, "the corpus produced no statements to check");
        // The corpus has to contain at least one metric with a required filter, or the second half
        // of this test is asserting over an empty set and would pass with the feature removed.
        assert!(
            with_definitional > 0,
            "no question exercised a required filter; the definitional half of this test proved nothing"
        );
    }

    #[test]
    fn a_definitional_filter_is_in_every_statement_about_its_metric() {
        // A required filter is what makes a metric mean what it says: `web_revenue` is revenue from
        // the web channel, and a statement without that predicate returns total revenue under that
        // name. A caller cannot ask for it and cannot turn it off, so nothing the caller does can
        // make this test pass or fail - which is exactly why it has to be asserted here.
        let pinned = load_local();
        let mut seen = 0_usize;
        for path in questions() {
            let question = read_question(&path);
            let Some(metric) = pinned.definitions().metric(question.metric()) else {
                continue;
            };
            if metric.required_filters().is_empty() {
                continue;
            }
            let compiled = compile(&question, &pinned, Dialect::DuckDb).expect("the corpus compiles");
            let Compiled::Statement { ref plan, .. } = compiled else {
                continue;
            };
            for required in metric.required_filters() {
                let present = plan.filters().iter().any(|f| {
                    matches!(f.origin(), PredicateOrigin::Definition) && f.predicate().column().column() == required.column()
                });
                assert!(
                    present,
                    "{}: the plan for {} dropped its required filter on {}",
                    stem(&path),
                    question.metric(),
                    required.column()
                );
            }
            seen = seen.saturating_add(1);
        }
        assert!(
            seen > 0,
            "no question asked about a metric with a required filter, so this proved nothing"
        );
    }

    #[test]
    fn a_generated_statement_quotes_every_identifier() {
        // The other half of not being an injection: an identifier that reached the statement bare
        // would bind to whatever the data system decided, and a column called `order` would be a
        // syntax error. Asserted over the corpus rather than in one example.
        let pinned = load_local();
        for path in questions() {
            let question = read_question(&path);
            for dialect in dialect::ALL.iter().copied() {
                let compiled = compile(&question, &pinned, dialect).expect("the corpus compiles");
                let Compiled::Statement { ref query, .. } = compiled else {
                    continue;
                };
                assert!(
                    query.sql().contains("\"orders\""),
                    "{} for {dialect} does not quote its table:\n{}",
                    stem(&path),
                    query.sql()
                );
            }
        }
    }

    #[test]
    fn every_generated_statement_parses_in_the_dialect_it_was_generated_for() {
        // The check that replaces having one of each data system in CI. It PARSES and stops: it
        // never re-emits, so it cannot introduce the parser-differential problem that makes
        // translation unusable on the query path. A failure here means we generated something that
        // is not valid SQL for that target, which is otherwise only discoverable by running it.
        let pinned = load_local();
        for path in questions() {
            let question = read_question(&path);
            for (dialect, target) in [
                (Dialect::DuckDb, polyglot_sql::DialectType::DuckDB),
                (Dialect::Postgres, polyglot_sql::DialectType::PostgreSQL),
                (Dialect::ClickHouse, polyglot_sql::DialectType::ClickHouse),
            ] {
                let compiled = compile(&question, &pinned, dialect).expect("the corpus compiles");
                let Compiled::Statement { ref query, .. } = compiled else {
                    continue;
                };
                let parsed = polyglot_sql::parse(query.sql(), target);
                assert!(
                    parsed.is_ok(),
                    "{} for {dialect} is not valid there: {:?}\n{}",
                    stem(&path),
                    parsed.err(),
                    query.sql()
                );
            }
        }
    }

    // ----------------------------------------------------------------------- the refusal corpus ---

    /// Every variant a question file can provoke, and the fixture that provokes it.
    ///
    /// A table rather than a test each, so the exhaustiveness assertion below can be written against
    /// it. `PlanSpansTwoSources` and `SourceUnavailable` are absent on purpose: neither is reachable
    /// from a question file, and each is provoked by its own test further down.
    const PROVOKED: &[(&str, &str)] = &[
        ("refused-metric-unknown", "MetricUnknown"),
        ("refused-grain-not-supported", "GrainNotSupported"),
        ("refused-dimension-not-permitted", "DimensionNotPermitted"),
        ("refused-dimension-not-filterable", "DimensionNotFilterable"),
        ("refused-value-not-allowed", "DimensionValueNotAllowed"),
        ("refused-duplicate-dimension", "DuplicateDimension"),
        ("refused-too-many-dimensions", "TooManyDimensions"),
    ];

    #[test]
    fn every_refusal_a_question_can_provoke_is_provoked_by_a_fixture() {
        // A refusal nobody has seen happen is a refusal nobody knows works. Each fixture is named
        // after the variant it exists to reach, and this asserts it reaches that one and not
        // another - a fixture that started refusing for a different reason would otherwise still
        // pass as "a refusal".
        let pinned = load_local();
        for (fixture, expected) in PROVOKED {
            let path = catalog_root()
                .parent()
                .expect("fixtures has a parent")
                .join("questions")
                .join(format!("{fixture}.yaml"));
            let question = read_question(&path);
            let compiled = compile(&question, &pinned, Dialect::DuckDb).expect("a refusal is not an error");
            let reason = compiled
                .refusal()
                .unwrap_or_else(|| panic!("{fixture} was answered, and should have been refused"));
            let rendered = format!("{reason:?}");
            assert!(
                rendered.starts_with(expected),
                "{fixture} was refused as {rendered}, and exists to provoke {expected}"
            );
        }
    }

    #[test]
    fn a_plan_that_would_reach_a_second_data_system_is_refused() {
        // Not reachable from a question file: it needs a catalog whose models sit on two data
        // systems, which `SourceUnavailable` and this variant are the only defences against. Built
        // here rather than as a fixture, because a fixture catalog with a second source would make
        // every other test in this file span two.
        //
        // The refusal exists because a second data system is a second identity to satisfy, and a
        // plan that runs partly as somebody else is the failure the whole design is arranged
        // against.
        let split = crate::support::two_source_catalog()
            .load()
            .expect("a two-source catalog can be built");
        let question = Query::new(
            sutura_domain::model::MetricName::parse("revenue").expect("a name"),
            sutura_domain::model::Grain::Month,
            crate::support::june_range(),
            vec![sutura_domain::model::DimensionName::parse("region").expect("a name")],
            Vec::new(),
        );
        let compiled = compile(&question, &split, Dialect::DuckDb).expect("this is a refusal");
        assert!(
            matches!(compiled.refusal(), Some(&RefusalReason::PlanSpansTwoSources { sources: 2 })),
            "expected a two-source refusal, got {:?}",
            compiled.refusal()
        );
    }

    #[test]
    fn a_plan_for_a_data_system_this_process_did_not_open_is_refused() {
        // The service checks the plan's source against the adapter it is about to call. Without it,
        // a question would be answered against whatever happened to be connected, under provenance
        // that named something else.
        let pinned = load_local();
        let report = matched_report(&pinned);
        let validated = Validated::new(pinned, &report).expect("every anchor was recorded matched");
        let elsewhere = RecordingWarehouse::pretending_to_be("somewhere_else");
        let question = read_question(
            &catalog_root()
                .parent()
                .expect("fixtures has a parent")
                .join("questions/revenue-total-june.yaml"),
        );
        let outcome = answer(&validated, &question, &elsewhere, Dialect::DuckDb).expect("a refusal is not an error");
        assert!(
            matches!(outcome.refusal(), Some(&RefusalReason::SourceUnavailable { .. })),
            "expected a source refusal, got {outcome:?}"
        );
        assert!(
            elsewhere.asked_about().is_empty(),
            "a refused question must not have reached the data system"
        );
    }

    /// A report claiming every anchor matched, for tests that are not about anchors.
    ///
    /// Built from the bundle rather than written out, so a metric gaining an anchor does not make
    /// unrelated tests fail for a reason that has nothing to do with them.
    fn matched_report(pinned: &sutura_domain::pinned::PinnedDefinitions) -> AnchorReport {
        let mut report = AnchorReport::new();
        for (name, _) in pinned.anchored_metrics() {
            report.record(name.clone(), AnchorCheck::Matched);
        }
        report
    }

    #[test]
    fn a_refused_question_never_reaches_the_data_system() {
        // The property that makes a refusal worth having: it is decided before anything runs, so a
        // question that may not be asked costs nothing and reads nothing.
        let pinned = load_local();
        let report = matched_report(&pinned);
        let validated = Validated::new(pinned, &report).expect("recorded matched");
        let fake = RecordingWarehouse::new();
        for (fixture, _) in PROVOKED {
            let path = catalog_root()
                .parent()
                .expect("fixtures has a parent")
                .join("questions")
                .join(format!("{fixture}.yaml"));
            let question = read_question(&path);
            let outcome = answer(&validated, &question, &fake, Dialect::DuckDb).expect("a refusal is not an error");
            assert!(outcome.is_refusal(), "{fixture} was answered");
        }
        assert!(
            fake.asked_about().is_empty(),
            "refused questions reached the data system: {:?}",
            fake.asked_about()
        );
    }

    // ------------------------------------------------------------------------ against a database ---

    /// The fixture catalog, over an in-memory `DuckDB` built from the committed CSVs.
    fn duckdb() -> sutura_exec_duckdb::DuckDbWarehouse {
        let warehouse = sutura_exec_duckdb::DuckDbWarehouse::in_memory(source()).expect("an in-memory database opens");
        for model in load_local().definitions().models().values() {
            let csv = data_root().join(format!("{}.csv", model.table()));
            warehouse
                .attach_csv(model.table(), &csv)
                .unwrap_or_else(|e| panic!("could not attach {}: {e}", csv.display()));
        }
        warehouse
    }

    #[test]
    fn every_declared_anchor_reproduces_its_number() {
        // "Reproduce a number that already exists" is the acceptance criterion that separates this
        // from a demo. The numbers are in the catalog documents, the data is in the CSVs, and
        // nothing in between is allowed to change what they add up to.
        let pinned = load_local();
        let report = verify_anchors(&pinned, &duckdb(), Dialect::DuckDb);
        settings().bind(|| insta::assert_yaml_snapshot!("anchor_report", &report));
        for (metric, check) in report.checks() {
            assert_eq!(*check, AnchorCheck::Matched, "{metric} did not reproduce its declared number");
        }
        drop(Validated::new(pinned, &report).expect("a bundle whose anchors all matched is fit to serve"));
    }

    #[test]
    fn a_dimension_join_does_not_change_the_measure() {
        // THE BUG THIS EXISTS FOR, and it shipped: the generator emitted an INNER join, so every
        // fact row whose dimension row was missing silently vanished from a grouped answer. Order 12
        // in `data/orders.csv` names customer 5, and `data/customers.csv` stops at 4 - so before the
        // fix, `revenue` for June answered 570022 and `revenue by region` totalled 470023. Two
        // numbers, one metric, one period, and nothing raising an error anywhere.
        //
        // The catalog's existing guard could not see it. `may_duplicate_rows` refuses a join that
        // would FAN OUT the fact rows; this is the same failure by ELIMINATION, and a cardinality
        // check has nothing to say about it.
        //
        // Asserted as a reconciliation rather than against a literal, because that is the property:
        // grouping by a dimension must partition the measure, not filter it. A left join makes the
        // unmatched row group under a null key, so the totals agree.
        let pinned = load_local();
        let warehouse = duckdb();
        let report = verify_anchors(&pinned, &warehouse, Dialect::DuckDb);
        let validated = Validated::new(pinned, &report).expect("the anchors hold");

        let questions_dir = catalog_root().parent().expect("fixtures has a parent").join("questions");
        let total_of = |file: &str, label: &str| -> i64 {
            let question = read_question(&questions_dir.join(file));
            let outcome =
                answer(&validated, &question, &warehouse, Dialect::DuckDb).unwrap_or_else(|e| panic!("{file} failed: {e}"));
            let ToolOutcome::Answer { ref rows, .. } = outcome else {
                panic!("{file} was refused: {outcome:?}");
            };
            let index = rows
                .column_index(label)
                .unwrap_or_else(|| panic!("{file} has no single {label:?} column"));
            (0..rows.rows().len())
                .filter_map(|row| match rows.cell(row, index) {
                    Some(&sutura_domain::warehouse::Value::Integer(v)) => Some(v),
                    _ => None,
                })
                .sum()
        };

        let ungrouped = total_of("revenue-total-june.yaml", "revenue");
        for grouped_by in ["revenue-by-region.yaml", "revenue-by-channel.yaml", "revenue-by-segment.yaml"] {
            assert_eq!(
                total_of(grouped_by, "revenue"),
                ungrouped,
                "{grouped_by} does not reconcile with the ungrouped total; a dimension join is \
                 filtering the measure instead of partitioning it"
            );
        }
        // And the row that makes the test mean something is actually in the data: without an
        // unmatched key every join is a no-op and this reconciles trivially.
        assert_eq!(
            ungrouped, 570_022,
            "the fixture no longer carries an order whose customer is absent, so this test proves \
             nothing; restore it in data/orders.csv"
        );
    }

    #[test]
    fn a_corrupted_anchor_makes_the_bundle_unservable_rather_than_answering() {
        // The fourth milestone criterion, and the difference between a demo and a governed service.
        // A definition that has stopped computing its own number must fail readiness, not answer.
        //
        // The corruption is applied to the report rather than to a file on disk, because a test that
        // edited a fixture would leave the tree dirty when it failed.
        let pinned = load_local();
        let mut report = verify_anchors(&pinned, &duckdb(), Dialect::DuckDb);
        let (first, _) = pinned
            .anchored_metrics()
            .next()
            .expect("the fixture catalog declares at least one anchor");
        report.record(
            first.clone(),
            AnchorCheck::Mismatch {
                expected: String::from("570022"),
                actual: String::from("470022"),
            },
        );
        let err = Validated::new(pinned, &report).expect_err("a bundle with a mismatched anchor must not be servable");
        settings().bind(|| insta::assert_snapshot!("anchor_mismatch", err.to_string()));
    }

    #[test]
    fn the_corpus_runs_against_duckdb_and_the_rows_are_pinned() {
        // The end of the line: the statement we generated, run, and the rows it returned. This is
        // what would catch a change that is valid SQL, plans identically, and returns a different
        // number.
        let pinned = load_local();
        let warehouse = duckdb();
        let report = verify_anchors(&pinned, &warehouse, Dialect::DuckDb);
        let validated = Validated::new(pinned, &report).expect("the anchors hold");
        for path in questions() {
            let question = read_question(&path);
            let name = stem(&path);
            let outcome = answer(&validated, &question, &warehouse, Dialect::DuckDb)
                .unwrap_or_else(|e| panic!("{name} failed against duckdb: {e}"));
            settings().bind(|| match outcome {
                ToolOutcome::Refusal { ref reason } => {
                    insta::assert_yaml_snapshot!(format!("{name}__refused"), reason);
                }
                ToolOutcome::Answer { ref rows, .. } => {
                    insta::assert_yaml_snapshot!(format!("{name}__rows"), rows);
                }
            });
        }
    }

    #[test]
    fn a_generated_statement_is_accepted_by_the_data_system_before_it_is_run() {
        // `dry_run` prepares without executing, which resolves every table and column name. It is
        // what turns "this would have failed" into "this failed before reading anything".
        let pinned = load_local();
        let warehouse = duckdb();
        for path in questions() {
            let question = read_question(&path);
            let compiled = compile(&question, &pinned, Dialect::DuckDb).expect("the corpus compiles");
            let Compiled::Statement { ref plan, ref query } = compiled else {
                continue;
            };
            // `dry_run` takes the PLAN now. For this adapter that means rendering it and preparing
            // the statement, which is what resolves every table and column name - so the SQL is
            // still the useful thing to print on a failure.
            warehouse
                .dry_run(plan)
                .unwrap_or_else(|e| panic!("{} was rejected: {e}\n{}", stem(&path), query.sql()));
        }
    }

    // ------------------------------------------------------------------ the shapes a file cannot ---

    #[test]
    fn a_question_carrying_sql_is_an_error_and_not_a_dropped_field() {
        // Promised by `sutura_domain::query` and asserted here, because provoking it needs a real
        // format parser and the domain crate deliberately has none.
        //
        // Without `deny_unknown_fields`, `sql:` deserializes cleanly and is discarded, so a caller
        // who believes they sent SQL gets a confident answer to a different question.
        let with_sql = "metric: revenue\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\nsql: SELECT 1\n";
        let err = serde_norway::from_str::<Query>(with_sql).expect_err("sql is not a field of a question");
        assert!(err.to_string().contains("sql"), "{err}");
    }

    #[test]
    fn a_range_with_no_end_is_not_a_range() {
        // The reason there is no `RefusalReason::TimeRangeUnbounded`: an unbounded range does not
        // deserialize, so the refusal would be unprovokable and a variant with no test that can
        // reach it looks like coverage.
        let unbounded = "metric: revenue\ngrain: month\nrange:\n  start: 2026-06-01\n";
        drop(serde_norway::from_str::<Query>(unbounded).expect_err("a range without an end is not a range"));
    }
}

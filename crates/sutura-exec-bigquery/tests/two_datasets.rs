//! **The cross-dataset leg**: the example corpus loaded into TWO datasets, and every metric whose
//! fact table and dimension table now live in different ones joined in one statement, against a
//! real project.
//!
//! # What this claims that no other leg here does
//!
//! `docs/adr/0019` decided how a table outside the connection's own dataset is addressed and pinned
//! it in goldens; `tests/acceptance.rs` then showed on a real project that a qualified path
//! *resolves* - the same table read three ways answering one pair of numbers, with a wrong dataset
//! refused as the control. **What neither reaches is a JOIN whose two sides live in two different
//! datasets**, which is the thing an operator does on day one, and which that record names as a gap
//! rather than a hedge:
//!
//! > No second dataset was reached, and no second project. [...] **A cross-project JOIN was not
//! > executed.** It is rendered, parse-checked, and pinned as one statement with one `JOIN` and one
//! > `SourceName` [...] but no local test can show a service performing it, and none pretends to.
//!
//! So this leg splits the corpus across two datasets - every model a relationship points AT into
//! the second, every other model into the first - and then asks the ordinary questions through the
//! ordinary path. A parse check cannot reach it: `docs/adr/0017` measured that a parse check cannot
//! see a function's argument order, and it cannot see a BINDING either.
//!
//! # What a green run here establishes, and what it does not
//!
//! | Claim | Established by a green run here |
//! | --- | --- |
//! | A statement whose `FROM` and whose `JOIN` name two different datasets is accepted, executed, and answers the engine's rows | yes |
//! | The qualifier is what bound the join, rather than a default dataset silently resolving the last part | yes - the dimension tables exist in the SECOND dataset ONLY, so the same question asked unqualified is refused |
//! | One statement, one source - Decision 2 of `docs/adr/0019` - executed rather than rendered | yes |
//! | A qualifier naming a dataset the credential cannot read is refused by the data system | yes, in the 404 shape. A dataset in a project the credential cannot read at all is the 403 shape, and that needs a second project |
//! | **Two PROJECTS** - a read whose leading path part is a project the credential does not bill to | **no.** One acceptance project is configured, so the leading part is still only proved to *resolve* (by `tests/acceptance.rs`) and never to cross. `docs/adr/0019`'s *"the one thing to re-run when a second project exists"* is still exactly that |
//! | Anything about identity | **no.** The credential is one service-account key for everybody who asks, so a green here is *accepted, and correct for that identity* |
//!
//! # Three decisions worth arguing with
//!
//! **A fourth target rather than more of `tests/corpus.rs`, and the reason is not the line cap.** It
//! is that this leg needs a SECOND DATASET the credential can write to, and folding it into
//! `just bigquery-acceptance` would make that leg fail for every developer and every CI run until
//! that dataset is provisioned - turning a leg people can run into one they cannot. So it has its
//! own task and its own app, and `bigquery-acceptance` filters this BINARY out, which is the shape
//! `tests/two_principals.rs` established for the same reason.
//!
//! **The two sides compile against two bundles, and that is forced rather than chosen.**
//! `sutura_exec_datafusion` registers one file per model with nothing above it, so a qualified path
//! names nothing it holds - `QualifiedTableUnreachable` is its refusal, and `docs/adr/0019` cites
//! the same fact as the reason the committed corpus is left unqualified. The engine therefore reads
//! the run's suffixed table names bare, and `BigQuery` reads the SAME names under two dataset
//! qualifiers. **The two bundles differ in exactly the qualifier and in nothing else** - same
//! models, same columns, same relationships, same metrics, same suffixed table names - which is
//! what makes the row comparison a comparison of ANSWERS rather than of two questions, and
//! [`tests::the_engine_s_bundle_stays_unqualified_and_the_other_is_qualified_over_the_same_tables`]
//! is the deterministic guard on it.
//!
//! **The comparison is the corpus leg's own.** [`crate::differential::agreement_between`] is shared
//! rather than restated, so no second notion of agreement is invented here - a looser copy in this
//! file would read as coverage while agreeing about less.
//!
//! # How to run it
//!
//! ```text
//! just bigquery-two-datasets
//! ```
//!
//! Which needs everything `just bigquery-acceptance` needs plus one value, and **its name is here
//! while its value is not, and will not be** - a dataset id is one of the things this repository
//! does not write down.
//!
//! | Variable | What it names |
//! | --- | --- |
//! | `SUTURA_BQ_SECOND_DATASET` | the second dataset, in the same project and the same LOCATION as `SUTURA_BQ_DATASET`, that this leg's joins read across |
//!
//! `test-infra/pulumi/google` creates it and grants the acceptance credential `dataEditor` on it, so
//! `just infra-up` followed by `just infra-set` is the mechanism rather than a sentence asking
//! somebody. **The location is the one way this is misconfigurable without a refusal that names
//! it:** `BigQuery` refuses a query whose tables live in two locations, and it says so - the leg
//! goes red on its first statement rather than answering anything.
//!
//! **`#[ignore]`d for `tests/acceptance.rs`'s reasons** - a fork's pull request sees no secret, a
//! gate needing a cloud project fails for an environment reason, and the nix sandbox has no network
//! - and, like that file, an unconfigured run **fails rather than skips**. Four tests here are not
//! `#[ignore]`d: they are controls on this file's own split, they read no environment and open no
//! socket, so a defect in the thing both live legs rest on fails in the gate every change runs.
//!
//! # What one run costs, and it is less than the corpus leg
//!
//! Every load is a `CREATE TABLE AS SELECT` over a literal array, which scans nothing, and every
//! statement in [`tests::the_default_dataset_cannot_answer_what_the_two_datasets_answer`] is a dry
//! run, which the endpoint charges nothing for and which uses no slots. Only
//! [`tests::a_metric_whose_tables_live_in_two_datasets_answers_the_rows_the_engine_answers`]
//! executes, at the 10 MiB-per-table minimum, and `support::bounds` caps every job at a gibibyte
//! regardless.
//!
//! **It WRITES to both datasets**, and every table it writes carries this run's own token and a
//! 24-hour expiration - the mechanism telekom/sutura#119 put in, unchanged and now applied twice.
//! Two runs against one pair of datasets create, read and drop only their own tables.

// `required-features = ["wire", "fixtures"]` is declared on the target in `Cargo.toml`, for the
// reason `tests/acceptance.rs` states: cargo skips the target rather than compiling an empty binary.

// The corpus reaches OUTSIDE this crate, into `examples/single-player` - the dependency
// `tests/corpus.rs` documents against `flake.nix`'s source filter, and it is the same one here.

// `cfg(test)` around the whole file, which is the house pattern rather than a preference: clippy
// honours `allow-expect-in-tests` only for code inside a `#[cfg(test)]` item.
#[cfg(test)]
mod support;

// The corpus differential harness, shared with `tests/corpus.rs`: the example corpus, the engine
// beside it, and the one comparison both legs are judged by. Its header carries why sharing it is
// the point rather than a convenience.
#[cfg(test)]
mod differential;

// The per-run table naming harness, also shared: every table either leg writes carries this run's
// token, which is what stops two runs against one dataset replacing each other's fixtures.
#[cfg(test)]
mod naming;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use sutura_domain::catalog::{Definitions, Description, Metric, Model, Relationship};
    use sutura_domain::model::{DatasetName, ModelName, Qualification, QualifiedTable, TableQualifier};
    use sutura_domain::pinned::PinnedDefinitions;
    use sutura_domain::plan::{Executable, QueryPlan};
    use sutura_domain::query::ToolOutcome;
    use sutura_domain::warehouse::{PreFlight, Warehouse as _};
    use sutura_exec_bigquery::transport::DatasetId;
    use sutura_sql::Dialect;

    use crate::differential::{
        DIVIDES_BY_ZERO, GrantsWhatEachSideDeclares, a_caller, agreement_between, bundle, chain, drop_the_corpus,
        engine, load_the_corpus, loader, loading_bounds, posture_of_the_engine_presented, questions, read_question,
        source, stem,
    };
    use crate::naming::{run_token, suffixed_bundle};
    use crate::support::{Connection, Wired, bounds, named, opened, presented};

    /// The leg suffix the row-comparison leg's tables carry, beside the run's own token.
    ///
    /// Distinct from the control leg's for the reason the corpus leg's three are distinct from each
    /// other: the two live legs here run under nextest's default parallelism, and two legs sharing a
    /// suffix would replace the tables the other is reading.
    const ROWS_LEG: &str = "twodsrows";

    /// The leg suffix the refusal-control leg's tables carry.
    const CONTROL_LEG: &str = "twodsctl";

    /// A dataset name that exists in no project, for the negative control.
    ///
    /// The literal `tests/acceptance.rs` already uses, deliberately: `docs/adr/0019` records it as
    /// the one part of a refused path that needs no masking in a public log, because it is
    /// fictitious. Sharing the string keeps that true of one literal rather than of two.
    const NO_SUCH_DATASET: &str = "sutura_no_such_dataset";

    /// The second dataset, from the developer's own environment, or a panic naming it.
    ///
    /// [`crate::support::named`] fails rather than skips, and its own documentation carries why: a
    /// developer who typed `just bigquery-two-datasets` and got green ticks over one dataset has
    /// been told the opposite of the truth.
    fn second_dataset() -> DatasetName {
        DatasetName::parse(named(
            "SUTURA_BQ_SECOND_DATASET",
            "the second dataset - same project, same location as SUTURA_BQ_DATASET - that this leg's joins \
             read across",
        ))
        .expect("a dataset name parses")
    }

    /// Every model a relationship points AT.
    ///
    /// **Derived from the bundle rather than named, which is what keeps a table name out of this
    /// file.** A join target is what a `JOIN` reads, so moving exactly those into the second dataset
    /// is what makes every join in the corpus cross a dataset boundary. A catalog that grew a fifth
    /// model is covered with no edit here, and one whose relationships all pointed at one model
    /// would fail the deterministic control below rather than quietly stop crossing anything.
    fn join_targets(pinned: &PinnedDefinitions) -> BTreeSet<ModelName> {
        pinned
            .definitions()
            .relationships()
            .values()
            .map(|relationship| relationship.target_model().clone())
            .collect()
    }

    /// The bundle with every model's table qualified: join targets in `dimensions`, the rest in
    /// `facts`.
    ///
    /// **The seam this whole leg rests on.** `sutura_semantic::compile` reads each model's table
    /// PATH into the plan, so qualifying the models here is what makes the rendered statement name
    /// two datasets - and nothing else about the bundle moves: the columns, the relationships and
    /// the metrics are cloned, and each table's own NAME is the one the loader wrote. The digest is
    /// recomputed by `pin` over the qualified definitions, exactly as
    /// [`crate::naming::suffixed_bundle`] does over the suffixed ones.
    ///
    /// **Both sides are qualified, the facts included, and that is deliberate.** Leaving the facts
    /// bare would work - a bare name resolves in the job's `defaultDataset` - and would leave the
    /// statement naming one dataset, so the claim would rest on a default rather than on two paths
    /// a reader of the statement can see.
    fn across_two_datasets(
        pinned: &PinnedDefinitions,
        facts: &DatasetName,
        dimensions: &DatasetName,
    ) -> PinnedDefinitions {
        let targets = join_targets(pinned);
        let models: Vec<Model> = pinned
            .definitions()
            .models()
            .values()
            .map(|model| {
                let dataset = if targets.contains(model.name()) { dimensions } else { facts };
                qualified(model, dataset)
            })
            .collect();
        reassembled(pinned, models)
    }

    /// The same bundle with only the JOIN TARGETS' dataset replaced.
    ///
    /// **One qualifier wrong and nothing else**, which is what makes the refusal it provokes
    /// attributable: the fact tables stay where the loader put them and stay readable, so a
    /// statement that fails does so because of the dataset named above the dimension table.
    fn dimensions_moved_to(pinned: &PinnedDefinitions, elsewhere: &DatasetName) -> PinnedDefinitions {
        let targets = join_targets(pinned);
        let models: Vec<Model> = pinned
            .definitions()
            .models()
            .values()
            .map(|model| {
                if targets.contains(model.name()) {
                    qualified(model, elsewhere)
                } else {
                    model.clone()
                }
            })
            .collect();
        reassembled(pinned, models)
    }

    /// One model, with its table qualified by `dataset` and everything else unchanged.
    fn qualified(model: &Model, dataset: &DatasetName) -> Model {
        Model::new(
            model.name().clone(),
            model.source().clone(),
            QualifiedTable::new(
                Some(TableQualifier::in_dataset(dataset.clone())),
                model.table_name().clone(),
            ),
            model.columns().clone(),
            Description::parse(model.description()).expect("a loaded description reparses"),
        )
    }

    /// The bundle re-pinned over a new set of models, everything else cloned verbatim.
    fn reassembled(pinned: &PinnedDefinitions, models: Vec<Model>) -> PinnedDefinitions {
        let relationships: Vec<Relationship> = pinned.definitions().relationships().values().cloned().collect();
        let metrics: Vec<Metric> = pinned.definitions().metrics().values().cloned().collect();
        let definitions = Definitions::assemble(models, relationships, metrics)
            .expect("qualifying table paths keeps the cross-references consistent");
        PinnedDefinitions::pin(
            pinned.version().clone(),
            definitions,
            pinned.knowledge().clone(),
            pinned.manifest().clone(),
        )
        .expect("a qualified bundle pins like the original")
    }

    /// The distinct datasets a plan's statement names, above the fact table and above every join.
    ///
    /// The COUNT is what the live legs assert on and the names are never printed: a dataset id is a
    /// resource, and a test's own output is a place one would otherwise be written down.
    fn datasets_named(plan: &QueryPlan) -> BTreeSet<DatasetName> {
        core::iter::once(plan.table())
            .chain(plan.joins().iter().map(|join| join.table()))
            .filter_map(|table| table.qualifier().map(|qualifier| qualifier.dataset().clone()))
            .collect()
    }

    /// The plan a question compiles to against `pinned`, or a panic saying why there was none.
    ///
    /// A compile-side refusal and a federated split are both defects HERE rather than outcomes: the
    /// caller has already chosen a question this bundle plans.
    fn planned(path: &Path, pinned: &PinnedDefinitions) -> Box<QueryPlan> {
        let question = read_question(path);
        match sutura_semantic::compile(&question, pinned).expect("the bundle is consistent") {
            sutura_semantic::Compiled::Planned { plan } => plan,
            other => panic!("{} did not compile to a single plan: {other:?}", stem(path)),
        }
    }

    /// The first corpus question whose plan really reads two datasets.
    ///
    /// **Derived, and a panic if there is none**, because *the corpus contains a question that
    /// joins* is a precondition of everything below rather than something to discover as an empty
    /// loop. A catalog whose relationships stopped producing a join would fail here by name instead
    /// of leaving every leg green over statements that cross nothing.
    fn a_crossing_question(pinned: &PinnedDefinitions) -> PathBuf {
        for path in questions() {
            let question = read_question(&path);
            let Ok(sutura_semantic::Compiled::Planned { plan }) = sutura_semantic::compile(&question, pinned) else {
                continue;
            };
            if datasets_named(&plan).len() > 1 {
                return path;
            }
        }
        panic!("no corpus question compiles to a plan reading two datasets, so this leg would prove nothing");
    }

    /// The adapter opened to LOAD into a dataset the caller names.
    ///
    /// **A connection whose dataset is replaced rather than a second reader of the environment.**
    /// [`Connection::required`] owns the credential, the billing project and the parse of each, and
    /// a second copy of that would be a second answer to *who pays*. Only the dataset moves, which
    /// is the one thing this leg needs a second of - and `load_fixture` renders the table
    /// UNQUALIFIED, so the job's `defaultDataset` is what puts it in the second dataset, exactly as
    /// it puts the corpus leg's tables in the first.
    fn loader_in(dataset: &DatasetName) -> Wired {
        let mut connection = Connection::required();
        connection.dataset = DatasetId::parse(dataset.as_str()).expect("a dataset id parses");
        opened(source(), connection, loading_bounds())
    }

    /// Two fictitious dataset names, for every test here that opens no socket.
    ///
    /// Literals of this file's own, so the deterministic controls assert against names they wrote
    /// rather than against configuration - and so their output carries no resource.
    fn fictional() -> (DatasetName, DatasetName) {
        (
            DatasetName::parse("sutura_facts_here").expect("a dataset name parses"),
            DatasetName::parse("sutura_dimensions_there").expect("a dataset name parses"),
        )
    }

    #[test]
    fn every_join_target_moves_to_the_second_dataset_and_every_fact_stays_in_the_first() {
        // **The split itself, and BOTH halves have to be non-empty or this leg crosses nothing.** A
        // bundle whose every model was a join target would put the whole corpus in one dataset, and
        // every live leg below would then pass while proving what `tests/corpus.rs` already proves.
        let (facts, dimensions) = fictional();
        let committed = bundle();
        let split = across_two_datasets(&committed, &facts, &dimensions);
        let targets = join_targets(&committed);
        assert!(!targets.is_empty(), "the example catalog declares no relationship, so nothing joins");

        let mut placed: BTreeMap<ModelName, DatasetName> = BTreeMap::new();
        for model in split.definitions().models().values() {
            let qualifier = model
                .table()
                .qualifier()
                .unwrap_or_else(|| panic!("{} was left unqualified by the split", model.name()));
            drop(placed.insert(model.name().clone(), qualifier.dataset().clone()));
        }
        let in_the_second: BTreeSet<&ModelName> = placed
            .iter()
            .filter(|&(_, dataset)| *dataset == dimensions)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            in_the_second,
            targets.iter().collect::<BTreeSet<&ModelName>>(),
            "the models in the second dataset are not exactly the join targets"
        );
        assert!(
            in_the_second.len() < placed.len(),
            "every model went to the second dataset, so no join crosses a boundary"
        );
    }

    #[test]
    fn a_corpus_question_that_joins_renders_one_statement_naming_both_datasets() {
        // **Decision 2 of `docs/adr/0019`, over the CORPUS rather than over a hand-built plan:** two
        // datasets reached by one credential are ONE source, so a question spanning them compiles to
        // a single plan and renders as a single statement with a `JOIN` - not to a federated pair of
        // legs. What the live leg below adds is that a service performs it; what this adds is that
        // the example catalog really produces such a statement, which is a property worth failing in
        // the gate every change runs rather than only where a project is configured.
        let (facts, dimensions) = fictional();
        let split = across_two_datasets(&bundle(), &facts, &dimensions);
        let path = a_crossing_question(&split);
        let plan = planned(&path, &split);
        assert_eq!(
            datasets_named(&plan).len(),
            2,
            "{}: the plan does not read two datasets",
            stem(&path)
        );

        let rendered = sutura_sql::generate::generate(&plan, Dialect::BigQuery).expect("the plan renders as GoogleSQL");
        let sql = rendered.sql();
        assert!(sql.contains(facts.as_str()), "the statement does not name the fact dataset: {sql}");
        assert!(
            sql.contains(dimensions.as_str()),
            "the statement does not name the dimension dataset: {sql}"
        );
        assert!(sql.contains("JOIN"), "the statement carries no JOIN: {sql}");
        // ONE statement, which is what *one job* rests on: a rendered query carrying a separator
        // would be two things submitted under one plan.
        assert!(!sql.contains(';'), "the statement is not one statement: {sql}");
        assert_eq!(rendered.source(), &source(), "the statement names a source no adapter was opened under");
    }

    #[test]
    fn the_engine_s_bundle_stays_unqualified_and_the_other_is_qualified_over_the_same_tables() {
        // **The guard on the one way the two sides could be handed the wrong bundle.** The engine
        // registers one file per model with nothing above it, so a qualified path names nothing it
        // holds - the plain bundle therefore has to stay `TableOnly` for every model and the
        // `BigQuery` bundle has to be `Dataset` for every model, over the SAME table names. Any
        // other combination is one side asking about tables the other never wrote, which is a
        // comparison of two questions dressed as a comparison of two answers.
        let (facts, dimensions) = fictional();
        let plain = suffixed_bundle(&bundle(), "runonetoken", ROWS_LEG);
        let split = across_two_datasets(&plain, &facts, &dimensions);
        for model in plain.definitions().models().values() {
            assert_eq!(
                model.table().qualification(),
                Qualification::TableOnly,
                "{} carries a qualifier the engine cannot resolve",
                model.name()
            );
        }
        for model in split.definitions().models().values() {
            assert_eq!(
                model.table().qualification(),
                Qualification::Dataset,
                "{} was not qualified, so its statement would rely on a default dataset",
                model.name()
            );
        }
        let names = |pinned: &PinnedDefinitions| -> Vec<String> {
            pinned
                .definitions()
                .models()
                .values()
                .map(|model| model.table_name().to_string())
                .collect()
        };
        assert_eq!(names(&plain), names(&split), "the two bundles name different tables");
    }

    #[test]
    fn moving_the_dimensions_elsewhere_changes_one_qualifier_and_leaves_the_facts_where_they_were() {
        // **What makes the live negative control attributable.** If the mutation moved the fact
        // tables too, the refusal it provokes would be *nothing in this statement exists* rather
        // than *the dataset named above the join target does not*.
        let (facts, dimensions) = fictional();
        let split = across_two_datasets(&bundle(), &facts, &dimensions);
        let absent = DatasetName::parse(NO_SUCH_DATASET).expect("a dataset name parses");
        let moved = dimensions_moved_to(&split, &absent);
        let targets = join_targets(&split);
        let mut elsewhere = 0_usize;
        for model in moved.definitions().models().values() {
            let expected = if targets.contains(model.name()) {
                elsewhere = elsewhere.saturating_add(1);
                &absent
            } else {
                &facts
            };
            let qualifier = model.table().qualifier().expect("the split qualified every model");
            assert_eq!(
                qualifier.dataset(),
                expected,
                "{} is not in the dataset the mutation intended",
                model.name()
            );
        }
        assert!(elsewhere > 0, "the mutation moved nothing, so it provokes no refusal");
        assert_ne!(dimensions, absent, "the mutation's dataset is the one it was meant to replace");
    }

    #[test]
    #[ignore = "needs a real BigQuery project and TWO datasets, named in the developer's own environment"]
    fn a_metric_whose_tables_live_in_two_datasets_answers_the_rows_the_engine_answers() {
        // **The requirement, executed.** The corpus is loaded into two datasets, every question is
        // asked of both the engine and `BigQuery` through `sutura_app::answer`, and the rows are
        // compared by the corpus leg's own `agreement_between` - CONTENT and ORDER, exactly, with no
        // tolerance for either.
        //
        // **The `BigQuery` side's bundle is VALIDATED against the endpoint** rather than against the
        // engine, and that is not a detail: `verify_and_validate` re-runs every anchor the catalog
        // declares, so the numbers somebody CERTIFIED are reproduced over the cross-dataset join
        // before a single question is asked. A bundle that cannot be validated is a deployment that
        // refuses to serve, which is what `LocalService::start` does with this same call.
        let token = run_token();
        let committed = bundle();
        let plain = suffixed_bundle(&committed, &token, ROWS_LEG);

        let connection = Connection::required();
        let facts = DatasetName::parse(connection.dataset.as_str()).expect("the connection's dataset is a name");
        let dimensions = second_dataset();
        assert_ne!(
            facts, dimensions,
            "SUTURA_BQ_SECOND_DATASET names the same dataset as SUTURA_BQ_DATASET, so nothing would cross"
        );
        let split = across_two_datasets(&plain, &facts, &dimensions);
        let warehouse = opened(source(), connection, bounds());

        let loaded = across(&committed, &plain, &dimensions);
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let engine = sutura_app::Warehouses::of(engine(&committed, &plain));
        let there = sutura_app::Warehouses::of(warehouse);
        let here = sutura_app::verify_and_validate(plain.clone(), &engine).expect("the anchors hold against the engine");
        let over_there = sutura_app::verify_and_validate(split.clone(), &there)
            .expect("the anchors hold against a cross-dataset join at the endpoint");
        let locally = GrantsWhatEachSideDeclares {
            presented: posture_of_the_engine_presented,
        };
        let remotely = GrantsWhatEachSideDeclares { presented };

        let (mut compared, mut refused, mut excluded, mut crossed) = (0_usize, 0_usize, 0_usize, 0_usize);
        for path in questions() {
            let name = stem(&path);
            let question = read_question(&path);
            let from_engine = sutura_app::answer(&here, &question, &a_caller(), &locally, &engine, 1 << 30);
            let from_bigquery = sutura_app::answer(&over_there, &question, &a_caller(), &remotely, &there, 1 << 30);
            let (one, other) = match (from_engine, from_bigquery) {
                (Ok(one), Ok(other)) => (one.into_outcome(), other.into_outcome()),
                (Err(ref locally), Err(ref remotely)) => {
                    // The corpus leg's one exclusion, for the reason its header gives: both sides
                    // fail and the reasons legitimately differ. Asserted by NAME, so a second
                    // question failing on both sides is a defect rather than a second exclusion.
                    assert_eq!(name, DIVIDES_BY_ZERO, "{name}: both sides failed and only one question may");
                    assert!(
                        chain(locally).contains("is not a finite number"),
                        "{name}: the engine failed otherwise"
                    );
                    assert!(
                        chain(remotely).contains("the data system did not answer"),
                        "{name}: BigQuery failed somewhere other than at the data system"
                    );
                    excluded = excluded.saturating_add(1);
                    continue;
                }
                (one, other) => panic!(
                    "{name}: one side answered and the other did not\n  engine: {one:?}\n  bigquery: {other:?}"
                ),
            };
            match (one, other) {
                (ToolOutcome::Answer { rows: ref a, .. }, ToolOutcome::Answer { rows: ref b, .. }) => {
                    agreement_between(&name, a, b);
                    compared = compared.saturating_add(1);
                    if datasets_named(&planned(&path, &split)).len() > 1 {
                        crossed = crossed.saturating_add(1);
                    }
                }
                (ToolOutcome::Refusal { reason: ref a }, ToolOutcome::Refusal { reason: ref b }) => {
                    // A refusal is decided by the compiler, above both adapters, so the two must
                    // always agree - and a qualifier changes nothing about that.
                    assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "{name}: the engine and BigQuery refused for different reasons"
                    );
                    refused = refused.saturating_add(1);
                }
                (one, other) => panic!(
                    "{name}: one side answered and the other refused\n  engine: {one:?}\n  bigquery: {other:?}"
                ),
            }
        }
        // The corpus leg's own identity, so this leg cannot silently shrink either: a floor alone
        // would pass a run that `continue`d past half the corpus.
        let total = questions().len();
        assert_eq!(
            compared.saturating_add(refused).saturating_add(excluded),
            total,
            "{compared} agreed + {refused} refused + {excluded} excluded is not the {total} questions in the corpus"
        );
        assert!(compared > 8, "only {compared} questions produced rows from both sides");
        // **The assertion this whole file exists for.** Without it, every number above could have
        // been produced by a corpus that never joined anything.
        assert!(
            crossed > 0,
            "{compared} questions agreed and not one of them read two datasets, so nothing crossed"
        );
        tidy(&plain, &dimensions);
        println!(
            "bigquery-two-datasets: {compared} answers agreed exactly on content AND order, {crossed} of them over \
             a join spanning two datasets, {refused} refusals agreed, {excluded} excluded, {total} in the corpus"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project and TWO datasets, named in the developer's own environment"]
    fn the_default_dataset_cannot_answer_what_the_two_datasets_answer() {
        // **The two negative controls, without which the leg above is worth nothing** - and they are
        // free, because a dry run uses no slots and the endpoint charges nothing for one.
        //
        //   * *a qualifier naming a dataset the credential cannot read is refused* - the same
        //     question with ONE qualifier wrong. It has to be refused by the service rather than
        //     silently answered from a default dataset, which is the wrong-number failure
        //     `docs/adr/0019` opens with, one layer further out.
        //   * the UNQUALIFIED form of the same question, which has to be refused too - and this is
        //     the half that makes *the qualifier is what bound the join* a measurement rather than
        //     an inference. The dimension tables exist in the SECOND dataset ONLY, so a service that
        //     resolved the last part in the job's `defaultDataset` finds nothing there.
        //
        // **The positive leg is here as well, deliberately:** two refusals with no acceptance beside
        // them is the shape a fixture that never loaded would also produce.
        //
        // What is asserted of each refusal is that it came from the DATA SYSTEM - the endpoint's own
        // error survived `#[source]` - and never the endpoint's words. Hyrum's Law applies to a
        // service's message as much as to ours and this repository pins none. Through
        // `sutura_app::answer` the same failure is a `ServiceError::Warehouse`, which the driving
        // port maps to `SurfaceFailure::Warehouse`: a data system that did not answer, and never a
        // `ToolOutcome::Refusal`, which is the governance distinction `docs/adr/0005` cares about.
        let token = run_token();
        let committed = bundle();
        let plain = suffixed_bundle(&committed, &token, CONTROL_LEG);

        let connection = Connection::required();
        let facts = DatasetName::parse(connection.dataset.as_str()).expect("the connection's dataset is a name");
        let dimensions = second_dataset();
        let split = across_two_datasets(&plain, &facts, &dimensions);
        let absent = DatasetName::parse(NO_SUCH_DATASET).expect("a dataset name parses");
        let elsewhere = dimensions_moved_to(&split, &absent);
        let warehouse = opened(source(), connection, bounds());

        let loaded = across(&committed, &plain, &dimensions);
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let path = a_crossing_question(&split);
        let name = stem(&path);

        let verdict = warehouse
            .dry_run(Executable::Query(&planned(&path, &split)), &presented())
            .unwrap_or_else(|e| panic!("{name}: the endpoint did not accept the cross-dataset statement: {e:?}"));
        assert_eq!(
            verdict,
            PreFlight::Accepted,
            "{name}: the cross-dataset statement was not accepted"
        );

        let wrong_qualifier = warehouse
            .dry_run(Executable::Query(&planned(&path, &elsewhere)), &presented())
            .err()
            .unwrap_or_else(|| {
                panic!("{name}: a dataset the credential cannot read was ANSWERED, so a qualifier binds nothing")
            });
        assert!(
            core::error::Error::source(&wrong_qualifier).is_some(),
            "{name}: the endpoint's own error did not survive #[source]: {wrong_qualifier:?}"
        );

        let no_qualifier = warehouse
            .dry_run(Executable::Query(&planned(&path, &plain)), &presented())
            .err()
            .unwrap_or_else(|| {
                panic!(
                    "{name}: the unqualified question was answered from the default dataset, which holds no \
                     dimension table - so the two-dataset leg proves nothing about a qualifier"
                )
            });
        assert!(
            core::error::Error::source(&no_qualifier).is_some(),
            "{name}: the endpoint's own error did not survive #[source]: {no_qualifier:?}"
        );

        tidy(&plain, &dimensions);
        println!(
            "bigquery-two-datasets: the two-dataset statement was accepted, and the same question was refused both \
             with one qualifier naming a dataset that is not there and with no qualifier at all"
        );
    }

    /// The corpus in two datasets: join targets into `dimensions`, every other model into the
    /// connection's own.
    ///
    /// One function rather than the four lines it takes, because BOTH live legs load the same way
    /// and a load that differed between them would be two fixtures under one claim.
    fn across(committed: &PinnedDefinitions, suffixed: &PinnedDefinitions, dimensions: &DatasetName) -> usize {
        let targets = join_targets(suffixed);
        let into_facts = loader();
        let into_dimensions = loader_in(dimensions);
        load_the_corpus(committed, suffixed, &|model| {
            if targets.contains(model.name()) {
                &into_dimensions
            } else {
                &into_facts
            }
        })
    }

    /// This run's tables, dropped from both datasets.
    ///
    /// `drop_the_corpus` names every model's table and the statement is `DROP TABLE IF EXISTS`, so
    /// running it against each dataset removes what that dataset holds and is a no-op for the rest.
    /// The tidy half of cleanup; the 24-hour expiration every `CREATE` carries is the guarantee half
    /// - see `crate::differential::drop_the_corpus`.
    fn tidy(suffixed: &PinnedDefinitions, dimensions: &DatasetName) {
        drop_the_corpus(suffixed, &loader());
        drop_the_corpus(suffixed, &loader_in(dimensions));
    }
}

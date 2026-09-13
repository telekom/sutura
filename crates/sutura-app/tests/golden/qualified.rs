//! What a table outside the connection's own dataset renders as, in every dialect `sutura-sql`
//! writes.
//!
//! **The fixtures are hand-built and the snapshots are not, and the reason is `golden/legs.rs`'s
//! reason:** the question corpus cannot produce these. A question has no field that names a table, so
//! a qualified path comes from a catalog document - and the shipped corpus under
//! `examples/single-player` is deliberately NOT qualified, because the two data systems that execute
//! it register one file per model and would have nothing to resolve `project.dataset.table` against.
//! Qualifying the corpus would have moved every existing golden and broken the executed axis to
//! demonstrate a rendering. So the rendering gets its own fixtures, and the *execution* claim is made
//! where it can actually be made: against a real `BigQuery`, in
//! `crates/sutura-exec-bigquery/tests/acceptance.rs`.
//!
//! | Fixture | What it is | What it shows |
//! | --- | --- | --- |
//! | `qualified-dataset` | `sales.orders` | a schema qualifier, which three of the four targets resolve |
//! | `qualified-project` | `analytics-prod.sales.orders` | a project above a dataset, which one target resolves |
//! | `qualified-cross-project-join` | that fact table joined to a dimension table in **another project** | the headline claim: a cross-project join is ONE statement, one job and one source |
//!
//! **The hyphen in the project part is the fixture doing work rather than decoration.** A real
//! `BigQuery` project id carries one - the credential this repository's acceptance leg runs under
//! does - and a hyphen is the character that would be a subtraction operator to anything that
//! tokenized the path instead of splitting it. `sutura_sql::generate::table_path` carries why the
//! split is sound.
//!
//! **Every cell handles BOTH outcomes, and that is what keeps a fifth dialect covered without an
//! edit here:** a target renders a path no deeper than `Dialect::qualification` says it resolves, and
//! refuses anything deeper as `GenerateError::QualificationUnsupported`. Which of the two a given
//! (fixture, dialect) pair gets is read off that declaration rather than listed, so the tests below
//! cannot drift from it.
//!
//! **A refused pair is asserted on the typed variant and its fields rather than snapshotted.** A
//! snapshot of a message would pin the wording; what a caller of `generate` matches on is the variant
//! and what a reader needs from it is which path and which two depths.

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{
    Aggregate, ColumnName, DatasetName, DimensionName, Grain, JoinType, MetricName, ProjectName, Qualification, QualifiedTable,
    RelationshipName, SourceName, TableName, TableQualifier,
};
use sutura_domain::plan::{
    AmbiguousTables, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
    PredicateOrigin, QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::warehouse::ParamValue;
use sutura_sql::generate::GenerateError;
use sutura_sql::{Dialect, generate};

use crate::shared::{
    appears_bare, assert_absent_as_text, assert_one_placeholder_per_parameter, settings, without_string_literals,
};

/// The fact table's own name - the last part of every path below, and the name every column in these
/// plans is qualified by.
const FACT: &str = "orders";

/// The dimension table this suite puts in a second project.
const DIMENSION: &str = "customers";

fn source() -> SourceName {
    // **One source name across every fixture here, the cross-project join included, and that is the
    // assertion rather than the setup.** A source is a credential plus a billing project, not a
    // project: two projects reached by one credential in one statement is ONE source, so nothing here
    // has a second `SourceName` to give. `sutura_semantic::plan` is where a source count becomes a
    // split or a `PlanSpansTooManySources`, and carries the same boundary.
    SourceName::parse("warehouse").expect("a fixture source is a source")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a fixture table is a table")
}

fn dataset(name: &str) -> DatasetName {
    DatasetName::parse(name).expect("a fixture dataset is a dataset")
}

fn project(name: &str) -> ProjectName {
    ProjectName::parse(name).expect("a fixture project is a project")
}

fn column(table_name: &str, column_name: &str) -> PlanColumn {
    PlanColumn::new(
        table(table_name),
        ColumnName::parse(column_name).expect("a fixture column is a column"),
    )
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a fixture date is a date"),
        Date::parse("2026-07-01").expect("a fixture date is a date"),
    )
    .expect("a fixture range is a range")
}

/// The two range bounds, which every plan carries because a `TimeRange` has no unbounded form.
///
/// A parsed [`PlanBindings`] rather than two vectors beside each other: the alias this used to
/// return said which was which and nothing made them agree, which is what
/// `sutura_domain::plan::bindings` is about.
fn bounds() -> PlanBindings {
    PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: column(FACT, "order_date"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: column(FACT, "order_date"),
                    param: 1,
                },
            ),
        ],
        vec![
            ParamValue::Date(Date::parse("2026-06-01").expect("a fixture date is a date")),
            ParamValue::Date(Date::parse("2026-07-01").expect("a fixture date is a date")),
        ],
    )
    .expect("the fixture's two range bounds bind in placeholder order")
}

/// A `sum(amount_cents)` over `path`, optionally joined to a dimension table at `joined`.
///
/// One builder for all three fixtures, so what differs between them is the PATH and nothing else -
/// which is what makes a diff between two of their snapshots readable as a statement about
/// qualification.
fn plan_over(path: QualifiedTable, joined: Option<QualifiedTable>) -> QueryPlan {
    let bindings = bounds();
    let mut joins: Vec<PlanJoin> = Vec::new();
    let mut keys: Vec<PlanKey> = Vec::new();
    if let Some(remote) = joined {
        joins.push(PlanJoin::new(
            RelationshipName::parse("orders_customer").expect("a fixture relationship is one"),
            remote,
            // `ManyToOne`, which is the only cardinality a dimension hop may have: the catalog
            // refuses one that could duplicate the fact rows and change the sum.
            JoinType::ManyToOne,
            column(FACT, "customer_id"),
            column(DIMENSION, "id"),
        ));
        keys.push(PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("region").expect("a fixture dimension is one")),
            column(DIMENSION, "region_code"),
        ));
    }
    QueryPlan::new(
        source(),
        MetricName::parse("revenue").expect("a fixture metric is a metric"),
        // Every fixture here ends its paths in two DIFFERENT names, so the set parses. The pair that
        // does not is a test of its own, one function below.
        StatementTables::parse(path, joins).expect("the fixtures name two distinguishable tables"),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, column(FACT, "order_date")),
        keys,
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column(FACT, "amount_cents"),
            },
        },
        ResultLabel::measure(&MetricName::parse("revenue").expect("a fixture metric is a metric")),
        bindings,
        june(),
    )
}

/// The three fixtures, named as their snapshots are.
fn fixtures() -> Vec<(&'static str, QueryPlan)> {
    let in_dataset = QualifiedTable::new(Some(TableQualifier::in_dataset(dataset("sales"))), table(FACT));
    let in_project = QualifiedTable::new(
        Some(TableQualifier::in_project(project("analytics-prod"), dataset("sales"))),
        table(FACT),
    );
    // The dimension table in a DIFFERENT project, reached by the same credential. This is the case
    // the whole feature exists for.
    let remote_dimension = QualifiedTable::new(
        Some(TableQualifier::in_project(project("reference-data"), dataset("crm"))),
        table(DIMENSION),
    );
    vec![
        ("qualified-dataset", plan_over(in_dataset, None)),
        ("qualified-project", plan_over(in_project.clone(), None)),
        ("qualified-cross-project-join", plan_over(in_project, Some(remote_dimension))),
    ]
}

/// The deepest path this plan names, over the `FROM` table and every joined one.
///
/// Read off the plan rather than hard-coded per fixture, because it is what decides whether a target
/// renders or refuses - and a fixture whose join is deeper than its fact table would otherwise be
/// classified by the shallower half.
fn deepest(plan: &QueryPlan) -> Qualification {
    core::iter::once(plan.table().qualification())
        .chain(plan.joins().iter().map(|join| join.table().qualification()))
        .max()
        .unwrap_or(Qualification::TableOnly)
}

/// The statement, pinned, for every fixture this target can resolve.
fn pins_what_it_can_resolve(dialect: Dialect) {
    let mut pinned = 0_usize;
    for (name, plan) in fixtures() {
        if deepest(&plan) > dialect.qualification() {
            continue;
        }
        let query = generate(&plan, dialect).unwrap_or_else(|e| panic!("{name} for {dialect} would not render: {e}"));
        settings(dialect.as_str()).bind(|| {
            insta::assert_snapshot!(format!("{name}__sql"), query.sql());
        });
        pinned = pinned.saturating_add(1);
    }
    // Every target resolves at least the bare-table case, and two of the three fixtures here are
    // deeper than `DuckDb` goes - so this is the latch that says a target which resolves NOTHING has
    // not silently emptied this test.
    if dialect.qualification() > Qualification::TableOnly {
        assert!(pinned > 0, "{dialect} resolves a qualifier and pinned no statement");
    }
}

/// A path deeper than the target resolves is refused, and the refusal names both depths.
fn refuses_what_it_cannot_resolve(dialect: Dialect) {
    let mut refused = 0_usize;
    for (name, plan) in fixtures() {
        let carries = deepest(&plan);
        if carries <= dialect.qualification() {
            continue;
        }
        let error = generate(&plan, dialect).expect_err("a path deeper than the target resolves is refused");
        // The variant and its fields, not the sentence. What a caller matches on is the variant; what
        // a reader needs is which path and which two depths, and both are typed.
        //
        // **The alternative this rules out is the wrong-number bug issue #83 reports**: dropping the
        // part that does not fit reads the table of that name in whatever the connection defaults to
        // and answers about it under a certified metric name.
        match error {
            GenerateError::QualificationUnsupported {
                dialect: at,
                ref table,
                carries: stated,
                resolves,
            } => {
                assert_eq!(at, dialect, "{name} was refused for the wrong dialect");
                assert_eq!(stated, carries, "{name} reported a depth its plan does not carry");
                assert_eq!(resolves, dialect.qualification(), "{name} reported the wrong target depth");
                assert!(
                    table.contains(FACT) || table.contains(DIMENSION),
                    "{name} for {dialect} refused a path naming neither of this suite's tables: {table}"
                );
            }
            other => panic!("{name} for {dialect} was refused for the wrong reason: {other:?}"),
        }
        refused = refused.saturating_add(1);
    }
    // `BigQuery` resolves every fixture here, so it legitimately refuses none. Every other target
    // must refuse at least one, or the fixture set has stopped reaching past it.
    if dialect.qualification() < Qualification::ProjectAndDataset {
        assert!(
            refused > 0,
            "{dialect} resolves less than a project and refused nothing; the fixtures no longer reach past it"
        );
    }
}

/// Every part of the path is quoted, and none of them appears bare.
///
/// **The claim `golden/dialects.rs` makes over the corpus, made over a PATH - which is the thing that
/// claim could not see before.** The corpus has no qualified table, so nothing there asserted that a
/// dataset or a project name reaches the statement quoted. Both halves are here for the reason that
/// file gives at length: the positive half stops it passing on a generator that emits nothing, and
/// the negative half reads the haystack with the quoted spans and the string literals stripped, which
/// is exactly where an unquoted name could have leaked to.
fn quotes_every_part_of_the_path(dialect: Dialect) {
    let quote = dialect.identifier_quote().character();
    let mut checked = 0_usize;
    for (name, plan) in fixtures() {
        if deepest(&plan) > dialect.qualification() {
            continue;
        }
        let query = generate(&plan, dialect).expect("a resolvable path renders");
        let stripped = without_string_literals(query.sql(), dialect);

        // Every part of every path in this plan, individually.
        let mut parts: BTreeSet<String> = BTreeSet::new();
        for path in core::iter::once(plan.table()).chain(plan.joins().iter().map(PlanJoin::table)) {
            parts.insert(String::from(path.name().as_str()));
            if let Some(qualifier) = path.qualifier() {
                parts.insert(String::from(qualifier.dataset().as_str()));
                if let Some(project_name) = qualifier.project() {
                    parts.insert(String::from(project_name.as_str()));
                }
            }
        }
        assert!(
            parts.len() > 1,
            "{name} names one part, so this test is asserting nothing about a path"
        );

        for part in &parts {
            assert!(
                query.sql().contains(&format!("{quote}{part}{quote}")),
                "{name} for {dialect} does not carry the path part {part:?} quoted with {quote:?}:\n{}",
                query.sql()
            );
            assert!(
                !appears_bare(&stripped, part.as_str()),
                "{name} for {dialect} carries the path part {part:?} unquoted:\n{}\nwith the quoted \
                 spans and the string literals removed:\n{stripped}",
                query.sql()
            );
            checked = checked.saturating_add(1);
        }
    }
    if dialect.qualification() > Qualification::TableOnly {
        assert!(checked > 0, "{dialect} resolves a qualifier and checked no path part");
    }
}

/// A qualified statement is still valid SQL for the target it was generated for.
///
/// The corpus check, over a construct the corpus does not contain. It matters most for `BigQuery`,
/// where the identifier quote is a backtick and a double quote is a STRING - so a path quoted the
/// wrong way there is not a syntax error, it is a statement about different values. A dot after a
/// string literal is what the parser does catch, which is why this bites at all.
fn parses_in_the_dialect_it_was_generated_for(dialect: Dialect, target: polyglot_sql::DialectType) {
    for (name, plan) in fixtures() {
        if deepest(&plan) > dialect.qualification() {
            continue;
        }
        let query = generate(&plan, dialect).expect("a resolvable path renders");
        let parsed = polyglot_sql::parse(query.sql(), target);
        assert!(
            parsed.is_ok(),
            "{name} for {dialect} is not valid there: {:?}\n{}",
            parsed.err(),
            query.sql()
        );
    }
}

/// Qualifying a table changes the `FROM` clause and nothing about the values.
///
/// A path is not a value and must not become one, and the two range bounds are still bound. Read
/// through the same helpers the corpus axis uses, so this cannot become a second opinion about what
/// the no-injection claim is.
fn binds_every_value_rather_than_writing_it(dialect: Dialect) {
    for (name, plan) in fixtures() {
        if deepest(&plan) > dialect.qualification() {
            continue;
        }
        let query = generate(&plan, dialect).expect("a resolvable path renders");
        assert_one_placeholder_per_parameter(name, dialect, plan.params().len(), query.sql());
        for value in ["2026-06-01", "2026-07-01"] {
            assert_absent_as_text(name, dialect, "the range bound", value, query.sql());
        }
    }
}

/// One cell of this axis.
macro_rules! cell {
    ($name:ident, $dialect:expr, $target:expr) => {
        mod $name {
            #[test]
            fn a_path_this_target_resolves_is_rendered_and_pinned() {
                super::pins_what_it_can_resolve($dialect);
            }

            #[test]
            fn a_path_deeper_than_this_target_resolves_is_refused_rather_than_shortened() {
                super::refuses_what_it_cannot_resolve($dialect);
            }

            #[test]
            fn every_part_of_a_path_is_quoted() {
                super::quotes_every_part_of_the_path($dialect);
            }

            #[test]
            fn every_qualified_statement_parses_here() {
                super::parses_in_the_dialect_it_was_generated_for($dialect, $target);
            }

            #[test]
            fn no_value_reaches_a_qualified_statement_as_text() {
                super::binds_every_value_rather_than_writing_it($dialect);
            }
        }
    };
}

crate::adapters::registered!(dialects: cell);

/// The plans themselves, so a fixture edit is a reviewable diff rather than a Rust literal nobody
/// reads twice - and so the serialized form of a path is pinned.
///
/// **The serialized form is the point of this snapshot rather than a side effect.** `QualifiedTable`
/// hand-writes `Serialize` to emit its own dotted text, because a derived one would write a struct
/// its own `Deserialize` refuses - the asymmetry that shipped here as a bug on `Date`. The definition
/// digest is taken over the serialized form, so this snapshot is what makes it visible that the digest
/// covers `analytics-prod.sales.orders` and not a field layout no catalog file contains.
#[test]
fn the_plans_serialize_with_their_paths_as_text() {
    settings("").bind(|| {
        for (name, plan) in fixtures() {
            insta::assert_yaml_snapshot!(format!("qualified_plan_{name}"), plan);
        }
    });
}

/// Two qualified tables whose paths end in the same name cannot both be in one statement.
///
/// **The reviewer's own reproduction, kept as the test.** Changing only the dimension table's name
/// from `customers` to `orders` in the cross-project fixture above used to render an `ON` clause
/// reading `orders.customer_id = orders.id` - one table compared with itself - beneath a `FROM` naming
/// `analytics-prod.sales.orders` and a `LEFT JOIN` naming `reference-data.crm.orders`, with every
/// projected column qualified by an identifier that named two tables. That is a plausible number under
/// a certified metric on any target that binds it to one side; the pinned `DuckDB` answers it with
/// `Binder Error: Ambiguous reference to table "orders"`.
///
/// It is asserted at plan CONSTRUCTION and not on a rendered string, because that is where the fix
/// is: `StatementTables::parse` is the only way to a `QueryPlan`, so there is no ambiguous plan for
/// any dialect to render. `sutura_domain::plan::tables` carries why a refusal rather than distinct
/// explicit aliases - the builder this workspace renders through cannot alias a joined table - and
/// `golden/service.rs` is where the same collision is provoked through a catalog and a question,
/// which is what makes the refusal a caller can see.
#[test]
fn two_paths_ending_in_one_name_are_refused_rather_than_rendered_under_one_alias() {
    let fact = QualifiedTable::new(
        Some(TableQualifier::in_project(project("analytics-prod"), dataset("sales"))),
        table(FACT),
    );
    // The same table NAME in another project, which is exactly the estate shape qualified paths exist
    // for and exactly the pair one statement cannot tell apart.
    let collides = QualifiedTable::new(
        Some(TableQualifier::in_project(project("reference-data"), dataset("crm"))),
        table(FACT),
    );
    let join = PlanJoin::new(
        RelationshipName::parse("orders_customer").expect("a fixture relationship is one"),
        collides.clone(),
        JoinType::ManyToOne,
        column(FACT, "customer_id"),
        column(FACT, "id"),
    );

    let refused = StatementTables::parse(fact.clone(), vec![join]).expect_err("one identifier, two tables");
    assert_eq!(
        refused,
        AmbiguousTables::OneIdentifierTwoTables {
            alias: table(FACT),
            first: fact.to_string(),
            second: collides.to_string(),
        }
    );
    // Named apart from the whole value so the accessor the plan stage reads is covered too: it is what
    // becomes the identifier in the caller-facing refusal.
    assert_eq!(refused.alias(), &table(FACT));
}

/// The same shape spelled to look like two names, which is the case an equality check let through.
///
/// `GoogleSQL` resolves an alias case-insensitively and a real `DuckDB` binds a quoted `orders`
/// qualifier against a table declared `Orders`, so `Orders` beside `orders` is one identifier on two
/// of the four targets. `StatementTables::parse` compares under `IdentifierCase::COARSEST` for that
/// reason, and `sutura_sql::Dialect::identifier_case` is where each target declares its own.
#[test]
fn two_paths_differing_only_in_the_case_of_their_last_part_are_refused_too() {
    let fact = QualifiedTable::new(Some(TableQualifier::in_dataset(dataset("sales"))), table("Orders"));
    let collides = QualifiedTable::new(Some(TableQualifier::in_dataset(dataset("crm"))), table("orders"));
    let join = PlanJoin::new(
        RelationshipName::parse("orders_customer").expect("a fixture relationship is one"),
        collides,
        JoinType::ManyToOne,
        column("Orders", "customer_id"),
        column("orders", "id"),
    );
    assert!(
        StatementTables::parse(fact, vec![join]).is_err(),
        "Orders and orders are one identifier on GoogleSQL and on DuckDB"
    );
}

/// A cross-project join is one statement, one source, and one thing to push down.
///
/// **The design claim the whole issue turns on, asserted rather than only written down.** Cross-project
/// in `BigQuery` is not federation: one credential, one job, one engine, a join the data system
/// performs. Routing it through a splitter and a client-side combiner would replace a pushed-down join
/// with a slower one and discard the pushdown that makes the adapter worth having.
///
/// **What this can and cannot show.** It shows that a plan naming two projects is one plan with one
/// `SourceName`, and that it renders as a single statement with both paths in it and a single `JOIN`.
/// It does not show that the service performs the join - nothing local can - which is what the
/// acceptance leg against a real project is for.
#[test]
fn a_cross_project_join_is_one_statement_and_one_source() {
    let (name, plan) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "qualified-cross-project-join")
        .expect("the cross-project fixture is in the set");

    // One source, and there is no second `SourceName` anywhere in the plan to disagree with it: a
    // `QueryPlan` has exactly one, which is what makes "one credential reaching two projects is one
    // source" a property of the type rather than of this fixture.
    assert_eq!(plan.source(), &source());
    // Two projects, and they differ - otherwise this fixture would be a same-project join wearing the
    // name of a cross-project one.
    let fact_project = plan.table().qualifier().and_then(|q| q.project()).cloned();
    let joined_project = plan
        .joins()
        .first()
        .and_then(|join| join.table().qualifier())
        .and_then(|q| q.project())
        .cloned();
    assert!(fact_project.is_some() && joined_project.is_some(), "{name} names no project");
    assert_ne!(fact_project, joined_project, "{name} does not reach a second project");

    // And it is ONE statement with ONE join, on the one target that resolves it.
    let sql = generate(&plan, Dialect::BigQuery)
        .expect("a cross-project statement renders for BigQuery")
        .sql()
        .to_owned();
    assert_eq!(sql.matches("JOIN").count(), 1, "one join, not two legs:\n{sql}");
    assert_eq!(sql.matches(';').count(), 0, "one statement, not a script:\n{sql}");
    for part in ["analytics-prod", "reference-data"] {
        assert!(sql.contains(part), "{part} is not in the statement:\n{sql}");
    }
}

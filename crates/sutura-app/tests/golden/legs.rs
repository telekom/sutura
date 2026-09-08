//! What one leg of a federated question renders as, expanded over every dialect `sutura-sql` writes.
//!
//! **The fixtures are hand-built and the snapshots are not, and the split is deliberate.** A corpus
//! of question files cannot produce a `LegPlan` the way it produces a `QueryPlan` - the splitter
//! decides both legs from a bundle's topology, not from the question - so these are written by
//! hand. What this axis pins is therefore the RENDERING: `sutura_sql::generate_leg` over each shape,
//! in each dialect, parse-checked in the dialect it was generated for. The leg plans themselves are
//! pinned too, as one snapshot of their serialized form, so a fixture edit is a reviewable diff
//! rather than a Rust literal nobody reads twice.
//!
//! **What no cell here reaches is the leg path a RELEASE executes, and the reason is structural
//! rather than a gap to fill.** The only leg-executing adapter a published binary links is the
//! engine, and it builds a logical plan and renders no SQL - so there is no statement for a golden
//! to pin and there cannot be one. That path's evidence is `sutura-exec-datafusion`'s conformance
//! binding, which holds a leg to the number the whole-plan case lands on, and the two-engine pass in
//! `tests/differential/federated.rs`. These four dialects are evidence about the renderer.
//!
//! **And the limit these fixtures carry about themselves:** nothing holds them against what
//! `sutura_semantic::plan` actually emits. That is why every reserved label below is taken from
//! `InternalLabel` rather than spelled - a hand-written one is where the fixture and the splitter
//! can drift with no cell to say so.
//!
//! **The fixtures are the federated form of questions that already exist in the corpus**, over the
//! same telco catalog, with `customers` imagined on a second data system - which is the case
//! `docs/adr/0007` is about. Three fact shapes and two lookup shapes:
//!
//! | Fixture | The question behind it | What it shows |
//! | --- | --- | --- |
//! | `fact-sum-over-a-local-join` | `recurring_revenue by product_family` | a term that descends as written, and a same-source hop staying a join |
//! | `fact-decomposed-average` | `mean_subscription_mrr` | an `Avg` travelling as a sum beside a count, **undivided** |
//! | `fact-distinct-keys` | `active_subscriptions by region` | the third shape, which is a `Fact` with an EMPTY `terms` list |
//! | `lookup-unfiltered` | the `customers` half of the same question | LEFT above, so no predicate at all |
//! | `lookup-filtered` | `active_subscriptions in the north` | a lookup carrying the question's value, bound |
//!
//! **No leg golden may carry a `LIMIT`.** `AGENTS.md` counts the SQL goldens that read `LIMIT 10001`
//! and `check-guidance` fails if that number drifts; a leg is not an answer, so a row cap on one
//! would refuse a question no answer was too large for. [`a_leg_carries_no_row_limit`] asserts it
//! per shape per dialect rather than trusting the generator, because the generator emitting none is
//! what makes it hard to get wrong and not what makes it impossible.

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, RelationshipName, SourceName, TableName,
};
use sutura_domain::plan::{
    InternalLabel, LegPlan, LegTerm, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey, PlanPredicate, PlanTerm,
    PredicateOrigin, ResultLabel, StatementTables,
};
use sutura_domain::warehouse::ParamValue;
use sutura_sql::{Dialect, generate_leg};

use crate::shared::{assert_absent_as_text, assert_one_placeholder_per_parameter, bound_value, settings};

/// The metric's own model, on the data system the question started from.
const FACT_TABLE: &str = "fct_subscription_monthly";

/// The dimension model this fixture set puts on a second data system.
const REMOTE_TABLE: &str = "dim_customer";

/// A same-source dimension model, so a hop that stays a join is covered too.
const LOCAL_DIMENSION_TABLE: &str = "dim_product";

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a fixture source is a source")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a fixture table is a table")
}

fn column(table_name: &str, column_name: &str) -> PlanColumn {
    PlanColumn::new(
        table(table_name),
        ColumnName::parse(column_name).expect("a fixture column is a column"),
    )
}

fn key(label: &str, table_name: &str, column_name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse(label).expect("a fixture dimension is a dimension")),
        column(table_name, column_name),
    )
}

/// The key the two legs are joined on, under the label the splitter gives it.
///
/// **Taken from `InternalLabel` rather than spelled, and that is what makes the statements below
/// evidence about the real scheme.** These fixtures are hand-built rather than derived, and a
/// hand-written link label is a place where the fixture and the splitter can
/// disagree without any test noticing; they did, and the label the splitter chose was a legal
/// dimension name, which is `telekom/sutura#325`'s F2. The rendered statements are therefore also
/// the parse check for a reserved label: `parses_in_the_dialect_it_was_generated_for` asks each of
/// the four targets' PARSERS whether an alias in this namespace is valid there.
///
/// **A parser is not the venue that decides, and the internal namespace is the one place in this
/// repository where that gap is load-bearing.** Every internal label starts with the character
/// `InvalidIdentifier::BadFirstCharacter` refuses *because* it is legal in some dialects and not
/// others, so a parse check here is exactly the shape `telekom/sutura#92` established as blind. What
/// runs it is `an_internal_label_survives_as_an_alias_at_the_service` in
/// `crates/sutura-exec-bigquery/tests/acceptance.rs`, against the one target whose documentation
/// restricts a column NAME to a letter or an underscore first. `label.rs` carries the table of what
/// each venue establishes, and which two targets are still parser-only.
fn link_key(table_name: &str) -> PlanKey {
    PlanKey::new(ResultLabel::internal(InternalLabel::Link), column(table_name, "customer_key"))
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a fixture date is a date"),
        Date::parse("2026-07-01").expect("a fixture date is a date"),
    )
    .expect("a fixture range is a range")
}

fn month_bucket() -> PlanBucket {
    PlanBucket::new(ResultLabel::bucket(), Grain::Month, column(FACT_TABLE, "month"))
}

/// The two range bounds and the metric's own status filter, in parameter order.
///
/// Definitional every one of them: the bounds come from the question's range through the metric's
/// time column, and `status = 'active'` is part of what `recurring_revenue` and
/// `active_subscriptions` mean. A leg carries them for the same reason a whole plan does - a
/// definitional predicate a leg dropped would answer a different question one source at a time.
fn definitional_filters() -> Vec<PlanFilter> {
    vec![
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::AtOrAfter {
                column: column(FACT_TABLE, "month"),
                param: 0,
            },
        ),
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::Before {
                column: column(FACT_TABLE, "month"),
                param: 1,
            },
        ),
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::Equals {
                column: column(FACT_TABLE, "status"),
                param: 2,
            },
        ),
    ]
}

fn definitional_params() -> Vec<ParamValue> {
    vec![
        ParamValue::Date(Date::parse("2026-06-01").expect("a fixture date is a date")),
        ParamValue::Date(Date::parse("2026-07-01").expect("a fixture date is a date")),
        ParamValue::Text(String::from("active")),
    ]
}

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a fixture metric is a metric")
}

/// One carried leaf of the measure, projected under the label its POSITION gives it.
///
/// `position` and not a name, for [`link_key`]'s reason: the leaf labels are in the same reserved
/// namespace, and the scheme they replaced - `metric__{n}` - was both a legal dimension name and
/// able to cross the 63-character identifier limit a data system truncates silently.
fn term(aggregate: Aggregate, column_name: &str, position: usize) -> LegTerm {
    LegTerm::new(
        PlanTerm::Aggregate {
            aggregate,
            column: column(FACT_TABLE, column_name),
        },
        ResultLabel::internal(InternalLabel::Leaf(position)),
    )
}

/// `recurring_revenue by product_family`, split so the customer dimension is remote.
///
/// The sum descends as written, so this leg computes it and the combine adds the leg sums.
/// `product_family` is on the SAME data system, so it stays a join rather than becoming a second
/// leg - which is what `joins` holding same-source hops only means in practice. The join column is in
/// `keys` because the remote dimension has to be joined to something above, under [`link_key`]'s
/// reserved label rather than under its own physical name.
fn fact_sum_over_a_local_join() -> LegPlan {
    LegPlan::Fact {
        source: source("local"),
        metric: metric("recurring_revenue"),
        // Through the guard rather than around it: a fact leg's tables arrive as a checked set, so
        // this fixture cannot state a pair one statement could not tell apart. `plan::tables` is
        // where that is argued, and `golden/service.rs` is where the refusal is provoked through a
        // catalog and a question.
        tables: StatementTables::parse(
            table(FACT_TABLE),
            vec![PlanJoin::new(
                RelationshipName::parse("subscription_product").expect("a fixture relationship is a relationship"),
                table(LOCAL_DIMENSION_TABLE),
                JoinType::ManyToOne,
                column(FACT_TABLE, "product_key"),
                column(LOCAL_DIMENSION_TABLE, "product_key"),
            )],
        )
        .expect("two differently named fixture tables are distinguishable"),
        bucket: month_bucket(),
        keys: vec![
            key("product_family", LOCAL_DIMENSION_TABLE, "product_family"),
            link_key(FACT_TABLE),
        ],
        terms: vec![term(Aggregate::Sum, "mrr_cents", 0)],
        filters: definitional_filters(),
        params: definitional_params(),
        range: june(),
    }
}

/// `mean_subscription_mrr`, decomposed.
///
/// **The shape 0009's Decision 2 is about.** The mean of means is not the mean, so the average
/// travels as a sum beside a count and the division happens once, above every leg. Two term columns
/// side by side and nothing between them: there is no `NULLIF` and no `/` in this statement, and
/// `LegTerm` has no shape that could put one there.
fn fact_decomposed_average() -> LegPlan {
    LegPlan::Fact {
        source: source("local"),
        metric: metric("mean_subscription_mrr"),
        tables: StatementTables::only(table(FACT_TABLE)),
        bucket: month_bucket(),
        keys: vec![link_key(FACT_TABLE)],
        terms: vec![term(Aggregate::Sum, "mrr_cents", 0), term(Aggregate::Count, "mrr_cents", 1)],
        filters: definitional_filters(),
        params: definitional_params(),
        range: june(),
    }
}

/// `active_subscriptions by region`, whose measure does not descend at all.
///
/// An exact distinct count cannot be re-aggregated - adding two exact counts over-counts every key
/// the two legs share - so the distinct keys themselves travel and the count runs above. That makes
/// this leg a `Fact` with an **empty** `terms` list: it groups by its key list and projects it,
/// which is a distinct key set. Not a third variant, and the statement below is why: it renders
/// exactly like the aggregate shape minus the term columns.
fn fact_distinct_keys() -> LegPlan {
    LegPlan::Fact {
        source: source("local"),
        metric: metric("active_subscriptions"),
        tables: StatementTables::only(table(FACT_TABLE)),
        bucket: month_bucket(),
        keys: vec![link_key(FACT_TABLE), key("subscription_key", FACT_TABLE, "subscription_key")],
        terms: Vec::new(),
        filters: definitional_filters(),
        params: definitional_params(),
        range: june(),
    }
}

/// The `customers` half of the same question, on a second data system, carrying no filter.
///
/// No bucket, no terms, no range, no metric - and with no filter, no `WHERE` clause at all. That
/// last one is why `generate_leg` cannot reuse `generate`'s unconditional fold: a whole plan always
/// carries the two bounds of its range, and this shape carries nothing.
fn lookup_unfiltered() -> LegPlan {
    LegPlan::Lookup {
        source: source("crm"),
        table: table(REMOTE_TABLE).into(),
        keys: vec![link_key(REMOTE_TABLE), key("region", REMOTE_TABLE, "region")],
        filters: Vec::new(),
        params: Vec::new(),
    }
}

/// The same lookup with the question's own filter pushed into it: `region = 'north'`.
///
/// The value is the CALLER's, which is what makes this fixture worth having: a new entry point is a
/// new place for a value to reach a statement as text, and [`no_leg_statement_carries_a_value`]
/// reads this one.
fn lookup_filtered() -> LegPlan {
    LegPlan::Lookup {
        source: source("crm"),
        table: table(REMOTE_TABLE).into(),
        keys: vec![link_key(REMOTE_TABLE), key("region", REMOTE_TABLE, "region")],
        filters: vec![PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: column(REMOTE_TABLE, "region"),
                param: 0,
            },
        )],
        params: vec![ParamValue::Text(String::from("north"))],
    }
}

/// Every leg shape, by the name its snapshots carry.
///
/// A list rather than a test each, so the assertions below are written over the whole set and a
/// sixth fixture is covered by all of them the moment it is added here.
fn shapes() -> Vec<(&'static str, LegPlan)> {
    vec![
        ("fact-sum-over-a-local-join", fact_sum_over_a_local_join()),
        ("fact-decomposed-average", fact_decomposed_average()),
        ("fact-distinct-keys", fact_distinct_keys()),
        ("lookup-unfiltered", lookup_unfiltered()),
        ("lookup-filtered", lookup_filtered()),
    ]
}

/// The leg plans themselves, pinned once.
///
/// Dialect-independent, so it is off the per-dialect matrix: a leg plan is a function of the split
/// and of nothing a renderer decides. It is here because the fixtures are hand-written rather than
/// derived - this file's header says why - and a hand-written fixture that changed silently would
/// move five statements with it.
#[test]
fn every_leg_shape_is_pinned_as_a_plan() {
    for (name, leg) in shapes() {
        settings("").bind(|| {
            insta::assert_yaml_snapshot!(format!("leg_{name}"), leg);
        });
    }
}

/// One statement per shape, in one dialect, with its parameters.
fn pins_the_statement_and_its_parameters(dialect: Dialect) {
    let mut checked = 0_usize;
    for (name, leg) in shapes() {
        let query = generate_leg(&leg, dialect).unwrap_or_else(|e| panic!("{name} would not render for {dialect}: {e}"));
        assert_eq!(
            query.source(),
            leg.source(),
            "{name} rendered for a data system the leg does not name"
        );
        settings(dialect.as_str()).bind(|| {
            insta::assert_snapshot!(format!("leg_{name}__sql"), query.sql());
            insta::assert_yaml_snapshot!(format!("leg_{name}__params"), query.params());
        });
        checked = checked.saturating_add(1);
    }
    assert!(checked > 0, "there are no leg shapes to render");
}

/// The parse check, per shape per dialect.
///
/// It PARSES and stops, for the reason the mono-source corpus does: re-emitting would introduce the
/// parser-differential problem that makes translation unusable. A failure here means `generate_leg`
/// produced something that is not valid SQL for that target, which is otherwise only discoverable by
/// running it - and there is nothing to run a leg against yet.
///
/// **So a green here establishes four PARSERS and nothing about four services**, which matters most
/// for the reserved alias `link_key` renders: see that function, and `label.rs` for the venue table.
fn parses_in_the_dialect_it_was_generated_for(dialect: Dialect, target: polyglot_sql::DialectType) {
    let mut checked = 0_usize;
    for (name, leg) in shapes() {
        let query = generate_leg(&leg, dialect).expect("a leg fixture renders");
        let parsed = polyglot_sql::parse(query.sql(), target);
        assert!(
            parsed.is_ok(),
            "{name} for {dialect} is not valid there: {:?}\n{}",
            parsed.err(),
            query.sql()
        );
        checked = checked.saturating_add(1);
    }
    assert!(checked > 0, "there are no leg shapes to parse");
}

/// No value a leg carries reaches its statement as text.
///
/// **A new entry point is a new place for that to be got wrong**, which is the whole reason this is
/// asserted again rather than inherited from the mono-source corpus: `generate_leg` folds its own
/// predicates and builds its own placeholders, and a version of it that formatted a value into the
/// statement would satisfy every other test in this file.
///
/// **The assertions themselves live in `shared` and are the same ones the question corpus makes.**
/// They were a copy of that module's haystack when this file was written, and the copy is what let
/// one hole exist in two places: a value the generator wrapped in double quotes was stripped out of
/// the haystack before the search ran, so a mutation that inlined one passed here and there both.
/// `shared::assert_absent_as_text` and `shared::assert_one_placeholder_per_parameter` carry the
/// searches, the positive count and the limit that divides them.
///
/// Three assertions, because no two of them alone pass only on a correct generator: the statement names
/// one bind parameter per value the leg carries, the values are absent from the statement, AND they
/// are present in the parameter list - so a generator that dropped the predicate entirely fails here
/// instead of reading as clean.
fn no_leg_statement_carries_a_value(dialect: Dialect) {
    let mut with_params = 0_usize;
    for (name, leg) in shapes() {
        let query = generate_leg(&leg, dialect).expect("a leg fixture renders");
        let expected: Vec<String> = leg.params().iter().map(bound_value).collect();
        // Counted against the LEG's parameters, for the reason the question corpus counts against
        // the plan's: a generator that inlined a value and dropped it from its own list is still one
        // placeholder short.
        assert_one_placeholder_per_parameter(name, dialect, leg.params().len(), query.sql());
        for value in &expected {
            assert_absent_as_text(name, dialect, "the value", value.as_str(), query.sql());
            with_params = with_params.saturating_add(1);
        }
        let bound: BTreeSet<String> = query.params().iter().map(bound_value).collect();
        for value in &expected {
            assert!(
                bound.contains(value),
                "{name} for {dialect} did not bind {value:?}; the predicate is missing rather than \
                 inlined:\n{}",
                query.sql()
            );
        }
    }
    // At least one shape has to carry a parameter, or the loop above asserted over an empty set.
    assert!(
        with_params > 0,
        "no leg fixture carried a parameter; this test proved nothing"
    );
}

/// A leg carries no row cap, and the reason is not tidiness.
///
/// `MAX_ROWS` caps ONE ANSWER's rows and `QueryPlan::row_limit` is how an adapter asks for one more
/// than the cap, so that a result AT the cap is distinguishable from one the cap cut short. A leg is
/// not an answer: the same number applied per leg would refuse a question no answer was too large
/// for, and applied to a distinct-key leg it would refuse the exact count 0009 decided to pull up.
/// What bounds a leg is the byte budget at the conversion boundary, which arrives with the code that
/// converts.
fn a_leg_carries_no_row_limit(dialect: Dialect) {
    for (name, leg) in shapes() {
        let query = generate_leg(&leg, dialect).expect("a leg fixture renders");
        let shouted = query.sql().to_uppercase();
        assert!(
            !shouted.contains("LIMIT"),
            "{name} for {dialect} carries a row cap:\n{}",
            query.sql()
        );
        assert!(
            !shouted.contains("FETCH"),
            "{name} for {dialect} carries a row cap in the other spelling:\n{}",
            query.sql()
        );
    }
}

/// A lookup leg renders neither a time bucket nor an aggregate.
///
/// The type says so - `LegPlan::Lookup` has no `bucket` field and no `terms` field - and this is the
/// other end of that claim, read off the statement: a reader of the SQL can see that a dimension
/// table is not being truncated by a date it does not have, and that nothing is being aggregated on
/// a data system with no combiner above it.
fn a_lookup_leg_renders_no_bucket_and_no_aggregate(dialect: Dialect) {
    for name in ["lookup-unfiltered", "lookup-filtered"] {
        let Some((_, leg)) = shapes().into_iter().find(|(fixture, _)| *fixture == name) else {
            panic!("there is no fixture called {name}");
        };
        let query = generate_leg(&leg, dialect).expect("a leg fixture renders");
        let shouted = query.sql().to_uppercase();
        assert!(
            !shouted.contains("DATE_TRUNC"),
            "{name} for {dialect} truncated a date on a dimension table:\n{}",
            query.sql()
        );
        for aggregate in ["SUM(", "COUNT(", "AVG(", "MIN(", "MAX("] {
            assert!(
                !shouted.contains(aggregate),
                "{name} for {dialect} aggregated with {aggregate}:\n{}",
                query.sql()
            );
        }
        // And it does group, which is what makes it a DISTINCT key set rather than one row per
        // dimension row. Without this half the assertions above would pass on a projection.
        assert!(
            shouted.contains("GROUP BY"),
            "{name} for {dialect} projects its keys without making them distinct:\n{}",
            query.sql()
        );
    }
}

/// One cell of the dialect axis, over the leg shapes rather than over the question corpus.
macro_rules! cell {
    ($name:ident, $dialect:expr, $target:expr) => {
        mod $name {
            #[test]
            fn every_leg_shape_renders_and_every_statement_is_pinned() {
                super::pins_the_statement_and_its_parameters($dialect);
            }

            #[test]
            fn every_leg_statement_parses_here() {
                super::parses_in_the_dialect_it_was_generated_for($dialect, $target);
            }

            #[test]
            fn no_leg_statement_carries_a_question_literal() {
                super::no_leg_statement_carries_a_value($dialect);
            }

            #[test]
            fn a_leg_statement_carries_no_row_limit() {
                super::a_leg_carries_no_row_limit($dialect);
            }

            #[test]
            fn a_lookup_leg_has_no_bucket_and_no_measure_in_its_statement() {
                super::a_lookup_leg_renders_no_bucket_and_no_aggregate($dialect);
            }
        }
    };
}

crate::adapters::registered!(dialects: cell);

/// Every shape renders for every dialect the renderer compiles for, counted rather than assumed.
///
/// The cells above are the per-dialect assertions and this is the coverage claim over them: it reads
/// `dialect::ALL`, which is `sutura-sql`'s own list, so a dialect added there with no registry line
/// still has every leg shape rendered here. It is the leg half of what
/// `dialects::every_dialect_the_renderer_supports_is_registered` does for the question corpus - and
/// it counts, so a `shapes()` that quietly returned nothing would fail rather than pass over an empty
/// loop.
#[test]
fn every_leg_shape_renders_in_every_compiled_dialect() {
    let mut rendered = 0_usize;
    for dialect in sutura_sql::dialect::ALL {
        for (name, leg) in shapes() {
            let query = generate_leg(&leg, *dialect).unwrap_or_else(|e| panic!("{name} would not render for {dialect}: {e}"));
            assert!(!query.sql().is_empty(), "{name} for {dialect} rendered an empty statement");
            rendered = rendered.saturating_add(1);
        }
    }
    assert_eq!(
        rendered,
        shapes().len() * sutura_sql::dialect::ALL.len(),
        "one statement per shape per dialect, and nothing skipped"
    );
    assert!(rendered > 0, "there is nothing to render");
}

/// The decomposed average, asserted on the statement rather than on the type.
///
/// The type half is in `sutura_domain::plan::leg` - `LegTerm` holds a `PlanTerm` and there is no
/// shape that divides, pinned by a `compile_fail` doctest with a compiling twin. This is the half a
/// reviewer can read: two aggregate columns in one projection, and no division anywhere in the
/// statement. The bug it closes is specific - `ZeroDenominator::Null` renders as `NULLIF(d, 0)`, so
/// applied inside a leg a subgroup with a zero denominator becomes null, the `SUM` above skips
/// nulls, and that subgroup's numerator is dropped from the answer instead of nulling it.
#[test]
fn an_aggregate_leg_projects_a_decomposed_average_as_two_terms_undivided() {
    for dialect in sutura_sql::dialect::ALL {
        let query = generate_leg(&fact_decomposed_average(), *dialect).expect("the fixture renders");
        let sql = query.sql();
        // Compared case-insensitively, and that is not fussiness: `ClickHouse` renders the sum as
        // `sum(` and the count as `COUNT(`, in one statement. A case-sensitive assertion here reads
        // as a claim about the aggregate and is a claim about one dialect's capitalisation.
        let shouted = sql.to_uppercase();
        assert!(
            shouted.contains("SUM(") && shouted.contains("COUNT("),
            "the two halves of the average did not both travel for {dialect}:\n{sql}"
        );
        assert!(
            !shouted.contains("NULLIF"),
            "a zero guard reached a leg for {dialect}, which is what silently drops a subgroup:\n{sql}"
        );
        assert!(!sql.contains('/'), "a division reached a leg for {dialect}:\n{sql}");
        assert!(
            !shouted.contains("AVG("),
            "the average was computed per leg for {dialect}, which is not the average:\n{sql}"
        );
    }
}

/// The distinct-key shape, asserted the same way.
///
/// A `Fact` with an empty `terms` list projects its keys and groups by them, which is a distinct key
/// set - not a count. A count here would be the wrong number twice over: adding two exact distinct
/// counts over-counts every key the two legs share, which is the reason 0009 pulls the rows up at
/// all.
#[test]
fn a_fact_leg_with_no_terms_projects_keys_rather_than_a_count() {
    for dialect in sutura_sql::dialect::ALL {
        let query = generate_leg(&fact_distinct_keys(), *dialect).expect("the fixture renders");
        let shouted = query.sql().to_uppercase();
        assert!(
            !shouted.contains("COUNT("),
            "the distinct-key leg counted instead of carrying its keys for {dialect}:\n{}",
            query.sql()
        );
        assert!(
            shouted.contains("GROUP BY"),
            "the distinct-key leg did not group, so its keys are not distinct for {dialect}:\n{}",
            query.sql()
        );
        // Quoted with the dialect's own character rather than a literal `"` - BigQuery uses a
        // backtick, and a hard-coded double quote failed here rather than passing vacuously.
        let quote = dialect.identifier_quote().character();
        // The physical join column, the pulled key - and the reserved ALIAS the link is projected
        // under, which is the half a leading digit makes worth asserting per dialect: unquoted it
        // would not be an identifier at all in any of the four.
        for name in [
            String::from("subscription_key"),
            String::from("customer_key"),
            InternalLabel::Link.label(),
        ] {
            assert!(
                query.sql().contains(&format!("{quote}{name}{quote}")),
                "the distinct-key leg does not project {name:?} quoted with {quote:?} for {dialect}:\n{}",
                query.sql()
            );
        }
    }
}

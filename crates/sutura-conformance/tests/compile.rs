//! Calls the public compile packs over fake catalog ports, never a warehouse or catalog adapter.
//! Expected JSON and SQL are authored here, not obtained from the compiler/renderer under test.
//! These fixtures cover mono plans and refusals, not the federated rendering branch or real catalog
//! registrations. New API calls cannot be replayed on a base without that API; mutation evidence is
//! required for their assertions, separately from the base-compatible dependency gate regression.

#![cfg(test)]
#![cfg(feature = "compile")]

use std::cell::Cell;

use serde_json::{Value, json};
use sutura_conformance::compile::{self, Case, Expected, Fault, Golden, Rendering};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities, UnfaithfulDeclaration};
use sutura_domain::catalog::{Definitions, Description, Metric, Model};
use sutura_domain::knowledge::{
    Capability, Caveat, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, NoteName, Referent,
};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};
use sutura_domain::query::{Query, RefusalReason};
use sutura_domain::warehouse::ParamValue;
use sutura_sql::{Dialect, GeneratedQuery};

#[derive(Debug, thiserror::Error)]
#[error("fixture load refused")]
struct Unavailable;

#[derive(Clone, Copy)]
enum Distortion {
    None,
    Prose,
    Measure,
    Unstable,
    Unavailable,
}

struct Catalog<const GOLDEN: bool, const DECLARATION: u8 = 0> {
    distortion: Distortion,
    reads: Cell<usize>,
}

impl<const GOLDEN: bool, const DECLARATION: u8> Catalog<GOLDEN, DECLARATION> {
    const fn new(distortion: Distortion) -> Self {
        Self {
            distortion,
            reads: Cell::new(0),
        }
    }
}

impl<const GOLDEN: bool, const DECLARATION: u8> SemanticCatalog for Catalog<GOLDEN, DECLARATION> {
    type Error = Unavailable;
    const KIND: CatalogKind = if GOLDEN { CatalogKind::Golden } else { CatalogKind::Declaring };

    fn capabilities() -> MetadataCapabilities {
        match DECLARATION {
            1 => MetadataCapabilities::nothing(),
            2 => MetadataCapabilities::everything(),
            _ => declared(),
        }
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let previous = self.reads.replace(self.reads.get() + 1);
        match self.distortion {
            Distortion::Unavailable => Err(Unavailable),
            Distortion::Prose => Ok(bundle("different prose", Aggregate::CountDistinct)),
            Distortion::Measure => Ok(bundle("reference", Aggregate::Sum)),
            Distortion::Unstable if previous > 0 => Ok(bundle("changed", Aggregate::CountDistinct)),
            Distortion::None | Distortion::Unstable => Ok(oracle()),
        }
    }
}

fn declared() -> MetadataCapabilities {
    MetadataCapabilities::of(
        DefinitionCapabilities::of([
            DefinitionKind::Structure,
            DefinitionKind::Descriptions,
            DefinitionKind::Metrics,
            DefinitionKind::Grains,
        ]),
        KnowledgeCapabilities::of([]),
    )
}

fn bundle(prose: &str, aggregate: Aggregate) -> PinnedDefinitions {
    let model = Model::new(
        ModelName::parse("t").expect("model"),
        SourceName::parse("local").expect("source"),
        TableName::parse("t").expect("table"),
        ["day", "amount"]
            .into_iter()
            .map(|name| ColumnName::parse(name).expect("column"))
            .collect(),
        Description::parse(prose).expect("prose"),
    );
    let metric = Metric::new(
        MetricName::parse("total").expect("metric"),
        ModelName::parse("t").expect("model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            aggregate,
            ColumnName::parse("amount").expect("column"),
        ))),
        vec![],
        ColumnName::parse("day").expect("column"),
        std::iter::once(Grain::Day).collect(),
        vec![],
        None,
        Description::parse("reference").expect("prose"),
    )
    .expect("metric");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("compile-fixture").expect("version"),
        Definitions::assemble(vec![model], vec![], vec![metric]).expect("definitions"),
        Knowledge::none(),
        ContributionManifest::single(SourceName::parse("local").expect("source"), Contribution::of(declared())),
    )
    .expect("pinned fixture")
}

fn oracle() -> PinnedDefinitions {
    bundle("reference", Aggregate::CountDistinct)
}

fn golden() -> Catalog<true> {
    Catalog::new(Distortion::None)
}

fn declaring() -> Catalog<false> {
    // This is deliberately NOT the oracle. A declaring catalog owes fidelity, not these contents.
    Catalog::new(Distortion::Prose)
}

fn question(metric: &str) -> Query {
    Query::new(
        MetricName::parse(metric).expect("metric"),
        Grain::Day,
        TimeRange::new(Date::new(2026, 1, 1).expect("date"), Date::new(2026, 1, 3).expect("date")).expect("range"),
        vec![],
        vec![],
    )
}

fn plan() -> Value {
    json!({
        "source": "local", "metric": "total", "table": "t", "joins": [],
        "bucket": {"label": "period", "grain": "day", "column": {"table": "t", "column": "day"}},
        "keys": [], "measure": {"simple": {"term": {"aggregate": {
            "aggregate": "count_distinct", "column": {"table": "t", "column": "amount"}
        }}}}, "measure_label": "total",
        "filters": [
            {"origin": "definition", "predicate": {"at_or_after": {"column": {"table": "t", "column": "day"}, "param": 0}}},
            {"origin": "definition", "predicate": {"before": {"column": {"table": "t", "column": "day"}, "param": 1}}}
        ],
        "params": [{"Date": "2026-01-01"}, {"Date": "2026-01-03"}],
        "range": {"start": "2026-01-01", "end": "2026-01-03"}, "max_rows": 10000
    })
}

fn statement(dialect: Dialect) -> GeneratedQuery {
    let (quote, bucket, first, second, nulls) = match dialect {
        Dialect::DuckDb => ('"', "CAST(DATE_TRUNC('day', \"t\".\"day\") AS DATE)", "?", "?", ""),
        Dialect::Postgres => ('"', "CAST(DATE_TRUNC('day', \"t\".\"day\") AS DATE)", "$1", "$2", ""),
        Dialect::ClickHouse => ('"', "CAST(dateTrunc('day', \"t\".\"day\") AS DATE)", "?", "?", ""),
        Dialect::BigQuery => ('`', "CAST(DATE_TRUNC(`t`.`day`, DAY) AS DATE)", "?", "?", " NULLS LAST"),
    };
    let q = quote;
    let sql = format!(
        "SELECT {bucket} AS {q}period{q}, COUNT(DISTINCT {q}t{q}.{q}amount{q}) AS {q}total{q} FROM {q}t{q} WHERE {q}t{q}.{q}day{q} >= {first} AND {q}t{q}.{q}day{q} < {second} GROUP BY {bucket} ORDER BY {bucket}{nulls} LIMIT 10001"
    );
    GeneratedQuery::new(
        SourceName::parse("local").expect("source"),
        sql,
        vec![
            ParamValue::Date(Date::new(2026, 1, 1).expect("date")),
            ParamValue::Date(Date::new(2026, 1, 3).expect("date")),
        ],
    )
}

fn renderings() -> Vec<Rendering> {
    sutura_sql::dialect::ALL
        .iter()
        .map(|dialect| Rendering::new(*dialect, vec![statement(*dialect)]))
        .collect()
}

fn refusal() -> RefusalReason {
    RefusalReason::MetricUnknown {
        metric: MetricName::parse("missing").expect("metric"),
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case::new("planned", question("total"), Expected::Planned(plan()), renderings()),
        Case::new("refused", question("missing"), Expected::Refused(refusal()), vec![]),
    ]
}

sutura_conformance::compile_packs! {
    adapter: golden_binding, catalog: crate::Catalog<true>, open: crate::golden,
    golden, oracle: crate::oracle, cases: crate::cases,
}
sutura_conformance::compile_packs! {
    adapter: declaring_binding, catalog: crate::Catalog<false>, open: crate::declaring, declaring,
}

#[test]
fn catalog_faults_keep_load_fidelity_determinism_and_oracle_distinct() {
    let missing = Catalog::<true>::new(Distortion::Unavailable);
    let result = compile::declaration_matches(&missing);
    assert!(matches!(result, Err(Fault::Load(Unavailable))));
    assert_eq!(
        std::error::Error::source(&result.expect_err("load fails"))
            .expect("source")
            .to_string(),
        "fixture load refused"
    );
    assert!(matches!(
        compile::declaration_matches(&Catalog::<true, 1>::new(Distortion::None)),
        Err(Fault::Declaration(UnfaithfulDeclaration::Undeclared { .. }))
    ));
    assert!(matches!(
        compile::declaration_matches(&Catalog::<true, 2>::new(Distortion::None)),
        Err(Fault::Declaration(UnfaithfulDeclaration::Unprovided { .. }))
    ));
    assert!(matches!(
        compile::repeats_its_digest(&Catalog::<true>::new(Distortion::Unstable)),
        Err(Fault::Unstable)
    ));
    for distortion in [Distortion::Prose, Distortion::Measure] {
        let catalog = Catalog::<true>::new(distortion);
        let witness = Golden::new(&catalog).expect("golden");
        assert!(matches!(compile::agrees_with_oracle(&witness, &oracle()), Err(Fault::Oracle)));
    }
    assert!(Golden::new(&declaring()).is_err());
}

#[test]
fn knowledge_alone_must_agree_with_the_oracle() {
    let catalog = golden();
    let witness = Golden::new(&catalog).expect("golden");
    let reference = oracle();
    compile::agrees_with_oracle(&witness, &reference).expect("unchanged oracle");
    let knowledge = Knowledge::assemble(
        reference.definitions(),
        KnowledgeInput::new(
            KnowledgeCapabilities::of([Capability::Caveats]),
            vec![],
            vec![Caveat::new(
                NoteName::parse("counting").expect("note name"),
                vec![Referent::Metric {
                    metric: MetricName::parse("total").expect("metric"),
                }],
                NoteBody::parse("Counts distinct amounts, not rows.").expect("note body"),
            )],
            vec![],
            vec![],
        ),
    )
    .expect("knowledge about the existing metric");
    let changed = PinnedDefinitions::pin(
        reference.version().clone(),
        reference.definitions().clone(),
        knowledge,
        reference.manifest().clone(),
    )
    .expect("only knowledge changed");
    assert_eq!(reference.definitions(), changed.definitions());
    assert_ne!(reference.knowledge(), changed.knowledge());
    assert!(matches!(compile::agrees_with_oracle(&witness, &changed), Err(Fault::Oracle)));
}

#[test]
fn plan_and_typed_refusal_are_compared_before_rendering() {
    let catalog = golden();
    let witness = Golden::new(&catalog).expect("golden");
    let mut changed = plan();
    changed["max_rows"] = json!(1);
    for (query, expected) in [
        (question("total"), Expected::Planned(changed)),
        (question("total"), Expected::Federated(plan())),
        (
            question("missing"),
            Expected::Refused(RefusalReason::MetricUnknown {
                metric: MetricName::parse("other").expect("metric"),
            }),
        ),
        (
            question("missing"),
            Expected::Refused(RefusalReason::GrainNotSupported {
                metric: MetricName::parse("missing").expect("metric"),
                grain: Grain::Day,
            }),
        ),
    ] {
        // No renderings: a check reordered after dialect validation would report Dialects instead.
        let result = compile::matches_cases(&witness, &[Case::new("changed", query, expected, vec![])]);
        assert!(matches!(result, Err(Fault::Outcome { case: "changed" })), "{result:?}");
    }
    compile::matches_cases(&witness, &cases()).expect("unchanged plan, refusal and statements");
}

#[test]
fn every_dialect_compares_sql_parameters_and_source() {
    let catalog = golden();
    let witness = Golden::new(&catalog).expect("golden");
    for dialect in sutura_sql::dialect::ALL {
        let original = statement(*dialect);
        let alternatives = [
            GeneratedQuery::new(
                original.source().clone(),
                String::from("SELECT 0"),
                original.params().to_vec(),
            ),
            GeneratedQuery::new(original.source().clone(), original.sql().to_owned(), vec![]),
            GeneratedQuery::new(
                SourceName::parse("other").expect("source"),
                original.sql().to_owned(),
                original.params().to_vec(),
            ),
        ];
        for changed in alternatives {
            let mut expected: Vec<Rendering> = sutura_sql::dialect::ALL
                .iter()
                .filter(|other| *other != dialect)
                .map(|other| Rendering::new(*other, vec![statement(*other)]))
                .collect();
            expected.push(Rendering::new(*dialect, vec![changed]));
            let result = compile::matches_cases(
                &witness,
                &[Case::new("changed", question("total"), Expected::Planned(plan()), expected)],
            );
            assert!(
                matches!(result, Err(Fault::Statement { case: "changed", dialect: found }) if found == *dialect),
                "{result:?}"
            );
        }
    }
}

#[test]
fn empty_and_incomplete_populations_are_not_passes() {
    let catalog = golden();
    let witness = Golden::new(&catalog).expect("golden");
    assert!(matches!(compile::matches_cases(&witness, &[]), Err(Fault::EmptyCorpus)));
    let mut missing = renderings();
    missing.pop();
    let duplicated = sutura_sql::dialect::ALL
        .iter()
        .map(|_| Rendering::new(Dialect::DuckDb, vec![statement(Dialect::DuckDb)]))
        .collect();
    for expected in [missing, duplicated] {
        assert!(matches!(
            compile::matches_cases(
                &witness,
                &[Case::new(
                    "population",
                    question("total"),
                    Expected::Planned(plan()),
                    expected
                )]
            ),
            Err(Fault::Dialects { case: "population" })
        ));
    }
    assert!(matches!(
        compile::matches_cases(
            &witness,
            &[Case::new(
                "refused",
                question("missing"),
                Expected::Refused(refusal()),
                renderings()
            )]
        ),
        Err(Fault::Dialects { case: "refused" })
    ));
}

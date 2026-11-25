//! A Foundry object named with dots, rendered as one quoted identifier aliased to its local name.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Definitions, Description, JoinKey, JoinKeys, Model, Relationship};
use sutura_domain::model::{
    Aggregate, ColumnName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::plan::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    ResultLabel, StatementTables,
};
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::cardinality::DeclaredKey;

use super::super::{GenerateError, generate, generate_key_probe};
use crate::dialect::{ALL, Dialect};

/// A Foundry plan over the dotted object `com.palantir.system.Evolution`, joined to the dotted
/// object `com.palantir.system.EvolutionProposal`.
fn dotted_object_plan() -> QueryPlan {
    use sutura_domain::model::{ObjectPath, QualifiedTable};
    use sutura_domain::plan::{PlanJoin, PlanJoinKey};

    let object = |dotted: &str| {
        QualifiedTable::object(
            ObjectPath::parse(dotted)
                .expect("dotted names parse")
                .expect("a dotted name is an object path"),
        )
        .expect("the local name is a name")
    };
    let evolution = object("com.palantir.system.Evolution");
    let proposal = object("com.palantir.system.EvolutionProposal");
    let col = |table: &QualifiedTable, name: &str| {
        PlanColumn::new(
            table.name().clone(),
            ColumnName::parse(name).expect("a test column is a column"),
        )
    };
    let date = |raw: &str| Date::parse(raw).expect("a test date is a date");
    let metric = MetricName::parse("evolutions").expect("a test metric is a metric");
    let join = PlanJoin::new(
        RelationshipName::parse("com_palantir_system_Evolution_proposal").expect("a relationship name"),
        proposal.clone(),
        JoinType::ManyToOne,
        sutura_domain::nonempty::NonEmpty::one(PlanJoinKey::Equal {
            origin: col(&evolution, "proposalId"),
            target: col(&proposal, "proposalId"),
        }),
    );
    let bindings = PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: col(&evolution, "created"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: col(&evolution, "created"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(date("2026-06-01")), ParamValue::Date(date("2026-07-01"))],
    )
    .expect("two range bounds bind in placeholder order");
    QueryPlan::new(
        SourceName::parse("ontology").expect("a test source is a source"),
        metric.clone(),
        StatementTables::parse(evolution.clone(), vec![join]).expect("two tables, two local names"),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, col(&evolution, "created")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Count,
                column: col(&evolution, "evolutionId"),
            },
        },
        ResultLabel::measure(&metric),
        bindings,
        TimeRange::new(date("2026-06-01"), date("2026-07-01")).expect("a test range is a range"),
    )
}

#[test]
fn foundry_names_a_dotted_object_as_one_quoted_identifier_aliased_to_its_local_name() {
    let query = generate(&dotted_object_plan(), Dialect::Foundry).expect("a dotted object renders for Foundry");
    let sql = query.sql();
    for fragment in [
        "FROM `com.palantir.system.Evolution` AS `com_palantir_system_Evolution`",
        "JOIN `com.palantir.system.EvolutionProposal` AS `com_palantir_system_EvolutionProposal`",
        "`com_palantir_system_Evolution`.`created`",
        "`com_palantir_system_Evolution`.`proposalId` = `com_palantir_system_EvolutionProposal`.`proposalId`",
    ] {
        assert!(sql.contains(fragment), "missing {fragment:?} in:\n{sql}");
    }
    assert!(
        !sql.contains("`com`.`palantir`"),
        "a dotted object was split into a path:\n{sql}"
    );
    assert!(
        polyglot_sql::parse(sql, super::super::dialect_type(Dialect::Foundry)).is_ok(),
        "{sql}"
    );
}

#[test]
fn every_other_dialect_refuses_a_dotted_object() {
    let plan = dotted_object_plan();
    for dialect in ALL.iter().copied().filter(|dialect| *dialect != Dialect::Foundry) {
        let refused = generate(&plan, dialect);
        assert!(
            matches!(
                refused,
                Err(GenerateError::QualificationUnsupported {
                    carries: sutura_domain::model::Qualification::Object,
                    ..
                })
            ),
            "{dialect} must refuse a dotted object, not render it: {refused:?}"
        );
    }
}

#[test]
fn a_foundry_key_probe_names_a_dotted_object_as_one_quoted_identifier() {
    use sutura_domain::model::{ObjectPath, QualifiedTable};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let evolution = QualifiedTable::object(
        ObjectPath::parse("com.palantir.system.Evolution")
            .expect("dotted names parse")
            .expect("a dotted name is an object path"),
    )
    .expect("the local name is a name");
    let relationship = Relationship::new(
        RelationshipName::parse("orders_evolution").expect("a test relationship is a relationship"),
        ModelName::parse("orders").expect("a test model is a model"),
        ModelName::parse("com_palantir_system_Evolution").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column("evolution_id"),
            target: column("evolution_id"),
        }])
        .expect("a test relationship declares one key"),
    );
    let source = || SourceName::parse("ontology").expect("a test source is a source");
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("orders").expect("a test model is a model"),
                source(),
                TableName::parse("orders").expect("a test table is a table"),
                [column("evolution_id")],
                Description::default(),
            ),
            Model::new(
                ModelName::parse("com_palantir_system_Evolution").expect("a test model is a model"),
                source(),
                evolution,
                [column("evolution_id")],
                Description::default(),
            ),
        ],
        vec![relationship.clone()],
        Vec::new(),
    )
    .expect("two models and one join are consistent");
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");
    let sql = generate_key_probe(&key, Dialect::Foundry)
        .expect("a one-key probe renders for Foundry")
        .sql()
        .to_owned();
    assert!(
        sql.contains("FROM `com.palantir.system.Evolution` AS `com_palantir_system_Evolution`"),
        "the probe must count the dotted object, not a table of its local name:\n{sql}"
    );
    assert!(
        polyglot_sql::parse(&sql, super::super::dialect_type(Dialect::Foundry)).is_ok(),
        "{sql}"
    );
}

//! The catalogs that exist to provoke ONE decision each, built in code rather than as documents.
//!
//! **A sibling of the oracle rather than part of it, and `cargo xtask max-lines` is why:** a
//! hand-written catalog is a list of literals, the parent file grows with every model the corpus
//! declares, and adding the third fake here took it past a thousand lines. `devco/max-lines-ignore`
//! refuses any pattern under `crates/` on purpose, so the answer is to split.
//!
//! **What these three share is what makes them a file rather than three unrelated fakes.** None of
//! them is compared against a document, none of them ever executes, and each carries the shape one
//! decision needs and nothing else - so a reduction that would be dishonest in the oracle (two
//! models instead of eleven metrics, no prose, no anchor) is the correct shape here. Each type's own
//! doc comment says which decision it is for.
//!
//! **"Refusal" is the wrong word for one of the three now, and the module is named for the group
//! rather than renamed for the change.** [`TwoSourceCatalog`] was written when a question reaching a
//! second data system was refused; the splitter serves that case, so what it provokes is a SPLIT.
//! Its own doc comment says so where it used to claim a refusal.
//!
//! They are a child module of `oracle`, so the literal helpers up there - `column`, `dimension`,
//! `source`, `version` - are in scope without being made public to the suite.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, Metric, Model, Relationship};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{CatalogKind, PinnedDefinitions, SemanticCatalog};

use super::super::Never;
use super::{column, dimension, source, version};

/// The snapshot and its customers, with `customers` moved to a second data system.
///
/// **It exists to provoke one plan SHAPE, and it used to be a refusal.** A question whose join
/// reaches a second data system was declined before anything ran, because a second data system is a
/// second identity to satisfy; `docs/adr/0007`'s splitter serves exactly two by splitting the
/// question into a fact leg and a lookup leg, so what this fake now provokes is
/// `sutura_semantic::Compiled::Federated`. The refusal it was built for is
/// `PlanSpansTooManySources`, which needs a THIRD source and no fake here has one.
///
/// **Two models and one metric rather than the whole catalog, and the reduction is deliberate.**
/// Nothing here ever executes and nothing compares it against a document, so carrying eleven
/// metrics would be eleven more literals to keep in step with a directory this type is not a
/// statement of. What it does have to carry is the shape the refusal needs: a metric on the LOCAL
/// model whose dimension is reached `via` a relationship whose target sits ELSEWHERE. The
/// definitional filter the real `recurring_revenue` declares is left off for the same reason - it
/// changes no plan that is refused before planning finishes.
pub(crate) fn two_source_catalog() -> TwoSourceCatalog {
    TwoSourceCatalog
}

/// See [`two_source_catalog`].
pub(crate) struct TwoSourceCatalog;

impl SemanticCatalog for TwoSourceCatalog {
    type Error = Never;

    /// **Declaring**, the narrow pole of the fidelity test: two models on two data systems and none
    /// of the kinds a plan would only consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// **Five declared absences, which is what makes this the narrow end of the fidelity test.** Two
    /// models on two data systems, one metric, one join that licenses one dimension - and no prose,
    /// no definitional filter, no value allowlist and no anchor, because the refusal this fake exists
    /// to provoke happens before any of those would matter. Its own doc comment says so; this line is
    /// the same reduction stated where a caller could read it.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let subscriptions = Model::new(
            ModelName::parse("subscriptions").expect("a name"),
            source(),
            TableName::parse("fct_subscription_monthly").expect("a name"),
            BTreeSet::from([
                column("month"),
                column("subscription_key"),
                column("customer_key"),
                column("mrr_cents"),
            ]),
            Description::default(),
        );
        let customers = Model::new(
            ModelName::parse("customers").expect("a name"),
            SourceName::parse("elsewhere").expect("a name"),
            TableName::parse("dim_customer").expect("a name"),
            BTreeSet::from([column("customer_key"), column("region")]),
            Description::default(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("subscription_customer").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            column("customer_key"),
            ModelName::parse("customers").expect("a name"),
            column("customer_key"),
            JoinType::ManyToOne,
        )];
        let recurring_revenue = Metric::new(
            MetricName::parse("recurring_revenue").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
            Vec::new(),
            column("month"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::from([dimension("region", "region", Some("subscription_customer"), None)]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![subscriptions, customers], joins, vec![recurring_revenue])
            .expect("a two-source catalog is still internally consistent");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}

// ------------------------------------------------- a catalog whose two tables share a name ---

/// Two models in two datasets whose tables are both called `orders`.
///
/// It exists to provoke one refusal, and that refusal is the one a review reproduced: a column in a
/// plan is qualified by the LAST part of a table path, so two paths ending the same way render under
/// one implicit alias and the `ON` clause compares one table with itself. A real `DuckDB` answers such a
/// statement with `Binder Error: Ambiguous reference to table "orders"`; a target that binds it to one
/// side instead returns a number under a certified metric name.
///
/// **The catalog LOADS, and that is the design decision rather than an oversight.** Unlike a colliding
/// label, a physical table name is not something an author can rename, and same-name tables across
/// datasets are the normal shape of the estate qualified paths exist for - so the metric stays
/// authorable and only a question that actually puts both tables in one statement is declined.
/// `sutura_domain::plan::tables` is where that is argued and where the guard lives.
///
/// **Two models, one relationship and one metric, for the reason [`TwoSourceCatalog`] gives:** nothing
/// here executes and nothing compares it against a document, so what it carries is the shape the
/// refusal needs and nothing else. The metric has TWO dimensions rather than one, and that is the
/// exception: one is reached through the colliding join and one is not, so a test can show the refusal
/// is about the QUESTION rather than about the metric. Both models are on ONE source, which is the
/// half that makes this about aliasing rather than about federation.
pub(crate) fn same_name_tables_catalog() -> SameNameTablesCatalog {
    SameNameTablesCatalog
}

/// See [`same_name_tables_catalog`].
pub(crate) struct SameNameTablesCatalog;

impl SemanticCatalog for SameNameTablesCatalog {
    type Error = Never;

    /// **Declaring**, for [`TwoSourceCatalog`]'s reason: it supplies part of the model and none of
    /// the kinds a plan would consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// The same five declared absences [`TwoSourceCatalog`] declares, and for the same reason: the
    /// refusal this fake provokes happens before prose, a definitional filter, an allowlist or an
    /// anchor would matter.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \
                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \
                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let fact = Model::new(
            ModelName::parse("sales_orders").expect("a name"),
            source(),
            QualifiedTable::parse("analytics_prod.sales.orders").expect("a path"),
            BTreeSet::from([column("order_date"), column("customer_id"), column("amount_cents")]),
            Description::default(),
        );
        // The SAME table name, in another dataset of another project, reached by the same credential.
        // One source, two paths, one implicit alias.
        let lookup = Model::new(
            ModelName::parse("crm_orders").expect("a name"),
            source(),
            QualifiedTable::parse("reference_data.crm.orders").expect("a path"),
            BTreeSet::from([column("customer_id"), column("region")]),
            Description::default(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("order_crm").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            column("customer_id"),
            ModelName::parse("crm_orders").expect("a name"),
            column("customer_id"),
            JoinType::ManyToOne,
        )];
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            // Two dimensions on purpose: one needs the colliding join and one does not, so the test
            // can show that the refusal is about the QUESTION rather than about the metric.
            BTreeMap::from([
                dimension("region", "region", Some("order_crm"), None),
                dimension("customer", "customer_id", None, None),
            ]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![fact, lookup], joins, vec![revenue])
            .expect("two tables of one name are still internally consistent - the QUESTION is what is refused");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}

// -------------------------- a catalog whose FEDERATED fact leg reads two tables of one name ---

/// [`SameNameTablesCatalog`]'s collision, on a catalog that also reaches a SECOND data system.
///
/// **It exists because the guard that catches the collision was reached by one of the two plan
/// shapes and not by the other, and the second shape is the one that renders the wrong number.**
/// `sutura_semantic::plan` splits a two-source question into a fact leg and a lookup leg; the fact
/// leg keeps every SAME-SOURCE hop as a `JOIN` of its own, so the whole ambiguity
/// [`SameNameTablesCatalog`] provokes is available inside one leg's statement - and until the change
/// this fake arrived with, the splitter built that leg by struct literal and never asked
/// `sutura_domain::plan::StatementTables::parse` about it.
///
/// **Three models rather than two, and each one is load-bearing.** The fact model and the
/// same-source dimension model are the collision - two paths ending in `orders`, one credential, one
/// statement. The third model sits on `elsewhere`, and it is what makes the question FEDERATE rather
/// than take the whole-answer path the sibling fake already covers: without it the plan stage would
/// never reach the splitter, and the bypass would stay invisible.
pub(crate) fn federated_same_name_tables_catalog() -> FederatedSameNameTablesCatalog {
    FederatedSameNameTablesCatalog
}

/// See [`federated_same_name_tables_catalog`].
pub(crate) struct FederatedSameNameTablesCatalog;

impl SemanticCatalog for FederatedSameNameTablesCatalog {
    type Error = Never;

    /// **Declaring**, for [`TwoSourceCatalog`]'s reason: it supplies part of the model and none of
    /// the kinds a plan would consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// The same five declared absences [`TwoSourceCatalog`] declares, and for the same reason.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \
                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \
                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let fact = Model::new(
            ModelName::parse("sales_orders").expect("a name"),
            source(),
            QualifiedTable::parse("analytics_prod.sales.orders").expect("a path"),
            BTreeSet::from([column("order_date"), column("customer_id"), column("amount_cents")]),
            Description::default(),
        );
        // The SAME table name in another dataset of another project, on the SAME source - so the
        // splitter keeps it as a join on the fact leg and both paths land in one statement.
        let crm = Model::new(
            ModelName::parse("crm_orders").expect("a name"),
            source(),
            QualifiedTable::parse("reference_data.crm.orders").expect("a path"),
            BTreeSet::from([column("customer_id"), column("segment")]),
            Description::default(),
        );
        // The second data system, and the only reason this question federates at all.
        let geo = Model::new(
            ModelName::parse("geo").expect("a name"),
            SourceName::parse("elsewhere").expect("a name"),
            TableName::parse("dim_region").expect("a name"),
            BTreeSet::from([column("customer_id"), column("region")]),
            Description::default(),
        );
        let joins = vec![
            Relationship::new(
                RelationshipName::parse("order_crm").expect("a name"),
                ModelName::parse("sales_orders").expect("a name"),
                column("customer_id"),
                ModelName::parse("crm_orders").expect("a name"),
                column("customer_id"),
                JoinType::ManyToOne,
            ),
            Relationship::new(
                RelationshipName::parse("order_geo").expect("a name"),
                ModelName::parse("sales_orders").expect("a name"),
                column("customer_id"),
                ModelName::parse("geo").expect("a name"),
                column("customer_id"),
                JoinType::ManyToOne,
            ),
        ];
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            // `segment` reaches the colliding same-source table, `region` reaches the second data
            // system, and `customer` reaches neither - so one question can federate WITH the
            // collision, and another can federate without it.
            BTreeMap::from([
                dimension("segment", "segment", Some("order_crm"), None),
                dimension("region", "region", Some("order_geo"), None),
                dimension("customer", "customer_id", None, None),
            ]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![fact, crm, geo], joins, vec![revenue])
            .expect("two tables of one name are still internally consistent - the QUESTION is what is refused");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}

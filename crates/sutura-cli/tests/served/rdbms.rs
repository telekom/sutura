//! A real dictionary loaded by the served HTTP binary, checked against an independent bundle.
//! This checks catalog metadata and provenance; it does not execute a query or exercise MCP.

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use crate::harness::{TOKEN, VERSION, rdbms_settings, start_configured, v1};

fn expected() -> PinnedDefinitions {
    let columns = [
        ("month", "date", "The subscription month."),
        ("mrr_cents", "bigint", "Monthly recurring revenue in cents."),
    ]
    .into_iter()
    .map(|(name, data_type, description)| {
        Column::from_metadata(
            ColumnName::parse(name).expect("a fixture column name parses"),
            Some(data_type),
            Some(description),
            None,
        )
        .expect("a fixture column description parses")
    });
    let model = Model::new(
        ModelName::parse("subscriptions").expect("the fixture model name parses"),
        SourceName::parse("warehouse").expect("the fixture source alias parses"),
        QualifiedTable::parse("public.fct_subscription_monthly").expect("the fixture table parses"),
        columns,
        Description::parse("Monthly subscription records.").expect("the fixture description parses"),
    );
    let declared = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([
            DefinitionKind::Descriptions,
            DefinitionKind::Relationships,
            DefinitionKind::ColumnTypes,
            DefinitionKind::ColumnDescriptions,
        ]),
        KnowledgeCapabilities::none(),
    );
    PinnedDefinitions::pin(
        DefinitionVersion::parse(VERSION).expect("the served version parses"),
        Definitions::assemble(vec![model], vec![], vec![]).expect("the fixture definitions assemble"),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("dictionary").expect("the catalog name parses"),
            Contribution::of(declared),
        ),
    )
    .expect("the expected bundle hashes")
}

#[test]
fn an_rdbms_catalog_boots_and_lists_from_the_served_binary() {
    let case = "rdbms-boot-and-list";
    let Some((settings, _guard)) = rdbms_settings(case) else {
        return;
    };
    // Drop the process before its dictionary and the shared fixture-load lock.
    let deployment = start_configured(case, &settings);
    let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["metrics"], serde_json::json!([]));
    assert_eq!(body["provenance"]["definition_version"], VERSION);
    assert_eq!(
        body["provenance"]["definition_digest"],
        expected().digest().as_str(),
        "the served RDBMS bundle differs from the independent fixture: {}",
        reply.body
    );
}

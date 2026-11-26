use super::*;
use std::collections::BTreeSet;
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Audience, AudienceGrant, Column, Definitions, Description, GrantedAudiences, JoinKey, JoinKeys, Model, Relationship,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{AudienceId, ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

fn bundle() -> PinnedDefinitions {
    let model = |name: &str, audience| {
        Model::new(
            ModelName::parse(name).unwrap(),
            SourceName::parse("warehouse").unwrap(),
            TableName::parse(name).unwrap(),
            ["tenant", "id"].map(|name| Column::new(ColumnName::parse(name).unwrap(), None, Description::default(), None)),
            Description::default(),
        )
        .with_audience(audience)
    };
    let restricted = Audience::Restricted(AudienceGrant::parse(BTreeSet::from([AudienceId::parse("finance").unwrap()])).unwrap());
    let relationship = |name: &str, origin, target| {
        Relationship::new(
            RelationshipName::parse(name).unwrap(),
            ModelName::parse(origin).unwrap(),
            ModelName::parse(target).unwrap(),
            JoinType::ManyToOne,
            JoinKeys::of(
                ["tenant", "id"]
                    .map(|name| JoinKey::Equal {
                        origin: ColumnName::parse(name).unwrap(),
                        target: ColumnName::parse(name).unwrap(),
                    })
                    .to_vec(),
            )
            .unwrap(),
        )
    };
    let definitions = Definitions::assemble(
        vec![
            model("events", Audience::Open),
            model("apps", Audience::Open),
            model("hidden_apps", restricted.clone()),
            model("hidden_origin", restricted),
        ],
        vec![
            relationship("events_apps", "events", "apps"),
            relationship("events_hidden", "events", "hidden_apps"),
            relationship("hidden_origin_apps", "hidden_origin", "apps"),
        ],
        Vec::new(),
    )
    .unwrap();
    let capabilities = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test").unwrap(),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(SourceName::parse("warehouse").unwrap(), Contribution::of(capabilities)),
    )
    .unwrap()
}

#[test]
fn described_relationships_keep_composite_keys_and_hide_an_invisible_endpoint() {
    let bundle = bundle();
    let view = ScopedView::granted_by(&bundle, GrantedAudiences::none());
    let content = CatalogContent::of_with_physical_schema(&view, CatalogProse::Omitted, None, true, None, None, None);
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(
        json["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["name"] == "events")
            .unwrap()["relationships"],
        serde_json::json!([{
            "name":"events_apps", "from":"events", "to":"apps", "cardinality":"many_to_one",
            "keys":[{"from":"tenant", "to":"tenant"}, {"from":"id", "to":"id"}]
        }])
    );
    let text = content.as_text();
    assert!(
        text.contains("relationship events_apps: events -> apps (many_to_one)"),
        "{text}"
    );
    assert!(text.contains("key tenant = tenant") && text.contains("key id = id"), "{text}");
    assert!(
        !text.contains("events_hidden") && !text.contains("hidden_apps") && !text.contains("hidden_origin"),
        "{text}"
    );
    let prompt = sutura_app::prompt::physical_schema(
        &view,
        CatalogProse::Omitted,
        sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES,
    );
    assert!(
        prompt.contains("Relationship `events_apps`: `events` -> `apps` (`many_to_one`)"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("events_hidden") && !prompt.contains("hidden_apps") && !prompt.contains("hidden_origin"),
        "{prompt}"
    );
    let disabled = serde_json::to_value(CatalogContent::of(&view, CatalogProse::Omitted, None)).unwrap();
    assert!(
        disabled["models"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|model| model.get("relationships").is_none())
    );
    let full = CatalogContent::of_with_physical_schema(
        &ScopedView::everything(&bundle),
        CatalogProse::Omitted,
        None,
        true,
        None,
        None,
        None,
    );
    assert_eq!(
        serde_json::to_value(full).unwrap()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["relationships"].as_array().unwrap().len())
            .sum::<usize>(),
        3
    );
}

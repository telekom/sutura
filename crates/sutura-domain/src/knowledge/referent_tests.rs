//! Who is shown a note that names a model or a column: the model's own audience decides.

use std::collections::BTreeSet;

use super::tests::{audience_id, column, glossary_entry, only_glossary};
use super::{Knowledge, Referent};
use crate::capabilities::MetadataCapabilities;
use crate::catalog::{Audience, AudienceGrant, Definitions, Description, GrantedAudiences, Model};
use crate::model::{ModelName, SourceName, TableName};
use crate::pinned::view::ScopedView;
use crate::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

#[test]
fn a_restricted_models_note_is_shown_only_to_a_caller_holding_its_grant() {
    let source = SourceName::parse("local").expect("a test source is a source");
    let subscriptions = ModelName::parse("subscriptions").expect("a test model is a model");
    let finance = Audience::Restricted(AudienceGrant::parse(BTreeSet::from([audience_id("finance")])).expect("one id grants"));
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                subscriptions.clone(),
                source.clone(),
                TableName::parse("fct_subscription_monthly").expect("a test table is a table"),
                BTreeSet::from([column("mrr_cents")]),
                Description::default(),
            )
            .with_audience(finance),
        ],
        vec![],
        vec![],
    )
    .expect("one model and no metric are consistent");
    let knowledge = Knowledge::assemble(
        &definitions,
        only_glossary(vec![
            glossary_entry(
                "billing table",
                &[],
                Referent::Model {
                    model: subscriptions.clone(),
                },
            ),
            glossary_entry(
                "billed amount",
                &[],
                Referent::Column {
                    model: subscriptions,
                    column: column("mrr_cents"),
                },
            ),
        ]),
    )
    .expect("every entry names a declared model or column");
    let pinned = PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        knowledge.clone(),
        ContributionManifest::single(
            source,
            Contribution::of(MetadataCapabilities::produced(&definitions, &knowledge)),
        ),
    )
    .expect("the test definitions hash");
    let terms = |grants: &[&str]| -> Vec<String> {
        let granted = GrantedAudiences::of(grants.iter().map(|grant| audience_id(grant)).collect());
        knowledge
            .scoped(&ScopedView::granted_by(&pinned, granted))
            .glossary()
            .keys()
            .map(|term| String::from(term.as_str()))
            .collect()
    };
    // A model with an audience is withheld unless the caller holds a grant of it: no grant and a
    // grant of some other audience see neither the note on the model nor the one on its column.
    assert_eq!(terms(&[]), Vec::<String>::new());
    assert_eq!(terms(&["legal"]), Vec::<String>::new());
    assert_eq!(terms(&["finance"]), vec!["billed amount", "billing table"]);
}

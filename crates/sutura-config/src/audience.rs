//! The deployment's own mapping from a verified group claim to `docs/adr/0028`'s audience
//! identifiers.
//!
//! **Deliberately deployment policy and not part of the pinned bundle.** The catalog author
//! classifies metadata under portable audience identifiers; a deployer separately decides which of
//! their identity provider's groups map onto each one. Changing this mapping does not move the
//! definition digest - `docs/adr/0028`'s "the catalog declaration moves the digest; the deployment
//! mapping does not" - and it is not a scope: a scope is part of the deployed authorization-server
//! contract, and adding a metric must not require adding one.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::model::{AudienceId, InvalidIdentifier};

/// Why a mapping this operator wrote is not a usable one.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidAudienceMapping {
    /// One of the audience identifiers a group maps to is not a usable one.
    #[error("group {group}'s mapping names an audience that is not a usable identifier")]
    Identifier {
        group: String,
        #[source]
        cause: InvalidIdentifier,
    },
}

/// Maps a verified caller's `groups` claim values onto `docs/adr/0028` audience identifiers.
///
/// **Absence is the safe default.** An unconfigured mapping grants nothing to anyone, which reads a
/// restricted metric as invisible rather than open - the fail-closed direction this workspace
/// prefers on the query path. That is the opposite failure mode from the catalog's own audience
/// field, which has no default at all for the reason stated there: here the empty map is a genuine
/// answer (no groups mapped yet), while there a missing declaration must not parse into one.
#[derive(Debug, Clone, Default)]
pub struct AudienceMapping {
    granted: BTreeMap<String, BTreeSet<AudienceId>>,
}

impl AudienceMapping {
    /// Parses an operator's `security.audience_mapping`.
    pub fn parse(raw: BTreeMap<String, BTreeSet<String>>) -> Result<Self, InvalidAudienceMapping> {
        let mut granted = BTreeMap::new();
        for (group, audiences) in raw {
            let mut ids = BTreeSet::new();
            for audience in audiences {
                let id = AudienceId::parse(&audience).map_err(|cause| InvalidAudienceMapping::Identifier {
                    group: group.clone(),
                    cause,
                })?;
                let _newly_named = ids.insert(id);
            }
            drop(granted.insert(group, ids));
        }
        Ok(Self { granted })
    }

    /// Every audience this deployment grants a caller whose verified `groups` claim named any of
    /// `claimed`.
    ///
    /// **A group this mapping does not name contributes nothing and vetoes nothing** -
    /// `docs/adr/0028`'s table: any mapped group among several is sufficient, and an unmapped group
    /// in a mixed claim neither grants nor cancels a mapped one. Union rather than intersection is
    /// what makes that true: each claimed group is looked up independently and its audiences (if
    /// any) are added to the result.
    #[must_use]
    pub fn granted_for<'a>(&self, claimed: impl Iterator<Item = &'a str>) -> BTreeSet<AudienceId> {
        let mut granted = BTreeSet::new();
        for group in claimed {
            if let Some(audiences) = self.granted.get(group) {
                granted.extend(audiences.iter().cloned());
            }
        }
        granted
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::AudienceMapping;
    use sutura_domain::model::AudienceId;

    fn id(raw: &str) -> AudienceId {
        AudienceId::parse(raw).expect("a test audience id is one")
    }

    fn mapping() -> AudienceMapping {
        AudienceMapping::parse(BTreeMap::from([
            (String::from("finance-team"), BTreeSet::from([String::from("finance")])),
            (String::from("platform-eng"), BTreeSet::from([String::from("engineering")])),
        ]))
        .expect("a test mapping is well formed")
    }

    #[test]
    fn an_unmapped_group_neither_grants_nor_vetoes_a_mapped_one() {
        let granted = mapping().granted_for(["finance-team", "some-unrelated-group"].into_iter());
        assert_eq!(granted, BTreeSet::from([id("finance")]));
    }

    #[test]
    fn an_empty_mapping_grants_nothing() {
        let granted = AudienceMapping::default().granted_for(std::iter::once("finance-team"));
        assert!(granted.is_empty());
    }

    #[test]
    fn two_mapped_groups_union_their_audiences() {
        let granted = mapping().granted_for(["finance-team", "platform-eng"].into_iter());
        assert_eq!(granted, BTreeSet::from([id("finance"), id("engineering")]));
    }

    #[test]
    fn an_unusable_identifier_is_refused_by_name() {
        let err = AudienceMapping::parse(BTreeMap::from([(
            String::from("finance-team"),
            BTreeSet::from([String::from("not an identifier!")]),
        )]))
        .expect_err("a character an identifier refuses is not one");
        assert!(err.to_string().contains("finance-team"), "{err}");
    }
}

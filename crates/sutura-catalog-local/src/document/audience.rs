//! The on-disk shape of a metric's audience declaration - `docs/adr/0028-who-may-see-a-metric.md`.
//!
//! Its own file for the reason `document.rs`'s own header gives for `knowledge`: that file is
//! already near `cargo xtask max-lines`'s thousand-line cap, and this is a separate concern from the
//! fields around it - a document DECLARES who may see a metric, and nothing here decides who is
//! asking.
//!
//! **Externally tagged, like `measure`.** `audience: open` is a bare scalar because
//! [`AudienceDoc::Open`] is a unit variant; `audience: { restricted: [finance] }` names a non-empty
//! list. No `#[serde(default)]` on the field this type parses into
//! ([`crate::document::MetricDoc::audience`]) - a missing declaration is a parse error naming the
//! metric, never a silent *open*, which is the property `docs/adr/0028` states as non-negotiable.

use std::collections::BTreeSet;

use sutura_domain::catalog::{Audience, AudienceGrant, InvalidAudienceGrant};
use sutura_domain::model::{AudienceId, InvalidIdentifier};

/// What a document writes for a metric's audience.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudienceDoc {
    Open,
    Restricted(BTreeSet<String>),
}

/// Why an audience declaration a document parsed cannot become the domain's [`Audience`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidAudienceDeclaration {
    #[error(transparent)]
    Identifier(InvalidIdentifier),
    #[error(transparent)]
    Grant(InvalidAudienceGrant),
}

impl AudienceDoc {
    /// Into the domain type, parsing every identifier the restricted arm names.
    pub fn into_domain(self) -> Result<Audience, InvalidAudienceDeclaration> {
        match self {
            Self::Open => Ok(Audience::Open),
            Self::Restricted(raw) => {
                let mut identifiers = BTreeSet::new();
                for one in raw {
                    let parsed = AudienceId::parse(&one).map_err(InvalidAudienceDeclaration::Identifier)?;
                    let _newly_named = identifiers.insert(parsed);
                }
                let grant = AudienceGrant::parse(identifiers).map_err(InvalidAudienceDeclaration::Grant)?;
                Ok(Audience::Restricted(grant))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::catalog::{Audience, AudienceGrant};
    use sutura_domain::model::AudienceId;

    use super::AudienceDoc;

    /// Parses the VALUE an `audience:` key carries, through the same `singleton_map` adapter
    /// `MetricDoc::audience` reads with - not `AudienceDoc`'s own derived `Deserialize`, which
    /// would need `!restricted` YAML-tag syntax nobody authoring a catalog file writes.
    #[derive(Debug, serde::Deserialize)]
    struct Wrapper {
        #[serde(with = "serde_norway::with::singleton_map")]
        audience: AudienceDoc,
    }

    /// `value` is a bare scalar (`"open"`) or an already-indented nested mapping
    /// (`"  restricted: [finance]"`), exactly as it appears under `audience:` in a real document.
    fn parse(value: &str) -> Result<AudienceDoc, serde_norway::Error> {
        serde_norway::from_str::<Wrapper>(&format!("audience: {value}\n")).map(|wrapper| wrapper.audience)
    }

    #[test]
    fn open_is_a_bare_scalar() {
        let doc = parse("open").expect("open is a unit variant");
        assert_eq!(doc.into_domain().expect("open always converts"), Audience::Open);
    }

    #[test]
    fn restricted_names_a_non_empty_set_of_identifiers() {
        let doc = parse("\n  restricted: [finance, exec]").expect("restricted is a newtype variant");
        let expected = Audience::Restricted(
            AudienceGrant::parse(std::collections::BTreeSet::from([
                AudienceId::parse("finance").expect("a test id is one"),
                AudienceId::parse("exec").expect("a test id is one"),
            ]))
            .expect("two ids grant"),
        );
        assert_eq!(doc.into_domain().expect("two valid ids convert"), expected);
    }

    #[test]
    fn an_empty_restricted_list_is_refused_rather_than_read_as_nobody() {
        let doc = parse("\n  restricted: []").expect("an empty list is still a newtype variant");
        let _refused = doc.into_domain().expect_err("an empty restricted grant is not an audience");
    }

    #[test]
    fn an_unusable_identifier_is_refused_by_name() {
        let doc = parse("\n  restricted: [\"not an identifier!\"]").expect("the list itself is readable");
        let err = doc.into_domain().expect_err("a character an identifier refuses is not one");
        assert!(matches!(err, super::InvalidAudienceDeclaration::Identifier(_)), "{err}");
    }

    #[test]
    fn an_unknown_spelling_is_refused_by_name() {
        let err = parse("hidden").expect_err("hidden is not open or restricted");
        assert!(err.to_string().contains("hidden"), "{err}");
    }
}

//! Who may see one metric: `docs/adr/0028-who-may-see-a-metric.md`. The declaration only -
//! filtering is `crate::pinned::view`.
//!
//! **A missing declaration never means everyone**: `super::Metric::new` takes an [`Audience`]
//! with no default.

use std::collections::BTreeSet;

use crate::model::AudienceId;

/// Why a restricted audience is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAudienceGrant {
    /// A restricted grant naming nobody is an unaskable metric, not a narrower audience -
    /// `docs/adr/0028` treats the two as different declarations.
    #[error("a restricted audience must name at least one identifier")]
    Empty,
}

/// One or more audience identifiers a restricted metric is visible to. Parsed rather than a bare
/// `BTreeSet<AudienceId>`, so [`InvalidAudienceGrant::Empty`] is refused once, at assembly.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AudienceGrant(BTreeSet<AudienceId>);

impl AudienceGrant {
    pub fn parse(identifiers: BTreeSet<AudienceId>) -> Result<Self, InvalidAudienceGrant> {
        if identifiers.is_empty() {
            return Err(InvalidAudienceGrant::Empty);
        }
        Ok(Self(identifiers))
    }

    /// Does this grant intersect a caller's granted set?
    #[inline]
    #[must_use]
    pub fn intersects(&self, granted: &BTreeSet<AudienceId>) -> bool {
        self.0.iter().any(|identifier| granted.contains(identifier))
    }

    #[inline]
    #[must_use]
    pub const fn identifiers(&self) -> &BTreeSet<AudienceId> {
        &self.0
    }
}

/// What a deployment mapped a verified caller's group claim onto, for one request. Owned, not
/// borrowed: unlike a pinned bundle this set is small, built per request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrantedAudiences(BTreeSet<AudienceId>);

impl GrantedAudiences {
    /// Nothing granted.
    #[inline]
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// What was granted.
    #[inline]
    #[must_use]
    pub const fn of(granted: BTreeSet<AudienceId>) -> Self {
        Self(granted)
    }

    #[inline]
    #[must_use]
    pub const fn as_set(&self) -> &BTreeSet<AudienceId> {
        &self.0
    }
}

/// Who may see one metric, under the definition digest. Two cases and no third: open to every
/// verified caller, or restricted to a non-empty [`AudienceGrant`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Audience {
    /// Visible to every verified caller, and to a deployment that verifies none.
    Open,
    /// Visible only to a caller the deployment mapped into one of these identifiers.
    Restricted(AudienceGrant),
}

impl Audience {
    /// Is this metric visible to a caller mapped to this set of audiences?
    ///
    /// Takes the granted set, not a caller or a token: mapping a verified claim to it is a
    /// deployment's own job, done above this crate.
    #[inline]
    #[must_use]
    pub fn visible_to(&self, granted: &BTreeSet<AudienceId>) -> bool {
        match self {
            Self::Open => true,
            Self::Restricted(grant) => grant.intersects(granted),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Audience, AudienceGrant, InvalidAudienceGrant};
    use crate::model::AudienceId;

    fn id(raw: &str) -> AudienceId {
        AudienceId::parse(raw).expect("a test audience id is one")
    }

    #[test]
    fn an_empty_restricted_grant_is_refused_rather_than_read_as_nobody() {
        assert_eq!(AudienceGrant::parse(BTreeSet::new()), Err(InvalidAudienceGrant::Empty));
    }

    #[test]
    fn open_is_visible_to_a_caller_granted_nothing_at_all() {
        assert!(Audience::Open.visible_to(&BTreeSet::new()));
    }

    #[test]
    fn a_restricted_metric_is_visible_only_to_an_intersecting_grant() {
        let audience = Audience::Restricted(AudienceGrant::parse(BTreeSet::from([id("finance")])).expect("one id grants"));
        assert!(!audience.visible_to(&BTreeSet::new()));
        assert!(!audience.visible_to(&BTreeSet::from([id("engineering")])));
        assert!(audience.visible_to(&BTreeSet::from([id("engineering"), id("finance")])));
    }
}

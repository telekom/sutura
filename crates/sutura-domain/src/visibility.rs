//! Which of one source's tables a caller is shown.
//!
//! Decided by that source under the caller's own credential -
//! `docs/adr/20261125083027-catalog-visibility-per-caller.md`.
//!
//! **This narrows what a caller SEES. It is not a data control.** The bundle a question compiles
//! against stays the deployment's; a question on a model hidden here still reaches the source, and
//! the source's own refusal under the caller's token (leg 2) is what keeps the rows away.

use std::collections::BTreeSet;

use crate::identity::Presented;
use crate::model::{ModelName, QualifiedTable};
use crate::warehouse::deadline::Deadline;

/// The tables one source answered it shows a credential's holder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisibleTables(BTreeSet<QualifiedTable>);

impl VisibleTables {
    #[must_use]
    pub const fn of(tables: BTreeSet<QualifiedTable>) -> Self {
        Self(tables)
    }

    /// Whether the source listed `table` for this caller.
    #[must_use]
    pub fn shows(&self, table: &QualifiedTable) -> bool {
        self.0.contains(table)
    }
}

/// A source asked, under one caller's own credential, which of its tables that caller can see.
///
/// Takes the [`Presented`] leg a broker minted and [`crate::identity::Minted::agreeing_with`]
/// checked, never a raw token: the caller's credential reaches this port by the same path it reaches
/// a [`crate::warehouse::Warehouse`].
pub trait CallerListing {
    /// Why the source did not answer. Every variant is a failure: an implementor that cannot list
    /// says so rather than answering an empty or a full set.
    type Error: core::error::Error + Send + Sync + 'static;

    /// The tables `credential`'s holder can see at this source, read before `deadline`.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] when the source was not reached, refused the credential, or answered a shape
    /// the implementor does not read.
    fn visible_to(&self, credential: &Presented, deadline: Deadline) -> Result<VisibleTables, Self::Error>;
}

/// The models one caller is not shown, decided per request above this crate.
///
/// Only ever removes: [`crate::pinned::view::ScopedView::hiding`] takes it, and no value of it can
/// widen what a view shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HiddenModels(BTreeSet<ModelName>);

impl HiddenModels {
    /// Nothing hidden: a deployment that declared no per-caller visibility.
    #[must_use]
    pub const fn none() -> Self {
        Self(BTreeSet::new())
    }

    #[must_use]
    pub const fn of(models: BTreeSet<ModelName>) -> Self {
        Self(models)
    }

    /// Both sets: what either hides stays hidden.
    #[must_use]
    pub fn and(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }

    #[must_use]
    pub fn hides(&self, model: &ModelName) -> bool {
        self.0.contains(model)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

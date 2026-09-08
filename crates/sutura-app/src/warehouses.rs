//! The data systems this process opened, keyed by the name a plan selects them with.
//!
//! # Why a registry rather than one warehouse
//!
//! A plan resolves to a single source for one answer - a question spanning two is federated, and
//! answered where every registered adapter declares `Warehouse::EXECUTES_LEGS` and refused as
//! `FederationNotExecutable` where one does not (three or more are refused at plan time) - but a
//! *deployment* holds as many as its catalog names, and until now the service held exactly one. That
//! made two facts indistinguishable: "this question is for a data system nobody configured" and "this
//! question is for the other one of the two we opened". The first is a refusal an operator has to fix
//! and the second is an ordinary question.
//!
//! So the lookup moves out of the adapter and into a registry keyed by [`SourceName`], and two things
//! follow from that rather than being added:
//!
//! - `answer` selects the warehouse the *plan* named instead of comparing the plan against the one
//!   adapter it was handed, so `RefusalReason::SourceUnavailable` now means what its name says: no
//!   data system is configured under that name.
//! - the anchor pass runs each metric's anchor against the warehouse for *that metric's* source, so a
//!   bundle spanning two configured sources verifies rather than reporting every anchor on the second
//!   one as a source mismatch.
//!
//! # The limit, and it is the reason step ten exists
//!
//! **Every entry is the same adapter type.** `Warehouses<W>` is generic in one `W`, so a deployment
//! can hold two file sources over two directories, or two databases behind one adapter - and cannot
//! hold a file engine and a `BigQuery` adapter at once. Federating across *different* data systems
//! needs a closed enum over the registered adapter types or dynamic dispatch, and which of those is a
//! decision with a record rather than a change to this file: `Warehouse` carries a required associated
//! constant, so it is not object-safe, and that was decided where the constant is declared.
//!
//! What this shape does buy today is the whole of what the boot checks need: more than one source
//! configured, each declaring its own posture, and an answer that says which posture produced it.

use std::collections::BTreeMap;

use sutura_domain::model::SourceName;
use sutura_domain::source::{ExecutedAs, SourcePosture};
use sutura_domain::warehouse::Warehouse;

/// The data systems this process opened.
///
/// Keyed by each adapter's own [`Warehouse::source`] rather than by a name the caller passes
/// alongside it, so the key and the adapter cannot disagree about which source this is - the same
/// reason `PinnedDefinitions::pin` computes its digest from the definitions it stores.
#[derive(Debug, Clone)]
pub struct Warehouses<W> {
    by_source: BTreeMap<SourceName, W>,
}

/// Two adapters were registered for one source.
///
/// Its own type rather than a silent overwrite, because whichever adapter lost would then be the one
/// nobody opened and nothing would say so. It is not the same failure as a duplicate *alias* in the
/// settings tree - that one is refused before any adapter is built - it is a composition root that
/// built two adapters naming one source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "two data systems were opened for source `{at}`, and a plan naming it could only reach one \
     of them. Register one adapter per source"
)]
pub struct SourceAlreadyOpen {
    /// `at` rather than `source`, because `thiserror` reads a field called `source` as the
    /// `Error::source` chain and a `SourceName` there does not compile.
    at: SourceName,
}

impl SourceAlreadyOpen {
    /// Which source was registered twice.
    ///
    /// Named `at` rather than `source` for the reason `PostureNotDeliverable::at` gives: `thiserror`'s
    /// derive gives this type an `Error::source`, and `clippy::same_name_method` is denied.
    #[inline]
    #[must_use]
    pub const fn at(&self) -> &SourceName {
        &self.at
    }
}

impl<W> Warehouses<W>
where
    W: Warehouse,
{
    /// One data system. The canonical constructor, and the only way a registry comes into existence.
    ///
    /// Infallible, because one adapter cannot collide with itself. Every deployment that answers
    /// anything has at least one, so there is deliberately no empty form: a registry with nothing in
    /// it would refuse every question with `SourceUnavailable`, which is a running service that
    /// cannot work - and the composition root refuses that before a listener is bound instead.
    #[must_use]
    pub fn of(one: W) -> Self {
        let mut by_source = BTreeMap::new();
        drop(by_source.insert(one.source().clone(), one));
        Self { by_source }
    }

    /// A second data system, or a refusal naming the source that was already open.
    ///
    /// Consumes and returns, so a registry is built in one expression and there is no half-built state
    /// for something else to read.
    pub fn and(mut self, another: W) -> Result<Self, SourceAlreadyOpen> {
        let source = another.source().clone();
        if self.by_source.contains_key(&source) {
            return Err(SourceAlreadyOpen { at: source });
        }
        drop(self.by_source.insert(source, another));
        Ok(self)
    }

    /// The data system a plan naming `source` runs on, if this process opened one.
    ///
    /// `Option` rather than a refusal, because who turns an absence into a refusal depends on what is
    /// asking: the query path answers `RefusalReason::SourceUnavailable`, and the anchor pass records
    /// `NotExecutedReason::SourceNotConfigured` against the metric. Deciding here would make one of
    /// those two the other's wording.
    #[must_use]
    pub fn get(&self, source: &SourceName) -> Option<&W> {
        self.by_source.get(source)
    }

    /// Every open data system, in source order.
    pub fn each(&self) -> impl Iterator<Item = (&SourceName, &W)> {
        self.by_source.iter()
    }

    /// How many data systems are open. At least one, because [`Self::of`] is the only way in.
    #[must_use]
    pub fn count(&self) -> usize {
        self.by_source.len()
    }

    /// The posture each open data system was handed, as a startup log reads it.
    ///
    /// Read off the adapters rather than off a settings tree, for the reason
    /// `sutura_domain::warehouse::Warehouse::posture` gives: a summary derived from configuration
    /// reports what was configured rather than what was built.
    pub fn postures(&self) -> impl Iterator<Item = (&SourceName, &SourcePosture)> {
        self.by_source.iter().map(|(name, warehouse)| (name, warehouse.posture()))
    }

    /// The execution record for an answer that ran on `source` and nowhere else.
    ///
    /// The one place a mono-source answer's provenance comes from, so the posture in an answer is the
    /// posture the adapter that executed it was holding. `None` when nothing is open for that source,
    /// which is the case the caller has already turned into a refusal by the time it asks.
    #[must_use]
    pub fn executed_on(&self, source: &SourceName) -> Option<ExecutedAs> {
        self.by_source
            .get(source)
            .map(|warehouse| ExecutedAs::of(source.clone(), warehouse.posture().clone()))
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};

    use super::Warehouses;
    use crate::tests_support::FixedWarehouse;

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source is a source")
    }

    fn shared(text: &str) -> SourcePosture {
        SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(AcknowledgementReason::parse(text).expect("a test reason is a reason")),
        }
    }

    #[test]
    fn a_registry_is_keyed_by_each_adapters_own_source_and_refuses_a_second_one_for_it() {
        let registry = Warehouses::of(FixedWarehouse::new(source("local"), shared("a directory of CSVs")));
        assert_eq!(registry.count(), 1);
        assert!(registry.get(&source("local")).is_some());
        assert!(
            registry.get(&source("warehouse")).is_none(),
            "a source nobody opened has no adapter, which is what the query path refuses on"
        );

        let two = registry
            .and(FixedWarehouse::new(source("warehouse"), SourcePosture::ImpersonationAtSource))
            .expect("a second source is a second data system");
        assert_eq!(two.count(), 2);
        assert_eq!(
            two.postures()
                .map(|(name, posture)| (name.as_str(), posture.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            vec![("local", "shared-service-user"), ("warehouse", "impersonation-at-source")],
            "each source reports the posture its own adapter was handed"
        );

        // Refused rather than overwritten: whichever adapter lost would be the one nobody opened,
        // and nothing would say so.
        // `map_err` and a match rather than `expect_err`, because `Warehouses<W>` is `Debug` only when
        // `W` is and a warehouse adapter need not be - which is the same reason `LocalService` writes
        // its `Debug` by hand.
        let Err(collision) = two.and(FixedWarehouse::new(source("local"), SourcePosture::ImpersonationAtSource)) else {
            panic!("one source is one data system");
        };
        assert_eq!(collision.at(), &source("local"));
    }

    #[test]
    fn the_execution_record_carries_the_posture_of_the_adapter_that_would_run_it() {
        // The mechanism behind `a_posture_is_recorded_in_provenance_per_leg`: the record is built from
        // the adapter, so a settings tree that disagreed with what was built could not change it.
        let registry = Warehouses::of(FixedWarehouse::new(source("local"), SourcePosture::ImpersonationAtSource))
            .and(FixedWarehouse::new(source("files"), shared("a directory of CSVs")))
            .expect("two sources");

        let record = registry.executed_on(&source("local")).expect("local is open");
        assert_eq!(
            record.posture(&source("local")).map(SourcePosture::as_str),
            Some("impersonation-at-source")
        );
        assert_eq!(
            record.legs().count(),
            1,
            "a mono-source answer records one leg and not one per open adapter"
        );

        let record = registry.executed_on(&source("files")).expect("files is open");
        assert_eq!(
            record.posture(&source("files")).map(SourcePosture::as_str),
            Some("shared-service-user")
        );
        assert!(registry.executed_on(&source("nowhere")).is_none());
    }
}

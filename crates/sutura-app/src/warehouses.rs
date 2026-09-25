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
//! # The limit, and what a caller does about it
//!
//! **Every entry of ONE `Warehouses<W>` is the same adapter type.** `Warehouse` carries a required
//! associated constant (`IMPERSONATION`), so it is not object-safe and `dyn Warehouse` is
//! unavailable - a heterogeneous set has to be a closed enum over the registered adapter types,
//! decided where that constant is declared. This crate still holds no adapter type, by the same
//! `[dependencies]` this header always described: [`Warehouses::into_mapped`] is generic in TWO
//! adapter types and imports neither, so a composition root builds the concrete registry each
//! source's own posture check needs and then erases it into whichever closed enum that root
//! declares over the adapters it linked - one normal edge outward, never one in.
//!
//! What this shape buys is the whole of what the boot checks need: more than one source configured,
//! of more than one kind if the caller's own enum covers it, each declaring its own posture, and an
//! answer that says which posture produced it.

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

    /// Every adapter, wrapped by `wrap` into a second registry over a second type.
    ///
    /// **Adapter-agnostic, and that is the whole reason it belongs here rather than at a
    /// composition root.** A build that links more than one kind erases each source's own adapter
    /// behind a closed enum it declares - `sutura_app::warehouses`'s own header names the enum as
    /// the remedy for the limit this file states - and that enum lives OUTSIDE this crate, one
    /// normal edge away, because "which adapters a process holds is a property of the BUILD". This
    /// method is what lets a root build the concrete registry it already knows how to build (one
    /// call per source, one `deliverable_by` check against that source's own constant) and THEN
    /// erase it, rather than threading the enum through every step that constructs an adapter.
    ///
    /// Total rather than fallible: `self`'s keys are already distinct by construction (every entry
    /// passed through [`Self::of`] or [`Self::and`], both of which refuse a collision), and `wrap`
    /// changes no key - so the second registry cannot collide either.
    pub fn into_mapped<U, F>(self, mut wrap: F) -> Warehouses<U>
    where
        U: Warehouse,
        F: FnMut(W) -> U,
    {
        Warehouses {
            by_source: self
                .by_source
                .into_iter()
                .map(|(name, adapter)| (name, wrap(adapter)))
                .collect(),
        }
    }

    /// Every entry of `other`, added to `self` - [`Self::and`]'s whole-registry sibling.
    ///
    /// A composition root that opened more than one KIND builds one registry per kind (each still
    /// concrete, so `deliverable_by` still checks a real adapter constant) and erases each into the
    /// SAME closed enum before reaching here - this is the step that turns "several registries of
    /// one erased type" into the one registry a heterogeneous build serves.
    pub fn merge(mut self, other: Self) -> Result<Self, SourceAlreadyOpen> {
        for (source, adapter) in other.by_source {
            if self.by_source.contains_key(&source) {
                return Err(SourceAlreadyOpen { at: source });
            }
            drop(self.by_source.insert(source, adapter));
        }
        Ok(self)
    }

    /// The execution record for an answer that ran on `source` and nowhere else.
    ///
    /// The one place a mono-source answer's provenance comes from, so the posture in an answer is the
    /// posture the adapter that executed it was holding. `None` when nothing is open for that source,
    /// which is the case the caller has already turned into a refusal by the time it asks.
    ///
    /// One leg, so one entry: [`ExecutedAs`] is non-empty by construction and has no `remove`. The
    /// federated path builds its own two-leg record from both adapters - which may name two
    /// different postures since `docs/adr/0040`.
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
    use sutura_domain::warehouse::Warehouse as _;

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

    #[test]
    fn mapping_a_registry_keeps_every_source_and_the_wrap_runs_once_per_entry() {
        // The mechanism a heterogeneous registry is built from: a caller maps a CONCRETE registry
        // into a second type without touching a key, so two calls of this - one per kind a build
        // links - and an `and` between the results is the whole of "erase, then merge". Mapped to
        // the SAME type here (a closed enum's variant constructor is exactly this shape, one
        // argument in and one value of a wider type out), so the assertion is on the registry
        // rather than on a second adapter type this crate would have to import to prove it.
        let concrete = Warehouses::of(FixedWarehouse::new(source("local"), SourcePosture::ImpersonationAtSource))
            .and(FixedWarehouse::new(source("files"), shared("a directory of CSVs")))
            .expect("two sources");

        let mut wrapped_count = 0;
        let mapped: Warehouses<FixedWarehouse> = concrete.into_mapped(|warehouse| {
            wrapped_count += 1;
            warehouse
        });

        assert_eq!(
            wrapped_count, 2,
            "the wrap runs once per entry, not once for the whole registry"
        );
        assert_eq!(mapped.count(), 2, "mapping changes no key, so the count survives");
        assert_eq!(
            mapped.get(&source("local")).map(|warehouse| warehouse.posture().as_str()),
            Some("impersonation-at-source")
        );
        assert_eq!(
            mapped.get(&source("files")).map(|warehouse| warehouse.posture().as_str()),
            Some("shared-service-user")
        );
    }

    #[test]
    fn merging_two_registries_holds_every_source_of_both() {
        let files = Warehouses::of(FixedWarehouse::new(source("local"), shared("a directory of CSVs")));
        let bigquery = Warehouses::of(FixedWarehouse::new(source("warehouse"), SourcePosture::ImpersonationAtSource));

        let merged = files.merge(bigquery).expect("two disjoint registries merge");
        assert_eq!(merged.count(), 2);
        assert!(merged.get(&source("local")).is_some());
        assert!(merged.get(&source("warehouse")).is_some());
    }

    #[test]
    fn merging_two_registries_that_share_a_source_is_refused() {
        // The refusal, proven and not just its predicate: `merge` must not silently keep one of the
        // two adapters registered for the colliding source.
        let one = Warehouses::of(FixedWarehouse::new(source("local"), shared("a directory of CSVs")));
        let two = Warehouses::of(FixedWarehouse::new(source("local"), SourcePosture::ImpersonationAtSource));

        let Err(collision) = one.merge(two) else {
            panic!("one source open in both registries is a collision, not a silent choice");
        };
        assert_eq!(collision.at(), &source("local"));
    }
}

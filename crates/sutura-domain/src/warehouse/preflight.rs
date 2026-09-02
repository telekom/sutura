//! What a data system answered when it was asked whether the bundle's tables are there.
//!
//! **A module of its own rather than three types in `warehouse.rs`**, and the seam is a real one:
//! nothing here is about a plan, a credential or a row. It is the vocabulary for one question a
//! composition root asks once, at boot, before a listener is bound - *does this data system hold the
//! tables the bundle names* - and the reason it needs a vocabulary at all is that there are three
//! honest answers and only two of them are about the tables.
//!
//! # The asymmetry this exists to close
//!
//! A `files` deployment whose catalog names a table with no file behind it does not start: the
//! engine is given one file per model, and a missing one is a refusal naming the model. A networked
//! data system has no such step - the tables live in the dataset, and this process learns whether
//! one is there when a question reaches it. So the same mistyped table name cost a boot refusal on
//! one kind of deployment and a failed answer for whoever asked first on the other.
//!
//! [`Warehouse::preflight`](crate::warehouse::Warehouse::preflight) is the port that closes it, and
//! this is the answer it returns.
//!
//! **The links here are `crate::`-prefixed rather than `super::`-prefixed on purpose.** The API
//! reference pages are generated from these doc comments and copied through verbatim, and
//! `AGENTS.md` records a MEASURED boundary: `crate::`-prefixed links do not warn under
//! `mkdocs build --strict`, while another shape aborted it. `super::` was never measured, so it is
//! not the form to find out with.

use std::collections::BTreeSet;

use crate::model::QualifiedTable;

/// A non-empty set of tables a data system was asked about and does not hold.
///
/// **A newtype whose `parse` refuses an empty set, so `AllBut(nothing)` is unrepresentable rather
/// than checked.** The variant it fills is read by a composition root as *refuse to start and name
/// these*, and a set with no names in it would produce a refusal naming nothing - the shape of a
/// boot failure an operator cannot act on. There is one way to get one and it takes the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsentTables(BTreeSet<QualifiedTable>);

/// Why a set of absent tables is not one.
///
/// One variant, and it is an enum rather than a unit struct for the reason every other error in this
/// domain is one: a second reason has somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotAbsent {
    /// Nothing was named. Say [`TablesPresent::All`] instead.
    #[error("an absent-tables set names nothing, and a refusal that names no table cannot be acted on")]
    Nothing,
}

impl AbsentTables {
    /// Parses a set of tables a data system does not hold.
    ///
    /// **The canonical constructor**, and [`TablesPresent::of`] is the only caller that matters: it
    /// routes an empty set to [`TablesPresent::All`], so no call site has to decide what an empty
    /// set means.
    pub fn parse(tables: BTreeSet<QualifiedTable>) -> Result<Self, NotAbsent> {
        if tables.is_empty() {
            return Err(NotAbsent::Nothing);
        }
        Ok(Self(tables))
    }

    /// The tables, for a refusal that names them.
    #[inline]
    #[must_use]
    pub const fn named(&self) -> &BTreeSet<QualifiedTable> {
        &self.0
    }
}

/// What a data system said about the tables it was asked for.
///
/// **[`Self::NotAsked`] is not [`Self::All`], and no caller can read it as one.** That is the shape
/// [`PreFlight`](crate::warehouse::PreFlight) already uses and it is here for the same reason: the port's
/// default has to be *nothing to report*, because an adapter that cannot ask a data system cheaply
/// must not be forced to lie - and a default of "every table is there" is exactly that lie, told at
/// boot, in the one place a deployment is deciding whether to serve at all. An adapter that really
/// looked and found everything answers [`Self::All`]; one that did not look answers the default, and
/// a composition root can tell which it got.
///
/// **The third outcome is not a variant here, deliberately.** A data system that could not be
/// asked at all - a credential with no permission to list, a dataset that is not there, an endpoint
/// that did not answer - is an `Err` from the port, not a variant of this enum. Two reasons, and the
/// first is the one that decides it: *could not verify* and *this table is absent* must not collapse
/// into one message, because an operator told the wrong one fixes the wrong thing, and the adapter's
/// own error type is where the reason lives in the detail an operator needs. The second is that a
/// variant would need a reason field, and a reason field in a domain enum is either a bounded string
/// nobody owns or an erased cause the domain has no vocabulary for.
///
/// **The limit, stated with the claim:** what this reports is that a table EXISTS. It says nothing
/// about the columns a model names on it, and nothing about whether the identity that asked can
/// read it: a listing grant and a read grant are two grants. An anchor is what covers both, for the
/// metrics that have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TablesPresent {
    /// The adapter did not ask. The port's default, and the honest answer for an adapter that has
    /// already answered this question another way - a file engine is given its tables at boot, so a
    /// second check would be a check on the set it just built.
    NotAsked,
    /// The data system was asked and holds every table it was asked about.
    All,
    /// The data system was asked and does not hold these.
    AllBut(AbsentTables),
}

impl TablesPresent {
    /// The answer for an adapter that asked, from the tables it found absent.
    ///
    /// **One constructor, and it is what keeps the empty case from being a decision at a call
    /// site.** An adapter computes the difference between what it was asked about and what it holds
    /// and hands the result over; an empty difference is [`Self::All`], which is the only honest
    /// reading of *I asked and nothing was missing*.
    #[must_use]
    pub fn of(absent: BTreeSet<QualifiedTable>) -> Self {
        AbsentTables::parse(absent).map_or(Self::All, Self::AllBut)
    }

    /// Whether the adapter looked at all.
    ///
    /// Read by a composition root's log line, which says a different thing for a deployment nobody
    /// verified than for one that was verified clean.
    #[inline]
    #[must_use]
    pub const fn was_asked(&self) -> bool {
        !matches!(*self, Self::NotAsked)
    }

    /// The tables that are not there, if any were named.
    ///
    /// `None` for both [`Self::NotAsked`] and [`Self::All`], which is correct for a caller asking
    /// *what do I refuse over* and is exactly why [`Self::was_asked`] is a separate question.
    #[inline]
    #[must_use]
    pub const fn absent(&self) -> Option<&AbsentTables> {
        match *self {
            Self::NotAsked | Self::All => None,
            Self::AllBut(ref tables) => Some(tables),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{AbsentTables, NotAbsent, TablesPresent};
    use crate::identity::Presented;
    use crate::model::{QualifiedTable, SourceName};
    use crate::plan::{AnchorPlan, Executable};
    use crate::source::{ImpersonationCapability, SourcePosture};
    use crate::warehouse::{AnchorRows, RowSet, Warehouse};

    fn table(raw: &str) -> QualifiedTable {
        QualifiedTable::parse(raw).expect("a test table path parses")
    }

    #[test]
    fn a_refusal_that_names_no_table_cannot_be_constructed() {
        // The whole reason `AbsentTables` is a newtype: `AllBut(empty)` would reach a composition
        // root as *these tables are missing* and name none of them.
        assert_eq!(AbsentTables::parse(BTreeSet::new()), Err(NotAbsent::Nothing));
        assert_eq!(TablesPresent::of(BTreeSet::new()), TablesPresent::All);
    }

    #[test]
    fn asking_and_finding_nothing_missing_is_not_the_same_answer_as_not_asking() {
        // The `PreFlight::NotAsked` property, one port method over: a default that read as *verified*
        // would make an adapter that cannot look indistinguishable from one that looked and was
        // satisfied - at boot, which is where the difference decides whether to serve.
        assert_ne!(TablesPresent::All, TablesPresent::NotAsked);
        assert!(TablesPresent::All.was_asked());
        assert!(!TablesPresent::NotAsked.was_asked());
        assert!(TablesPresent::All.absent().is_none());
        assert!(TablesPresent::NotAsked.absent().is_none());
    }

    #[test]
    fn an_answer_naming_absent_tables_carries_them_for_a_refusal_to_print() {
        let asked: BTreeSet<QualifiedTable> = [table("dim_customer"), table("analytics.fct_orders")].into_iter().collect();
        let answered = TablesPresent::of(asked.clone());
        assert_eq!(
            answered.absent().map(AbsentTables::named),
            Some(&asked),
            "the refusal has to be able to name every table the data system does not hold"
        );
    }

    /// An adapter that answers the port's required methods and nothing else.
    struct Silent {
        source: SourceName,
        posture: SourcePosture,
    }

    impl Warehouse for Silent {
        type Error = core::fmt::Error;

        const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

        fn source(&self) -> &SourceName {
            &self.source
        }

        fn posture(&self) -> &SourcePosture {
            &self.posture
        }

        fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
            Err(core::fmt::Error)
        }

        fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
            Err(core::fmt::Error)
        }
    }

    #[test]
    fn an_adapter_that_takes_the_default_reports_nothing_rather_than_claiming_everything() {
        // The property that makes the method defaulted safe to add: the engine and every adapter
        // written before this port existed compile unchanged and say *I did not look*, which a
        // composition root reads as *nothing to refuse over* and not as *verified*.
        let silent = Silent {
            source: SourceName::parse("warehouse").expect("a source name parses"),
            posture: SourcePosture::ImpersonationAtSource,
        };
        let asked: BTreeSet<QualifiedTable> = core::iter::once(table("dim_customer")).collect();
        assert_eq!(
            silent.preflight(&asked).expect("the default answers rather than failing"),
            TablesPresent::NotAsked
        );
    }
}

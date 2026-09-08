//! What a data system answered when it was asked whether the bundle's tables are there.
//!
//! **A module of its own rather than part of `warehouse.rs`**, and the seam is a real one:
//! nothing here is about a plan, a credential or a row. It is the vocabulary for one question a
//! composition root asks once, at boot, before a listener is bound - *does this data system hold the
//! tables the bundle names*. Not asking, an absence, and an inventory that could not establish
//! an answer must remain distinct from verified presence.
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

use core::fmt;
use core::num::NonZeroU64;
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

/// A non-empty set of tables a data system was asked about and did not answer for either way.
///
/// **The set an answer that fell short of its own inventory leaves behind**, and it is a third set
/// rather than a second reading of [`AbsentTables`] because the two license different sentences: a
/// table in that one is one the data system says it does not have, and a table in this one is one
/// the data system's own answer did not reach. Collapsing them is the defect this type exists to
/// remove - a listing that named no table beside a total claiming several read as *every table in
/// the bundle is absent*, which sends an operator to fix a catalog that was never wrong.
///
/// Non-empty by the same construction and for the same reason: a boot outcome that names no table
/// is one nobody can act on. An empty difference is [`TablesPresent::All`], because a table the
/// answer DID name is one the answer accounted for whatever its total said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaccountedTables(BTreeSet<QualifiedTable>);

/// Why a set of unaccounted-for tables is not one.
///
/// One variant, an enum for [`NotAbsent`]'s reason: a second reason has somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotUnaccounted {
    /// Nothing was named. Say [`TablesPresent::All`] instead.
    #[error("an unaccounted-for set names nothing, and an outcome that names no table cannot be acted on")]
    Nothing,
}

impl UnaccountedTables {
    /// Parses a set of tables a data system's own answer did not reach.
    ///
    /// **The canonical constructor.** An adapter computes the difference between what it asked about
    /// and what an incomplete or unreadable inventory actually named, and hands the result over.
    ///
    /// # Errors
    ///
    /// [`NotUnaccounted::Nothing`] for an empty set.
    pub fn parse(tables: BTreeSet<QualifiedTable>) -> Result<Self, NotUnaccounted> {
        if tables.is_empty() {
            return Err(NotUnaccounted::Nothing);
        }
        Ok(Self(tables))
    }

    /// The tables, for an outcome that names them.
    #[inline]
    #[must_use]
    pub const fn named(&self) -> &BTreeSet<QualifiedTable> {
        &self.0
    }

    /// How many tables the answer did not reach.
    ///
    /// Read beside the shortfall by both roots, because the two are different numbers and a sentence
    /// carrying one of them reads as a claim about the other.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`, and it exists because `clippy::len_without_is_empty` asks for it.
    ///
    /// The type is non-empty by construction, so this is a constant with a name rather than a
    /// question worth asking - which is itself the honest reading of the invariant.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many of these tables a gap of this size can actually explain.
    ///
    /// **Never more than there are, and that clamp is the whole method.** A shortfall is a count of
    /// tables the data system did not account for ANYWHERE in the dataset, and this set is the part
    /// of it the bundle happens to name - so the two are independent numbers and the first can be
    /// the larger. Review reproduced the sentence that comes of pairing them raw: *at most 9 of the
    /// 2 table(s)*, on the very shape this check exists for, because an identified count of zero
    /// makes the shortfall the dataset's whole table count.
    ///
    /// **It lives here rather than in each composition root** for the reason `models_by_table` does:
    /// two roots each remembering a `min` is the rule held by recall that this repository does not
    /// accept. A root reads this and renders it.
    #[inline]
    #[must_use]
    pub fn explained_by(&self, shortfall: NonZeroU64) -> usize {
        usize::try_from(shortfall.get()).unwrap_or(usize::MAX).min(self.0.len())
    }
}

impl fmt::Display for UnaccountedTables {
    /// The table paths, comma separated - a LIST and never a sentence.
    ///
    /// The sentence belongs to a composition root, which is the seam `sutura_app::preflight` states
    /// at length. What is here is the half both roots would otherwise write twice, which is the same
    /// argument that moved `models_by_table` inward.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, table) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{table}")?;
        }
        Ok(())
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
/// **A data system that could not be ASKED is still not a variant here, deliberately.** A credential
/// with no permission to list, a dataset that is not there, an endpoint that did not answer - each
/// is an `Err` from the port, not a variant of this enum. Two reasons, and the first is the one that
/// decides it: *could not verify* and *this table is absent* must not collapse into one message,
/// because an operator told the wrong one fixes the wrong thing, and the adapter's own error type is
/// where the reason lives in the detail an operator needs. The second is that a variant would need a
/// reason field, and a reason field in a domain enum is either a bounded string nobody owns or an
/// erased cause the domain has no vocabulary for.
///
/// **[`Self::Unaccounted`] is not that outcome and is a variant for exactly the reasons that keep it
/// out.** The data system WAS asked and it DID answer; what it did not do is account for its own
/// inventory, which is a property of the answer rather than a failure to get one. So there is no
/// foreign cause to carry: the payload is a set of table paths and one count this adapter computed.
/// [`Self::UnreadableInventory`] carries the same set without inventing a count when none was readable.
/// And an `Err` would have been the wrong channel twice over - `Warehouse::preflight_was_refused`
/// puts everything that is not an authorization failure in the WARNING half, so the shape a
/// cross-check exists to catch would have reached a root as *serving anyway*. `docs/adr/0018` and
/// `telekom/sutura#275` carry that argument; a refusal is a VALUE here for the same reason
/// `ToolOutcome::Refusal` is one on the query path.
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
    /// The inventory reported a total the adapter could not read and no readable table IDs.
    ///
    /// These tables are neither established present nor absent. Unlike [`Self::Unaccounted`],
    /// there is no shortfall to count or bound. This is an answered inventory, not an error fetching
    /// one, and a boot must refuse without blaming the catalog's table declarations.
    UnreadableInventory(UnaccountedTables),
    /// The data system was asked, answered, and its answer did not account for every table it said
    /// it holds - so whether it holds these is not established either way.
    ///
    /// **The point of the variant is that this is NOT [`Self::AllBut`]**, and the difference is what
    /// an operator is sent to do. A data system whose inventory reports a total and then names fewer
    /// tables than the total claims has left a gap, and any table the bundle names that is missing
    /// from what it DID name may be sitting in that gap. Reporting such a table absent is rounding a
    /// shortfall down to zero and blaming the catalog for it.
    ///
    /// **Presence is unaffected, which is why this variant is not simply *could not check*.** A
    /// table the answer named is a table the data system really has; a short inventory cannot
    /// un-name an entry it carried. So an answer that fell short and still named everything the
    /// bundle asks about is [`Self::All`], and only the tables it did not reach land here.
    ///
    /// **What it does NOT separate, stated where the claim is:** an inventory can fall short because
    /// its shape changed under the adapter, or because a table was created or dropped while it was
    /// being read. Both leave the same gap over the same tables, and this answer says only that the
    /// gap is there. A root that wanted to tell them apart would need evidence no data system offers
    /// at boot.
    Unaccounted {
        /// The tables this answer reached no conclusion about.
        ///
        /// **It is not a count of what is missing, and the shortfall beside it is the bound.** At
        /// most [`UnaccountedTables::explained_by`] of these can be sitting in the gap, so a set
        /// LARGER than the shortfall says the rest really are absent - without saying which, because
        /// the data system named none of them. A root that rendered the two numbers as one thing
        /// would print *three tables unaccounted for* beside a gap of one, which is a review finding
        /// on the first version of this variant; both roots say *at most N of these M* for that
        /// reason.
        ///
        /// **And the bound runs BOTH ways, which the second review round found:** a shortfall counts
        /// tables the data system did not account for anywhere in the dataset, and this set is only
        /// the part of it the bundle names, so the shortfall can be the LARGER number - *at most 9
        /// of the 2*. `explained_by` is the clamp, and it is a method rather than a `min` each root
        /// remembers.
        tables: UnaccountedTables,
        /// How many tables the data system said it holds that its own answer did not account for.
        ///
        /// **[`NonZeroU64`] rather than a pair of totals**, and both halves of that are deliberate.
        /// A shortfall of zero is not a shortfall, so the type refuses one rather than a caller
        /// remembering to; and one number cannot be handed over the wrong way round, which two
        /// adjacent counts of the same type can - the mistake `crate::model::QualifiedTable`'s
        /// neighbours in `sutura_exec_bigquery::transport::DatasetAddress` were reshaped over.
        shortfall: NonZeroU64,
    },
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
    ///
    /// **An exhaustive match and not `!matches!(NotAsked)`, which is a review finding rather than
    /// style.** The negated form is a DEFAULTED arm: [`Self::absent`] and `sutura_app::preflight::ask`
    /// both refuse an additional variant at compile time, and this one would have compiled silently and
    /// answered *it looked* about a variant nobody had classified. The compiler holds here what the
    /// two matches beside it already held.
    #[inline]
    #[must_use]
    pub const fn was_asked(&self) -> bool {
        match *self {
            Self::NotAsked => false,
            Self::All | Self::AllBut(_) | Self::Unaccounted { .. } | Self::UnreadableInventory(_) => true,
        }
    }

    /// The tables that are not there, if any were named.
    ///
    /// `None` for both [`Self::NotAsked`] and [`Self::All`], which is correct for a caller asking
    /// *what do I refuse over* and is exactly why [`Self::was_asked`] is a separate question.
    ///
    /// **`None` for [`Self::Unaccounted`] too, and that arm is the whole point of this method having
    /// one.** A table an answer did not reach is not a table the data system said it does not hold,
    /// and a caller that read the two as one number is the caller this variant exists to stop.
    #[inline]
    #[must_use]
    pub const fn absent(&self) -> Option<&AbsentTables> {
        match *self {
            Self::NotAsked | Self::All | Self::Unaccounted { .. } | Self::UnreadableInventory(_) => None,
            Self::AllBut(ref tables) => Some(tables),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use core::num::NonZeroU64;

    use super::{AbsentTables, NotAbsent, NotUnaccounted, TablesPresent, UnaccountedTables};
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

    #[test]
    fn an_outcome_that_names_no_unaccounted_table_cannot_be_constructed() {
        // `AbsentTables`' property, for the set that licenses the other sentence: an outcome reaching
        // a composition root as *these were not accounted for* and naming none of them is a boot
        // failure nobody can act on.
        assert_eq!(UnaccountedTables::parse(BTreeSet::new()), Err(NotUnaccounted::Nothing));
    }

    #[test]
    fn a_table_an_answer_did_not_reach_is_not_a_table_the_data_system_said_it_does_not_hold() {
        // **The distinction the variant exists for.** Both outcomes name tables and both stop a
        // boot, and a caller that read `absent()` to find out what to say would have reported a gap
        // in the data system's own inventory as a mistyped `table:` - which is `telekom/sutura#275`
        // exactly: the right direction, the wrong reason, and an operator sent to fix a catalog that
        // was never wrong.
        let asked: BTreeSet<QualifiedTable> = core::iter::once(table("dim_customer")).collect();
        let unaccounted = TablesPresent::Unaccounted {
            tables: UnaccountedTables::parse(asked.clone()).expect("a non-empty set parses"),
            shortfall: NonZeroU64::new(3).expect("three is not zero"),
        };
        assert!(
            unaccounted.absent().is_none(),
            "an answer that did not reach a table has not said the table is missing: {unaccounted:?}"
        );
        assert_ne!(
            unaccounted,
            TablesPresent::of(asked),
            "and it is not the absent answer either"
        );
        assert!(unaccounted.was_asked(), "the data system was asked and it did answer");
    }

    #[test]
    fn an_unreadable_inventory_is_an_answer_without_an_absence_or_count() {
        let named = BTreeSet::from([table("dim_customer")]);
        let answered = TablesPresent::UnreadableInventory(UnaccountedTables::parse(named.clone()).expect("nonempty"));
        assert!(answered.was_asked());
        assert!(answered.absent().is_none());
        let TablesPresent::UnreadableInventory(tables) = answered else {
            panic!("the count-free answer must retain its shape");
        };
        assert_eq!(tables.named(), &named);
    }

    #[test]
    fn a_gap_can_never_explain_more_tables_than_the_answer_named() {
        // **The bound runs both ways, and the second direction is a review finding.** A shortfall
        // counts tables the data system did not account for anywhere in the DATASET; this set is
        // only the part of it the bundle names. So the gap can be the larger number - and it is
        // exactly the larger number on the shape this whole check exists for, because an identified
        // count of zero makes the shortfall the dataset's entire table count. Paired raw, both roots
        // printed *at most 9 of the 2 table(s)*.
        let two: BTreeSet<QualifiedTable> = [table("dim_customer"), table("fct_orders")].into_iter().collect();
        let named = UnaccountedTables::parse(two).expect("a non-empty set parses");
        assert_eq!(
            named.explained_by(NonZeroU64::new(9).expect("nine is not zero")),
            2,
            "a gap of nine over two tables explains two of them, not nine"
        );
        assert_eq!(
            named.explained_by(NonZeroU64::new(1).expect("one is not zero")),
            1,
            "and a gap smaller than the set is still the bound - the clamp is one-sided"
        );
    }

    #[test]
    fn an_unaccounted_outcome_renders_the_tables_it_could_not_reach() {
        // The half both composition roots would otherwise write twice: a LIST, in the registry's
        // order, with the sentence left to the root that has a reader.
        let tables: BTreeSet<QualifiedTable> = [table("fct_orders"), table("dim_customer")].into_iter().collect();
        let named = UnaccountedTables::parse(tables).expect("a non-empty set parses");
        assert_eq!(named.to_string(), "dim_customer, fct_orders");
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

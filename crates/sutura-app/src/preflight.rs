//! Asking every open data system whether it holds the tables the bundle names.
//!
//! **The decision sequence, once, for every composition root that has one** - and it is here rather
//! than copied into each because review measured the copy: each helper underneath was
//! byte-identical between `sutura-serve` and `sutura-cli`, and none of them contains a word an
//! operator reads. [`models_by_table`] is a pure query over [`PinnedDefinitions`], which is a
//! `sutura-domain` type, and [`AbsentBehind`]'s rendering is a list of names rather than a sentence.
//!
//! **What is NOT here is the sentence and the sink**, and that is the seam rather than an omission.
//! A root has to say what an operator should do about each outcome, in the words that fit its own
//! transport, through the sink that transport actually delivers on - `tracing` behind a subscriber
//! for a server, standard error for a process launched on a pipe with no subscriber installed. So
//! [`ask`] returns one [`Verdict`] per source and prints nothing, which also makes each root's
//! rendering a pure function its own suite can assert on. Before this the soft outcome was printed
//! from inside the decision and no test could see it - so the mechanism the argument for that sink
//! rests on was held by review, which `AGENTS.md` does not accept as held.
//!
//! `sutura_domain::warehouse::Warehouse::preflight` is the port, and its own documentation carries
//! why an answered inventory and *could not verify* are different outcomes.
//!
//! **The limit, stated with the claim:** what a pre-flight establishes is that a table EXISTS. Not
//! that the columns a model names are on it, and not that a question's identity may read it - a
//! listing grant and a read grant are two grants. An anchor covers both, for the metrics that have
//! one.

use core::fmt;
use core::num::NonZeroU64;
use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::model::{ModelName, QualifiedTable, SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::preflight::{TablesPresent, UnaccountedTables};

use crate::warehouses::Warehouses;

/// What one data system answered about the tables one bundle names in it.
///
/// A root treats two failures differently, and an incomplete or unreadable inventory is not a
/// finding about the catalog. The port answers five things and fails in one way,
/// and that one failure splits on `Warehouse::preflight_was_refused`: a data system that REFUSED to
/// be listed will refuse identically on every launch and the fix is one grant, while one that could
/// not be reached is a condition that passes. A root that collapsed them would either stop a
/// deployment that would have worked or hide the check being off in the deployment least likely to
/// read a startup log.
///
/// **[`Self::Unaccounted`] is a REFUSAL, not a failure to get an answer.** The
/// data system answered; its answer did not account for its own inventory. Reading that as
/// [`Self::Absent`] is what `telekom/sutura#275` is - a shortfall rounded down to zero and charged
/// to the catalog - and reading it as [`Self::Unverified`] would be worse still: that is the warning
/// half, so the one shape the cross-check exists to catch would end in a deployment that serves.
///
/// Generic in the adapter's error so the cause travels: nothing here can read `W::Error`, and the
/// root that composed the adapter is the one that can flatten it.
#[derive(Debug)]
pub enum Verdict<E> {
    /// Asked, and every table is there. Carries how many, for a line that says so.
    Present {
        /// How many tables the bundle names in this data system.
        asked: usize,
    },
    /// The adapter did not report - `TablesPresent::NotAsked`, the port's default.
    ///
    /// **Not readable as verified**, which is the property the port's own answer type exists to
    /// keep: a root that printed nothing here would make the one outcome meaning *nothing checked
    /// this* indistinguishable from a data system that really looked.
    NotReported {
        /// How many tables the bundle names in this data system.
        asked: usize,
    },
    /// Asked, and these tables are not there. A refusal, and the models to name in it.
    Absent(AbsentBehind),
    /// An unreadable inventory established neither presence nor absence for these tables.
    /// A refusal without a count or model names: no catalog declaration was shown wrong.
    UnreadableInventory(UnaccountedTables),
    /// Asked, answered, and the answer did not account for every table the data system said it
    /// holds - so these tables are neither established present nor established absent.
    ///
    /// **It names tables and not the models behind them, which is the one place this verdict
    /// deliberately says less than [`Self::Absent`].** A model is what an operator opens to fix a
    /// `table:` that is wrong, and nothing here says a `table:` is wrong: the catalog may be
    /// entirely right and the data system's own answer incomplete. Naming models would send an
    /// operator to exactly the file `telekom/sutura#275` is about them being sent to wrongly.
    Unaccounted {
        /// The tables the data system's answer did not reach.
        tables: UnaccountedTables,
        /// How many tables it said it holds that its own answer did not account for.
        shortfall: NonZeroU64,
    },
    /// The data system refused to be asked: this identity may not list it.
    Refused {
        /// The adapter's own error, for a root to flatten into its message.
        cause: E,
    },
    /// The data system could not be asked, for a reason that is not a refusal.
    Unverified {
        /// How many tables went unverified.
        asked: usize,
        /// The adapter's own error, for a root to flatten into its message.
        cause: E,
    },
}

/// The tables a data system does not hold, each with the models that named it.
///
/// **Keyed by the TABLE and carrying the models, because that is the direction a refusal reads in:**
/// the data system answered about a table, and the operator has to open a model to fix it. Two
/// models over one table is ordinary - a bundle may declare several over one fact table - so the
/// value is a set, and [`Display`](fmt::Display) renders every one of them.
///
/// Non-empty by construction: it is built only from a `TablesPresent::AllBut`, whose own newtype
/// refuses an empty set, so a refusal that names nothing is unrepresentable rather than checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsentBehind(BTreeMap<QualifiedTable, BTreeSet<ModelName>>);

impl AbsentBehind {
    /// The absent tables and the models behind each, for a root that renders its own shape.
    #[inline]
    #[must_use]
    pub const fn named(&self) -> &BTreeMap<QualifiedTable, BTreeSet<ModelName>> {
        &self.0
    }

    /// How many tables are absent.
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
}

impl fmt::Display for AbsentBehind {
    /// One clause per absent table, naming the models that named it.
    ///
    /// **Both halves on purpose:** the table path is what the data system disagreed with, and the
    /// model is the file an operator has to open. A message carrying only one of them sends them
    /// looking for the other.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, (table, models)) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str("; ")?;
            }
            write!(f, "table {table}, named by model(s) [")?;
            for (position, model) in models.iter().enumerate() {
                if position > 0 {
                    f.write_str(", ")?;
                }
                f.write_str(model.as_str())?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

/// The boot policy over one [`Verdict`]: this deployment refuses, or it carries on and says so.
///
/// **The partition is the thing that was held by recall in two composition roots.** Each root
/// matched all seven verdicts and decided per arm which ones return an error, and the two agreed
/// only because someone kept them agreeing - so a source whose outcome one root refused and the
/// other served was a two-file edit away. Stating it once makes the two roots' boot behaviour the
/// same fact rather than the same intention.
///
/// **`Err` for a refusal here, and that is not the query path's rule inverted.** A governance
/// refusal lives inside the `Ok` where a CALLER could mistake it for a hiccup and retry; this is
/// boot, the outcome is that the process does not start, and both roots already answered `Err` for
/// exactly these four outcomes, carrying a rendered sentence. What changed is that the four are now
/// a type rather than prose a caller would have had to parse.
///
/// **The limit, stated with the claim.** This is a type saying which outcomes refuse. It does not
/// confine a root to asking: [`Verdict`] is still public, because *what the data system answered*
/// and *what this deployment does about it* are two questions, and the port's own answer is what an
/// adapter's suite asserts on. A future root that matches [`Verdict`] directly and re-decides the
/// split is what review has to catch; no type here stops it.
///
/// Which outcome belongs on which side is [`Verdict`]'s own documentation, and changing it is
/// `telekom/sutura#141`'s decision rather than a call site's.
#[derive(Debug)]
pub enum Refusal<E> {
    /// The data system refused to be listed: this identity may not ask.
    Refused {
        /// The adapter's own error, for a root to flatten into its message.
        cause: E,
    },
    /// Asked, answered, and these tables are not there - with the models that named them.
    Absent(AbsentBehind),
    /// An unreadable inventory established neither presence nor absence for these tables.
    UnreadableInventory(UnaccountedTables),
    /// The answer did not account for every table the data system said it holds.
    Unaccounted {
        /// The tables the data system's answer did not reach.
        tables: UnaccountedTables,
        /// How many tables it said it holds that its own answer did not account for.
        shortfall: NonZeroU64,
    },
}

/// The boot policy's other side: this deployment serves, and a root says what was established.
///
/// **Every one of these is a line a root emits, including the two clean ones.** `NotReported` is
/// the outcome meaning *nothing verified this*, so a root that printed nothing for it would make it
/// indistinguishable from a data system that really looked - which is why silence is not one of the
/// shapes here.
#[derive(Debug)]
pub enum Notice<E> {
    /// Asked, and every table is there. Carries how many, for a line that says so.
    Present {
        /// How many tables the bundle names in this data system.
        asked: usize,
    },
    /// The adapter did not report - `TablesPresent::NotAsked`, the port's default.
    NotReported {
        /// How many tables the bundle names in this data system.
        asked: usize,
    },
    /// The data system could not be asked, for a reason that is not a refusal.
    Unverified {
        /// How many tables went unverified.
        asked: usize,
        /// The adapter's own error, for a root to flatten into its message.
        cause: E,
    },
}

/// The two sides of the boot policy: a notice this deployment serves with, or a refusal it stops on.
///
/// A named alias because `clippy::type_complexity` refuses the bare `Result` at this arity, and the
/// name is the better half of that trade rather than a suppression: the split IS the decision, so a
/// signature that says *boot policy* reads as the thing being returned and not as two halves a
/// caller has to recombine. It stays a `Result` so `?` in a composition root keeps working.
pub type BootPolicy<E> = Result<Notice<E>, Refusal<E>>;

impl<E> Verdict<E> {
    /// Splits this verdict the one way both composition roots split it.
    ///
    /// Exhaustive over [`Verdict`], so a variant added to the port's answer is a compile error
    /// here - at the one place that has to decide which side of the boot policy it falls on -
    /// rather than a silently-served outcome in whichever root forgot it.
    ///
    /// # Errors
    ///
    /// The four outcomes this deployment does not start on: a refused listing, a table the data
    /// system does not hold, an unreadable inventory, and an inventory that did not account for
    /// itself.
    pub fn boot_policy(self) -> BootPolicy<E> {
        match self {
            Self::Present { asked } => Ok(Notice::Present { asked }),
            Self::NotReported { asked } => Ok(Notice::NotReported { asked }),
            Self::Unverified { asked, cause } => Ok(Notice::Unverified { asked, cause }),
            Self::Refused { cause } => Err(Refusal::Refused { cause }),
            Self::Absent(absent) => Err(Refusal::Absent(absent)),
            Self::UnreadableInventory(tables) => Err(Refusal::UnreadableInventory(tables)),
            Self::Unaccounted { tables, shortfall } => Err(Refusal::Unaccounted { tables, shortfall }),
        }
    }
}

/// One data system's name and what it answered.
///
/// The name is BORROWED from the registry rather than cloned: the registry outlives the answer at
/// every call site, and a clone here would be one taken to satisfy a signature rather than to own
/// anything.
#[derive(Debug)]
pub struct Asked<'source, E> {
    source: &'source SourceName,
    verdict: Verdict<E>,
}

impl<'source, E> Asked<'source, E> {
    /// Which data system answered.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &'source SourceName {
        self.source
    }

    /// What it answered.
    #[inline]
    #[must_use]
    pub const fn verdict(&self) -> &Verdict<E> {
        &self.verdict
    }

    /// The answer, owned, for a root that has to move the cause out of it.
    #[must_use]
    pub fn into_verdict(self) -> Verdict<E> {
        self.verdict
    }
}

/// Asks each open data system once whether it holds the tables the bundle names in it.
///
/// **One call per data system, and per DATASET underneath, never per model** - which is what makes
/// this affordable at startup: the tables go over as a set, so a bundle of forty models on one
/// dataset costs one metadata read. An adapter reading more than one dataset makes one call per
/// dataset, which is still a set rather than a model.
///
/// A data system the bundle names no model in is skipped rather than asked about nothing: it would
/// otherwise cost a round trip to be told about an empty set, and the port's own empty-set arm
/// answers `NotAsked`, which a root would then have to explain.
///
/// **It prints nothing and refuses nothing.** Both are the caller's, for the reason this module's
/// own documentation gives: the words and the sink belong to the transport, and a decision that
/// printed would not be assertable.
///
/// The order is the registry's, which is the source name's - so two roots asking the same question
/// report it in the same order, and a test can name the answer it expects rather than search for it.
pub fn ask<'engines, W>(pinned: &PinnedDefinitions, engines: &'engines Warehouses<W>) -> Vec<Asked<'engines, W::Error>>
where
    W: Warehouse,
{
    let mut answers: Vec<Asked<'engines, W::Error>> = Vec::new();
    for (source, engine) in engines.each() {
        let behind = models_by_table(pinned, source);
        if behind.is_empty() {
            continue;
        }
        let asked: BTreeSet<QualifiedTable> = behind.keys().cloned().collect();
        let verdict = match engine.preflight(&asked) {
            Ok(TablesPresent::All) => Verdict::Present { asked: asked.len() },
            Ok(TablesPresent::NotAsked) => Verdict::NotReported { asked: asked.len() },
            Ok(TablesPresent::AllBut(missing)) => Verdict::Absent(AbsentBehind(
                missing
                    .named()
                    .iter()
                    .map(|table| (table.clone(), behind.get(table).cloned().unwrap_or_default()))
                    .collect(),
            )),
            // Carried straight through rather than joined against the bundle, because the models are
            // the wrong half of this answer - the variant's own documentation says why.
            Ok(TablesPresent::Unaccounted { tables, shortfall }) => Verdict::Unaccounted { tables, shortfall },
            Ok(TablesPresent::UnreadableInventory(tables)) => Verdict::UnreadableInventory(tables),
            // The split the port documents: an authorization failure will fail identically on every
            // launch and one grant fixes it, everything else is a condition that passes. Which one it
            // was is the ADAPTER's to say, because `W::Error` is its own type and nothing here reads it.
            Err(cause) if engine.preflight_was_refused(&cause) => Verdict::Refused { cause },
            Err(cause) => Verdict::Unverified {
                asked: asked.len(),
                cause,
            },
        };
        answers.push(Asked { source, verdict });
    }
    answers
}

/// Every table the bundle's models sit behind, whichever data system holds it.
///
/// A pure query over a `sutura-domain` type, here rather than in each composition root for
/// [`models_by_table`]'s reason: it names nothing an operator reads. Both roots compared the result
/// against what their engine attached, from a body that was byte-identical in the two of them.
#[must_use]
pub fn served_tables(served: &PinnedDefinitions) -> BTreeSet<TableName> {
    served
        .definitions()
        .models()
        .values()
        .map(|model| model.table_name().clone())
        .collect()
}

/// The bundle being served names tables that are not the ones attached behind it.
///
/// **A type rather than the `String` both roots built**, and the two sets rather than a rendered
/// sentence: a caller that wants to act on which tables moved can read them, and the sentence is
/// [`Display`](fmt::Display) for the roots that only want to print it. Both roots printed the SAME
/// sentence - measured byte-identical - so it is not wording that belongs to a transport, and it
/// moved with the comparison instead of being copied a third time.
///
/// Non-empty by construction: [`refuse_unattached`] is the only constructor and returns `Ok` when
/// both sets are empty, so a mismatch that names nothing is unrepresentable rather than checked.
///
/// **The limit, stated with the claim.** What this compares is TABLE NAMES between two loads of one
/// catalog directory. It does not establish that a table which is attached holds the columns a
/// model names, and it says nothing about a data system a root never attached anything for - the
/// pre-flight above is that half, for the sources that can answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablesChanged {
    missing: BTreeSet<TableName>,
    extra: BTreeSet<TableName>,
}

impl TablesChanged {
    /// Tables the bundle being served names with no table attached behind them.
    #[inline]
    #[must_use]
    pub const fn missing(&self) -> &BTreeSet<TableName> {
        &self.missing
    }

    /// Tables attached for a model the bundle being served no longer names.
    #[inline]
    #[must_use]
    pub const fn extra(&self) -> &BTreeSet<TableName> {
        &self.extra
    }
}

impl fmt::Display for TablesChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the catalog changed while this process was starting: the engine was opened for the bundle \
             loaded first, and the bundle being served names different tables. Served with no table \
             attached: [{missing}]. Attached and no longer served: [{extra}]. Refusing to serve a model \
             whose questions would fail at query time",
            missing = names(self.missing.iter()),
            extra = names(self.extra.iter())
        )
    }
}

impl core::error::Error for TablesChanged {}

/// The tables the bundle being served names, against the tables the engine actually holds.
///
/// **One home for a comparison both composition roots made from byte-identical bodies.** The two
/// sets come from two `load()` calls on the same catalog directory; a model added between them is
/// refused here rather than served with no table behind it, which would fail the first question
/// against it at query time.
///
/// Two sets rather than a bundle and an engine, so the comparison is unit-testable without a digest,
/// a knowledge declaration or a data system - [`served_tables`] is the other half and is one map
/// over a public accessor.
///
/// Both directions are refused, and the second is not pedantry: a table attached for a model the
/// served bundle no longer names means the catalog directory changed between two loads seconds
/// apart, and whatever else moved with it is the part nobody has looked at.
///
/// # Errors
///
/// Either set holding a table the other does not, as a [`TablesChanged`] carrying both differences.
pub fn refuse_unattached(serving: &BTreeSet<TableName>, attached: &BTreeSet<TableName>) -> Result<(), TablesChanged> {
    let missing: BTreeSet<TableName> = serving.difference(attached).cloned().collect();
    let extra: BTreeSet<TableName> = attached.difference(serving).cloned().collect();
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(TablesChanged { missing, extra })
}

/// One line of table names, for a message an operator has to act on.
fn names<'table>(tables: impl Iterator<Item = &'table TableName>) -> String {
    tables.map(TableName::as_str).collect::<Vec<&str>>().join(", ")
}

/// Which models sit behind each table one source's part of the bundle names.
///
/// Private, because every caller wants [`ask`]'s answer rather than this map - and it lives here
/// rather than in two composition roots because it names nothing an operator reads and nothing an
/// adapter declares: it is a query over a bundle.
fn models_by_table(pinned: &PinnedDefinitions, source: &SourceName) -> BTreeMap<QualifiedTable, BTreeSet<ModelName>> {
    let mut behind: BTreeMap<QualifiedTable, BTreeSet<ModelName>> = BTreeMap::new();
    for model in pinned.definitions().models().values() {
        if model.source() == source {
            behind.entry(model.table().clone()).or_default().insert(model.name().clone());
        }
    }
    behind
}

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use core::num::NonZeroU64;
    use std::collections::BTreeSet;

    use sutura_domain::identity::Presented;
    use sutura_domain::model::{QualifiedTable, SourceName, TableName};
    use sutura_domain::plan::{AnchorPlan, Executable};
    use sutura_domain::source::{ImpersonationCapability, SourcePosture};
    use sutura_domain::warehouse::deadline::Deadline;
    use sutura_domain::warehouse::preflight::{TablesPresent, UnaccountedTables};
    use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

    use super::{Notice, Refusal, TablesChanged, Verdict, ask, refuse_unattached};
    use crate::warehouses::Warehouses;

    /// A data system that answers the pre-flight from what a test handed it, and records the asking.
    struct Answers {
        source: SourceName,
        answer: Answering,
        asked: RefCell<Vec<usize>>,
        refused: bool,
    }

    /// One boot-policy case: a registry that answers, whether the policy refuses, and the
    /// wording for the assertion. Named because `clippy::type_complexity` refuses the tuple
    /// inline, and the name carries which `bool` it is.
    type PolicyCase = (Warehouses<Answers>, bool, &'static str);

    /// What a test hands the fake to answer a pre-flight with.
    type Answering = fn(&BTreeSet<QualifiedTable>) -> Result<TablesPresent, CouldNotAsk>;

    /// The one failure this fake can report.
    #[derive(Debug)]
    struct CouldNotAsk;

    impl core::fmt::Display for CouldNotAsk {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("the data system could not be listed")
        }
    }

    impl core::error::Error for CouldNotAsk {}

    impl Warehouse for Answers {
        type Error = CouldNotAsk;

        const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

        fn source(&self) -> &SourceName {
            &self.source
        }

        fn posture(&self) -> &SourcePosture {
            &SourcePosture::ImpersonationAtSource
        }

        fn execute(
            &self,
            _executable: Executable<'_>,
            _presented: &Presented,
            _deadline: Deadline,
        ) -> Result<ResultBatches, Self::Error> {
            Err(CouldNotAsk)
        }

        fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
            Err(CouldNotAsk)
        }

        fn preflight(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
            self.asked.borrow_mut().push(tables.len());
            (self.answer)(tables)
        }

        fn preflight_was_refused(&self, _error: &Self::Error) -> bool {
            self.refused
        }
    }

    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a test source is a source")
    }

    fn opened(answer: Answering) -> Warehouses<Answers> {
        registry(answer, false)
    }

    fn refusing(answer: Answering) -> Warehouses<Answers> {
        registry(answer, true)
    }

    fn registry(answer: Answering, refused: bool) -> Warehouses<Answers> {
        Warehouses::of(Answers {
            source: source(),
            answer,
            asked: RefCell::new(Vec::new()),
            refused,
        })
    }

    fn table(raw: &str) -> QualifiedTable {
        QualifiedTable::from(TableName::parse(raw).expect("a test table is a table"))
    }

    /// Two models over two tables on one source.
    fn bundle() -> sutura_domain::pinned::PinnedDefinitions {
        crate::tests_support::bundle_over(&[
            ("customers", "warehouse", "dim_customer"),
            ("orders", "warehouse", "fct_orders"),
        ])
    }

    /// TWO models over ONE table, which is the shape `AbsentBehind`'s value being a set is for.
    fn two_models_one_table() -> sutura_domain::pinned::PinnedDefinitions {
        crate::tests_support::bundle_over(&[("orders", "warehouse", "fct_orders"), ("returns", "warehouse", "fct_orders")])
    }

    #[test]
    fn the_whole_bundle_is_one_question_and_not_one_per_model() {
        // The cost argument the port is shaped around, asserted rather than claimed: two models
        // behind two tables are ONE call carrying both.
        let engines = opened(|_asked| Ok(TablesPresent::All));
        let answered = ask(&bundle(), &engines);
        assert_eq!(answered.len(), 1, "one answer per data system");
        assert!(
            matches!(answered[0].verdict(), Verdict::Present { asked: 2 }),
            "{:?}",
            answered[0].verdict()
        );
        let engine = engines.get(&source()).expect("the fake is registered under that alias");
        assert_eq!(
            *engine.asked.borrow(),
            vec![2],
            "one call carrying both tables, not one call per model"
        );
    }

    #[test]
    fn a_table_a_data_system_did_not_account_for_is_not_an_absence_and_carries_no_model() {
        // **The decision `telekom/sutura#275` settles, at the layer that maps the port's answer.**
        // Both outcomes name tables and both stop a boot; what separates them is the sentence a root
        // is licensed to write, and this one may not say a `table:` is wrong. So the verdict carries
        // the tables and the size of the gap and deliberately NOT the models behind them.
        let engines = opened(|asked| {
            Ok(TablesPresent::Unaccounted {
                tables: UnaccountedTables::parse(
                    asked
                        .iter()
                        .filter(|table| table.name().as_str() == "fct_orders")
                        .cloned()
                        .collect(),
                )
                .expect("the bundle names fct_orders"),
                shortfall: NonZeroU64::new(4).expect("four is not zero"),
            })
        });
        let answered = ask(&bundle(), &engines);
        let Verdict::Unaccounted {
            ref tables,
            ref shortfall,
        } = *answered[0].verdict()
        else {
            panic!(
                "a data system that did not account for its own tables has not established an absence: {:?}",
                answered[0].verdict()
            )
        };
        assert_eq!(tables.to_string(), "fct_orders", "the table the answer never reached");
        assert_eq!(shortfall.get(), 4, "and how many tables it left out of its own total");
    }

    #[test]
    fn an_absent_table_comes_back_with_the_models_that_named_it_and_nothing_else() {
        let engines = opened(|asked| {
            Ok(TablesPresent::of(
                asked
                    .iter()
                    .filter(|table| table.name().as_str() == "fct_orders")
                    .cloned()
                    .collect(),
            ))
        });
        let answered = ask(&bundle(), &engines);
        let Verdict::Absent(ref absent) = *answered[0].verdict() else {
            panic!("a table that is not there is an absence: {:?}", answered[0].verdict())
        };
        assert_eq!(absent.len(), 1, "one table is absent, not both: {absent}");
        assert_eq!(
            absent.named().get(&table("fct_orders")).map(BTreeSet::len),
            Some(1),
            "the absent table carries the one model that named it: {absent}"
        );
        assert_eq!(
            absent.to_string(),
            "table fct_orders, named by model(s) [orders]",
            "the rendering names the table AND the model"
        );
    }

    #[test]
    fn two_models_over_one_absent_table_are_both_named() {
        // **The whole reason `AbsentBehind`'s value is a SET**, and nothing exercised it before:
        // a bundle may declare several models over one fact table, and an operator told about only
        // one of them has half the edit. Both names, in one clause, in reading order.
        let engines = opened(|asked| Ok(TablesPresent::of(asked.clone())));
        let answered = ask(&two_models_one_table(), &engines);
        let Verdict::Absent(ref absent) = *answered[0].verdict() else {
            panic!("the table is absent: {:?}", answered[0].verdict())
        };
        assert_eq!(
            absent.to_string(),
            "table fct_orders, named by model(s) [orders, returns]",
            "one clause for the table, both models inside it"
        );
        let engine = engines.get(&source()).expect("the fake is registered under that alias");
        assert_eq!(
            *engine.asked.borrow(),
            vec![1],
            "two models over one table is ONE table to ask about"
        );
    }

    #[test]
    fn an_adapter_that_did_not_look_is_not_reported_as_one_that_found_everything() {
        // `NotReported` and `Present` are two verdicts, so no root can render the port's default as
        // verification. The port's own suite covers the default itself; this covers the mapping.
        let engines = opened(|_asked| Ok(TablesPresent::NotAsked));
        let answered = ask(&bundle(), &engines);
        assert!(
            matches!(answered[0].verdict(), Verdict::NotReported { asked: 2 }),
            "{:?}",
            answered[0].verdict()
        );
    }

    #[test]
    fn an_unreadable_inventory_reaches_the_root_without_model_names_or_a_count() {
        let engines = opened(|asked| {
            Ok(TablesPresent::UnreadableInventory(
                UnaccountedTables::parse(asked.clone()).expect("nonempty"),
            ))
        });
        let answers = ask(&bundle(), &engines);
        let Verdict::UnreadableInventory(tables) = answers[0].verdict() else {
            panic!("the count-free refusal must survive mapping: {:?}", answers[0].verdict());
        };
        assert_eq!(tables.to_string(), "dim_customer, fct_orders");
    }

    #[test]
    fn a_refusal_and_an_outage_are_two_verdicts_from_the_same_error_value() {
        // **The split, and the pair is what makes it mean something:** the same `Err`, the same
        // variant, and the only difference is what the adapter says about its own failure.
        let refused = refusing(|_asked| Err(CouldNotAsk));
        assert!(
            matches!(ask(&bundle(), &refused)[0].verdict(), Verdict::Refused { .. }),
            "an adapter that says it was refused is a refusal"
        );
        let outage = opened(|_asked| Err(CouldNotAsk));
        assert!(
            matches!(ask(&bundle(), &outage)[0].verdict(), Verdict::Unverified { asked: 2, .. }),
            "an adapter that says it was not refused is an outage, and carries what went unverified"
        );
    }

    #[test]
    fn a_data_system_the_bundle_names_no_model_in_is_never_asked() {
        // No round trip to be told about an empty set, and no verdict for a root to explain.
        let engines = opened(|_asked| Ok(TablesPresent::All));
        let elsewhere = crate::tests_support::bundle_over(&[("customers", "elsewhere", "dim_customer")]);
        assert!(ask(&elsewhere, &engines).is_empty(), "nothing to ask about is no answer");
        let engine = engines.get(&source()).expect("the fake is registered under that alias");
        assert!(engine.asked.borrow().is_empty(), "and no call was made");
    }

    #[test]
    fn the_answer_names_the_source_it_came_from() {
        // A root's message has to name the data system, and a registry with two of them would
        // otherwise be a set of verdicts nobody could attribute.
        let engines = opened(|_asked| Ok(TablesPresent::All));
        let answered = ask(&bundle(), &engines);
        assert_eq!(*answered[0].source(), source(), "the answer carries its own source");
    }
    /// A table-name set, the way both composition roots' suites built one.
    fn tables(names: &[&str]) -> BTreeSet<TableName> {
        names
            .iter()
            .map(|raw| TableName::parse(raw).expect("a test table is a table"))
            .collect()
    }

    /// Moved here from `sutura-serve`'s own suite with its assertions unchanged: the function it
    /// drives moved, and this is where its one home is now.
    ///
    /// The startup sequence loads the catalog TWICE - the engine is opened for the first bundle and
    /// the service validates and serves the second - so a model added to the catalog directory
    /// between the two calls was served with nothing attached behind it. `answer` cannot catch that:
    /// its only check on the engine is that the source NAME matches, so the first question about the
    /// new metric came back as an error from the engine rather than as a refusal at startup.
    #[test]
    fn a_model_with_no_table_attached_behind_it_does_not_serve() {
        let changed = refuse_unattached(
            &tables(&["fact_subscription", "dim_customer"]),
            &tables(&["fact_subscription"]),
        )
        .expect_err("a served model with no attached table does not serve");
        let sentence = changed.to_string();
        assert!(
            sentence.contains("Served with no table attached: [dim_customer]"),
            "{sentence}"
        );
        assert!(sentence.contains("Attached and no longer served: []"), "{sentence}");
        assert!(
            sentence.contains("the catalog changed while this process was starting"),
            "{sentence}"
        );
    }

    /// Moved from `sutura-serve`'s suite, assertions unchanged.
    ///
    /// The other direction, and not pedantry: it means the catalog directory changed between two
    /// loads seconds apart. This one would answer every question correctly, which is exactly why it
    /// has to be loud - whatever else moved in that edit is the part nobody has looked at.
    #[test]
    fn a_table_attached_for_a_model_no_longer_served_does_not_serve_either() {
        let changed = refuse_unattached(
            &tables(&["fact_subscription"]),
            &tables(&["fact_subscription", "dim_customer"]),
        )
        .expect_err("an attached table for nothing served does not serve");
        let sentence = changed.to_string();
        assert!(sentence.contains("Served with no table attached: []"), "{sentence}");
        assert!(
            sentence.contains("Attached and no longer served: [dim_customer]"),
            "{sentence}"
        );
    }

    /// Moved from `sutura-serve`'s suite, and it carries `sutura-cli`'s half of the same case too:
    /// that root's own copy asserted the two directions and the agreeing pair over its own table
    /// names, against a body measured byte-identical to serve's.
    ///
    /// The check has to be silent when nothing changed, which is every start. An empty catalog is
    /// already refused earlier, by each root's `open_engine`, so the empty pair is not a case this
    /// decides.
    #[test]
    fn the_two_loads_agreeing_is_the_ordinary_case_and_serves() {
        refuse_unattached(&tables(&["fact_subscription"]), &tables(&["fact_subscription"]))
            .expect("two bundles that agree start");
        assert!(
            refuse_unattached(
                &tables(&["dim_customer", "fact_subscription"]),
                &tables(&["fact_subscription", "dim_customer"])
            )
            .is_ok(),
            "the comparison is over sets, so declaration order is not a difference"
        );
    }

    /// The mismatch is a VALUE both differences can be read off, not only a sentence.
    ///
    /// This is what the roots could not do while each built a `String`: the refusal now carries the
    /// two sets, so a caller that wants to act on which tables moved has them, and the sentence is
    /// one `Display` rather than two copies. Both directions at once, because a single edit to a
    /// catalog directory can add one model and drop another.
    #[test]
    fn the_mismatch_carries_both_differences_as_sets() {
        let changed = refuse_unattached(
            &tables(&["dim_customer", "fact_order"]),
            &tables(&["fact_order", "dim_product"]),
        )
        .expect_err("a bundle naming one table and attaching another has changed");
        assert_eq!(*changed.missing(), tables(&["dim_customer"]), "served with nothing behind it");
        assert_eq!(*changed.extra(), tables(&["dim_product"]), "attached and no longer named");
        assert_eq!(
            changed,
            TablesChanged {
                missing: tables(&["dim_customer"]),
                extra: tables(&["dim_product"]),
            },
            "and the value is comparable, so a caller can assert on it without reading prose"
        );
    }

    /// The boot policy: which of the port's seven answers this deployment refuses to start on.
    ///
    /// **One statement of the split both composition roots used to make arm by arm.** Asserted over
    /// `ask`'s real output rather than over hand-built verdicts, so what an adapter answered is
    /// carried through the mapping to whether a deployment starts. Every answer the port can give is
    /// reachable from this fake: it returns `TablesPresent::NotAsked` directly, and
    /// `preflight_was_refused` is a field on it, so one `Err` answers as both `Refused` and
    /// `Unverified`.
    ///
    /// **Nothing here holds that these cases stay exhaustive.** An answer added to the port is a
    /// compile error in `boot_policy`, which is where the decision lives - this array keeps passing,
    /// and it is a reviewer who notices it was not extended.
    #[test]
    fn the_boot_policy_refuses_the_four_outcomes_and_serves_the_others() {
        // The registries are owned by the array for the whole loop, because an answer BORROWS the
        // source name off the registry it came from.
        let cases: [PolicyCase; 7] = [
            (opened(|_asked| Ok(TablesPresent::All)), false, "every table present"),
            (
                opened(|_asked| Ok(TablesPresent::NotAsked)),
                false,
                "an adapter that does not report",
            ),
            (
                opened(|_asked| Err(CouldNotAsk)),
                false,
                "a data system that could not be asked",
            ),
            (
                refusing(|_asked| Err(CouldNotAsk)),
                true,
                "a data system that refused to be listed",
            ),
            (
                opened(|asked| {
                    Ok(TablesPresent::of(
                        asked
                            .iter()
                            .filter(|table| table.name().as_str() == "fct_orders")
                            .cloned()
                            .collect(),
                    ))
                }),
                true,
                "a table the data system does not hold",
            ),
            (
                opened(|asked| {
                    Ok(TablesPresent::UnreadableInventory(
                        UnaccountedTables::parse(asked.clone()).expect("the bundle names tables"),
                    ))
                }),
                true,
                "an inventory this deployment could not read",
            ),
            (
                opened(|asked| {
                    Ok(TablesPresent::Unaccounted {
                        tables: UnaccountedTables::parse(asked.clone()).expect("the bundle names tables"),
                        shortfall: NonZeroU64::new(1).expect("one is not zero"),
                    })
                }),
                true,
                "an inventory that did not account for itself",
            ),
        ];
        for (engines, refuses, what) in &cases {
            let policy = ask(&bundle(), engines)
                .into_iter()
                .next()
                .expect("the bundle names a model in the fake")
                .into_verdict()
                .boot_policy();
            assert_eq!(policy.is_err(), *refuses, "the boot policy for {what}");
        }
    }

    /// The partition keeps each outcome's own payload, so a root can still render its own sentence.
    ///
    /// **The reason this is asserted rather than assumed:** a split that dropped the cause, the
    /// absent tables or the shortfall would compile and would silently cost every root's message the
    /// part an operator acts on.
    #[test]
    fn the_partition_carries_each_outcome_s_payload_through() {
        let absent_engines = opened(|asked| {
            Ok(TablesPresent::of(
                asked
                    .iter()
                    .filter(|table| table.name().as_str() == "fct_orders")
                    .cloned()
                    .collect(),
            ))
        });
        let Err(Refusal::Absent(behind)) = ask(&bundle(), &absent_engines)
            .into_iter()
            .next()
            .expect("the bundle names a model in the fake")
            .into_verdict()
            .boot_policy()
        else {
            panic!("a table that is not there is a refusal that names it")
        };
        assert!(
            behind.to_string().contains("fct_orders"),
            "the absent table and the models behind it survive the split: {behind}"
        );

        let outage = opened(|_asked| Err(CouldNotAsk));
        let Ok(Notice::Unverified { asked, cause }) = ask(&bundle(), &outage)
            .into_iter()
            .next()
            .expect("the bundle names a model in the fake")
            .into_verdict()
            .boot_policy()
        else {
            panic!("a data system that could not be asked is a notice, not a refusal")
        };
        assert_eq!(asked, 2, "how many tables went unverified");
        assert_eq!(
            cause.to_string(),
            "the data system could not be listed",
            "and the adapter's own cause, for the root to flatten"
        );
    }
}

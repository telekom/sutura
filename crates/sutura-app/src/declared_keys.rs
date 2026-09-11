//! Holding a bundle's cardinality declarations against the data, once, at boot.
//!
//! # What the declaration buys, and what it cost when nothing checked it
//!
//! A `many_to_one` is what licenses a join to be measure-preserving. The whole-answer path renders a
//! `JOIN` on the strength of it and a federated lookup leg renders `GROUP BY` on the strength of the
//! same sentence, and **the two spend it differently**: a duplicated dimension row adds a measure
//! twice on one data system and is collapsed away on two. Measured over the derived corpus, one
//! extra row for a customer key that already existed made
//! `crates/sutura-app/tests/differential/federated.rs` report `29138` against `22765` for the same
//! question - and neither topology refused.
//!
//! `sutura_domain::warehouse::cardinality` carries the measurement and the arithmetic. This module
//! is the boot path that asks.
//!
//! # Why it refuses the bundle rather than the question
//!
//! What the declaration claims is a property of the target TABLE, not of any one question, so it
//! cannot be contradicted inside one answer's statement - which is exactly why the two topologies
//! could disagree without either being wrong about its own plan. Refused here, both topologies agree
//! again: a bundle whose declaration the data contradicts is not [`Validated`](crate::Validated), so
//! neither of them serves, and the refusal names the model, the table and the column.
//!
//! **So a CALLER never sees this refusal, and that is the shape rather than a gap in it.** The
//! deployment does not start, so there is no question for a wrong number to come back to. **The
//! operator is the audience**, and the refusal is written for them.
//!
//! **On the MONO path a question-time refusal is not available at all**, and the reason is the plan
//! surface rather than effort: the fan-out is per FACT ROW, a plan carries no row identity -
//! `sutura_domain::query` keeps row ids off the tool surface deliberately - and the doubling happens
//! inside the aggregate, so nothing one statement can project counts it. The three things that would
//! work each cost more than they buy: a second statement per joined question, a row identity added
//! to the plan, or a de-duplicating subquery, which absorbs the violation rather than refusing it and
//! makes the number depend on the declaration instead of on the data.
//!
//! **On the FEDERATED path that argument is too strong, and review is why it now says so.**
//! `FederatedPlan::combine` already raises
//! [`FederatedFailure::AmbiguousLink`](sutura_domain::plan::FederatedFailure::AmbiguousLink) - a
//! refusal that exists, is typed, and is reached today for duplicates that DISAGREE in a column the
//! question projects. What blinds it to the agreeing case is only the lookup leg's own `GROUP BY`,
//! and there are two ways to unblind it, which differ in what each costs and in what each needs
//! changing:
//!
//! * **Drop the leg's `GROUP BY`.** Agreeing duplicates then arrive as separate rows and the
//!   existing guard fires with no change to the combiner at all - `lookups_by_link` refuses the
//!   second ROW for a link value. It costs the rows the collapse was removing, which under a
//!   satisfied declaration is none: the leg projects the link plus its dimension columns, so the
//!   grouping is a no-op exactly when the declaration holds.
//! * **Keep it and project `COUNT(*)` per group** - one extra integer column and zero extra rows.
//!   Cheaper on the wire, and **not free**: the guard counts rows, so the combiner needs one more
//!   condition beside the existing one to read that column. Saying it would fire *unchanged* would
//!   be the overstatement this file is otherwise careful about.
//!
//! Neither is done here, and the reason is the property this module is for: the mono path would
//! still answer, so one topology would refuse and the other would answer - which is
//! `telekom/sutura#354` re-created one layer down. Either becomes worth doing beside a mono-path
//! answer, or on its own as defence in depth for the window below. Both are named so that *cannot be
//! observed at question time* is not read as true of both paths; it is true of the mono path.
//!
//! **The cost of that shape is stated below as the second limit**: because the audience is the
//! operator at startup, a violation that arrives while the process is running reaches a caller as a
//! number, exactly as it did before this module existed.
//!
//! # The four outcomes, and which two refuse
//!
//! | Outcome | What happens |
//! | --- | --- |
//! | Counted, and the data contradicts the declaration | [`NotValidated::DeclaredKeyNotUnique`](sutura_domain::pinned::NotValidated::DeclaredKeyNotUnique), naming the model, the table, the column and the two counts |
//! | The adapter answered `Err` | [`NotValidated::DeclaredKeyNotCounted`](sutura_domain::pinned::NotValidated::DeclaredKeyNotCounted), carrying the adapter's own message and every cause beneath it |
//! | The adapter took the port's default | Passes. [`KeyUniqueness::NotAsked`](sutura_domain::warehouse::cardinality::KeyUniqueness::NotAsked) means *nobody counted*, and refusing on it would stop every deployment whose data system has no cheap way to ask - it is a fact about what was LINKED, not about a run |
//! | No data system is configured under the target model's source | Passes. Nothing can execute a question over that model either, so refusing here would refuse a bundle for a reason the query path already covers |
//!
//! **`Err` refuses, and that is the opposite of what the pre-flight does with the same shape.** The
//! difference is where each runs. `sutura_app::preflight::ask` runs in a composition root, where
//! *could not verify* has somewhere to go - a `WARN` line and a deployment that serves - and it
//! splits *refused* from *unreachable* through
//! [`Warehouse::preflight_was_refused`](sutura_domain::warehouse::Warehouse::preflight_was_refused)
//! so an operator is not told to retry a missing grant. This runs inside the operation that mints
//! the proof, which has exactly two outcomes and no line to print, and the rule already there for a
//! statement the data system would not run is refusal. **Silence was the third option**, and it is
//! the one measured as the defect: it made *this identity may not read the dimension table* - the
//! identity-relevant case in an identity-aware runtime - indistinguishable from a clean check.
//!
//! **Why the split is not reproduced here**, stated rather than left as an omission: that predicate
//! is overridden by exactly one adapter in this workspace, and that adapter takes this method's
//! default. A typed split would be a control that cannot fire on any adapter that counts. Refusing
//! in both cases and printing the cause is loud for either; telling them apart wants the predicate
//! implemented by an adapter that can, and is a slice of its own.
//!
//! **What is still quiet is the port's default**, and only that: a deployment whose adapter never
//! counts reads, from its own log, exactly like one whose declarations were checked and held. See
//! the mechanism note below for how much of that a test closes.
//!
//! **How much of that quiet a mechanism closes, precisely, because *a control that can be absent
//! without anyone knowing* is the shape this repository calls the defect:**
//!
//! * A REGISTERED data system that stops counting reddens in CI.
//!   `data_systems::…::whether_it_counts_a_declared_join_key_is_what_the_boot_check_can_use_it_for`
//!   asks every entry of the golden matrix's `data_systems` registry and requires
//!   [`KeyUniqueness::Counted`](sutura_domain::warehouse::cardinality::KeyUniqueness::Counted) with
//!   the fixture's own row count - so the default, an `Err`, and a probe that resolved the wrong
//!   table are each a red cell rather than a quiet pass. That is what stops this check from
//!   becoming absent by an edit, and it is measured: neutering the probe in each of the three
//!   adapters reddens that adapter's cell. * A LIVE deployment whose probe fails is no longer quiet
//!   - it does not start, and the refusal carries the data system's own complaint. * **An adapter
//!   outside that registry is still quiet**, and there is one: `BigQuery` takes the default, and it
//!   is absent from the `data_systems` registry because no published artifact links the crate. So a
//!   dimension model on a dataset is unchecked, nothing says so at boot, and no test here can see
//!   it. What closes that is the adapter implementing the method, not another gate.
//!
//! **The second limit is time, and it is the whole process lifetime.** This is a boot check over the
//! table as it stood then, which is the same limit an anchor carries - and `verify_and_validate` has
//! one production reach, with no reload and no readiness re-validation, so for a long-running server
//! the window is not *until the next start* in any bounded sense: it is **indefinite**. A row
//! inserted a minute after boot reaches every caller from then on as a number, because the answer
//! path is unchanged underneath: a whole-answer `JOIN` still adds the measure twice, and a federated
//! lookup leg still groups by the columns it projects, so `AmbiguousLink` still fires only for
//! duplicates that DISAGREE in a column the question projects. The `COUNT(*)`-per-group option named
//! above is the one thing that would narrow it, and it narrows the federated half only.

use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyNotCounted, KeyNotUnique, KeyUniqueness};

use crate::warehouses::Warehouses;

/// Asks every data system whether the keys the bundle declares unique really are.
///
/// Runs BEFORE the anchors in [`verify_and_validate`](crate::verify_and_validate), and the order is
/// a diagnostic decision rather than an arbitrary one: a duplicated dimension key is a table an
/// operator can go and look at, while an anchor mismatch caused by one reads as a broken definition.
/// **It is not the cheaper half, and the earlier version of this line said it was.** An anchor is a
/// range-filtered aggregate a catalog author chose; this is an UNFILTERED `COUNT(DISTINCT ..)` over
/// a whole dimension table, which for a large one is plausibly the more expensive of the two -
/// unmeasured either way, and paid by every CLI invocation as well as every server start. The order
/// is a diagnostic choice on its own, not a cost one.
pub(crate) fn hold<W>(pinned: &PinnedDefinitions, warehouses: &Warehouses<W>) -> Result<(), NotValidated>
where
    W: Warehouse,
{
    let definitions = pinned.definitions();
    for relationship in definitions.relationships().values() {
        // A relationship whose join type promises nothing about its target - `one_to_many` - yields
        // no key, and so does one the definitions could not resolve, which a loaded bundle cannot
        // contain. Both are *nothing to ask* rather than a violation, and the constructor is where
        // that distinction is made so that it is made once.
        let Ok(key) = DeclaredKey::promised_by(relationship, definitions) else {
            continue;
        };
        let Some(warehouse) = warehouses.get(key.source()) else {
            continue;
        };
        // THE credential-free call, and the one `#[expect]` that lets it compile. `clippy.toml`
        // bans this method for `verify_anchor`'s reason - it reaches a data system under whatever
        // identity the deployment configured, with no caller to mint one for - so this expectation
        // and the registry cell's are the whole permitted set, and a third is a diff a reviewer
        // sees.
        #[expect(
            clippy::disallowed_methods,
            reason = "the boot path is one of the two permitted callers of a port method that executes with no \
                      credential; the ban exists so that this and the registry cell are the only places it is called"
        )]
        let answered = warehouse.declared_key(key);
        // **The adapter's error is carried, not dropped.** It was discarded here, at the one point
        // it was still typed, which left *this identity may not read the dimension table* looking
        // exactly like a clean check. `flatten` walks it to text because `W::Error` cannot cross
        // into the domain - the same boundary the anchor path crosses the same way.
        let counts = match answered {
            Ok(KeyUniqueness::Counted(counts)) => counts,
            // A fact about what was linked rather than about a run. The module header says what it
            // costs and what closes it.
            Ok(KeyUniqueness::NotAsked) => continue,
            Err(cause) => {
                let (message, chain) = crate::flatten(&cause);
                return Err(NotValidated::DeclaredKeyNotCounted(Box::new(KeyNotCounted::of(
                    &key, message, chain,
                ))));
            }
        };
        // `found` is `None` where the counts hold the declaration up, so the clean case is the
        // constructor's answer rather than a condition written here and possibly written wrongly.
        if let Some(violation) = KeyNotUnique::found(&key, counts) {
            return Err(NotValidated::DeclaredKeyNotUnique(Box::new(violation)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

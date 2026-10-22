//! The federated question: two legs, the plan that names them, and the port the combine happens
//! through.
//!
//! **This module gives [`crate::federation`] its caller.** `Descent::of` and `Federation::of` are
//! total classifications that nothing used to execute; a [`FederatedPlan`] is the small, closed
//! contract a splitter fills with facts, and [`combiner`]'s port is what turns two legs' results
//! back into one answer's rows.
//!
//! **The combine used to be a method here and is not any more.** `docs/adr/0039` step 3 replaced
//! `FederatedPlan::combine` - a pure domain function that walked rows one cell at a time - with a
//! `DataFusion` plan in an adapter, under the owner instruction *no hand row handling*. What that
//! leaves here is the plan TYPE, its refusals, the label scheme both halves read, and the port; the
//! four accessors [`FederatedPlan::bucket_label`], [`FederatedPlan::measure_label`],
//! [`FederatedPlan::federation`] and [`FederatedPlan::lookups`] exist because an
//! implementor above this crate cannot read a private field. What is deliberately not published is
//! anything a combiner could use to invent a column.
//!
//! **What the splitter and the combiner agree on, and it is one function.** A fact leg's terms are
//! projected under labels, and the combiner has to find each term's column *by* its label - the
//! mistake this design refuses to make is the two halves agreeing by review. [`labels`] is that one
//! function: the splitter names the fact leg's terms with it and a combiner re-derives the same
//! names from the same [`Federation`] and looks them up in the fact leg's result. There is no second
//! copy of the naming rule to drift.
//!
//! **Those labels live in a namespace a question cannot reach, which is [`label`]'s job.** A leg's
//! result carries public dimension labels beside the internal ones, so an internal label spelled as
//! an identifier is a label a legal dimension name can collide with - reproduced. [`InternalLabel`]
//! is the type that cannot be spelled by one.
//!
//! **The division cannot happen in a leg, and that survives the combine moving out.** The
//! [`Above`](crate::federation::Above) tree carries the only
//! [`ZeroDenominator`](crate::measure::ZeroDenominator) in the federated path, and
//! [`FederatedPlan::federation`] hands a combiner that tree rather than a per-leg guard - a guard
//! applied inside a leg is the wrong number this shape exists to prevent.
//!
//! **A measure that does not decompose is pulled up, and the plan holds that as a key rather than a
//! term.** An exact distinct count has no function that adds per-group counts back up, so its leaf
//! ([`Carried::Keys`](crate::federation::Carried::Keys)) is a key of the fact leg - one row per
//! distinct value - and the combine counts the distinct values above. [`FederatedPlan::new`] checks
//! that the fact leg projects each such key under the leaf's own label.

/// The second driven port, and the pair of leg results it takes.
///
/// `docs/adr/0007` designed it here and recorded that it was not built; step 3 of
/// `docs/adr/0039` is what builds it.
pub mod combiner;

/// The deterministic refusals a federated answer can carry.
///
/// A combiner's own error is classified into one by
/// [`FederationCombiner::answer_not_well_formed`].
mod failure;

/// What equality over a link column means, and the classifier from a declared type.
///
/// Declares no test module, for the reason [`label`] gives.
mod link;
mod lookup;

/// The reserved label namespace, and the one function that assigns it.
///
/// Its own module because it is what the splitter, the combiner and the leg goldens all read the
/// spelling from, and because a namespace is a thing to reason about on its own. It declares no test
/// module: a test module declared from a file `test-causality` reverts is never compiled, and the
/// proof it then reports is vacuous - every assertion about it is in `tests.rs`.
pub mod label;

use crate::federation::Federation;
use crate::model::{MetricName, SourceName};
use crate::plan::PlanBucket;
use crate::plan::ResultLabel;
use crate::plan::leg::LegPlan;
use crate::query::{Top, TopBy, TopDirection};
use crate::warehouse::{MalformedRowSet, RowSet, Value};

pub use combiner::{Cut, ExecuteError, Executed, FederationCombiner, LegResult, LegRunner, Legs, LegsMiswired};
#[cfg(any(test, feature = "fixtures"))]
pub use combiner::{NothingCombined, RefusingCombiner};
pub use failure::FederatedAnswerRefusal;
pub use label::{InternalLabel, labels};
pub use link::{LinkKind, QuotedColumnType};
pub use lookup::{LegAt, LookupLeg, LookupLegs, LookupLegsError};

/// The one federated shape this workspace combines: a fact leg on one source and one to
/// [`MAX_LOOKUP_LEGS`](crate::query::MAX_LOOKUP_LEGS) lookup legs, each on another source than the
/// leg it joins onto and linked to it by a single column.
///
/// **One fact leg as a named field, the lookup legs as [`LookupLegs`].** A match over [`LegPlan`]
/// is exhaustive, so the fact leg *is* the [`Fact`](LegPlan::Fact) variant and each lookup leg the
/// [`Lookup`](LegPlan::Lookup) one, and a plan with no lookup leg or more than the cap is a type that
/// does not exist rather than a count a caller checks. The shape is deliberately the one
/// [`crate::plan::leg`] pins in its goldens: the metric's own rows (and any same-source dimension)
/// form the fact leg, and a dimension on another data system forms a lookup leg. The final answer groups by the answer's keys - each
/// named by which leg's result it is read from, in question order - bucketed and measured under the
/// metric's own name.
///
/// **The [`serde::Serialize`] derive exists for the CLI's plan dump and nothing else.** A plan is
/// serialized to be printed; `FederatedPlan` does not derive [`serde::Deserialize`], so a plan cannot
/// be reconstructed from its serialized form and no field here is a request a caller writes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FederatedPlan {
    metric: MetricName,
    measure_label: ResultLabel,
    bucket: PlanBucket,
    /// The metric's own share of the question: the same-source rows and leaves.
    fact: LegPlan,
    /// A second fact leg over a different fact model, when the measure's ratio terms name two
    /// models (`telekom/sutura#780`). Two facts never share a `FROM`: each is its own statement,
    /// aggregated on its own and joined above on the link and the time bucket, and
    /// [`FederatedPlan::new`] refuses a second fact that cannot be joined.
    second_fact: Option<LegPlan>,
    /// The other data systems' shares: one lookup leg per crossing relationship, each with the leg
    /// it joins onto and its own join direction.
    lookups: LookupLegs,
    /// The combine tree above the legs, and the metric that names its leaves.
    federation: Federation,
    /// The answer's group-by keys in question order, each naming which leg's result it is read from.
    ///
    /// This is the one honest statement of the answer's column order, matching the mono path which
    /// emits dimensions as the question ordered them. Fact keys are read from the fact result,
    /// lookup keys from the lookup result, and the two never overlap because a dimension belongs to
    /// exactly one leg.
    keys: Vec<AnswerKey>,
    /// The federated `top`, ranked above the combine - `github.com/telekom/sutura#777`'s case 2,
    /// the only case the splitter can produce: a `top` not pushed to a fact leg ranks and
    /// truncates the combined answer after the combiner returns it. `None` when the
    /// question carries no `top`.
    top: Option<Top>,
    /// How the first link's two keys compare when its relationship declares it. `None` is equality of
    /// kind. A later lookup leg's link is always compared exactly: one declaration, one link.
    #[serde(skip_serializing_if = "Option::is_none")]
    link_comparison: Option<LinkKind>,
}

/// Which leg's result an answer key is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LegSide {
    /// The metric's own leg.
    Fact,
    /// The second data system's leg.
    Lookup,
}

/// One group-by key of the answer: which leg owns it, and the label it carries in that leg's result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnswerKey {
    side: LegAt,
    label: ResultLabel,
}

impl AnswerKey {
    /// A key read from the fact leg's result, under `label`.
    #[inline]
    pub const fn fact(label: ResultLabel) -> Self {
        Self {
            side: LegAt::Fact,
            label,
        }
    }

    /// A key read from the result of the lookup leg at `leg`, under `label`.
    #[inline]
    pub const fn lookup(leg: usize, label: ResultLabel) -> Self {
        Self {
            side: LegAt::Lookup(leg),
            label,
        }
    }

    /// Which leg this key is read from.
    #[inline]
    pub const fn side(&self) -> LegAt {
        self.side
    }

    /// The text this key carries in its leg's result.
    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
    }
}

impl FederatedPlan {
    /// Constructs a federated plan from its legs and the answer's key order.
    ///
    /// A `Result` constructor is this workspace's convention for a value with an invariant: a plan
    /// that is not a fact leg beside lookup legs, or that names one data system on a leg and the leg
    /// it joins onto, is not a plan and cannot be built.
    ///
    /// **There is no link-label parameter, and that is the F2 fix's structural half.** The label the
    /// legs are joined under used to be two `String` arguments, and the splitter filled both with the
    /// physical remote join column's text - which is a legal dimension name, so a legal question
    /// produced two fact columns under one label. It is now [`InternalLabel::Link`], a constant of
    /// the scheme rather than data on the plan: there is no argument for a caller to spell, nothing
    /// for two legs to disagree about, and the constructor requires each lookup leg and the leg it
    /// joins onto to project its own.
    // The constructor takes the shape of the question as the splitter decided it; a bundle of named
    // fields is the alternative, and a `Vec` would let a caller omit or duplicate a leg - the two
    // instantiations it exists to forbid.
    pub fn new(
        metric: MetricName,
        measure_label: ResultLabel,
        bucket: PlanBucket,
        fact: LegPlan,
        second_fact: Option<LegPlan>,
        lookups: LookupLegs,
        federation: Federation,
        keys: Vec<AnswerKey>,
    ) -> Result<Self, FederatedPlanError> {
        if !matches!(fact, LegPlan::Fact { .. }) {
            return Err(FederatedPlanError::NotFact {
                source_name: fact.source().clone(),
            });
        }
        let leg_at = |at: LegAt| match at {
            LegAt::Fact => Some(&fact),
            LegAt::Lookup(position) => lookups.legs().get(position).map(LookupLeg::leg),
        };
        // A leg and the leg it joins onto on one data system are one statement, not two. Two legs
        // of one system that are not neighbours - a chain that crosses away and comes back - are
        // two statements and are served.
        for (position, lookup) in lookups.legs().iter().enumerate() {
            let leg = lookup.leg();
            if !matches!(leg, LegPlan::Lookup { .. }) {
                return Err(FederatedPlanError::NotLookup {
                    source_name: leg.source().clone(),
                });
            }
            let onto = leg_at(lookup.onto()).ok_or_else(|| FederatedPlanError::NotLookup {
                source_name: leg.source().clone(),
            })?;
            if onto.source() == leg.source() {
                return Err(FederatedPlanError::SameSource {
                    source_name: leg.source().clone(),
                });
            }
            let link = InternalLabel::Link(position).label();
            for (side, on) in [(LegAt::Lookup(position), leg), (lookup.onto(), onto)] {
                if !leg_has_key(on, &link) {
                    return Err(FederatedPlanError::KeyNotOnLeg { side, label: link });
                }
            }
        }
        // A second fact leg arrives only for a cross-model ratio (`telekom/sutura#780`). It is a
        // `Fact` of its own - a second statement, so the two facts never share a `FROM`, which is
        // what makes the chasm trap impossible; the source it reads may be the first fact's, since
        // two statements on one source are still two statements. The two must share a key label -
        // the column they join on above. Without one the join is impossible, which is the chasm
        // trap made a type refusal rather than a NULL-padded row.
        let link = InternalLabel::Link(0).label();
        if let Some(second) = &second_fact {
            if !matches!(second, LegPlan::Fact { .. }) {
                return Err(FederatedPlanError::NotFact {
                    source_name: second.source().clone(),
                });
            }
            let shared = fact
                .keys()
                .iter()
                .filter(|k1| second.keys().iter().any(|k2| k1.label() == k2.label()))
                .count();
            if shared == 0 {
                return Err(FederatedPlanError::FactsShareNoKey);
            }
            if !leg_has_key(second, &link) {
                return Err(FederatedPlanError::KeyNotOnLeg {
                    side: LegAt::Fact,
                    label: link,
                });
            }
            // Each fact groups by the link and nothing else. A key only one fact carries would
            // group that fact alone, fan the other's rows out across it, and answer a null key for
            // every row only the other fact reached - the chasm trap and a NULL-padded dimension.
            if let Some(extra) = fact.keys().iter().chain(second.keys()).find(|key| key.label() != link) {
                return Err(FederatedPlanError::FactKeyNotShared {
                    label: String::from(extra.label()),
                });
            }
        }
        for key in &keys {
            if !leg_at(key.side()).is_some_and(|leg| leg_has_key(leg, key.label())) {
                return Err(FederatedPlanError::KeyNotOnLeg {
                    side: key.side(),
                    label: String::from(key.label()),
                });
            }
        }
        // D9: `bucket` and `fact`'s own embedded bucket and terms are three independently supplied
        // arguments, and the one production splitter (`sutura_semantic::plan::federated_plan`)
        // derives all three from the same local values - the bucket by cloning one `PlanBucket`, the
        // terms by zipping `federation.carried()` with `labels(&federation)`. Checked here so a
        // future producer that stops doing that fails at construction rather than combining under a
        // bucket the fact leg never grouped by, or a term the federation never asked for.
        // `clippy::unreachable` refuses the macro here, so the `else` arm is the same refusal
        // `is_fact` above already returned for this exact shape - a second `NotFact` rather than a
        // panic, for a branch the type still has to answer even though nothing can reach it.
        let LegPlan::Fact {
            bucket: fact_bucket,
            terms: fact_terms,
            ..
        } = &fact
        else {
            return Err(FederatedPlanError::NotFact {
                source_name: fact.source().clone(),
            });
        };
        if fact_bucket != &bucket {
            return Err(FederatedPlanError::BucketMismatch);
        }
        // The first fact leg carries the leaves whose model is `None` (the metric's own - a
        // splitter erases an explicit own model through `Federation::of_metric`), the second fact
        // leg the rest. Checked with or without a second leg, so a leaf naming another model can
        // never be folded into the first leg's statement: that would put two fact tables under
        // one `FROM`, the chasm trap this plan exists to make impossible.
        let all_labels: Vec<InternalLabel> = labels(&federation);
        let (first, second): (Vec<_>, Vec<_>) = federation
            .carried()
            .into_iter()
            .zip(all_labels.iter())
            .partition(|(leaf, _)| leaf.model().is_none());
        // A pulled-up leaf is a key of the first fact leg under its own label, not a term: the
        // second fact leg of a cross-model ratio carries terms only (the catalog refuses a distinct
        // count in one at load), and a leaf the first leg does not project cannot be counted above.
        if (second_fact.is_none() && !second.is_empty()) || second.iter().any(|(leaf, _)| leaf.is_pulled_up()) {
            return Err(FederatedPlanError::TermsDoNotMatchFederation);
        }
        for (_, label) in first.iter().filter(|(leaf, _)| leaf.is_pulled_up()) {
            let label = label.label();
            if !leg_has_key(&fact, &label) {
                return Err(FederatedPlanError::KeyNotOnLeg {
                    side: LegAt::Fact,
                    label,
                });
            }
        }
        let first_expected: Vec<String> = first
            .iter()
            .filter(|(leaf, _)| !leaf.is_pulled_up())
            .map(|(_, l)| l.label())
            .collect();
        let second_expected: Vec<String> = second.iter().map(|(_, l)| l.label()).collect();
        let first_matches = fact_terms.len() == first_expected.len()
            && fact_terms
                .iter()
                .zip(&first_expected)
                .all(|(term, expected_label)| term.label() == expected_label);
        if !first_matches {
            return Err(FederatedPlanError::TermsDoNotMatchFederation);
        }
        if let Some(second) = &second_fact {
            let LegPlan::Fact {
                bucket: second_bucket,
                terms: second_terms,
                ..
            } = second
            else {
                return Err(FederatedPlanError::NotFact {
                    source_name: second.source().clone(),
                });
            };
            // Both facts bucket through ONE column, so the combiner's join on the bucket label
            // compares like with like.
            if second_bucket != &bucket {
                return Err(FederatedPlanError::BucketMismatch);
            }
            let second_matches = second_terms.len() == second_expected.len()
                && second_terms
                    .iter()
                    .zip(&second_expected)
                    .all(|(term, expected_label)| term.label() == expected_label);
            if !second_matches {
                return Err(FederatedPlanError::TermsDoNotMatchFederation);
            }
        }
        Ok(Self {
            metric,
            measure_label,
            bucket,
            fact,
            second_fact,
            lookups,
            federation,
            keys,
            top: None,
            link_comparison: None,
        })
    }

    /// Attaches the federated `top` - `github.com/telekom/sutura#777` - so
    /// the combiner's caller knows the answer still needs ranking and truncating
    /// after the legs are joined.
    ///
    /// A builder rather than a constructor argument, for
    /// [`QueryPlan::with_top`](crate::plan::QueryPlan::with_top)'s reason: every existing caller of
    /// [`Self::new`] keeps its argument list, and a plan built without it is byte-for-byte one
    /// built before this field existed.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: Top) -> Self {
        self.top = Some(top);
        self
    }

    /// Case 2's `top`, if this plan carries one. See [`Self::with_top`].
    #[inline]
    pub const fn top(&self) -> Option<Top> {
        self.top
    }

    /// Records the comparison the link's relationship declares, for the combiner to read.
    ///
    /// A builder rather than a constructor argument, so [`Self::new`] keeps its argument list.
    #[inline]
    #[must_use]
    pub const fn with_link_comparison(mut self, comparison: Option<LinkKind>) -> Self {
        self.link_comparison = comparison;
        self
    }

    /// The comparison the first link declares, or `None` for equality of kind.
    ///
    /// The combiner reads it for the first lookup leg's link and the second fact leg's, so whatever
    /// writes it names the first lookup leg's relationship.
    #[inline]
    pub const fn link_comparison(&self) -> Option<LinkKind> {
        self.link_comparison
    }

    /// Every leg, each once: the fact leg, the second fact leg if present, then each lookup leg in
    /// plan order.
    ///
    /// The one list of a plan's legs. The cut, the credential request, the posture record and the
    /// legs a combiner runs all read it, so no lookup leg can be left out of one of them.
    pub fn legs(&self) -> Vec<&LegPlan> {
        core::iter::once(&self.fact)
            .chain(self.second_fact.as_ref())
            .chain(self.lookups.legs().iter().map(LookupLeg::leg))
            .collect()
    }

    /// The fact leg.
    pub const fn fact(&self) -> &LegPlan {
        &self.fact
    }

    /// The second fact leg, when the measure's ratio terms name two fact models.
    pub const fn second_fact(&self) -> Option<&LegPlan> {
        self.second_fact.as_ref()
    }

    /// The lookup legs, each with the leg it joins onto and its own join direction.
    pub const fn lookups(&self) -> &LookupLegs {
        &self.lookups
    }

    /// The metric this answer is measured in.
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    /// Every data system this plan reads from, in execution order.
    pub fn sources(&self) -> impl Iterator<Item = &crate::model::SourceName> + '_ {
        self.legs().into_iter().map(LegPlan::source)
    }

    /// The answer's group-by keys, in question order.
    pub fn keys(&self) -> &[AnswerKey] {
        &self.keys
    }

    /// The time bucket the answer groups by: its label, grain and column.
    #[inline]
    pub const fn bucket(&self) -> &PlanBucket {
        &self.bucket
    }

    /// The label the fact leg projected its time bucket under, which the answer groups by.
    ///
    /// **Four accessors arrived with the combiner port and this is the first of them.** The combine
    /// used to be a method here and read these fields directly; an implementor above this crate
    /// cannot, so the plan publishes what a combine needs and nothing more. What is deliberately
    /// NOT published is anything a combiner could use to invent a column: every one of these is a
    /// label the splitter already assigned or a tree it already built.
    #[inline]
    pub fn bucket_label(&self) -> &str {
        self.bucket.label()
    }

    /// The label the answer's measure is emitted under - the metric's own certified name.
    #[inline]
    pub fn measure_label(&self) -> &str {
        self.measure_label.as_str()
    }

    /// The combine tree above the legs, and the metric that names its leaves.
    ///
    /// Every leaf it carries has a re-aggregating function, because [`Self::new`] refused a plan
    /// whose leaf did not - so an implementor's own unsupported-aggregate arm is unreachable
    /// through this constructor rather than absent.
    #[inline]
    pub const fn federation(&self) -> &Federation {
        &self.federation
    }
}

/// Why a federated plan could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FederatedPlanError {
    /// The leg meant to be the fact leg is not a [`LegPlan::Fact`].
    #[error("the fact leg reads `{source_name}`, which is not a fact leg")]
    NotFact { source_name: SourceName },
    /// The leg meant to be the lookup leg is not a [`LegPlan::Lookup`].
    #[error("the lookup leg reads `{source_name}`, which is not a lookup leg")]
    NotLookup { source_name: SourceName },
    /// A lookup leg and the leg it joins onto name the same data system, which is one statement and
    /// not two legs.
    #[error("a lookup leg and the leg it joins onto both read from `{source_name}`")]
    SameSource { source_name: SourceName },
    /// An answer key names a column the leg it belongs to does not project.
    #[error("the {side:?} leg projects no key `{label}`")]
    KeyNotOnLeg { side: LegAt, label: String },
    /// The bucket handed to this constructor is not the fact leg's own.
    ///
    /// Unreachable through the one production splitter, which builds both from one local value -
    /// see [`FederatedPlan::new`]'s own comment for why this is checked anyway.
    #[error("the plan's bucket does not match the fact leg's own bucket")]
    BucketMismatch,
    /// The fact leg's terms do not name the labels its federation expects, in order.
    ///
    /// Same reason as [`BucketMismatch`](Self::BucketMismatch): the one production splitter derives
    /// both from `labels(&federation)` in one pass.
    #[error("the fact leg's terms do not match the labels its federation expects")]
    TermsDoNotMatchFederation,
    /// Two fact legs share no key label, so the join above them is impossible.
    ///
    /// The chasm-trap guard as a type refusal: without a shared dimension key to join on, a
    /// combined answer is not a certified number but two unrelated row sets, so the plan does not
    /// exist rather than producing one. **Reached only by direct construction:** the one
    /// production splitter projects `InternalLabel::Link` on both fact legs unconditionally.
    #[error("the two fact legs share no key, so no join is possible")]
    FactsShareNoKey,
    /// A two-fact plan whose fact legs group by a key besides the link (`telekom/sutura#780`).
    ///
    /// Reached only by direct construction: `plan()` refuses a question grouped by a dimension
    /// only one fact reaches as `CrossModelRatioWithoutSharedDimension`, and this holds the same
    /// shape as a type.
    #[error("a fact leg of a two-fact plan groups by `{label}`, which is not the link both facts share")]
    FactKeyNotShared { label: String },
}

/// Whether a [`LegPlan`] projects a key under `label`.
fn leg_has_key(leg: &LegPlan, label: &str) -> bool {
    leg.keys().iter().any(|key| key.label() == label)
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the accessor impl and the RANK, which is a property of the answer's own column order \
              rather than of a plan's fields, span one type and are kept apart for readability"
)]
impl FederatedPlan {
    /// Case 2's rank - `github.com/telekom/sutura#777`: a combiner already sorted `combined`
    /// ascending by its own key cells, nulls last; this re-sorts it by `top`'s own criterion,
    /// stably, so two rows tied on that criterion keep the key order they already have, and then
    /// keeps `top.n()` of them.
    ///
    /// **The column position, not a label lookup.** [`FederationCombiner::combine`]'s own doc states the answer's
    /// column order - every key, then the bucket, then the measure - so [`TopBy::Metric`] is the
    /// last column and [`TopBy::Period`] the one before it, by construction rather than by name.
    /// That is a property of every [`FederatedPlan`]'s own combined answer rather than of one
    /// instance's fields, which is why this takes no `&self`: it is associated with the type
    /// whose contract it reads, not with a value of it.
    ///
    /// **Nulls sort last regardless of [`TopDirection`]**, the same contract
    /// `sutura_sql::generate`'s own `ordered_nulls_last` states for the rendered path: a null means
    /// there was nothing to rank, and that sorts after every value either way.
    pub fn rank(combined: &RowSet, top: Top) -> Result<RowSet, MalformedRowSet> {
        let columns = combined.columns().to_vec();
        let mut rows = combined.rows().to_vec();
        // Measure is the last column, bucket the one before it - see this method's own doc.
        let primary = match top.by() {
            TopBy::Metric => columns.len().saturating_sub(1),
            TopBy::Period => columns.len().saturating_sub(2),
        };
        let desc = matches!(top.direction(), TopDirection::Desc);
        rows.sort_by(|a, b| {
            let (Some(a_cell), Some(b_cell)) = (a.get(primary), b.get(primary)) else {
                return std::cmp::Ordering::Equal;
            };
            rank_order(a_cell, b_cell, desc)
        });
        rows.truncate(usize::try_from(top.n().get()).unwrap_or(usize::MAX));
        RowSet::new(columns, rows)
    }
}

/// A total order over key cells: the mono path's null placement and its numeric order, never its
/// rendered text.
///
/// **What "matching the mono path" reaches, stated before the argument for it.** The null placement
/// and the by-value numeric order are the contract, and both are asserted. **Text is not:** this
/// compares `&str` by bytes, while the mono path's text order is whatever **collation** the serving
/// data system applies - `docs/adr/0012` records that collation as *unstated by the plan* and
/// *differing per system*, which is why its own conformance packs re-sort text by bytes rather than
/// trusting a source's locale. So for a text key on a target whose collation is not byte order -
/// `Postgres` under a non-`C` locale orders `Business` after `business`, byte order puts it before -
/// one certified metric still comes back in one order from one data system and another from two.
/// That is the class of defect the null half of this comparator closes, surviving for text keys.
/// `telekom/sutura#92` scoped itself out of it explicitly - its *not in scope* is *"ordering
/// stability where the question itself does not determine an order. This is only about null placement
/// within an order the plan already asks for."* - so it is a limit rather than a regression, and
/// closing it means declaring a text collation per dialect the way `Dialect::identifier_case` is
/// declared.
///
/// **`ASC NULLS LAST`, which is the whole of the ordered-result contract and not this file's
/// choice.** A whole-answer plan emits `ORDER BY <key> ASC NULLS LAST` -
/// `sutura_sql::generate::ordered_nulls_last`, which `telekom/sutura#92` decided after a live run
/// found the dialects disagreeing about null placement, and which makes every target converge
/// on the engine's own order. This comparator ranked a null FIRST, so one certified metric came back
/// in one order from one data system and in another order from two, with no golden able to see it -
/// a golden pins statement text, and this path emits none. The placement is stated in both places
/// for the same reason it is stated in the SQL: a default is not a contract.
///
/// Integers order by value, then reals by value, then text by bytes, and a null after all of them -
/// so a numeric column is ordered numerically (`9` before `10`) and not by its string form
/// (`"10"` before `"9"`). Cells of different scalar types never compare equal. A result column in a
/// data system has one logical type, so the cross-type ranks decide nothing an `ORDER BY` decides;
/// what they buy is a TOTAL order, which is what makes the sort deterministic for a column
/// [`RowSet`] permits to be mixed.
fn compare_cells(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    const fn rank(value: &Value) -> u8 {
        match value {
            Value::Integer(_) => 0,
            Value::Real(_) => 1,
            Value::Text(_) => 2,
            // Last, and the one rank that is a contract rather than a tie-break. See above.
            Value::Null => 3,
        }
    }
    let order = rank(a).cmp(&rank(b));
    if order != Equal {
        return order;
    }
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.get().total_cmp(&y.get()),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        // The sole same-rank pair not caught above is Null/Null, and different ranks returned early.
        _ => Equal,
    }
}

/// [`FederatedPlan::rank`]'s own comparator: a `top` orders by VALUE in the requested direction,
/// but a null still sorts last regardless of it - the same split
/// `sutura_sql::generate::top::ordering` keeps between the primary key (which `desc` reverses) and
/// `NULLS LAST` (which it does not).
fn rank_order(a: &Value, b: &Value, desc: bool) -> std::cmp::Ordering {
    match (matches!(a, Value::Null), matches!(b, Value::Null)) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => {
            let order = compare_cells(a, b);
            if desc { order.reverse() } else { order }
        }
    }
}

#[cfg(test)]
mod tests;

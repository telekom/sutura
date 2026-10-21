//! The drill-across plan: several metrics, each answered by its own arm, joined above on the
//! answer's keys and time bucket.
//!
//! **No new leg shape.** A metric that reaches a remote dimension is an [`Arm::Joined`], the
//! [`FederatedPlan`] the splitter already builds; a metric whose dimensions are all on its own source
//! is an [`Arm::Local`], one fact leg grouped by the answer's keys and the bucket. Every arm is
//! aggregated on its own, so no leg reads two fact tables and the chasm trap stays impossible.
//!
//! **What this type holds, and what it does not.** It holds the arms and refuses a set of arms that
//! cannot be joined: two arms whose keys, bucket or time range differ, one metric twice, an arm
//! carrying a `top`, or more legs than [`MAX_LEGS`]. A `top` cuts its arm before the join, so that arm
//! answers a null for each group it cut that another arm has, and the answer reads that null as "no
//! rows"; a `top` for the whole question, for example by period after the join, is not built here.
//! The plan names no join and no engine; the combine is a port's concern. `sutura_semantic` builds
//! it for a question over several metrics that cannot share one statement.
//!
//! **The cap is a property of the plan.** Each leg's result is held whole before the combine, so the
//! memory a question can hold grows with its leg count. [`MAX_LEGS`] bounds it at construction. The
//! refusal a caller sees, [`RefusalReason::QuestionNeedsTooManyLegs`](crate::query::RefusalReason),
//! is raised by the planner when [`DrillAcrossPlan::new`] refuses a plan over the cap.
//!
//! `docs/adr/20261018144926-drill-across-for-several-metrics` records the decision and its limits.

mod merge;

pub use merge::{ArmLegs, DrillAcrossCombiner};

use crate::calendar::TimeRange;
use crate::federation::Federation;
use crate::model::{Grain, MetricName, SourceName};
use crate::nonempty::NonEmpty;
use crate::plan::federated::{FederatedPlan, labels};
use crate::plan::leg::LegPlan;
use crate::plan::{AnswerKey, PlanBucket, ResultLabel};

/// The most legs one question may run, over every arm.
pub const MAX_LEGS: usize = 6;

/// One metric's share of a drill-across question.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Arm {
    /// A metric that reaches a remote dimension: a fact leg beside a lookup leg.
    ///
    /// Boxed with the local arm: both are hundreds of bytes, and an enum is as large as its largest.
    Joined(Box<FederatedPlan>),
    /// A metric whose dimensions are all on its own source.
    Local(Box<LocalArm>),
}

/// A metric answered by one fact leg, which no lookup leg joins.
///
/// Each answer key, and each distinct count's leaf key, is on the leg. The leg can group by more
/// keys, so the combine aggregates it to the answer's keys and bucket. Its metric, bucket and
/// measure label are read off the leg and the metric rather than passed beside them, so no argument
/// can disagree with the leg.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LocalArm {
    metric: MetricName,
    measure_label: ResultLabel,
    bucket: PlanBucket,
    fact: LegPlan,
    federation: Federation,
    keys: Vec<ResultLabel>,
}

impl LocalArm {
    /// An arm over `fact`, whose leaves `federation` names and whose answer keys are `keys`.
    ///
    /// # Errors
    ///
    /// [`DrillAcrossPlanError::NotFact`] for a leg that is not a fact leg;
    /// [`DrillAcrossPlanError::TermsDoNotMatchFederation`] for terms that are not the federation's
    /// labels in order, or a leaf that names another model - a second fact is a [`Arm::Joined`]'s;
    /// [`DrillAcrossPlanError::KeyNotOnLeg`] for an answer key, or a pulled-up leaf's key, that the
    /// leg does not project.
    pub fn new(fact: LegPlan, federation: Federation, keys: Vec<ResultLabel>) -> Result<Self, DrillAcrossPlanError> {
        let LegPlan::Fact {
            metric,
            bucket,
            terms,
            keys: leg_keys,
            ..
        } = &fact
        else {
            return Err(DrillAcrossPlanError::NotFact {
                source_name: fact.source().clone(),
            });
        };
        let leaves = federation.carried();
        let leaf_labels = labels(&federation);
        let mismatch = || DrillAcrossPlanError::TermsDoNotMatchFederation { metric: metric.clone() };
        if leaves.iter().any(|leaf| leaf.model().is_some()) {
            return Err(mismatch());
        }
        let terms_expected = leaves
            .iter()
            .zip(&leaf_labels)
            .filter(|(leaf, _)| !leaf.is_pulled_up())
            .map(|(_, label)| label.label());
        if !terms.iter().map(|term| String::from(term.label())).eq(terms_expected) {
            return Err(mismatch());
        }
        let pulled_up = leaves
            .iter()
            .zip(&leaf_labels)
            .filter(|(leaf, _)| leaf.is_pulled_up())
            .map(|(_, label)| label.label());
        for label in pulled_up.chain(keys.iter().map(|key| String::from(key.as_str()))) {
            if !leg_keys.iter().any(|key| key.label() == label) {
                return Err(DrillAcrossPlanError::KeyNotOnLeg {
                    metric: metric.clone(),
                    label,
                });
            }
        }
        Ok(Self {
            measure_label: ResultLabel::measure(metric),
            metric: metric.clone(),
            bucket: bucket.clone(),
            federation,
            keys,
            fact,
        })
    }

    /// The fact leg.
    #[inline]
    pub const fn fact(&self) -> &LegPlan {
        &self.fact
    }

    /// The combine tree above the leg, and the metric that names its leaves.
    #[inline]
    pub const fn federation(&self) -> &Federation {
        &self.federation
    }

    /// The label the answer's measure is emitted under.
    #[inline]
    pub fn measure_label(&self) -> &str {
        self.measure_label.as_str()
    }
}

impl Arm {
    /// The metric this arm measures.
    pub fn metric(&self) -> &MetricName {
        match self {
            Self::Joined(plan) => plan.metric(),
            Self::Local(arm) => &arm.metric,
        }
    }

    /// The fact leg, which is [`Arm::legs`]' first.
    pub fn fact(&self) -> &LegPlan {
        match self {
            Self::Joined(plan) => plan.fact(),
            Self::Local(arm) => &arm.fact,
        }
    }

    /// The label this arm's measure is emitted under.
    pub fn measure_label(&self) -> &str {
        match self {
            Self::Joined(plan) => plan.measure_label(),
            Self::Local(arm) => arm.measure_label(),
        }
    }

    /// The answer's group-by labels in question order.
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Self::Joined(plan) => plan.keys().iter().map(AnswerKey::label).collect(),
            Self::Local(arm) => arm.keys.iter().map(ResultLabel::as_str).collect(),
        }
    }

    /// The time bucket this arm groups by.
    pub fn bucket(&self) -> &PlanBucket {
        match self {
            Self::Joined(plan) => plan.bucket(),
            Self::Local(arm) => &arm.bucket,
        }
    }

    /// Every leg this arm runs, in the order `execute_across` runs them: a joined arm's
    /// [`FederatedPlan::legs`].
    pub fn legs(&self) -> Vec<&LegPlan> {
        match self {
            Self::Joined(plan) => plan.legs(),
            Self::Local(arm) => vec![&arm.fact],
        }
    }
}

/// Two or more [`Arm`]s that share one set of keys, one bucket grain and one time range.
///
/// The keys, the bucket and the range are the first arm's, and every later arm is checked against
/// them, so the plan holds no copy that could disagree with an arm. A fact leg over another range
/// would leave the full join a null for a period it never read, which the answer reads as "no rows".
/// Each arm's time column may differ: only the label and grain must agree, and what an Arrow kind
/// mismatch between two buckets means is the combiner's to refuse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DrillAcrossPlan {
    arms: NonEmpty<Arm>,
}

impl DrillAcrossPlan {
    /// A plan over `first`, `second` and every arm of `rest`.
    ///
    /// Two arms are the least there is to join, so the signature takes two rather than a list that
    /// could hold fewer.
    ///
    /// # Errors
    ///
    /// [`DrillAcrossPlanError`] for one metric in two arms, an arm carrying a `top`, an arm whose
    /// keys, bucket or time range differ from the first arm's, or more than [`MAX_LEGS`] legs.
    pub fn new(first: Arm, second: Arm, rest: Vec<Arm>) -> Result<Self, DrillAcrossPlanError> {
        let every = || [&first, &second].into_iter().chain(&rest);
        let legs: usize = every().map(|arm| arm.legs().len()).sum();
        if legs > MAX_LEGS {
            return Err(DrillAcrossPlanError::TooManyLegs { legs, limit: MAX_LEGS });
        }
        let (keys, bucket) = (first.keys(), first.bucket());
        let range = ranges(&first).next();
        for (index, arm) in every().enumerate() {
            let metric = arm.metric();
            if let Arm::Joined(plan) = arm
                && plan.top().is_some()
            {
                return Err(DrillAcrossPlanError::ArmCarriesTop { metric: metric.clone() });
            }
            if every().take(index).any(|earlier| earlier.metric() == metric) {
                return Err(DrillAcrossPlanError::DuplicateMetric { metric: metric.clone() });
            }
            if arm.keys() != keys {
                return Err(DrillAcrossPlanError::KeysDiffer { metric: metric.clone() });
            }
            if (arm.bucket().label(), arm.bucket().grain()) != (bucket.label(), bucket.grain()) {
                return Err(DrillAcrossPlanError::BucketDiffers { metric: metric.clone() });
            }
            if ranges(arm).any(|leg_range| Some(leg_range) != range) {
                return Err(DrillAcrossPlanError::RangeDiffers { metric: metric.clone() });
            }
        }
        let mut tail = vec![second];
        tail.extend(rest);
        Ok(Self {
            arms: NonEmpty::of(first, tail),
        })
    }

    /// The arms, in question order of their metrics.
    #[inline]
    pub const fn arms(&self) -> &NonEmpty<Arm> {
        &self.arms
    }

    /// The group-by labels every arm answers under, in question order.
    pub fn keys(&self) -> Vec<&str> {
        self.arms.first().keys()
    }

    /// The label the time bucket is emitted under.
    pub fn bucket_label(&self) -> &str {
        self.arms.first().bucket().label()
    }

    /// The grain every arm buckets by.
    pub fn grain(&self) -> Grain {
        self.arms.first().bucket().grain()
    }

    /// Every leg of every arm, arm by arm.
    pub fn legs(&self) -> Vec<&LegPlan> {
        self.arms.iter().flat_map(Arm::legs).collect()
    }

    /// Every data system a leg reads, once per leg.
    pub fn sources(&self) -> impl Iterator<Item = &SourceName> + '_ {
        self.legs().into_iter().map(LegPlan::source)
    }
}

/// The time range of every fact leg of `arm`.
fn ranges(arm: &Arm) -> impl Iterator<Item = &TimeRange> {
    arm.legs().into_iter().filter_map(|leg| match leg {
        LegPlan::Fact { range, .. } => Some(range),
        LegPlan::Lookup { .. } => None,
    })
}

/// Why a drill-across plan, or one of its arms, could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DrillAcrossPlanError {
    /// The leg of a local arm is not a fact leg.
    #[error("the local arm's leg reads `{source_name}`, which is not a fact leg")]
    NotFact { source_name: SourceName },
    /// A local arm's leg does not project a key the arm needs.
    #[error("the leg of `{metric}` projects no key `{label}`")]
    KeyNotOnLeg { metric: MetricName, label: String },
    /// A local arm's terms are not its federation's leaves.
    #[error("the leg of `{metric}` carries terms that do not match its federation")]
    TermsDoNotMatchFederation { metric: MetricName },
    /// A joined arm carries a `top`, which a drill-across question refuses: it cuts that arm before
    /// the join, so that arm answers a null for each group it cut that another arm has, and the
    /// answer reads that null as "no rows".
    #[error(
        "the arm of `{metric}` carries a top, which cuts that arm before the join and leaves it a null for each group it cut"
    )]
    ArmCarriesTop { metric: MetricName },
    /// One metric is measured by two arms, which would emit one measure column twice.
    #[error("`{metric}` is measured by more than one arm")]
    DuplicateMetric { metric: MetricName },
    /// An arm answers under other keys, or in another order, than the first arm.
    #[error("the arm of `{metric}` groups by other keys than the first arm")]
    KeysDiffer { metric: MetricName },
    /// An arm buckets by another label or grain than the first arm.
    #[error("the arm of `{metric}` buckets by another label or grain than the first arm")]
    BucketDiffers { metric: MetricName },
    /// A fact leg reads another time range than the first arm's.
    #[error("a fact leg of `{metric}` reads another time range than the first arm")]
    RangeDiffers { metric: MetricName },
    /// The arms run more legs than one question may hold.
    #[error("the arms run {legs} legs, and a question may run at most {limit}")]
    TooManyLegs { legs: usize, limit: usize },
}

#[cfg(test)]
mod tests {
    //! What a [`DrillAcrossPlan`] and a [`LocalArm`] refuse to BE.
    //!
    //! Each refusal has a cell that builds the one shape the refusal names, and the negative control at
    //! the top builds the same arms with that shape removed.

    use crate::calendar::{Date, TimeRange};
    use crate::federation::Federation;
    use crate::measure::{AggregatedColumn, Measure, Term};
    use crate::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
    use crate::plan::leg::{LegPlan, LegTerm};
    use crate::plan::{
        AnswerKey, Arm, DrillAcrossPlan, DrillAcrossPlanError, FederatedPlan, InternalLabel, LocalArm, LookupLegs, MAX_LEGS,
        PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel, StatementTables, labels,
    };
    use crate::query::{Top, TopBy, TopDirection, TopN};

    const FACT: &str = "fct_usage";

    fn metric(name: &str) -> MetricName {
        MetricName::parse(name).expect("a test metric is a metric")
    }

    fn column(name: &str) -> ColumnName {
        ColumnName::parse(name).expect("a test column is a column")
    }

    fn table(name: &str) -> TableName {
        TableName::parse(name).expect("a test table is a table")
    }

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source is a source")
    }

    fn dimension_label(name: &str) -> ResultLabel {
        ResultLabel::dimension(&DimensionName::parse(name).expect("a test dimension is a dimension"))
    }

    fn key(label: ResultLabel, physical: &str) -> PlanKey {
        PlanKey::new(label, PlanColumn::new(table(FACT), column(physical)))
    }

    fn bucket(grain: Grain) -> PlanBucket {
        PlanBucket::new(ResultLabel::bucket(), grain, PlanColumn::new(table(FACT), column("month")))
    }

    fn sum(name: &str) -> Federation {
        Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            column(name),
        ))))
    }

    fn distinct() -> Federation {
        Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::CountDistinct,
            column("customer_key"),
        ))))
    }

    /// One placeholder term per leaf the leg carries as a term, labelled as the federation expects.
    fn terms_for(federation: &Federation) -> Vec<LegTerm> {
        labels(federation)
            .into_iter()
            .zip(federation.carried())
            .filter(|&(_, leaf)| !leaf.is_pulled_up())
            .map(|(label, _)| {
                LegTerm::new(
                    PlanTerm::CountIf {
                        column: PlanColumn::new(table(FACT), column("amount")),
                    },
                    ResultLabel::internal(label),
                )
            })
            .collect()
    }

    fn fact_leg(name: &str, grain: Grain, keys: Vec<PlanKey>, terms: Vec<LegTerm>) -> LegPlan {
        LegPlan::Fact {
            source: source("facts"),
            metric: metric(name),
            tables: StatementTables::only(table(FACT)),
            bucket: bucket(grain),
            keys,
            terms,
            bindings: PlanBindings::none(),
            range: TimeRange::new(
                Date::parse("2026-01-01").expect("a test start"),
                Date::parse("2026-02-01").expect("a test end"),
            )
            .expect("a test range"),
        }
    }

    /// `leg` with its time range replaced.
    fn ranged(mut leg: LegPlan, start: &str, end: &str) -> LegPlan {
        if let LegPlan::Fact { range: leg_range, .. } = &mut leg {
            *leg_range = TimeRange::new(
                Date::parse(start).expect("a test start"),
                Date::parse(end).expect("a test end"),
            )
            .expect("a test range");
        }
        leg
    }

    fn link() -> PlanKey {
        key(ResultLabel::internal(InternalLabel::Link(0)), "customer_key")
    }

    /// A metric reaching `region` on another source: answers by `product_family`, then `region`.
    fn joined_grained(name: &str, grain: Grain) -> Arm {
        let federation = sum("amount");
        let lookup = LegPlan::Lookup {
            source: source("geo"),
            table: table("dim_region").into(),
            keys: vec![link(), key(dimension_label("region"), "region")],
            bindings: PlanBindings::none(),
        };
        let plan = FederatedPlan::new(
            metric(name),
            ResultLabel::measure(&metric(name)),
            bucket(grain),
            fact_leg(
                name,
                grain,
                vec![key(dimension_label("product_family"), "product_family"), link()],
                terms_for(&federation),
            ),
            None,
            LookupLegs::one(lookup, true),
            federation,
            vec![
                AnswerKey::fact(dimension_label("product_family")),
                AnswerKey::lookup(0, dimension_label("region")),
            ],
        )
        .expect("a fact leg beside a lookup leg is a federated plan");
        Arm::Joined(Box::new(plan))
    }

    fn joined(name: &str) -> Arm {
        joined_grained(name, Grain::Month)
    }

    fn answer_keys() -> Vec<ResultLabel> {
        vec![dimension_label("product_family"), dimension_label("region")]
    }

    fn local_leg(name: &str, grain: Grain, federation: &Federation) -> LegPlan {
        fact_leg(
            name,
            grain,
            vec![
                key(dimension_label("product_family"), "product_family"),
                key(dimension_label("region"), "region"),
            ],
            terms_for(federation),
        )
    }

    fn local_arm(name: &str) -> LocalArm {
        let federation = sum("amount");
        LocalArm::new(local_leg(name, Grain::Month, &federation), federation, answer_keys()).expect("a local arm builds")
    }

    fn local(name: &str) -> Arm {
        Arm::Local(Box::new(local_arm(name)))
    }

    /// A metric reaching `region` through a second fact leg: both facts group by the link only, so
    /// the answer's one key is the lookup's.
    fn joined_over_two_facts(name: &str, second_start: &str, second_end: &str) -> Arm {
        let federation = sum("amount");
        let lookup = LegPlan::Lookup {
            source: source("geo"),
            table: table("dim_region").into(),
            keys: vec![link(), key(dimension_label("region"), "region")],
            bindings: PlanBindings::none(),
        };
        let second = ranged(
            fact_leg(name, Grain::Month, vec![link()], Vec::new()),
            second_start,
            second_end,
        );
        let plan = FederatedPlan::new(
            metric(name),
            ResultLabel::measure(&metric(name)),
            bucket(Grain::Month),
            fact_leg(name, Grain::Month, vec![link()], terms_for(&federation)),
            Some(second),
            LookupLegs::one(lookup, true),
            federation,
            vec![AnswerKey::lookup(0, dimension_label("region"))],
        )
        .expect("two facts beside a lookup leg are a federated plan");
        Arm::Joined(Box::new(plan))
    }

    fn top() -> Top {
        Top::new(
            TopN::parse(3).expect("three is a row count"),
            TopBy::Metric,
            TopDirection::Desc,
        )
    }

    #[test]
    fn a_joined_arm_and_a_local_arm_over_one_set_of_keys_construct() {
        let plan = DrillAcrossPlan::new(joined("revenue"), local("usage"), Vec::new()).expect("two arms over one key set");
        assert_eq!(plan.arms().len(), 2);
        assert_eq!(plan.keys(), ["product_family", "region"]);
        assert_eq!(plan.bucket_label(), ResultLabel::bucket().as_str());
        assert_eq!(plan.grain(), Grain::Month);
        assert_eq!(plan.legs().len(), 3);
        assert_eq!(
            plan.arms().iter().map(Arm::measure_label).collect::<Vec<_>>(),
            ["revenue", "usage"]
        );
        assert_eq!(
            plan.sources().map(SourceName::as_str).collect::<Vec<_>>(),
            ["facts", "geo", "facts"]
        );
    }

    #[test]
    fn one_metric_in_two_arms_does_not_construct() {
        let plan = DrillAcrossPlan::new(joined("revenue"), local("revenue"), Vec::new());
        assert_eq!(
            plan,
            Err(DrillAcrossPlanError::DuplicateMetric {
                metric: metric("revenue")
            })
        );
    }

    #[test]
    fn an_arm_with_another_set_of_keys_does_not_construct() {
        let federation = sum("amount");
        let fewer = LocalArm::new(
            fact_leg(
                "usage",
                Grain::Month,
                vec![key(dimension_label("product_family"), "product_family")],
                terms_for(&federation),
            ),
            federation,
            vec![dimension_label("product_family")],
        )
        .expect("a local arm over one key");
        let plan = DrillAcrossPlan::new(joined("revenue"), Arm::Local(Box::new(fewer)), Vec::new());
        assert_eq!(plan, Err(DrillAcrossPlanError::KeysDiffer { metric: metric("usage") }));
    }

    #[test]
    fn an_arm_with_the_same_keys_in_another_order_does_not_construct() {
        let federation = sum("amount");
        let swapped = LocalArm::new(
            local_leg("usage", Grain::Month, &federation),
            federation,
            vec![dimension_label("region"), dimension_label("product_family")],
        )
        .expect("a local arm over the same keys");
        let plan = DrillAcrossPlan::new(joined("revenue"), Arm::Local(Box::new(swapped)), Vec::new());
        assert_eq!(plan, Err(DrillAcrossPlanError::KeysDiffer { metric: metric("usage") }));
    }

    #[test]
    fn an_arm_with_another_grain_does_not_construct() {
        let federation = sum("amount");
        let weekly =
            LocalArm::new(local_leg("usage", Grain::Week, &federation), federation, answer_keys()).expect("a weekly local arm");
        let plan = DrillAcrossPlan::new(joined("revenue"), Arm::Local(Box::new(weekly)), Vec::new());
        assert_eq!(plan, Err(DrillAcrossPlanError::BucketDiffers { metric: metric("usage") }));
        let weekly_joined = DrillAcrossPlan::new(joined("revenue"), joined_grained("visits", Grain::Week), Vec::new());
        assert_eq!(
            weekly_joined,
            Err(DrillAcrossPlanError::BucketDiffers {
                metric: metric("visits")
            })
        );
    }

    #[test]
    fn an_arm_with_another_bucket_label_of_the_same_grain_does_not_construct() {
        let federation = sum("amount");
        let mut leg = local_leg("usage", Grain::Month, &federation);
        if let LegPlan::Fact { bucket: relabelled, .. } = &mut leg {
            *relabelled = PlanBucket::new(
                dimension_label("quarter"),
                Grain::Month,
                PlanColumn::new(table(FACT), column("month")),
            );
        }
        let arm = LocalArm::new(leg, federation, answer_keys()).expect("a local arm under another bucket label");
        let plan = DrillAcrossPlan::new(joined("revenue"), Arm::Local(Box::new(arm)), Vec::new());
        assert_eq!(plan, Err(DrillAcrossPlanError::BucketDiffers { metric: metric("usage") }));
    }

    #[test]
    fn an_arm_over_another_time_range_does_not_construct() {
        let federation = sum("amount");
        let later = LocalArm::new(
            ranged(local_leg("usage", Grain::Month, &federation), "2026-03-01", "2026-04-01"),
            federation,
            answer_keys(),
        )
        .expect("a local arm over a later range");
        let plan = DrillAcrossPlan::new(joined("revenue"), Arm::Local(Box::new(later)), Vec::new());
        assert_eq!(plan, Err(DrillAcrossPlanError::RangeDiffers { metric: metric("usage") }));
        let federation = sum("amount");
        let keyed = LocalArm::new(
            fact_leg(
                "usage",
                Grain::Month,
                vec![key(dimension_label("region"), "region")],
                terms_for(&federation),
            ),
            federation,
            vec![dimension_label("region")],
        )
        .expect("a local arm over the lookup's key");
        let second_fact = DrillAcrossPlan::new(
            joined_over_two_facts("revenue", "2026-03-01", "2026-04-01"),
            Arm::Local(Box::new(keyed)),
            Vec::new(),
        );
        assert_eq!(
            second_fact,
            Err(DrillAcrossPlanError::RangeDiffers {
                metric: metric("revenue")
            })
        );
    }

    #[test]
    fn a_metric_named_by_the_second_and_the_third_arm_does_not_construct() {
        let plan = DrillAcrossPlan::new(joined("a"), joined("b"), vec![local("b")]);
        assert_eq!(plan, Err(DrillAcrossPlanError::DuplicateMetric { metric: metric("b") }));
    }

    #[test]
    fn a_joined_arm_carrying_a_top_does_not_construct() {
        let ranked = |name: &str| {
            let Arm::Joined(plan) = joined(name) else {
                panic!("joined builds a joined arm");
            };
            Arm::Joined(Box::new((*plan).with_top(top())))
        };
        assert_eq!(
            DrillAcrossPlan::new(ranked("revenue"), local("usage"), Vec::new()),
            Err(DrillAcrossPlanError::ArmCarriesTop {
                metric: metric("revenue")
            })
        );
        assert_eq!(
            DrillAcrossPlan::new(local("usage"), ranked("revenue"), Vec::new()),
            Err(DrillAcrossPlanError::ArmCarriesTop {
                metric: metric("revenue")
            })
        );
    }

    #[test]
    fn a_plan_holds_the_cap_and_refuses_one_leg_past_it() {
        assert_eq!(MAX_LEGS, 6);
        let at_cap = DrillAcrossPlan::new(joined("a"), joined("b"), vec![joined("c")]).expect("three joined arms are six legs");
        assert_eq!(at_cap.legs().len(), MAX_LEGS);
        let past = DrillAcrossPlan::new(joined("a"), joined("b"), vec![joined("c"), local("d")]);
        assert_eq!(
            past,
            Err(DrillAcrossPlanError::TooManyLegs {
                legs: 7,
                limit: MAX_LEGS
            })
        );
    }

    #[test]
    fn a_local_arm_over_a_lookup_leg_does_not_construct() {
        let lookup = LegPlan::Lookup {
            source: source("geo"),
            table: table("dim_region").into(),
            keys: Vec::new(),
            bindings: PlanBindings::none(),
        };
        let arm = LocalArm::new(lookup, sum("amount"), Vec::new());
        assert_eq!(
            arm,
            Err(DrillAcrossPlanError::NotFact {
                source_name: source("geo")
            })
        );
    }

    #[test]
    fn a_local_arm_whose_terms_are_not_the_federations_does_not_construct() {
        let mismatch = Err(DrillAcrossPlanError::TermsDoNotMatchFederation { metric: metric("usage") });
        let distinct_over_a_sum_leg = LocalArm::new(local_leg("usage", Grain::Month, &sum("amount")), distinct(), answer_keys());
        assert_eq!(distinct_over_a_sum_leg, mismatch);
        let no_terms = LocalArm::new(
            fact_leg("usage", Grain::Month, Vec::new(), Vec::new()),
            sum("amount"),
            Vec::new(),
        );
        assert_eq!(no_terms, mismatch);
    }

    #[test]
    fn a_local_arm_with_a_leaf_on_another_model_does_not_construct() {
        let second = Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::on_model(
            Aggregate::Sum,
            column("amount"),
            ModelName::parse("visits").expect("a test model is a model"),
        ))));
        let arm = LocalArm::new(local_leg("usage", Grain::Month, &second), second, answer_keys());
        assert_eq!(
            arm,
            Err(DrillAcrossPlanError::TermsDoNotMatchFederation { metric: metric("usage") })
        );
    }

    #[test]
    fn a_local_arm_whose_leg_projects_no_answer_key_does_not_construct() {
        let federation = sum("amount");
        let arm = LocalArm::new(
            fact_leg("usage", Grain::Month, Vec::new(), terms_for(&federation)),
            federation,
            vec![dimension_label("region")],
        );
        assert_eq!(
            arm,
            Err(DrillAcrossPlanError::KeyNotOnLeg {
                metric: metric("usage"),
                label: String::from("region")
            })
        );
    }

    #[test]
    fn a_local_arm_over_a_distinct_count_needs_its_leafs_key_on_the_leg() {
        let leaf_label = ResultLabel::internal(InternalLabel::Leaf(0));
        let leg = |extra: Vec<PlanKey>| {
            let mut keys = vec![
                key(dimension_label("product_family"), "product_family"),
                key(dimension_label("region"), "region"),
            ];
            keys.extend(extra);
            fact_leg("uniques", Grain::Month, keys, terms_for(&distinct()))
        };
        let missing = LocalArm::new(leg(Vec::new()), distinct(), answer_keys());
        assert_eq!(
            missing,
            Err(DrillAcrossPlanError::KeyNotOnLeg {
                metric: metric("uniques"),
                label: leaf_label.as_str().to_owned()
            })
        );
        let keyed = LocalArm::new(leg(vec![key(leaf_label, "customer_key")]), distinct(), answer_keys());
        assert!(keyed.is_ok(), "the leaf's key is on the leg: {keyed:?}");
    }

    /// What [`execute_across`](super::DrillAcrossCombiner::execute_across) hands a combiner and the runner.
    mod execute_across {
        use std::cell::{Cell, RefCell};

        use super::{Arm, DrillAcrossPlan, joined, joined_over_two_facts, local};
        use crate::plan::federated::{ExecuteError, FederationCombiner, LegRunner, Legs};
        use crate::plan::{ArmLegs, DrillAcrossCombiner, LegPlan, NothingCombined, RefusingCombiner};
        use crate::warehouse::ResultBatches;
        use crate::warehouse::arrow::of_rows;

        fn batches() -> ResultBatches {
            of_rows(&[String::from("x")], &[]).expect("an empty result of one column is well formed")
        }

        /// Records each leg it runs, and fails `fail_on` when there is one, whatever its place in the run.
        struct Runner {
            ran: RefCell<Vec<LegPlan>>,
            fail_on: Option<LegPlan>,
        }

        impl Runner {
            fn answering() -> Self {
                Self {
                    ran: RefCell::default(),
                    fail_on: None,
                }
            }

            /// Fails the leg equal to `leg`, so a leg both arms hold, as their lookup, cannot be targeted.
            fn failing_on(leg: &LegPlan) -> Self {
                Self {
                    fail_on: Some(leg.clone()),
                    ..Self::answering()
                }
            }
        }

        impl LegRunner for Runner {
            type Error = usize;

            fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Self::Error> {
                let mut ran = self.ran.borrow_mut();
                ran.push(leg.clone());
                if self.fail_on.as_ref() == Some(leg) {
                    return Err(ran.len());
                }
                Ok(batches())
            }
        }

        /// Records the shape of every arm it is handed, and the ceiling.
        #[derive(Default)]
        struct Recording {
            shapes: RefCell<Vec<&'static str>>,
            merges: Cell<usize>,
            ceiling: Cell<u64>,
        }

        impl FederationCombiner for Recording {
            type Error = NothingCombined;

            fn combine(&self, _: &super::FederatedPlan, _: Legs<'_>, _: u64) -> Result<ResultBatches, Self::Error> {
                Err(NothingCombined)
            }

            fn working_set_exhausted(&self, _: &Self::Error) -> Option<u64> {
                None
            }

            fn answer_not_well_formed(&self, _: &Self::Error) -> Option<crate::plan::FederatedAnswerRefusal> {
                None
            }
        }

        impl DrillAcrossCombiner for Recording {
            fn merge(
                &self,
                _: &DrillAcrossPlan,
                arms: &[ArmLegs<'_>],
                working_set_bytes: u64,
            ) -> Result<ResultBatches, Self::Error> {
                self.merges.set(self.merges.get() + 1);
                self.ceiling.set(working_set_bytes);
                self.shapes.borrow_mut().extend(arms.iter().map(|arm| match arm {
                    ArmLegs::Joined(legs) if legs.second_fact().is_some() => "joined with a second fact",
                    ArmLegs::Joined(_) => "joined",
                    ArmLegs::Local { .. } => "local",
                }));
                Ok(batches())
            }
        }

        fn joined_arm(plan: &DrillAcrossPlan, at: usize) -> &super::FederatedPlan {
            match plan.arms().iter().nth(at).expect("the plan has the arm") {
                Arm::Joined(joined) => joined,
                Arm::Local(_) => panic!("arm {at} is joined"),
            }
        }

        fn two_facts_twice() -> DrillAcrossPlan {
            DrillAcrossPlan::new(
                joined_over_two_facts("revenue", "2026-01-01", "2026-02-01"),
                joined_over_two_facts("usage", "2026-01-01", "2026-02-01"),
                Vec::new(),
            )
            .expect("two arms over one key set")
        }

        #[test]
        fn every_leg_runs_once_in_the_plans_order_and_the_merge_gets_each_arm_in_its_own_shape() {
            let plan = DrillAcrossPlan::new(joined("revenue"), local("usage"), Vec::new()).expect("two arms over one key set");
            let (runner, combiner) = (Runner::answering(), Recording::default());
            combiner.execute_across(&plan, &runner, 4096).expect("every leg answers");
            let expected: Vec<LegPlan> = plan.legs().into_iter().cloned().collect();
            assert_eq!(*runner.ran.borrow(), expected);
            assert_eq!(*combiner.shapes.borrow(), ["joined", "local"]);
            assert_eq!((combiner.merges.get(), combiner.ceiling.get()), (1, 4096));
        }

        #[test]
        fn a_second_fact_runs_in_its_arms_legs_order_and_reaches_the_merge() {
            let plan = two_facts_twice();
            let (runner, combiner) = (Runner::answering(), Recording::default());
            combiner.execute_across(&plan, &runner, 1).expect("every leg answers");
            let expected: Vec<LegPlan> = plan.legs().into_iter().cloned().collect();
            assert_eq!(expected.len(), 6);
            assert_eq!(*runner.ran.borrow(), expected);
            assert_eq!(*combiner.shapes.borrow(), ["joined with a second fact"; 2]);
        }

        #[test]
        fn the_first_leg_to_fail_ends_the_run_before_a_later_leg_or_the_merge() {
            let plan = DrillAcrossPlan::new(joined("revenue"), local("usage"), Vec::new()).expect("two arms over one key set");
            let runner = Runner::failing_on(joined_arm(&plan, 0).lookups().legs()[0].leg());
            let combiner = Recording::default();
            let failure = combiner.execute_across(&plan, &runner, 1).expect_err("the second leg fails");
            assert!(matches!(failure, ExecuteError::Leg(2)), "{failure:?}");
            assert_eq!(runner.ran.borrow().len(), 2);
            assert_eq!(combiner.merges.get(), 0);
        }

        #[test]
        fn a_failing_second_fact_ends_the_run_before_a_later_arm_or_the_merge() {
            let plan = two_facts_twice();
            let second_fact = joined_arm(&plan, 0).second_fact().expect("the first arm has a second fact");
            let runner = Runner::failing_on(second_fact);
            let combiner = Recording::default();
            let failure = combiner.execute_across(&plan, &runner, 1).expect_err("the second fact fails");
            assert!(matches!(failure, ExecuteError::Leg(2)), "{failure:?}");
            assert_eq!(runner.ran.borrow().len(), 2);
            assert_eq!(combiner.merges.get(), 0);
        }

        #[test]
        fn a_failing_fact_of_a_later_arm_ends_the_run_before_its_lookup_and_second_fact_or_the_merge() {
            let plan = two_facts_twice();
            let runner = Runner::failing_on(joined_arm(&plan, 1).fact());
            let combiner = Recording::default();
            let failure = combiner
                .execute_across(&plan, &runner, 1)
                .expect_err("the later arm's fact fails");
            assert!(matches!(failure, ExecuteError::Leg(4)), "{failure:?}");
            assert_eq!(runner.ran.borrow().len(), 4);
            assert_eq!(combiner.merges.get(), 0);
        }

        #[test]
        fn a_merge_that_fails_is_the_combine_error_after_every_leg_ran() {
            let plan = DrillAcrossPlan::new(joined("revenue"), local("usage"), Vec::new()).expect("two arms over one key set");
            let runner = Runner::answering();
            let failure = RefusingCombiner
                .execute_across(&plan, &runner, 1)
                .expect_err("the refusing combiner merges nothing");
            assert!(matches!(failure, ExecuteError::Combine(NothingCombined)), "{failure:?}");
            assert_eq!(runner.ran.borrow().len(), 3);
        }
    }
}

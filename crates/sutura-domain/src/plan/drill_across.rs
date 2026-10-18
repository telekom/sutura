//! The drill-across plan: several metrics, each answered by its own arm, joined above on the
//! answer's keys and time bucket.
//!
//! **No new leg shape.** A metric that reaches a remote dimension is an [`Arm::Joined`], the
//! [`FederatedPlan`] the splitter already builds; a metric whose dimensions are all on its own source
//! is an [`Arm::Local`], one fact leg grouped by the answer's keys and the bucket. Every arm is
//! aggregated on its own, so no leg reads two fact tables and the chasm trap stays impossible.
//!
//! **What this type holds, and what it does not.** It holds the arms and refuses a set of arms that
//! cannot be joined: two arms whose keys or bucket differ, one metric twice, an arm carrying a `top`
//! (a `top` names no metric to rank by), or more legs than [`MAX_LEGS`]. It names no join and no
//! engine; the combine is a port's concern. Nothing builds this plan yet, so no question reaches it.
//!
//! **The cap is a property of the plan.** Each leg's result is held whole before the combine, so the
//! memory a question can hold grows with its leg count. [`MAX_LEGS`] bounds it at construction, and
//! the splitter reads the same constant to name the refusal a caller sees.
//!
//! `docs/adr/20261018144926-drill-across-for-several-metrics` records the decision and its limits.

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
/// The leg is grouped by the answer's keys and the bucket, so its rows are unique on them. Its
/// metric, bucket and measure label are read off the leg and the metric rather than passed beside
/// them, so no argument can disagree with the leg.
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

    /// Every leg this arm runs.
    pub fn legs(&self) -> Vec<&LegPlan> {
        match self {
            Self::Joined(plan) => plan.legs(),
            Self::Local(arm) => vec![&arm.fact],
        }
    }
}

/// Two or more [`Arm`]s that share one set of keys and one bucket grain.
///
/// The keys and the bucket are the first arm's, and every later arm is checked against them, so the
/// plan holds no copy that could disagree with an arm. Each arm's time column may differ: only the
/// label and grain must agree, and what an Arrow kind mismatch between two buckets means is the
/// combiner's to refuse.
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
    /// keys or bucket differ from the first arm's, or more than [`MAX_LEGS`] legs.
    pub fn new(first: Arm, second: Arm, rest: Vec<Arm>) -> Result<Self, DrillAcrossPlanError> {
        let every = || [&first, &second].into_iter().chain(&rest);
        let legs: usize = every().map(|arm| arm.legs().len()).sum();
        if legs > MAX_LEGS {
            return Err(DrillAcrossPlanError::TooManyLegs { legs, limit: MAX_LEGS });
        }
        let (keys, bucket) = (first.keys(), first.bucket());
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
    /// A joined arm carries a `top`, which a drill-across question refuses: it names no metric to rank by.
    #[error("the arm of `{metric}` carries a top, which a question over several metrics never ranks by")]
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
        AnswerKey, Arm, DrillAcrossPlan, DrillAcrossPlanError, FederatedPlan, InternalLabel, LocalArm, MAX_LEGS, PlanBindings,
        PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel, StatementTables, labels,
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

    fn link() -> PlanKey {
        key(ResultLabel::internal(InternalLabel::Link), "customer_key")
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
            lookup,
            true,
            federation,
            vec![
                AnswerKey::fact(dimension_label("product_family")),
                AnswerKey::lookup(dimension_label("region")),
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
    fn a_joined_arm_carrying_a_top_does_not_construct() {
        let Arm::Joined(ranked) = joined("revenue") else {
            panic!("joined builds a joined arm");
        };
        let plan = DrillAcrossPlan::new(Arm::Joined(Box::new((*ranked).with_top(top()))), local("usage"), Vec::new());
        assert_eq!(
            plan,
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
}

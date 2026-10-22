use std::collections::BTreeSet;

use super::{Filter, MetricNames, Query, RefusalReason, ToolOutcome, Top};
use crate::calendar::TimeRange;
use crate::model::{DimensionName, Grain, MetricName};

impl Query {
    pub const fn new(
        metrics: MetricNames,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self {
            metrics,
            grain,
            range,
            dimensions,
            filters,
            top: None,
        }
    }

    /// One metric, the shape every question asked before `github.com/telekom/sutura#968`. A
    /// convenience over [`Self::new`] for the overwhelmingly common case, so a caller asking about
    /// one metric writes one name rather than building a one-element [`MetricNames`].
    #[inline]
    #[must_use]
    pub const fn single(
        metric: MetricName,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self::new(MetricNames::one(metric), grain, range, dimensions, filters)
    }

    /// Attaches a `top` clause. A builder rather than a sixth constructor argument, so every
    /// existing caller of [`Self::new`] - a question without `top` - keeps its argument list; a
    /// question built this way is byte-for-byte one built without it, minus this one field.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: Top) -> Self {
        self.top = Some(top);
        self
    }

    /// Every metric this question asks about, in the order the caller listed them. At least one:
    /// see [`MetricNames`].
    #[inline]
    pub const fn metrics(&self) -> &MetricNames {
        &self.metrics
    }

    /// The first metric named - the whole of [`Self::metrics`] for a single-metric question, and
    /// what a caller who has not yet widened for multiple metrics reads.
    #[inline]
    pub const fn metric(&self) -> &MetricName {
        self.metrics.first()
    }

    #[inline]
    pub const fn grain(&self) -> Grain {
        self.grain
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub fn dimensions(&self) -> &[DimensionName] {
        &self.dimensions
    }

    #[inline]
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    #[inline]
    pub const fn top(&self) -> Option<Top> {
        self.top
    }

    /// The literal text this question carries, for the assertion that none of it reaches the SQL.
    ///
    /// It exists so the no-injection golden can be written as "no value from the question appears in
    /// the statement" rather than as a list of places to look, which is the form that goes stale the
    /// first time a field is added.
    pub fn literals(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::from([self.range.start().to_iso(), self.range.end().to_iso()]);
        out.extend(
            self.filters
                .iter()
                .flat_map(Filter::values)
                .map(|value| String::from(value.as_str())),
        );
        out
    }
}

impl RefusalReason {
    /// The machine-readable `code` a client or an agent branches on, shared by every transport.
    ///
    /// **The one place this is decided.** The HTTP and agent surfaces used to spell their own
    /// tables and nothing compared them, so a code could drift until the two transports disagreed
    /// about what a refusal was. Both now read [`RefusalReason::code`] and neither writes its own
    /// list, so there is one spelling for the whole surface.
    ///
    /// Being exhaustive with no wildcard arm, a variant added here either gets its code in the same
    /// edit or does not compile. The derivation is fixed by
    /// `the_code_is_the_variant_name_in_snake_case`: each code is the `snake_case` spelling
    /// of the variant's own name, read off this type's own `Serialize` rather than a list typed
    /// beside it - so a hand-written code that drifted from the variant fails that test, and the two
    /// transports, both reading this one method, cannot drift from each other without first drifting
    /// from the variant.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MetricUnknown { .. } => "metric_unknown",
            Self::TooManyMetrics { .. } => "too_many_metrics",
            Self::DuplicateMetricName { .. } => "duplicate_metric_name",
            Self::MultiMetricTopNotExecutable { .. } => "multi_metric_top_not_executable",
            Self::GrainNotSupported { .. } => "grain_not_supported",
            Self::DimensionNotPermitted { .. } => "dimension_not_permitted",
            Self::DimensionNotConformed { .. } => "dimension_not_conformed",
            Self::DimensionNotFilterable { .. } => "dimension_not_filterable",
            Self::DimensionValueNotAllowed { .. } => "dimension_value_not_allowed",
            Self::DuplicateDimension { .. } => "duplicate_dimension",
            Self::TooManyDimensions { .. } => "too_many_dimensions",
            Self::TooManyFilters { .. } => "too_many_filters",
            Self::ResultTooLarge { .. } => "result_too_large",
            Self::TimeRangeTooLong { .. } => "time_range_too_long",
            Self::PlanSpansTooManySources { .. } => "plan_spans_too_many_sources",
            Self::QuestionNeedsTooManyLegs { .. } => "question_needs_too_many_legs",
            Self::PlanNeedsTooManyLookupLegs { .. } => "plan_needs_too_many_lookup_legs",
            Self::FederationNotExecutable => "federation_not_executable",
            Self::FederationLinkAmbiguous { .. } => "federation_link_ambiguous",
            Self::FederationLinkCompound { .. } => "federation_link_compound",
            Self::FederationLinkTypeMismatch { .. } => "federation_link_type_mismatch",
            Self::FederatedAnswerNotWellFormed { .. } => "federated_answer_not_well_formed",
            Self::PlanTablesShareAnIdentifier { .. } => "plan_tables_share_an_identifier",
            Self::SourceUnavailable { .. } => "source_unavailable",
            Self::ResourcesExhausted { .. } => "resources_exhausted",
            Self::CredentialUnavailable { .. } => "credential_unavailable",
            Self::SourceRefused { .. } => "source_refused",
            Self::BillToNotAllowed { .. } => "bill_to_not_allowed",
            Self::NoBillingProject { .. } => "no_billing_project",
            Self::DeadlineExceeded { .. } => "deadline_exceeded",
            Self::BudgetExhausted { .. } => "budget_exhausted",
            Self::TopOverUncertifiedRows { .. } => "top_over_uncertified_rows",
            Self::CrossModelRatioWithoutSharedCalendar { .. } => "cross_model_ratio_without_shared_calendar",
            Self::CrossModelRatioSpansSources { .. } => "cross_model_ratio_spans_sources",
            Self::CrossModelRatioWithoutSharedDimension { .. } => "cross_model_ratio_without_shared_dimension",
            Self::CrossModelRatioNeedsOneLookupLeg { .. } => "cross_model_ratio_needs_one_lookup_leg",
        }
    }
}

impl ToolOutcome {
    #[inline]
    pub const fn is_refusal(&self) -> bool {
        matches!(*self, Self::Refusal { .. })
    }

    /// The refusal reason, if this is one. Convenience for tests and for an audit sink.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refusal { ref reason } => Some(reason),
            Self::Answer { .. } => None,
        }
    }
}

//! The implementor over the port, and the one axis it is distorted along.
//!
//! **One concept, and its own module because `bound.rs` crossed the unexemptable `max-lines`
//! cap**: everything here IS the fake - what it answers, how its answer differs from the corpus's,
//! and what it fails with - and nothing here is a binding, a fault cell or a venue. The seam was
//! already in the file's own section comments.
//!
//! The `Fake` deliberately produces the port's Arrow currency rather than domain rows, and
//! [`Distortion::UnmappableColumn`] is the distortion that only exists because of that.

use sutura_conformance::corpus;
use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Value, Warehouse};

/// How a fake's answer differs from the corpus's.
///
/// One axis with one variant per `Fault` the answer can produce, so each fault below is provoked
/// by exactly one distortion and a test cannot pass because two of them fired. One exception:
/// [`Self::ReversedOnlyForCase`] provokes no fault at all when it names the one case
/// `Case::order_is_asserted` marks `false` - that is the property it exists to test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distortion {
    /// None: the corpus's own answer, unchanged.
    Faithful,
    /// The right rows under the wrong column names.
    Relabelled,
    /// The right shape with one cell replaced.
    ANumberChanged,
    /// The right rows, back to front.
    Reversed,
    /// The named case's rows, back to front; every other case's answer is faithful.
    ///
    /// Separate from [`Self::Reversed`], which reverses every case and so cannot isolate one: a
    /// fake built on it faults on the first order-asserted case in `corpus::cases()` before ever
    /// reaching a case further down the list.
    ReversedOnlyForCase(&'static str),
    /// No answer at all.
    Silent,
    /// The right rows under a column whose Arrow type this workspace maps no cell of.
    ///
    /// **The one distortion that cannot be expressed in domain rows**, and that is what it is for:
    /// since `docs/adr/0039` step 2 the port hands back Arrow, so *an adapter returned a column
    /// nothing here can read* became a shape a fake has to be able to produce. Every other
    /// distortion goes through `arrow::of_row_set`, which only ever emits `Int64`, `Float64` and
    /// `Utf8` - all mapped - so no row-shaped fake could reach it.
    ///
    /// `Float32` is the type, deliberately: `sutura_domain::warehouse::arrow`'s own mapping refuses
    /// it on the argument that widening a 32-bit float to an `f64` makes two adapters disagree
    /// about a number neither of them got wrong. So this is a real column a real driver can produce
    /// rather than an invented one.
    UnmappableColumn,
}

/// What a fake's pre-flight says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// The port's own default: nobody looked.
    NotAsked,
    /// Asked and accepted.
    Accepted,
    /// Accepted with a real estimate, though this fake declares `PRICES_DRY_RUN = false` - the
    /// mismatch `Fault::EstimateDisagreesWithCapability` exists to catch.
    AcceptedWithAnEstimate,
    /// Asked and refused, which for a plan the adapter is held to answer is a fault.
    Refused,
}

/// Never returned by anything a conformance run is about: this fake computes nothing.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FakeFailure {
    #[error("the fake holds no answer for this executable")]
    NoSuchExecutable,
    #[error("this fake declines to execute a leg")]
    NoLeg,
    #[error("this fake was asked to be silent")]
    Silent,
    #[error("this fake refuses every pre-flight")]
    PreFlightRefused,
    #[error("the fake verifies no anchor")]
    NoAnchor,
    #[error("the fake built a result that is not rectangular")]
    Ragged(#[from] sutura_domain::warehouse::MalformedRowSet),
    #[error("the fake built a batch the schema it announced does not describe")]
    Unannounced(#[from] sutura_domain::warehouse::UnannouncedBatch),
    #[error("the fake could not build an Arrow batch")]
    Arrow(#[from] arrow_schema::ArrowError),
}

/// One implementor of the execution port, answering out of the corpus.
///
/// `LEGS` is a const parameter rather than two types, because `EXECUTES_LEGS` is an associated
/// constant and one distortion axis should not be written twice to vary it. `Fake<true>` and
/// `Fake<false>` are what the two bindings below are instantiated at. `PRICES` is the same move
/// for `PRICES_DRY_RUN`, defaulted `false` so every existing `Fake<LEGS>` use site keeps compiling
/// unchanged - only `a_missing_estimate_from_an_adapter_that_declares_it_prices_is_a_fault` below
/// names the other value.
pub(crate) struct Fake<const LEGS: bool, const PRICES: bool = false> {
    source: SourceName,
    posture: SourcePosture,
    distortion: Distortion,
    check: Check,
    /// Answers a leg even though `LEGS` is `false`, which is the one thing the refusing pack exists
    /// to catch.
    answers_a_leg_anyway: bool,
}

impl<const LEGS: bool, const PRICES: bool> Fake<LEGS, PRICES> {
    /// A fake that answers the corpus faithfully, checks nothing, and honours its declaration.
    pub(crate) fn faithful() -> Self {
        Self {
            source: corpus::source(),
            posture: corpus::posture(),
            distortion: Distortion::Faithful,
            check: Check::NotAsked,
            answers_a_leg_anyway: false,
        }
    }

    pub(crate) fn distorted(distortion: Distortion) -> Self {
        Self {
            distortion,
            ..Self::faithful()
        }
    }

    pub(crate) fn checking(check: Check) -> Self {
        Self {
            check,
            ..Self::faithful()
        }
    }

    /// A fake that both answers a pre-flight and is distorted, for the one cell that needs both.
    ///
    /// A constructor rather than a struct literal at the call site, and that is not style: the
    /// fields are private, so functional-record-update from outside this module does not compile -
    /// which is the property that keeps *what a fake is* one file's business.
    pub(crate) fn checking_and_distorted(check: Check, distortion: Distortion) -> Self {
        Self {
            distortion,
            check,
            ..Self::faithful()
        }
    }

    /// A fake that answers a leg although its own declaration says it does not - the one thing the
    /// refusing pack exists to catch.
    pub(crate) fn answering_a_leg_anyway() -> Self {
        Self {
            answers_a_leg_anyway: true,
            ..Self::faithful()
        }
    }

    /// The corpus's own answer for whatever it was handed.
    fn faithful_answer(executable: Executable<'_>) -> Option<RowSet> {
        match executable {
            Executable::Query(plan) => corpus::cases()
                .into_iter()
                .find(|case| case.plan() == plan)
                .map(|case| case.expected().clone()),
            Executable::Leg(leg) => {
                let case = corpus::leg_case();
                (case.leg() == leg).then(|| case.expected().clone())
            }
        }
    }

    /// Which named case this executable is, if any - what [`Distortion::ReversedOnlyForCase`]
    /// reads to decide whether to reverse.
    fn case_name(executable: Executable<'_>) -> Option<String> {
        match executable {
            Executable::Query(plan) => corpus::cases()
                .into_iter()
                .find(|case| case.plan() == plan)
                .map(|case| case.name().to_owned()),
            Executable::Leg(leg) => {
                let case = corpus::leg_case();
                (case.leg() == leg).then(|| case.name().to_owned())
            }
        }
    }

    /// The answer, distorted the one way this fake was built to distort it, as the port's own
    /// Arrow currency.
    ///
    /// Every distortion but [`Distortion::UnmappableColumn`] is expressed in domain rows and
    /// converted through `arrow::of_row_set` - the same builder the four row-speaking adapters use -
    /// so the distortion axis stays readable as rows. That one builds its batch directly, because
    /// its whole point is a column the row vocabulary cannot express.
    fn answer(&self, executable: Executable<'_>) -> Result<ResultBatches, FakeFailure> {
        if self.distortion == Distortion::Silent {
            return Err(FakeFailure::Silent);
        }
        if self.distortion == Distortion::UnmappableColumn {
            return Self::unmappable();
        }
        let faithful = Self::faithful_answer(executable).ok_or(FakeFailure::NoSuchExecutable)?;
        let columns = faithful.columns().to_vec();
        let mut rows = faithful.rows().to_vec();
        match self.distortion {
            Distortion::Faithful | Distortion::Silent | Distortion::UnmappableColumn => {}
            Distortion::Relabelled => {
                let renamed = columns.iter().map(|label| format!("{label}_")).collect();
                return Ok(of_row_set(&RowSet::new(renamed, rows)?)?);
            }
            Distortion::ANumberChanged => {
                if let Some(cell) = rows.first_mut().and_then(|row| row.last_mut()) {
                    *cell = Value::Integer(i64::MAX);
                }
            }
            Distortion::Reversed => rows.reverse(),
            Distortion::ReversedOnlyForCase(name) => {
                if Self::case_name(executable).as_deref() == Some(name) {
                    rows.reverse();
                }
            }
        }
        Ok(of_row_set(&RowSet::new(columns, rows)?)?)
    }

    /// One column of `Float32`, under the first corpus case's own first label.
    ///
    /// Built through `Accumulating` like every other result in this workspace, so what this fake
    /// produces is a well-formed [`ResultBatches`] whose schema agreement HELD - the refusal under
    /// test is the decode's, one step later, and a batch that failed the schema check instead would
    /// prove nothing about it.
    fn unmappable() -> Result<ResultBatches, FakeFailure> {
        use std::sync::Arc;

        let schema: arrow_schema::SchemaRef = Arc::new(arrow_schema::Schema::new(vec![arrow_schema::Field::new(
            "period",
            arrow_schema::DataType::Float32,
            true,
        )]));
        let batch = arrow_array::RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(arrow_array::Float32Array::from(vec![Some(1.5_f32)]))],
        )?;
        let mut accumulating = sutura_domain::warehouse::Accumulating::announcing(schema, 1, roomy());
        accumulating.push(batch)?;
        Ok(accumulating.finish())
    }
}

impl<const LEGS: bool, const PRICES: bool> Warehouse for Fake<LEGS, PRICES> {
    type Error = FakeFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = LEGS;
    const PRICES_DRY_RUN: bool = PRICES;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        match self.check {
            Check::NotAsked => Ok(PreFlight::NotAsked),
            Check::Accepted => Ok(PreFlight::Accepted { estimated_bytes: None }),
            Check::AcceptedWithAnEstimate => Ok(PreFlight::Accepted {
                estimated_bytes: Some(EstimatedBytes::parse(1)),
            }),
            Check::Refused => Err(FakeFailure::PreFlightRefused),
        }
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        // Matched exhaustively, for the reason every real adapter does: a fake that accepted subject
        // material it cannot use would let a pack pass an adapter that reported a leg as
        // impersonated when it ran shared.
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(FakeFailure::NoSuchExecutable);
            }
        }
        match executable {
            Executable::Query(_) => self.answer(executable),
            Executable::Leg(_) if LEGS || self.answers_a_leg_anyway => self.answer(executable),
            Executable::Leg(_) => Err(FakeFailure::NoLeg),
        }
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(FakeFailure::NoAnchor)
    }
}

/// A materialisation budget no fixture in this file comes near.
///
/// The bound under test here is never the byte budget - `sutura_domain::warehouse::arrow`'s own
/// cells own that - so a fixture that refused for crossing it would be testing its own size.
const fn roomy() -> sutura_domain::warehouse::ResultBudget {
    sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::MAX)
}

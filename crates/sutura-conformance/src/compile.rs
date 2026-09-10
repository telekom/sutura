//! The compile pack: what a metadata catalog must do with a question, end to end.
//!
//! [`crate::execute`] holds a data system to the answer it gives for a plan. This pack holds a
//! metadata catalog to the shape its own definitions give a question: load its
//! [`PinnedDefinitions`], run the question through `sutura_semantic::compile`, and hold the outcome
//! to the oracle - or, for a catalog that declares it supplies part of the model, to its own
//! declaration. It is the compile half of issue telekom/sutura#349's conformance rack, and it is a
//! REAL module in exactly the sense [`Behaviour`](crate::Behaviour) is one for the execute pack: a
//! deleted cell must redden rather than quietly shrink a green count.
//!
//! # How it mirrors [`crate::execute_packs`]
//!
//! The same four mechanisms, so a reader who knows one pack knows the other:
//!
//! | Execute pack | This pack |
//! | --- | --- |
//! | [`Behaviour`](crate::Behaviour) + `EVERY` + `index` + the const assert | [`CompileBehaviour`] + `EVERY` + `index` + the const assert |
//! | the `#[test]`s and [`census`](crate::census)'s `bound` are ONE repetition inside `execute_packs!` | the `#[test]`s and [`compile_census`]'s `bound` are ONE repetition inside [`compile_packs!`](crate::compile_packs) |
//! | `EXECUTES_LEGS` declaration, chcked by a `const` assert | [`SemanticCatalog::KIND`] declaration, checked by a `const` assert |
//!
//! And the one difference is what makes this pack a pair rather than a copy: **the GOLDEN/DECLARING
//! split** [`CatalogKind`] and `docs/adr/0016` draw. A catalog is held to the oracle only if it
//! declares itself *golden* - it can produce the whole model - which is enforced by a marker trait
//! bound, not by review (see [`GoldenCatalog`]).
//!
//! # What a green run does NOT establish
//!
//! - **Federated rendering.** The corpus is one source, one metric, one mono plan; nothing
//!   federates, so [`Compiled::Federated`] is a panic here rather than a case.
//! - **A live source, or identity forwarding.** Issue #349's stated surviving limits: the fixture is
//!   a hand-built catalog in this module, and no credential is minted for anything.
//! - **Every dialect.** The corpus declares it renders `DuckDb` and `Postgres`, and what
//!   [`generate`](fn@sutura_sql::generate) produces for them is what [`statement_is_the_oracles_own`]
//!   compares. `ClickHouse` and `BigQuery`
//!   are out of scope here, which is why their renderings are not pinned.
//! - **That the corpus is hard.** It is one aggregate over one metric plus four refusals - see
//!   [`questions`] below for why none of the execute corpus's harder shapes is repeated here.

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Anchor, AnchorValue, Definitions, Description, Dimension, DimensionValue, Metric, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName};
use sutura_domain::pinned::{CatalogKind, Contribution, ContributionManifest, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::{Filter, Query};
use sutura_semantic::{Compiled, compile};
use sutura_sql::Dialect;

/// One behaviour of the compile pack: the unit a test name, a failure report and a CI filter key on.
///
/// An enum rather than a string, for the reason [`crate::Behaviour`] is one: the pack's own list and
/// the tests the macro emits are compared by the compiler at one end and by [`compile_census`] at
/// the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileBehaviour {
    /// The compiled mono plan's serialized form equals the oracle's.
    Plan,
    /// For each dialect this corpus declares it renders to, the statement equals the oracle's.
    Statement,
    /// The bind parameters equal the oracle's.
    Params,
    /// The question this corpus expects to refuse reaches the variant it names, and not another.
    Refusal,
    /// What the catalog declared is exactly what its bundle produced.
    Fidelity,
    /// A second load produces the same digest.
    Repeat,
}

impl CompileBehaviour {
    /// Every behaviour a golden binding emits a test for, in the order it emits them.
    ///
    /// **The list [`compile_census`] compares a golden binding against**, and the pack's half of a
    /// pair that cannot be edited apart: [`compile_packs!`](crate::compile_packs) emits a `#[test]` AND its census element
    /// from one repetition, so a deleted test is a deleted element and this comparison reddens. That
    /// is the correction [`crate::Behaviour::EVERY`]'s own doc narrates for the execute pack; this
    /// list is where it is repeated rather than re-argued.
    ///
    /// The four golden-only behaviours come first, because that is the order the four are asked in
    /// across the golden cells; the two universal ones are what a DECLARING binding still emits.
    pub const EVERY: &'static [Self] = &[
        Self::Plan,
        Self::Statement,
        Self::Params,
        Self::Refusal,
        Self::Fidelity,
        Self::Repeat,
    ];

    /// The behaviours every catalog owes, golden or declaring.
    ///
    /// [`compile_census`] compares a DECLARING binding against this rather than against [`EVERY`](crate::compile::CompileBehaviour::EVERY),
    /// because a declaring adapter owns no oracle to be held to - it gets declaration fidelity and
    /// repeat-load determinism and nothing else. Keeping it a named subset of [`EVERY`](crate::compile::CompileBehaviour::EVERY) makes the
    /// two lists one vocabulary rather than a second enum.
    pub const UNIVERSAL: &'static [Self] = &[Self::Fidelity, Self::Repeat];

    /// This behaviour's position in [`Self::EVERY`].
    ///
    /// The same const pairing [`crate::Behaviour::index`] carries, and for the same reason: a
    /// variant deleted from `EVERY` shifts every later one, and the assertion after this impl walks
    /// `EVERY` requiring `EVERY[i].index() == i`, so the mutation is a compile error rather than a
    /// smaller green run. A NEW variant does not compile until it is given a position here, which is
    /// the direction a wildcard would have handed to a default.
    const fn index(self) -> usize {
        match self {
            Self::Plan => 0,
            Self::Statement => 1,
            Self::Params => 2,
            Self::Refusal => 3,
            Self::Fidelity => 4,
            Self::Repeat => 5,
        }
    }

    /// The name a report carries.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "the compiled plan is the oracle's plan",
            Self::Statement => "the rendered statement is the oracle's statement",
            Self::Params => "the bind parameters are the oracle's parameters",
            Self::Refusal => "a refused question reaches the variant the corpus names",
            Self::Fidelity => "the declaration is faithful to the bundle",
            Self::Repeat => "a second load pins the same digest",
        }
    }
}

// **[`CompileBehaviour::EVERY`] and [`CompileBehaviour::index`] are torn apart here unless they
// agree**, at compile time, in a `const` block - the exact guard [`crate::Behaviour`] earns in its
// own doc, transplanted for the same failure it prevents.
#[expect(
    clippy::indexing_slicing,
    reason = "a const block, where an out-of-range index fails the build rather than a run - the \
              whole point of asserting the pairing here rather than in a test"
)]
const _: () = {
    let mut position = 0;
    while position < CompileBehaviour::EVERY.len() {
        assert!(
            CompileBehaviour::EVERY[position].index() == position,
            "CompileBehaviour::EVERY and CompileBehaviour::index disagree - a behaviour deleted from EVERY shifts every later one, and a behaviour without a test is coverage no run earns"
        );
        position += 1;
    }
};

/// The marker that separates a **golden** catalog from a declaring one in this pack.
///
/// The analogue of `crate::tests::golden::catalogs::GoldenCatalog` in miniature: the golden-only
/// cell functions are bound on this marker, so a cell that requires the whole model cannot be
/// expanded for a catalog that does not implement it - the call does not typecheck. It is the
/// ROUTING copy of [`SemanticCatalog::KIND`] (`macro_rules!` cannot read an associated constant, so
/// the same fact is stated here in the form a binding can be bound on) and [`compile_packs!`](crate::compile_packs)'
/// `const` assert is where the two are torn unless they agree. The canonical declaration of the kind
/// is [`SemanticCatalog::KIND`] in the domain.
pub trait GoldenCatalog: SemanticCatalog {}

/// A catalog that could not be read, or a fixture that could not be put together.
///
/// The fixture catalogs in this module are `SemanticCatalog`s whose `Error` this fills, and the
/// corpus builders return it too, so every `.parse`/`.assemble`/`.pin` in a fixture travels through
/// `?` and only the pack boundary turns it into a panic.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// A name failed to parse as an identifier.
    #[error(transparent)]
    Names(#[from] sutura_domain::model::InvalidIdentifier),
    /// A dimension value or anchor value failed to parse.
    #[error(transparent)]
    Values(#[from] sutura_domain::catalog::InvalidDimensionValue),
    /// A date failed to parse or did not exist.
    #[error(transparent)]
    Date(#[from] sutura_domain::calendar::InvalidDate),
    /// A time range did not hold together.
    #[error(transparent)]
    Range(#[from] sutura_domain::calendar::InvalidTimeRange),
    /// A version label was not a version.
    #[error(transparent)]
    Version(#[from] sutura_domain::pinned::InvalidVersion),
    /// A definitions bundle was inconsistent at assembly.
    #[error(transparent)]
    Definitions(#[from] sutura_domain::catalog::InconsistentDefinitions),
    /// The bundle would not digest.
    #[error(transparent)]
    Digest(#[from] sutura_domain::definitions::NotDigestible),
    /// The knowledge and its declaration disagreed.
    #[error(transparent)]
    Knowledge(#[from] sutura_domain::knowledge::InconsistentKnowledge),
}

/// Turns a fixture `Err` into a test failure, with what was being built named.
///
/// The one seam through which every fallible fixture builder reaches a pack: a catalogue fixture is a
/// list of literals, so a parse failure here is a broken test rather than an input to handle - and
/// naming the `what` is what makes the panic say which builder was inconsistent.
#[expect(
    clippy::panic,
    reason = "a catalogue fixture is a list of literals, so a parse failure here is a broken \
             test, not input to handle"
)]
fn or_panic<T, E>(result: Result<T, E>, what: &str) -> T
where
    E: core::fmt::Display,
{
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {error}"),
    }
}

// ---------------------------------------------------------------- corpus: the questions ----------------------

/// The question this corpus compiles to a mono plan.
///
/// One metric (`revenue`) grouped by one declared dimension (`region`) over one month bucket,
/// filtered to a region the bundle vouches for. That is "one source, one metric" in issue #349's
/// own words for what the mono half of this pack demonstrates, and a question any of the four
/// refusal agents below trips a different guard on.
fn plan_question() -> Result<Query, FixtureError> {
    Ok(Query::new(
        metric_name("revenue")?,
        Grain::Month,
        june()?,
        vec![dimension_name("region")?],
        vec![Filter::new(dimension_name("region")?, value("emea")?)],
    ))
}

/// Every question this corpus compiles, in a stable order.
///
/// **The guard [`compile_census`] runs over so a deleted question cannot empty a golden cell.**
fn questions() -> Result<Vec<Query>, FixtureError> {
    Ok(vec![plan_question()?])
}

/// One question this corpus expects to be refused, and the variant it names.
///
/// `expected` is the `Debug` prefix of the [`RefusalReason`](sutura_domain::query::RefusalReason)
/// variant - `"MetricUnknown"`, say - so it is compared by prefix and does not re-state the payload.
/// A question that started refusing for a different reason would still be *a refusal* and would
/// otherwise pass: this is what pins it to the variant the corpus names.
struct RefusalAgent {
    question: Query,
    expected: &'static str,
}

impl RefusalAgent {
    const fn new(question: Query, expected: &'static str) -> Self {
        Self { question, expected }
    }
}

/// The four refusals this corpus provokes, one per guard.
///
/// Each is named after the variant it exists to reach, and the four are the ones a single metric
/// over a single table can reach without a relationship or a second source: an unknown metric, a
/// grain the metric did not declare, a dimension the metric did not declare, and a filter value the
/// dimension's allowlist does not hold. `refusal_reaches_the_variant_the_oracle_names` holds each to
/// its named variant rather than to "some refusal".
fn refusal_agents() -> Result<Vec<RefusalAgent>, FixtureError> {
    Ok(vec![
        RefusalAgent::new(
            Query::new(metric_name("does_not_exist")?, Grain::Month, june()?, Vec::new(), Vec::new()),
            "MetricUnknown",
        ),
        RefusalAgent::new(
            Query::new(metric_name("revenue")?, Grain::Week, june()?, Vec::new(), Vec::new()),
            "GrainNotSupported",
        ),
        RefusalAgent::new(
            Query::new(
                metric_name("revenue")?,
                Grain::Month,
                june()?,
                vec![dimension_name("customer")?],
                Vec::new(),
            ),
            "DimensionNotPermitted",
        ),
        RefusalAgent::new(
            Query::new(
                metric_name("revenue")?,
                Grain::Month,
                june()?,
                vec![dimension_name("region")?],
                vec![Filter::new(dimension_name("region")?, value("north_america")?)],
            ),
            "DimensionValueNotAllowed",
        ),
    ])
}

// -------------------------------------------------------------- the oracle and the two catalogs ----------

/// The dialects this corpus declares a mono plan renders to.
///
/// Two of the four [`Dialect`]s, and the split is a stated limit rather than an omission: this
/// corpus pins the two a one-table aggregate is most plainly exercised over, and adding the two
/// backends means adding them to this list and re-deriving the statements. `ClickHouse` and
/// `BigQuery` stay out of scope for the reason this module's header gives.
const DECLARED_DIALECTS: &[Dialect] = &[Dialect::DuckDb, Dialect::Postgres];

/// The capabilities a golden catalog - either half of the differential - declares.
///
/// Exactly what a bundle of one model and one metric carrying a dimension with an allowlist and an
/// anchor actually *has*: structure, metrics, grains, allowed values and an anchor, and nothing
/// else. Written as the five explicit kinds rather than `all()`, because this is a fixture mapping a
/// fixed bundle: a tenth kind must not widen this line by accident, and
/// [`fidelity_holds`] is what tears a line that does.
fn golden_capabilities() -> MetadataCapabilities {
    MetadataCapabilities::of(
        DefinitionCapabilities::of([
            DefinitionKind::Structure,
            DefinitionKind::Metrics,
            DefinitionKind::Grains,
            DefinitionKind::AllowedValues,
            DefinitionKind::Anchors,
        ]),
        KnowledgeCapabilities::none(),
    )
}

/// The catalog several registered catalogs read, stated a second time in Rust.
///
/// **The oracle, and it is deliberately a separate hand-built catalog from [`GoldenSubject`]**, for
/// the reason `sutura-app`'s `HandWrittenCatalog` is separate from the documents it transcribes:
/// two independent statements of one metric must produce the same plan, and the golden cells compare
/// the subject against this one so a mutation to either half reddens rather than passing as self-
/// agreement. It declares `Golden` only so its own declaration-fidelity cell holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct OracleCatalog;

impl GoldenCatalog for OracleCatalog {}

impl SemanticCatalog for OracleCatalog {
    type Error = FixtureError;

    const KIND: CatalogKind = CatalogKind::Golden;

    fn capabilities() -> MetadataCapabilities {
        golden_capabilities()
    }

    fn load(&self) -> Result<PinnedDefinitions, FixtureError> {
        pin_golden(
            golden_metric_transcribed_for_the_oracle()?,
            golden_model_transcribed_for_the_oracle()?,
        )
    }
}

/// The catalog under test for the golden arm of this pack.
///
/// A second, independent statement of the same one-metric corpus. [`plan_is_the_oracles_own`] and
/// its siblings compile a question through BOTH this and [`OracleCatalog`] and require them to
/// agree, which is what makes the golden cells differential rather than a copy of the oracle against
/// itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoldenSubject;

impl GoldenCatalog for GoldenSubject {}

impl SemanticCatalog for GoldenSubject {
    type Error = FixtureError;

    const KIND: CatalogKind = CatalogKind::Golden;

    fn capabilities() -> MetadataCapabilities {
        golden_capabilities()
    }

    fn load(&self) -> Result<PinnedDefinitions, FixtureError> {
        pin_golden(
            golden_metric_transcribed_for_the_subject()?,
            golden_model_transcribed_for_the_subject()?,
        )
    }
}

/// The catalog under test for the declaring arm of this pack.
///
/// Supplies **part** of the model - a model and a metric with a grain and nothing else - and says
/// so in its declaration, so it is measured by [`fidelity_holds`] against that declaration and by
/// [`repeat_load_is_stable`], and by no golden cell: it implements no [`GoldenCatalog`], which is
/// what makes the golden cells impossible to expand for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeclaringSubject;

impl SemanticCatalog for DeclaringSubject {
    type Error = FixtureError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, FixtureError> {
        let model = Model::new(
            model_name("declared_orders")?,
            source()?,
            table_name("declared_orders")?,
            BTreeSet::from([column("amount")?, column("occurred_at")?]),
            Description::default(),
        );
        let metric = Metric::new(
            metric_name("declared_revenue")?,
            model_name("declared_orders")?,
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")?))),
            Vec::new(),
            column("occurred_at")?,
            BTreeSet::from([Grain::Month]),
            Vec::new(),
            None,
            Description::default(),
        )?;
        let definitions = Definitions::assemble(vec![model], Vec::new(), vec![metric])?;
        pin(definitions, <Self as SemanticCatalog>::capabilities())
    }
}

/// Pins one golden bundle - the oracle's or the subject's - from its two independently transcribed
/// halves.
fn pin_golden(metric: Metric, model: Model) -> Result<PinnedDefinitions, FixtureError> {
    let definitions = Definitions::assemble(vec![model], Vec::new(), vec![metric])?;
    pin(definitions, golden_capabilities())
}

/// The bundle both golden halves must arrive at, pinned and digests matching whichever half.
fn pin(definitions: Definitions, declared: MetadataCapabilities) -> Result<PinnedDefinitions, FixtureError> {
    let knowledge = Knowledge::assemble(&definitions, KnowledgeInput::none())?;
    Ok(PinnedDefinitions::pin(
        version()?,
        definitions,
        knowledge,
        ContributionManifest::single(source()?, Contribution::of(declared)),
    )?)
}

// The four transcriptions, written out rather than shared, because sharing them is what would make
// the differential a copy of itself. Each pair (oracle model/metric and subject model/metric) uses
// the SAME literals today - that agreement is the claim the golden cells hold - and a drift between
// the two halves is exactly the defect this pack is built to catch. A reviewer should read them as
// "same catalog, rewritten", not as duplication to delete.

fn golden_model_transcribed_for_the_oracle() -> Result<Model, FixtureError> {
    Ok(Model::new(
        model_name("orders")?,
        source()?,
        table_name("orders")?,
        BTreeSet::from([column("amount")?, column("occurred_at")?, column("region")?]),
        Description::default(),
    ))
}

fn golden_metric_transcribed_for_the_oracle() -> Result<Metric, FixtureError> {
    Ok(Metric::new(
        metric_name("revenue")?,
        model_name("orders")?,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")?))),
        Vec::new(),
        column("occurred_at")?,
        BTreeSet::from([Grain::Day, Grain::Month]),
        vec![region_dimension()?],
        Some(anchor()?),
        Description::default(),
    )?)
}

fn golden_model_transcribed_for_the_subject() -> Result<Model, FixtureError> {
    Ok(Model::new(
        model_name("orders")?,
        source()?,
        table_name("orders")?,
        BTreeSet::from([column("amount")?, column("occurred_at")?, column("region")?]),
        Description::default(),
    ))
}

fn golden_metric_transcribed_for_the_subject() -> Result<Metric, FixtureError> {
    Ok(Metric::new(
        metric_name("revenue")?,
        model_name("orders")?,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")?))),
        Vec::new(),
        column("occurred_at")?,
        BTreeSet::from([Grain::Day, Grain::Month]),
        vec![region_dimension()?],
        Some(anchor()?),
        Description::default(),
    )?)
}

/// The one dimension both golden halves carry, so either can be grouped and filtered.
fn region_dimension() -> Result<Dimension, FixtureError> {
    Ok(Dimension::new(
        dimension_name("region")?,
        column("region")?,
        None,
        Some(BTreeSet::from([value("emea")?, value("apac")?])),
        Description::default(),
    ))
}

// ---------------------------------------------------------------- literals, built through `?` --------------

fn source() -> Result<SourceName, FixtureError> {
    Ok(SourceName::parse("example")?)
}

fn version() -> Result<sutura_domain::pinned::DefinitionVersion, FixtureError> {
    Ok(sutura_domain::pinned::DefinitionVersion::parse("compile-fixture-1")?)
}

fn column(raw: &str) -> Result<ColumnName, FixtureError> {
    Ok(ColumnName::parse(raw)?)
}

fn metric_name(raw: &str) -> Result<MetricName, FixtureError> {
    Ok(MetricName::parse(raw)?)
}

fn model_name(raw: &str) -> Result<ModelName, FixtureError> {
    Ok(ModelName::parse(raw)?)
}

fn dimension_name(raw: &str) -> Result<DimensionName, FixtureError> {
    Ok(DimensionName::parse(raw)?)
}

fn table_name(raw: &str) -> Result<sutura_domain::model::TableName, FixtureError> {
    Ok(sutura_domain::model::TableName::parse(raw)?)
}

fn value(raw: &str) -> Result<DimensionValue, FixtureError> {
    Ok(DimensionValue::parse(raw)?)
}

fn anchor() -> Result<Anchor, FixtureError> {
    Ok(Anchor::new(june()?, AnchorValue::parse("1000000")?))
}

/// June 2026, the range every question in this corpus is asked over.
fn june() -> Result<TimeRange, FixtureError> {
    Ok(TimeRange::new(Date::new(2026, 6, 1)?, Date::new(2026, 7, 1)?)?)
}

/// Loads whatever catalog a pack is bound to, once.
///
/// The fixture catalogs are fieldless, so a `Default` instance hands `load` its `&self`; a parse or
/// assembly failure is a broken fixture and surfaces as the pack's `Err`, which the emitted
/// `#[test]` turns into a panic.
fn pinned<C>() -> Result<PinnedDefinitions, C::Error>
where
    C: SemanticCatalog + Default,
{
    C::default().load()
}

// ---------------------------------------------------------------- the golden cells --------------------------

/// Compiles a question and returns its mono plan, or names what stopped it.
///
/// A refusal and a compile failure are both "not a plan" here but are different diagnoses, so they
/// panic with different words; a federated plan is a corpus defect and panics as one. Returning the
/// owned [`QueryPlan`](sutura_domain::plan::QueryPlan) (not a borrow) is what lets a caller hold it
/// while it also compiles the oracle's, since the value `compile` returns is a temporary this
/// function owns.
#[expect(
    clippy::panic,
    reason = "the compile pack runs straight under #[test]; a question this corpus compiles being \
             refused, federated, or failing to compile is a broken test, not production input"
)]
fn compiled_plan(pinned: &PinnedDefinitions, question: &Query, what: &str) -> Box<sutura_domain::plan::QueryPlan> {
    match compile(question, pinned) {
        Ok(Compiled::Planned { plan }) => plan,
        Ok(Compiled::Refused { reason }) => panic!("{what} answered a question this corpus compiles with a refusal: {reason:?}"),
        Ok(Compiled::Federated { .. }) => panic!("{what} federated a mono corpus question"),
        Err(cause) => panic!("{what} would not compile the corpus: {cause}"),
    }
}

/// [`CompileBehaviour::Plan`] - the compiled mono plan is the oracle's plan, in serialized form.
///
/// BOUND on [`GoldenCatalog`], so this cell cannot expand for a catalog that does not declare itself
/// golden: only a golden catalog owns an oracle to be held to.
pub fn plan_is_the_oracles_own<C>()
where
    C: GoldenCatalog + Default,
{
    let subject = or_panic(pinned::<C>(), "the catalog under test failed to load");
    let oracle = or_panic(pinned::<OracleCatalog>(), "the oracle failed to load");
    for (index, question) in or_panic(questions(), "the plan corpus").iter().enumerate() {
        let expected = compiled_plan(&oracle, question, "the oracle");
        let produced = compiled_plan(&subject, question, "the catalog under test");
        let expected_value = or_panic(serde_json::to_value(&expected), "the oracle plan would not serialise");
        let produced_value = or_panic(serde_json::to_value(&produced), "the subject plan would not serialise");
        assert_eq!(
            produced_value,
            expected_value,
            "plan {index} differs from the oracle; the {} catalog read the corpus differently",
            compile_adapter::<C>()
        );
    }
}

/// [`CompileBehaviour::Statement`] - for each declared dialect, the rendered statement equals the
/// oracle's.
///
/// BOUND on [`GoldenCatalog`], for the reason [`plan_is_the_oracles_own`] gives.
pub fn statement_is_the_oracles_own<C>()
where
    C: GoldenCatalog + Default,
{
    let subject = or_panic(pinned::<C>(), "the catalog under test failed to load");
    let oracle = or_panic(pinned::<OracleCatalog>(), "the oracle failed to load");
    for (index, question) in or_panic(questions(), "the plan corpus").iter().enumerate() {
        let expected = compiled_plan(&oracle, question, "the oracle");
        let produced = compiled_plan(&subject, question, "the catalog under test");
        for dialect in DECLARED_DIALECTS {
            let expected_statement = or_panic(sutura_sql::generate(&expected, *dialect), "the oracle would not render");
            let produced_statement = or_panic(sutura_sql::generate(&produced, *dialect), "the subject would not render");
            assert_eq!(
                produced_statement.sql(),
                expected_statement.sql(),
                "statement {index} for {dialect:?} differs from the oracle"
            );
        }
    }
}

/// [`CompileBehaviour::Params`] - the bind parameters equal the oracle's.
///
/// BOUND on [`GoldenCatalog`], for the reason [`plan_is_the_oracles_own`] gives. The statement and
/// its parameters are folded in plan order by [`sutura_sql::generate`](fn@sutura_sql::generate), so the parameters are
/// compared over one dialect the way the statement is, and over the same one.
pub fn params_are_the_oracles_own<C>()
where
    C: GoldenCatalog + Default,
{
    let subject = or_panic(pinned::<C>(), "the catalog under test failed to load");
    let oracle = or_panic(pinned::<OracleCatalog>(), "the oracle failed to load");
    for (index, question) in or_panic(questions(), "the plan corpus").iter().enumerate() {
        let expected = compiled_plan(&oracle, question, "the oracle");
        let produced = compiled_plan(&subject, question, "the catalog under test");
        let expected_params = or_panic(
            sutura_sql::generate(&expected, Dialect::DuckDb),
            "the oracle would not render",
        );
        let produced_params = or_panic(
            sutura_sql::generate(&produced, Dialect::DuckDb),
            "the subject would not render",
        );
        assert_eq!(
            produced_params.params(),
            expected_params.params(),
            "parameters {index} differ from the oracle"
        );
    }
}

/// [`CompileBehaviour::Refusal`] - a refused question reaches the variant the corpus names, not another.
///
/// BOUND on [`GoldenCatalog`], because "the variant the corpus names" is a statement only a
/// corpus-backed adapter has an oracle for. The expected is a `Debug` prefix, so a question that
/// started refusing for a different reason is caught rather than passing as *some refusal*.
#[expect(
    clippy::panic,
    reason = "the compile pack runs straight under #[test]; a corpus question answered instead of \
             refused is a broken test, not production input"
)]
pub fn refusal_reaches_the_variant_the_oracle_names<C>()
where
    C: GoldenCatalog + Default,
{
    let subject = or_panic(pinned::<C>(), "the catalog under test failed to load");
    for (case, agent) in or_panic(refusal_agents(), "the refusal corpus").iter().enumerate() {
        match compile(&agent.question, &subject) {
            Ok(Compiled::Refused { reason }) => {
                let rendered = format!("{reason:?}");
                assert!(
                    rendered.starts_with(agent.expected),
                    "refusal {case} was {rendered}, and exists to provoke {}",
                    agent.expected
                );
            }
            Ok(Compiled::Planned { .. } | Compiled::Federated { .. }) => {
                panic!("refusal {case} was answered by the catalog under test")
            }
            Err(cause) => panic!("refusal {case} failed to compile, not to be refused: {cause}"),
        }
    }
}

// ---------------------------------------------------------------- the universal cells ----------------------

/// [`CompileBehaviour::Fidelity`] - what the catalog declared is exactly what its bundle produced.
///
/// **UNIVERSAL: both a golden and a declaring catalog owe this**, and it is the assertion a
/// declaring adapter gets in place of the golden oracle - `docs/adr/0016`'s decision. It is two
/// directions in one [`MetadataCapabilities::checked_against`] call: everything declared was
/// produced, and nothing undeclared appears. A declaration widened beyond the bundle fails the
/// `Unprovided` direction and reddens here.
#[expect(
    clippy::panic,
    reason = "the compile pack runs straight under #[test]; a declaration that disagrees with its \
             bundle is a broken test, not production input"
)]
pub fn fidelity_holds<C>()
where
    C: SemanticCatalog + Default,
{
    let pinned = or_panic(pinned::<C>(), "the catalog under test failed to load");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    match C::capabilities().checked_against(&produced) {
        Ok(()) => {}
        Err(disagreement) => panic!(
            "the {}-style catalog's declaration and its bundle disagree: {disagreement}",
            compile_adapter::<C>()
        ),
    }
}

/// [`CompileBehaviour::Repeat`] - a second load produces the same digest.
///
/// **UNIVERSAL.** It loads the same bytes twice and compares the two digests, so what it can see is
/// a catalog that answers differently on a second read - a hash-ordered collection, a timestamp, a
/// source of randomness. It deliberately claims only determinism, because *the digest is a function
/// of CONTENT* is a claim about two different inputs and a fresh load is one input read twice.
pub fn repeat_load_is_stable<C>()
where
    C: SemanticCatalog + Default,
{
    let first = or_panic(pinned::<C>(), "the catalog under test failed to load");
    let second = or_panic(pinned::<C>(), "the catalog under test failed to load");
    assert_eq!(
        first.digest(),
        second.digest(),
        "a second load of the {} catalog moved the digest",
        compile_adapter::<C>()
    );
}

/// A name a report can carry for the catalog under test, without an associated constant on the port.
///
/// [`SemanticCatalog`] carries [`CatalogKind`] and not a display name, so the closest stable
/// label is the kind - which is also what the two census kinds actually differ on.
const fn compile_adapter<C>() -> &'static str
where
    C: SemanticCatalog,
{
    match C::KIND {
        CatalogKind::Golden => "golden",
        CatalogKind::Declaring => "declaring",
    }
}

// ---------------------------------------------------------------- the census -------------------------------

/// Asserts a binding emitted a test for exactly the behaviours its kind selects, and that the corpus
/// is not empty to be green over.
///
/// Three things mirror [`crate::census`]'s first three, and the missing two are the venue ones it
/// does not have: a compile catalog is a hand-built fixture with no socket to be absent, so there is
/// no floor to measure and no `NOT RUN` to print.
///
/// 1. **`bound` is the array the single `#[test]`/census repetition inside
///    [`compile_packs!`](crate::compile_packs) generated**, one element per emitted test, compared
///    against the behaviours the kind selects -
///    [`CompileBehaviour::EVERY`] for `"golden"`, [`CompileBehaviour::UNIVERSAL`] for `"declaring"`.
///    A deleted test is a deleted element and this reddens, which is the correction [`crate::census`]
///    narrates for the execute pack.
/// 2. the plan corpus is not empty, which is the state that would make [`plan_is_the_oracles_own`]
///    and its two siblings vacuously green.
/// 3. the refusal corpus is not empty, which is the state that would make
///    [`refusal_reaches_the_variant_the_oracle_names`] a green no-op.
///
/// What it cannot do is the same thing [`crate::census`] cannot: know that a behaviour's BODY
/// asserts anything. A pack that returned without comparing would pass here and everywhere else; the
/// cells above compare, which is the evidence `tests/compile.rs` is for.
#[expect(
    clippy::unreachable,
    reason = "compile_census is only dispatched to by the compile_packs macro with the two binding \
             kinds, so any other kind is a binding the macro cannot have emitted"
)]
pub fn compile_census(kind: &'static str, bound: &[CompileBehaviour]) {
    let expected = match kind {
        "golden" => CompileBehaviour::EVERY,
        "declaring" => CompileBehaviour::UNIVERSAL,
        _ => unreachable!("compile_census only ever sees the two binding kinds"),
    };
    assert_eq!(
        bound, expected,
        "the binding emitted {bound:?} and the {kind} kind defines {expected:?} - a behaviour \
         without a test is coverage this run did not earn"
    );
    let plan_cases = or_panic(questions(), "the plan corpus").len();
    let refusal_cases = or_panic(refusal_agents(), "the refusal corpus").len();
    assert!(plan_cases > 0, "the plan corpus is empty; every plan cell is vacuously green");
    assert!(
        refusal_cases > 0,
        "the refusal corpus is empty; the refusal cell is vacuously green"
    );
    println!(
        "conformance compile: {} behaviour(s) over {plan_cases} question(s) and {refusal_cases} refusal(s)",
        bound.len()
    );
}

/// Binds the compile pack to one catalog, as one named `#[test]` per behaviour.
///
/// ```ignore
/// // In this crate's own binding, `tests/compile.rs`.
/// use sutura_conformance::compile::{DeclaringSubject, GoldenSubject};
///
/// sutura_conformance::compile_packs! {
///     adapter: golden,
///     catalog: GoldenSubject,
///     golden,
/// }
///
/// sutura_conformance::compile_packs! {
///     adapter: declaring,
///     catalog: DeclaringSubject,
///     declaring,
/// }
/// ```
///
/// # The `golden`/`declaring` tag
///
/// `macro_rules!` cannot read [`SemanticCatalog::KIND`], so the kind is written at the binding the
/// way [`crate::execute_packs`]'s leg declaration is, and the arm's `const` assert is where the two
/// are torn unless they agree: tag a catalog against its own [`CatalogKind`] and the binding does
/// not build. The tag also selects WHICH behaviours the arm emits - a `golden` binding gets all six,
/// a `declaring` one gets [`CompileBehaviour::UNIVERSAL`] - and both feed [`compile_census`] the one
/// repetition's list, so either way a deleted cell reddens.
///
/// **The golden-only cells are additionally bound on [`GoldenCatalog`], so the tag is a second line
/// of defence and not the mechanism**: a golden cell's function takes `C: GoldenCatalog`, so a
/// `golden` tag handed a catalog that does not implement the marker does not compile even before the
/// `KIND` assert. The split holds by the type system rather than by review, which is the whole point
/// of the marker.
///
/// # What a binding names
///
/// - `catalog` is the under-test type, named from this crate's `compile` module. The cells load it
///   via [`SemanticCatalog::load`] and this crate supplies the corpus and the oracle.
/// - `adapter` is an ident that names the emitted module, so a binding can hold several catalogs and
///   filter by name the way the execute pack allows.
#[macro_export]
macro_rules! compile_packs {
    (adapter: $name:ident, catalog: $catalog:ty, golden $(,)?) => {
        #[cfg(test)]
        mod $name {
            const _: () = assert!(
                match <$catalog as $crate::sutura_domain::pinned::SemanticCatalog>::KIND {
                    $crate::sutura_domain::pinned::CatalogKind::Golden => true,
                    $crate::sutura_domain::pinned::CatalogKind::Declaring => false,
                },
                "a catalog bound with `golden` must declare KIND = Golden"
            );

            $crate::compile_packs!(
                @cells golden, $catalog,
                Plan => the_compiled_plan_is_the_oracles_own => plan_is_the_oracles_own,
                Statement => the_rendered_statement_is_the_oracles_own => statement_is_the_oracles_own,
                Params => the_bind_parameters_are_the_oracles_own => params_are_the_oracles_own,
                Refusal => a_refusal_reaches_the_variant_the_corpus_names
                    => refusal_reaches_the_variant_the_oracle_names,
                Fidelity => the_declaration_is_faithful_to_the_bundle => fidelity_holds,
                Repeat => a_second_load_pins_the_same_digest => repeat_load_is_stable,
            );
        }
    };

    (adapter: $name:ident, catalog: $catalog:ty, declaring $(,)?) => {
        #[cfg(test)]
        mod $name {
            const _: () = assert!(
                match <$catalog as $crate::sutura_domain::pinned::SemanticCatalog>::KIND {
                    $crate::sutura_domain::pinned::CatalogKind::Golden => false,
                    $crate::sutura_domain::pinned::CatalogKind::Declaring => true,
                },
                "a catalog bound with `declaring` must declare KIND = Declaring"
            );

            $crate::compile_packs!(
                @cells declaring, $catalog,
                Fidelity => the_declaration_is_faithful_to_the_bundle => fidelity_holds,
                Repeat => a_second_load_pins_the_same_digest => repeat_load_is_stable,
            );
        }
    };

    // The kind and catalog are forwarded flat, list unwrapped, because a `macro_rules!` recursion
    // that re-emits a bracketed list across a crate boundary does not re-parse it - the hop below
    // carries the triples as interpolated repetitions instead, which is the shape this crate's own
    // [`crate::execute_packs`] proves works from another crate.
    (
        @cells $kind:ident, $catalog:ty,
        $($variant:ident => $test_name:ident => $pack:ident),* $(,)?
    ) => {
        $crate::compile_packs!(
            @behaviours $kind, $catalog, $($variant => $test_name => $pack),*
        );
    };

    // ONE repetition, expanded twice: once into the `#[test]`s and once into the array
    // [`compile_census`] compares against the kind's expected list. A test cannot be deleted without
    // deleting its census element, and the element is what the pack's own list is compared to - the
    // same correction [`crate::execute_packs`] narrates, in the form this pack's kind tag selects.
    (
        @behaviours $kind:ident, $catalog:ty,
        $($variant:ident => $test_name:ident => $pack:ident),*
    ) => {
        $(
            #[test]
            fn $test_name() {
                // One call to the pack, which asserts directly: these are hand-built fixture
                // catalogs in the same crate the pack lives in, so there is no venue to stand up or
                // measure a fixture cost against. A failure here is a panic, exactly as it would be
                // after `crate::conduct` turned a `Fault` into one.
                $crate::compile::$pack::<$catalog>();
            }
        )*

        #[test]
        fn every_behaviour_this_kind_selects_has_a_test_here() {
            $crate::compile::compile_census(
                stringify!($kind),
                &[$($crate::compile::CompileBehaviour::$variant),*],
            );
        }
    };
}

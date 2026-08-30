//! The fakes and the oracle: the stand-ins that let the whole surface be tested with no data system.
//!
//! In `tests/support/mod.rs` rather than `tests/support.rs` so cargo does not build it as a test
//! target of its own. The oracle lives one file further in, in [`oracle`], and that split is a gate
//! rather than a preference: a hand-written catalog is a list of literals, so it grows with the
//! corpus, and `cargo xtask max-lines` fails at a thousand lines.
//!
//! **Nothing here is a registered adapter, and that line is the point.** `tests/adapters/mod.rs` holds
//! the registry and the two registration traits: an entry there is something somebody could deploy.
//! What is here cannot be deployed and is not meant to be:
//!
//! - [`HandWrittenCatalog`] is the **oracle**. Every registered catalog adapter is compared against
//!   it, and it is compared against nothing. Two adapters reading the same content must produce the
//!   same `Definitions`, and with one real adapter that claim is untestable - so the second statement
//!   of those definitions is written out in Rust, by hand, from the catalog documents. Generated from
//!   them it would agree by construction; sharing their parser it would share its bugs.
//! - [`RecordingWarehouse`] and [`CertifiedNumbers`] are **fakes**. Ports get fakes rather than mocked
//!   HTTP: the port is a Rust trait, so the honest stand-in is a type that implements it, and a test
//!   asserting on the text of an HTTP request would prove something about the test. They are what lets
//!   every refusal be checked with no database at all.
//! - [`TwoSourceCatalog`] provokes one refusal. It is built in code rather than as a catalog
//!   directory, because a corpus spanning two data systems would make every other test in the
//!   suite span two.
//!
//! Only one test target includes this module, because a fake is used where it is needed rather than
//! everywhere: `unused_imports` and `dead_code` are both `deny` in the workspace lint table, so an
//! item one target did not use would fail the build of the other.

mod oracle;

pub(crate) use oracle::{
    HandWrittenCatalog, TwoSourceCatalog, executable_definitions, june_range, oracle_definitions, oracle_knowledge,
    stated_knowledge, two_source_catalog,
};

use std::cell::RefCell;
use std::collections::BTreeMap;

use sutura_domain::identity::Presented;
use sutura_domain::model::{MetricName, SourceName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{AnchorPlan, Executable, QueryPlan};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, RowSet, Value, Warehouse};

use crate::adapters::source;

/// Why a stand-in could not answer. None of them can fail; the type exists because the ports
/// require one.
#[derive(Debug, thiserror::Error)]
#[error("the hand-written catalog cannot fail")]
pub(crate) struct Never;

/// The posture every fake in this file declares.
///
/// **One value behind a `OnceLock` rather than a field on five structs**, and that is honest rather
/// than lazy: every fake here executes nothing, over no data system, in this process - so there is
/// nowhere for a subject's credential to arrive, and they cannot differ. A per-fake field would let a
/// test set a posture the fake's own capability contradicts, which is the confusion the two
/// declarations exist to prevent.
///
/// It is a `OnceLock` so [`Warehouse::posture`] can hand back a `&'static` and none of the five fakes
/// needs a constructor argument it would have nothing to vary.
fn fake_posture() -> &'static SourcePosture {
    static POSTURE: std::sync::OnceLock<SourcePosture> = std::sync::OnceLock::new();
    POSTURE.get_or_init(|| SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a fake over no data system, executing in this process")
                .expect("a fixture reason is a reason"),
        ),
    })
}

/// What a fake in this file is handed to execute one leg as.
///
/// The deployment's own identity for the source, carrying the same acknowledgement
/// [`fake_posture`] declares - which is the only shape any of these fakes could honour, for the
/// reason that function gives: nothing here reaches a data system, so there is nowhere for a
/// subject's own credential to arrive.
fn fake_leg() -> Presented {
    // Read off `fake_posture` rather than written again, so the acknowledgement on the leg and the
    // posture the fake declares cannot drift into two different sentences.
    match *fake_posture() {
        SourcePosture::SharedServiceUser { ref declared } => Presented::SharedServiceUser {
            declared: declared.clone(),
        },
        SourcePosture::ImpersonationAtSource => panic!("the fakes' posture is the shared one, one function above"),
    }
}

/// The whole plan a fake was handed.
///
/// `Warehouse::execute` takes an `Executable` so an adapter cannot silently ignore one leg of a
/// federated answer. **None of the fakes here is where that distinction is exercised:** nothing in
/// this crate's tests hands a data system a `LegPlan`, and the leg shapes are pinned through the
/// renderer in `golden/legs.rs` rather than through a warehouse - because rendering a leg and
/// answering with one are different claims, and only the first is true today.
///
/// One function rather than an arm in each fake, so three stand-ins cannot grow three different
/// opinions about a case none of them can reach. A panic is the right shape for it: it is a broken
/// test rather than an input to handle, and it names the leg it was handed.
fn whole_plan(executable: Executable<'_>) -> &QueryPlan {
    match executable {
        Executable::Query(plan) => plan,
        Executable::Leg(leg) => panic!(
            "no fake in this suite answers a leg, and one arrived against {}",
            leg.table().as_str()
        ),
    }
}

// ------------------------------------------------------------------------ the fake warehouse ---

/// A warehouse that runs nothing and remembers what it was asked.
///
/// What lets every refusal be checked with no database. It is a fake rather than a mock of a wire
/// protocol: the port is a Rust trait, so the honest stand-in is a type that implements it. A test
/// asserting on the text of an HTTP request would prove something about the test.
pub(crate) struct RecordingWarehouse {
    source: SourceName,
    seen: RefCell<Vec<String>>,
}

impl RecordingWarehouse {
    pub(crate) fn new() -> Self {
        Self {
            source: source(),
            seen: RefCell::new(Vec::new()),
        }
    }

    /// A warehouse claiming to be some other data system, for the refusal that checks the plan's
    /// source against the adapter it is about to run on.
    pub(crate) fn pretending_to_be(name: &str) -> Self {
        Self {
            source: SourceName::parse(name).expect("a test source is a source"),
            seen: RefCell::new(Vec::new()),
        }
    }

    /// Which metrics this warehouse was asked about, in order.
    ///
    /// It records the plan's metric rather than a rendered statement, because the port takes a plan
    /// now and this adapter never renders one. What the tests need from it is "was it reached at
    /// all", which a metric name answers and a statement would only answer more verbosely.
    pub(crate) fn asked_about(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

impl Warehouse for RecordingWarehouse {
    type Error = Never;

    // Every fake here executes nothing over no data system, in this process, so there is nowhere
    // for a subject credential to arrive - the same answer the shipped engine gives.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn posture(&self) -> &SourcePosture {
        fake_posture()
    }

    fn source(&self) -> &SourceName {
        &self.source
    }

    // No `dry_run`: the port defaults it, and a fake that executes nothing has nothing to check.

    #[expect(
        clippy::unwrap_in_result,
        reason = "the fixed one-cell result is a literal, so a failure to build it is a broken \n                  test rather than an input to handle"
    )]
    fn execute(&self, executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        let plan = whole_plan(executable);
        self.seen.borrow_mut().push(String::from(plan.metric().as_str()));
        // One row of nothing, shaped so `RowSet::new` accepts it. A fake that returned plausible
        // numbers would invite a test to assert on them, and those numbers would be this file's
        // opinion rather than a data system's.
        Ok(RowSet::new(vec![String::from("recorded")], vec![vec![Value::Null]]).expect("one column and one cell is rectangular"))
    }

    // The anchor path runs the same body. It takes no credential, so what it says about identity is
    // what these fakes can honestly say: nothing reaches a data system here.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.execute(Executable::Query(plan.plan()), &fake_leg()).map(AnchorRows::of)
    }
}

// -------------------------------------------------------------- the certified-numbers fake ---

/// A data system that answers every anchor query with the number its catalog document certified.
///
/// It exists because `sutura_app::Validated` is minted by `sutura_app::verify_and_validate` and by
/// nothing else, so a test that is not *about* anchors still has to obtain its bundle from a real
/// verification pass. This is what makes that cheap, and it is what keeps the property this file is
/// built around: every refusal checked with no database at all.
///
/// **What it replaced was not a fake, it was a forgery.** The suite used to build an `AnchorReport`
/// by hand - `Matched` recorded for every anchored metric, nothing executed - and hand it to a
/// constructor that returned a bundle the service would serve. So `Validated` proved that this file
/// had asserted something, and the assertion was free. Here a statement is planned, pushed at a
/// warehouse, and the number that comes back is compared with the declared one; the only thing this
/// type gets to decide is what the data system says.
pub(crate) struct CertifiedNumbers {
    source: SourceName,
    numbers: BTreeMap<String, String>,
}

impl CertifiedNumbers {
    /// The declared number of every anchored metric in `pinned`.
    ///
    /// Read off the bundle rather than written out, so a metric gaining an anchor does not make an
    /// unrelated test fail for a reason that has nothing to do with it.
    pub(crate) fn of(pinned: &PinnedDefinitions) -> Self {
        Self {
            source: source(),
            numbers: pinned
                .anchored_metrics()
                .map(|(name, anchor)| (String::from(name.as_str()), String::from(anchor.value())))
                .collect(),
        }
    }

    /// The same data system, with one metric answering something else.
    ///
    /// A definition that has stopped computing its own number, which is the condition an anchor
    /// exists to catch and the one thing a bundle must not be servable after.
    pub(crate) fn misreporting(mut self, metric: &MetricName, value: &str) -> Self {
        drop(self.numbers.insert(String::from(metric.as_str()), String::from(value)));
        self
    }
}

impl Warehouse for CertifiedNumbers {
    type Error = Never;

    // Every fake here executes nothing over no data system, in this process, so there is nowhere
    // for a subject credential to arrive - the same answer the shipped engine gives.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn posture(&self) -> &SourcePosture {
        fake_posture()
    }

    fn source(&self) -> &SourceName {
        &self.source
    }

    // No `dry_run`: the port defaults it, and there is nothing to prepare for a number this type
    // already holds.

    #[expect(
        clippy::unwrap_in_result,
        reason = "the one-cell result is built from a literal shape, so a failure to build it is a \n                  broken test rather than an input to handle"
    )]
    fn execute(&self, executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        let plan = whole_plan(executable);
        // Labelled after the plan's metric, because that is the column an anchor check looks for. A
        // metric this fake holds no number for answers nothing, which reads as a mismatch rather
        // than as a pass.
        let label = String::from(plan.metric().as_str());
        let value = self.numbers.get(&label).cloned().unwrap_or_default();
        Ok(RowSet::new(vec![label], vec![vec![Value::Text(value)]]).expect("one column and one cell is rectangular"))
    }

    // The anchor path runs the same body, which is what makes this fake the one a bundle validates
    // against: `verify_and_validate` goes through here now rather than through `execute`.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.execute(Executable::Query(plan.plan()), &fake_leg()).map(AnchorRows::of)
    }
}

// ------------------------------------------------------------------- the wide-result fake ---

/// A data system that answers with a chosen number of rows, and remembers how many it was asked
/// for.
///
/// **The instrument for the row cap, and it has to be a fake for a reason worth writing down.**
/// `plan::MAX_ROWS` is ten thousand and the corpus CSVs hold a few hundred rows, so no catalog and
/// no question that would fit in this repository can reach it. Nor could a question file provoke it
/// even in principle: the refusal happens AFTER a data system has answered, and
/// `a_refused_question_never_reaches_the_data_system` asserts that every fixture in `PROVOKED` is
/// refused before anything runs. So the one thing this type decides is the row count, and the row
/// count is the whole of what the cap is about.
///
/// It records `QueryPlan::row_limit`, which is the other half of the mechanism: the cap can only be
/// enforced if the adapter was asked for one row MORE than it, because a result of exactly the cap is
/// otherwise indistinguishable from one the cap cut short.
pub(crate) struct WideResult {
    source: SourceName,
    rows: usize,
    asked_for: RefCell<Vec<u32>>,
}

impl WideResult {
    /// A data system whose every answer carries `rows` rows.
    pub(crate) fn of(rows: usize) -> Self {
        Self {
            source: source(),
            rows,
            asked_for: RefCell::new(Vec::new()),
        }
    }

    /// The row limits this data system was asked for, in order.
    pub(crate) fn asked_for(&self) -> Vec<u32> {
        self.asked_for.borrow().clone()
    }
}

impl Warehouse for WideResult {
    type Error = Never;

    // Every fake here executes nothing over no data system, in this process, so there is nowhere
    // for a subject credential to arrive - the same answer the shipped engine gives.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn posture(&self) -> &SourcePosture {
        fake_posture()
    }

    fn source(&self) -> &SourceName {
        &self.source
    }

    // No `dry_run`: the port defaults it, and this fake reads nothing to prepare.

    #[expect(
        clippy::unwrap_in_result,
        reason = "the one-column shape is a literal here, so a failure to build it is a broken test \n                  rather than an input to handle"
    )]
    fn execute(&self, executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        let plan = whole_plan(executable);
        self.asked_for.borrow_mut().push(plan.row_limit());
        // One column, so the shape is trivially rectangular and the only thing the test reads is how
        // many rows there are. The values are all the same on purpose: a fake that returned
        // plausible numbers would invite an assertion about them.
        let rows = vec![vec![Value::Integer(1)]; self.rows];
        Ok(RowSet::new(vec![String::from(plan.metric().as_str())], rows).expect("one column and one cell per row"))
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.execute(Executable::Query(plan.plan()), &fake_leg()).map(AnchorRows::of)
    }
}

// ------------------------------------------------------- the exhausted-engine fake ---

/// Why an engine that ran out of working memory could not answer.
///
/// A type of its own rather than [`Never`], because the whole point of the fake below is a failure -
/// and a fake whose error type cannot be constructed cannot express one.
#[derive(Debug, thiserror::Error)]
#[error("the working-set ceiling refused a reservation")]
pub(crate) struct Exhausted;

/// A data system whose engine hit its working-set ceiling.
///
/// **The instrument for the one thing the real adapter's suite cannot assert:** that
/// `sutura_app::answer` turns an exhausted engine into a `ToolOutcome::Refusal` rather than a
/// `ServiceError`. That decision lives above the port, so the honest stand-in is a type implementing
/// it - and it has to be a fake rather than the engine, because what is asserted is the branch taken
/// on the way out, not that an engine can be made to run out of memory. `pool/ceiling_tests.rs` in
/// `sutura-exec-datafusion` asserts the other half against a real one.
pub(crate) struct ExhaustedEngine {
    source: SourceName,
    ceiling_bytes: u64,
}

impl ExhaustedEngine {
    /// A data system that refuses every question against `ceiling_bytes`.
    pub(crate) fn at(ceiling_bytes: u64) -> Self {
        Self {
            source: source(),
            ceiling_bytes,
        }
    }
}

impl Warehouse for ExhaustedEngine {
    type Error = Exhausted;

    // Every fake here executes nothing over no data system, in this process, so there is nowhere
    // for a subject credential to arrive - the same answer the shipped engine gives.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn posture(&self) -> &SourcePosture {
        fake_posture()
    }

    fn source(&self) -> &SourceName {
        &self.source
    }

    // No `dry_run`: the port defaults it, and the contract is that a check reads no data - so there
    // is no reservation for a ceiling to refuse there, which is why `answer` does not treat its
    // failure as exhaustion either.

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(Exhausted)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(Exhausted)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        Some(self.ceiling_bytes)
    }
}

/// A data system that fails for some other reason.
///
/// The control for the fake above: the same shape, the same error type, and the port's default answer
/// to "was that the ceiling" - so a failure that is not exhaustion must still leave as an `Err`. Two
/// fakes rather than one with a flag, because a flag would let a test assert both branches while
/// exercising one code path in this file.
pub(crate) struct BrokenEngine {
    source: SourceName,
}

impl BrokenEngine {
    pub(crate) fn new() -> Self {
        Self { source: source() }
    }
}

impl Warehouse for BrokenEngine {
    type Error = Exhausted;

    // Every fake here executes nothing over no data system, in this process, so there is nowhere
    // for a subject credential to arrive - the same answer the shipped engine gives.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn posture(&self) -> &SourcePosture {
        fake_posture()
    }

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(Exhausted)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(Exhausted)
    }

    // `working_set_exhausted` is deliberately NOT overridden. The port defaults it to `None`, which
    // is the answer an adapter with no bounded pool gives, and taking the default is how this fake
    // says it has none.
}

/// The corpus bundle, validated the only way there is: by running its anchors.
///
/// For the tests that need a servable bundle and are about something else - a refusal, a source
/// mismatch. `sutura_app::verify_and_validate` is the whole of the path, so this cannot drift into
/// asserting a bundle is fit to serve without the anchors having been executed.
pub(crate) fn validated_bundle(pinned: PinnedDefinitions) -> sutura_app::Validated<PinnedDefinitions> {
    let certified = sutura_app::Warehouses::of(CertifiedNumbers::of(&pinned));
    sutura_app::verify_and_validate(pinned, &certified).expect("a catalog's own declared numbers reproduce themselves")
}

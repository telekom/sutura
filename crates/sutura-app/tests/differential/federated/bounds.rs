// The MODULE compiles everywhere and only the measurements are Linux-only, because the file-wide
// `#![cfg(target_os = "linux")]` this replaces made it invisible to every check a non-Linux host can
// run: `just ci` reads `builtins.currentSystem`, `builders` is empty here, so `checks.x86_64-linux.*`
// cannot be built at all. `telekom/sutura#929` is the cost - two compile errors reached CI's clippy
// leg through a clean merge while every local `just validate` reported the clippy leg green, and both
// verdicts were true: the darwin one compiled this file down to nothing. Type-checking is
// target-independent, `/proc` is not, so gating the tests and not the module keeps the measurement
// Linux-only and puts the types back under the local gate.
#![cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "the measurements this module supports only run on Linux")
)]

//! Fresh-child bounded measurements over every executable real federated-corpus topology.
//!
//! This measurement is Linux-only because its RSS high-water mark comes from `/proc`.
//!
//! The parent test starts one test-process child per question and topology. Each child opens its
//! topology, validates anchors, resets the peak window, executes exactly one question, then emits a
//! privacy-safe result census: topology, case index and outcome class, never question, model, row,
//! path or error. RSS is the fresh child's process high-water mark, including setup; pool peak is
//! only the largest one-source engine operator-reservation peak and is not a process-memory bound.
//! The recorder's pinned-dependency contract and the real-operator ceiling tests live in the engine
//! crate. This file checks the measurement boundary: complete census, released reservations,
//! configured ceiling, process peak, and a bounded child lifetime.

use std::num::NonZeroUsize;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use super::corpus::{derived, every_question};
use super::{BUDGET, bundle, posture, source, tables_on};
use crate::adapters::{BrokerCannotFail, deadline};
use crate::adapters::{a_caller, shared_credential};
use sutura_app::ServiceError;
use sutura_domain::query::ToolOutcome;
use sutura_domain::warehouse::UnreadableCell;
use sutura_exec_datafusion::{CombineError, DataFusionError};

const MEASURE_BOUNDS_CASE: &str = "MEASURE_BOUNDS_CASE";
const CHILD_TIMEOUT: Duration = Duration::from_secs(30);
/// Measured maximum 452 on the recorded corpus/host; power-of-two headroom keeps venue noise from
/// turning the measurement into a benchmark while still making an order-of-magnitude regression
/// fail.
const MAX_RSS_PER_POOL_BYTE: usize = 1024;
const EXPECTED_FAILURES: [&str; 2] = [
    "revenue-per-churned-subscription-january",
    "two-source-a-zero-denominator-that-fails",
];
const EXPECTED_METRIC: &str = "revenue_per_churned_subscription";

type MeasurementResult = Result<ToolOutcome, ServiceError<DataFusionError, BrokerCannotFail, CombineError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Topology {
    One,
    Two,
}

impl Topology {
    const ALL: [Self; 2] = [Self::One, Self::Two];

    const fn label(self) -> &'static str {
        match self {
            Self::One => "one_source",
            Self::Two => "two_source",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Case {
    topology: Topology,
    question: usize,
}

impl Case {
    fn selected() -> Option<Self> {
        let value = match std::env::var(MEASURE_BOUNDS_CASE) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent | std::env::VarError::NotUnicode(_)) => return None,
        };
        let (topology, question) = value.split_once(':')?;
        let topology = Topology::ALL.into_iter().find(|candidate| candidate.label() == topology)?;
        let question = question.parse::<usize>().ok()?;
        Some(Self { topology, question })
    }

    fn selector(self) -> String {
        format!("{}:{}", self.topology.label(), self.question)
    }
}

fn cases() -> Vec<Case> {
    let questions = every_question().len();
    Topology::ALL
        .into_iter()
        .flat_map(|topology| (0..questions).map(move |question| Case { topology, question }))
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Census {
    answered: usize,
    refused: usize,
    failed: usize,
}

impl Census {
    fn record(&mut self, name: &str, result: &MeasurementResult) {
        match result {
            Ok(ToolOutcome::Answer { .. }) => self.answered += 1,
            Ok(ToolOutcome::Refusal { .. }) => self.refused += 1,
            Err(error) if expected_failure(name, error) => self.failed += 1,
            Err(error) => panic!("{name}: measurement child returned an unexpected error: {error:?}"),
        }
    }

    fn total(&self) -> usize {
        self.answered + self.refused + self.failed
    }
}

fn expected_failure(name: &str, error: &ServiceError<DataFusionError, BrokerCannotFail, CombineError>) -> bool {
    if !EXPECTED_FAILURES.contains(&name) {
        return false;
    }
    match error {
        // **The mono path's shape since `docs/adr/0039` step 2's second half**: the port's currency
        // is Arrow, so the decode happens above every adapter and the interior's own `UnreadableCell`
        // arrives as the application's failure rather than wrapped in the engine's.
        //
        // The second pattern is the engine's own wrapper, kept because `verify_anchor` still reads
        // rows inside the adapter. One arm rather than two because `clippy::match_same_arms` is
        // denied and both shapes carry the same finding - the two patterns are what this asserts.
        ServiceError::Unreadable {
            cause: UnreadableCell::NotFinite { column, .. },
        }
        | ServiceError::Warehouse {
            cause: DataFusionError::Unreadable {
                cause: UnreadableCell::NotFinite { column, .. },
            },
        } => column == EXPECTED_METRIC,
        // **The FEDERATED path's shape since step 3**: the combiner takes the refusal itself, so a
        // non-finite measure never reaches here as a failure at all - it is
        // `RefusalReason::FederatedAnswerNotWellFormed`, which the census counts as refused rather
        // than failed. The arm this replaces matched `ServiceError::Federated`, which no longer
        // exists.
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Measurement {
    case: Case,
    census: Census,
    ceiling: usize,
    peak: usize,
    reserved: usize,
    rss_baseline: usize,
    rss: usize,
}

impl Measurement {
    fn json(&self) -> String {
        let status = match self.census {
            Census {
                answered: 1,
                refused: 0,
                failed: 0,
            } => "ok",
            Census {
                answered: 0,
                refused: 1,
                failed: 0,
            } => "refused",
            Census {
                answered: 0,
                refused: 0,
                failed: 1,
            } => "failed",
            _ => "malformed",
        };
        format!(
            "{{\"status\":\"{status}\",\"case\":\"{}\",\"topology\":\"{}\",\"ceiling\":{},\"peak\":{},\"reserved\":{},\"rss_baseline\":{},\"rss\":{},\"ratio\":{{\"numerator\":{},\"denominator\":{}}},\"answered\":{},\"refused\":{},\"failed\":{}}}",
            self.case.selector(),
            self.case.topology.label(),
            self.ceiling,
            self.peak,
            self.reserved,
            self.rss_baseline,
            self.rss,
            self.peak,
            self.rss,
            self.census.answered,
            self.census.refused,
            self.census.failed,
        )
    }
}

fn configured_ceiling() -> NonZeroUsize {
    let bytes = usize::try_from(sutura_config::WorkingSetCeiling::DEFAULT_BYTES)
        .expect("the configured default fits this target's address space");
    NonZeroUsize::new(bytes).expect("the configured default is positive")
}

fn measured(source: sutura_domain::model::SourceName) -> sutura_exec_datafusion::measurement::MeasuredWarehouse {
    sutura_exec_datafusion::measurement::MeasuredWarehouse::new(
        source,
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(configured_ceiling()),
    )
    .expect("a measured engine starts")
}

fn measured_on(
    corpus: &super::corpus::Derived,
    source_name: &sutura_domain::model::SourceName,
    pinned: &sutura_domain::pinned::PinnedDefinitions,
) -> sutura_exec_datafusion::measurement::MeasuredWarehouse {
    let child = measured(source_name.clone());
    for (table, csv) in tables_on(&corpus.data, source_name, pinned) {
        child
            .attach_csv(&table, &csv)
            .unwrap_or_else(|_| panic!("measurement child could not attach a table"));
    }
    child
}

fn execute(case: Case) -> Measurement {
    let corpus = derived();
    let pinned = match case.topology {
        Topology::One => bundle(&corpus.one_source),
        Topology::Two => bundle(&corpus.two_source),
    };
    let one = measured_on(corpus, &source(), &pinned);
    let warehouses = match case.topology {
        Topology::One => sutura_app::Warehouses::of(one),
        Topology::Two => sutura_app::Warehouses::of(one)
            .and(measured_on(corpus, &super::lookup_source(), &pinned))
            .expect("two measured sources have different names"),
    };
    let bundle = sutura_app::verify_and_validate(pinned, &warehouses).expect("the measured topology validates");
    for (_, child) in warehouses.each() {
        child.reset_peak();
    }
    let baseline_rss = rss_high_water();
    let (name, question) = every_question()
        .into_iter()
        .nth(case.question)
        .expect("the measurement case names a corpus question");
    let mut census = Census::default();
    // The real combiner, for the same reason the differential suite uses one: the child measures
    // the production federated path, and a stub that never assembles would make the two-source
    // topology's census and peak describe a path no deployment runs.
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    let result = sutura_app::answer(
        &bundle,
        &question,
        &a_caller(),
        &shared_credential(),
        &warehouses,
        &combiner,
        BUDGET,
        deadline(),
        &sutura_app::SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .map(sutura_app::Answered::into_outcome);
    census.record(&name, &result);
    let rss = rss_high_water().max(baseline_rss);
    let (peak, reserved) = warehouses.each().fold((0, 0), |(peak, reserved), (_, child)| {
        (peak.max(child.pool_peak()), reserved + child.pool_reserved())
    });
    Measurement {
        case,
        census,
        ceiling: configured_ceiling().get(),
        peak,
        reserved,
        rss_baseline: baseline_rss,
        rss,
    }
}

fn rss_high_water() -> usize {
    let status =
        std::fs::read_to_string("/proc/self/status").expect("the bounded measurement requires Linux /proc process accounting");
    rss_high_water_from(&status).expect("/proc/self/status carries a positive VmHWM reading")
}

fn rss_high_water_from(status: &str) -> Option<usize> {
    let kibibytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_ascii_whitespace()
        .next()?
        .parse::<usize>()
        .ok()?;
    NonZeroUsize::new(kibibytes)?.get().checked_mul(1024)
}

fn child_output(case: Case) -> Output {
    let mut child = Command::new(std::env::current_exe().expect("the test executable is known"))
        .arg("--exact")
        .arg("federated::bounds::every_corpus_question_has_one_fresh_child_outcome_in_each_topology")
        .arg("--nocapture")
        .env(MEASURE_BOUNDS_CASE, case.selector())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("a measurement child starts");
    let started = Instant::now();
    loop {
        if child.try_wait().expect("a measurement child can be observed").is_some() {
            return child.wait_with_output().expect("a completed measurement child can be read");
        }
        if started.elapsed() >= CHILD_TIMEOUT {
            child.kill().expect("an over-time measurement child can be stopped");
            let output = child.wait_with_output().expect("a stopped measurement child can be read");
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("measurement child {case:?} exceeded {CHILD_TIMEOUT:?}: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_child(case: Case) -> String {
    let output = child_output(case);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "measurement child {case:?} did not complete successfully: {stderr}"
    );
    let stdout = String::from_utf8(output.stdout).expect("a measurement child writes UTF-8");
    let records: Vec<_> = stdout.lines().filter(|line| line.starts_with("{\"status\"")).collect();
    assert_eq!(records.len(), 1, "a measurement child emits one record");
    String::from(records[0])
}

#[test]
#[cfg(target_os = "linux")]
fn every_corpus_question_has_one_fresh_child_outcome_in_each_topology() {
    if let Some(case) = Case::selected() {
        let measurement = execute(case);
        assert_eq!(measurement.census.total(), 1, "one child executes one question");
        assert_eq!(
            measurement.reserved, 0,
            "a completed question releases every operator reservation"
        );
        assert!(
            measurement.peak <= measurement.ceiling,
            "an operator peak cannot exceed the configured pool ceiling"
        );
        assert!(
            measurement.rss >= measurement.rss_baseline,
            "a process high-water mark cannot move backwards"
        );
        assert!(
            measurement.rss >= measurement.peak,
            "process RSS must contain the observed pool reservation"
        );
        if measurement.peak > 0 {
            assert!(
                measurement.rss <= measurement.peak.saturating_mul(MAX_RSS_PER_POOL_BYTE),
                "process RSS exceeds the measured operator-pool relationship"
            );
        }
        match measurement.census {
            Census {
                answered: 1,
                refused: 0,
                failed: 0,
            } => {
                assert!(measurement.peak > 0, "an answered query must reserve operator memory");
            }
            Census {
                answered: 0,
                refused: 1,
                failed: 0,
            }
            | Census {
                answered: 0,
                refused: 0,
                failed: 1,
            } => {}
            _ => panic!("a child has one valid outcome"),
        }
        println!("{}", measurement.json());
        return;
    }
    let cases = cases();
    for case in &cases {
        println!("{}", run_child(*case));
    }
    let expected_failed = cases
        .iter()
        .filter(|case| {
            every_question()
                .get(case.question)
                .is_some_and(|(name, _)| EXPECTED_FAILURES.contains(&name.as_str()))
        })
        .count();
    println!(
        "{{\"status\":\"manifest\",\"expected\":{},\"expected_failed\":{expected_failed}}}",
        cases.len()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn result_census_covers_each_question_once_per_topology() {
    let questions = every_question().len();
    let cases = cases();
    assert_eq!(cases.len(), questions * Topology::ALL.len());
    for topology in Topology::ALL {
        let selected: Vec<_> = cases
            .iter()
            .filter(|case| case.topology == topology)
            .map(|case| case.question)
            .collect();
        assert_eq!(selected, (0..questions).collect::<Vec<_>>());
    }
}

#[test]
#[cfg(target_os = "linux")]
fn widest_left_join_shape_executes_in_both_topologies() {
    let question = every_question()
        .iter()
        .position(|(name, _)| name == "two-source-a-same-source-orphan-beside-a-remote-one")
        .expect("the corpus contains the widest left-join shape");
    for topology in Topology::ALL {
        let record = run_child(Case { topology, question });
        assert!(record.contains("\"status\":\"ok\""), "{record}");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn the_distinct_key_refusal_has_no_execution_peak() {
    let question = every_question()
        .iter()
        .position(|(name, _)| name == "two-source-a-distinct-value-spanning-join-keys")
        .expect("the corpus contains the distinct-key shape");
    let record = run_child(Case {
        topology: Topology::Two,
        question,
    });
    assert!(record.contains("\"status\":\"refused\""), "{record}");
    assert!(record.contains("\"peak\":0"), "{record}");
}

#[test]
#[cfg(target_os = "linux")]
fn rss_high_water_and_ratio_are_machine_readable() {
    assert_eq!(rss_high_water_from("VmHWM:\t42 kB\n"), Some(42 * 1024));
    assert_eq!(rss_high_water_from("VmRSS:\t42 kB\n"), None);
    assert_eq!(rss_high_water_from("VmHWM:\t0 kB\n"), None);
    let record = Measurement {
        case: Case {
            topology: Topology::Two,
            question: 7,
        },
        census: Census {
            answered: 0,
            refused: 1,
            failed: 0,
        },
        ceiling: 1024,
        peak: 0,
        reserved: 0,
        rss_baseline: 40 * 1024,
        rss: 42 * 1024,
    }
    .json();
    assert_eq!(
        record,
        "{\"status\":\"refused\",\"case\":\"two_source:7\",\"topology\":\"two_source\",\"ceiling\":1024,\"peak\":0,\"reserved\":0,\"rss_baseline\":40960,\"rss\":43008,\"ratio\":{\"numerator\":0,\"denominator\":43008},\"answered\":0,\"refused\":1,\"failed\":0}"
    );
}

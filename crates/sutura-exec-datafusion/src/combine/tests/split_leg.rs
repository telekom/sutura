//! A lookup leg its source split: the combine reads every partition and answers as the whole leg.
//!
//! Through `FederationCombiner::execute` with a fake `LegRunner` whose `split` hands back the
//! partitions, so the cells hold the domain's reading of them and this engine's table over them
//! together. The bound a partition is refused for crossing is the source's own; here a fake partition
//! states it.

use core::sync::atomic::{AtomicUsize, Ordering};

use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{ExecuteError, FederationCombiner as _, LegRunner, LegsMiswired, PartitionReader};
use sutura_domain::warehouse::{ResultBatches, Value};

use crate::combine::CombineError;

use super::fixtures::{UNBOUNDED, combined, combiner, fact, fact_row, lookup, sum_plan};

/// Why a fake leg or partition did not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// The source failed to read the partition.
    Unread,
    /// The partition holds more rows than the per-stream bound.
    OverBound,
}

/// A runner whose lookup leg is split into `partitions`, each counted as it is read, and whose
/// lookup leg read whole is `whole`, counted too.
struct SplitRunner {
    fact: ResultBatches,
    whole: ResultBatches,
    partitions: Vec<Result<ResultBatches, Fault>>,
    reads: Vec<AtomicUsize>,
    whole_reads: AtomicUsize,
}

impl SplitRunner {
    fn new(fact: ResultBatches, whole: ResultBatches, partitions: Vec<Result<ResultBatches, Fault>>) -> Self {
        Self {
            fact,
            whole,
            reads: partitions.iter().map(|_| AtomicUsize::new(0)).collect(),
            partitions,
            whole_reads: AtomicUsize::new(0),
        }
    }

    fn reads(&self) -> Vec<usize> {
        self.reads.iter().map(|reads| reads.load(Ordering::SeqCst)).collect()
    }
}

impl LegRunner for SplitRunner {
    type Error = Fault;

    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Fault> {
        if matches!(leg, LegPlan::Lookup { .. }) {
            self.whole_reads.fetch_add(1, Ordering::SeqCst);
            return Ok(self.whole.clone());
        }
        Ok(self.fact.clone())
    }

    fn split(&self, _leg: &LegPlan) -> PartitionReader<'_, Fault> {
        Box::new(self.partitions.iter().zip(&self.reads).map(|(partition, reads)| {
            reads.fetch_add(1, Ordering::SeqCst);
            partition.clone()
        }))
    }
}

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

/// Three fact rows over two customers, `c1` and `c2`.
fn three_rows() -> ResultBatches {
    fact(vec![
        fact_row("A", text("c1"), Value::Integer(100)),
        fact_row("A", text("c2"), Value::Integer(200)),
        fact_row("B", text("c1"), Value::Integer(50)),
    ])
}

fn north() -> ResultBatches {
    lookup(vec![vec![text("c1"), text("north")]])
}

fn south() -> ResultBatches {
    lookup(vec![vec![text("c2"), text("south")]])
}

fn both() -> ResultBatches {
    lookup(vec![vec![text("c1"), text("north")], vec![text("c2"), text("south")]])
}

#[test]
fn every_partition_of_a_split_lookup_leg_is_read_once_and_answers_as_the_whole_leg() {
    let plan = sum_plan(true);
    let runner = SplitRunner::new(three_rows(), both(), vec![Ok(north()), Ok(south())]);
    let combiner = combiner();

    let answered = combiner
        .execute(combiner.plan(&plan), &runner, UNBOUNDED)
        .expect("a split lookup leg answers")
        .to_rows()
        .expect("a combined answer decodes");

    // The exact differential: the same question over the leg read whole.
    assert_eq!(answered, combined(&plan, &three_rows(), &both(), UNBOUNDED));
    assert_eq!(runner.reads(), [1, 1], "each partition is read once");
    assert_eq!(
        runner.whole_reads.load(Ordering::SeqCst),
        0,
        "a split leg is not read whole too"
    );
}

#[test]
fn a_partition_that_does_not_read_is_the_lookup_legs_error() {
    let plan = sum_plan(true);
    let runner = SplitRunner::new(three_rows(), both(), vec![Ok(north()), Err(Fault::Unread), Ok(south())]);
    let combiner = combiner();

    let answered = combiner.execute(combiner.plan(&plan), &runner, UNBOUNDED);

    assert!(
        matches!(answered, Err(ExecuteError::Leg(Fault::Unread))),
        "a partition's error is the leg's, not an answer over the partitions that read: {answered:?}"
    );
    assert_eq!(runner.reads(), [1, 1, 0], "the read stops at the partition that failed");
}

#[test]
fn a_partition_error_is_not_retried_as_the_whole_leg() {
    let plan = sum_plan(true);
    let runner = SplitRunner::new(three_rows(), both(), vec![Ok(north()), Err(Fault::OverBound)]);
    let combiner = combiner();

    let answered = combiner.execute(combiner.plan(&plan), &runner, UNBOUNDED);

    assert!(
        matches!(answered, Err(ExecuteError::Leg(Fault::OverBound))),
        "a partition over the bound is refused, not answered another way: {answered:?}"
    );
    assert_eq!(
        runner.whole_reads.load(Ordering::SeqCst),
        0,
        "a refused partition is not read again as the whole leg"
    );
}

#[test]
fn a_link_value_two_partitions_both_carry_is_refused_as_ambiguous() {
    let plan = sum_plan(true);
    let runner = SplitRunner::new(three_rows(), both(), vec![Ok(north()), Ok(north())]);
    let combiner = combiner();

    let answered = combiner.execute(combiner.plan(&plan), &runner, UNBOUNDED);

    assert!(
        matches!(answered, Err(ExecuteError::Combine(CombineError::AmbiguousLink))),
        "partitions are one table to the combine, so a tuple repeated across them is refused: {answered:?}"
    );
}

#[test]
fn a_split_into_no_partitions_is_refused_as_miswired_not_combined() {
    let plan = sum_plan(true);
    let runner = SplitRunner::new(three_rows(), both(), Vec::new());
    let combiner = combiner();

    let answered = combiner.execute(combiner.plan(&plan), &runner, UNBOUNDED);

    assert!(
        matches!(answered, Err(ExecuteError::Miswired(LegsMiswired::NoPartition))),
        "a leg with no partition stands for no result: {answered:?}"
    );
}

#![forbid(unsafe_code)]
//! How much of a query's wall time the row-shaped materialisation step costs.
//!
//! `github.com/telekom/sutura#915`. ADR 0007's port question is whether an Arrow-shaped transfer
//! would be faster than the row-shaped one this workspace uses today - `docs/adr/0007`, argued so
//! far from reading code. This is the number: the cost of building a
//! [`RowSet`](sutura_domain::warehouse::RowSet) - the shape a row-speaking adapter assembles, and
//! the one this crate's `NUMERIC` re-read and boot path still build over the driver's Arrow batches
//! - as a function of how many rows and columns a query returns.
//!
//! **The limit, stated where the claim is.** This does NOT measure a driver's own decode - this
//! crate's answers arrive as Arrow from the ADBC driver, and a bench target opens no connection.
//! What is measured instead is the driver-independent tail a row-shaped decode funnels into:
//! assembling already-typed
//! [`Value`](sutura_domain::warehouse::Value) cells into the row-shaped `Vec<Vec<Value>>`
//! [`RowSet::new`](sutura_domain::warehouse::RowSet::new) validates. That is the layer the
//! row-vs-Arrow argument is actually about - a driver's own decode cost is a separate, per-driver
//! question this harness does not settle.
//!
//! Run with `just bench`. Prints the host's own load average first - a number taken under
//! contention is not comparable with one taken idle, and this repository has both kinds on the
//! same machine within an hour.

use sutura_domain::warehouse::{MalformedRowSet, Real, RowSet, Value};

/// One cell, cycling through four [`Value`] shapes a Postgres answer holds: an integer column, a
/// text column, a real (`FLOAT8`) column and a `NULL`.
fn cell_for(row: usize, column: usize) -> Value {
    // `& 3`, not `% 4`: `integer_division_remainder_used` is denied here, and a cycle of 4 is a
    // power of two, so the mask is exact.
    match column & 3 {
        0 => Value::Integer(i64::try_from(row).unwrap_or(i64::MAX)),
        1 => Value::Text(format!("customer-{row:08}")),
        // A fixed magnitude, not a computed one: `float_arithmetic` is denied here too, and this
        // cell's cost is the allocation `RowSet::new` walks, not the number it holds.
        2 => Real::parse(1234.5).map_or(Value::Null, Value::Real),
        _ => Value::Null,
    }
}

fn rows_of(rows: usize, columns: usize) -> Vec<Vec<Value>> {
    (0..rows)
        .map(|row| (0..columns).map(|column| cell_for(row, column)).collect())
        .collect()
}

fn column_labels(columns: usize) -> Vec<String> {
    (0..columns).map(|index| format!("column_{index}")).collect()
}

/// Rows scale, columns fixed at a shape a wide dimensional cut actually returns.
#[divan::bench(args = [100, 1_000, 10_000, 100_000])]
fn assemble_by_row_count(bencher: divan::Bencher, rows: usize) {
    const COLUMNS: usize = 8;
    let labels = column_labels(COLUMNS);
    bencher.bench_local(|| -> Result<RowSet, MalformedRowSet> { RowSet::new(labels.clone(), rows_of(rows, COLUMNS)) });
}

/// Columns scale, rows fixed at a shape an ordinary answer actually returns.
#[divan::bench(args = [1, 4, 8, 16, 32, 64])]
fn assemble_by_column_count(bencher: divan::Bencher, columns: usize) {
    const ROWS: usize = 1_000;
    let labels = column_labels(columns);
    bencher.bench_local(|| -> Result<RowSet, MalformedRowSet> { RowSet::new(labels.clone(), rows_of(ROWS, columns)) });
}

fn main() {
    sutura_dev::bench_venue::print();
    divan::main();
}

#![forbid(unsafe_code)]
//! This adapter, held to the conformance packs, from its OWN crate.
//!
//! **Closes `telekom/sutura#710`.** `execute_packs!` bound three adapters and none of them was this
//! one - `crates/sutura-conformance/src/lib.rs`'s own header named the reason: every binding until
//! now stood a REAL data system up (`DuckDB`/`DataFusion` in-memory, `Postgres` a provisioned tier),
//! and `BigQuery` has no local tier a worktree can provision - it is cloud-only, and the nix sandbox
//! `just validate` runs in has no network at all.
//!
//! # A CANNED transport, not a live endpoint, and that is what makes this compile at all
//!
//! `open` below never dials anything: it builds [`Canned`], a [`JobTransport`] that answers `run`
//! and `validate` from a table it built ONCE, by rendering every corpus case's own plan through
//! `sutura_sql::generate(_, Dialect::BigQuery)` - the SAME function `BigQueryWarehouse::execute`
//! calls - and remembering the `(statement, params)` pair that rendering produced against that
//! case's own [`sutura_conformance::corpus::Case::expected`] rows, converted into the ARROW shapes
//! the driver delivers - an `arrow_array::RecordBatch` per case, carried by
//! `sutura_domain::warehouse::ResultBatches`. `run` looks the pair straight back up; a statement
//! this pack never rendered has no entry and the lookup fails loudly.
//!
//! **What that proves, stated so nobody reads this pack as a live-endpoint claim:** that this
//! adapter's OWN plumbing round-trips - the request it builds from a rendered plan, and the domain
//! `Value` that `ResultBatches::to_rows` decodes an Arrow batch back into, agree with what the
//! corpus says an answer to that plan looks like. **What it does NOT prove:** that a real
//! `GoogleSQL` endpoint, asked the rendered statement, returns those rows - nothing here executes
//! SQL. **And nothing in this repository makes that claim any more:** the leg that did was
//! `tests/corpus.rs` over the `wire`+`fixtures` features, and it was deleted with the HTTP
//! transport. The hosted `BigQuery` venue that is left - `just bigquery-declared-principal` - asks a
//! real dataset `SELECT SESSION_USER()` and runs no corpus case, so *the rendered statement is
//! accepted by a real endpoint* is unmeasured on this tree rather than measured elsewhere.
//!
//! **Why not `Fixture::Absent` when nothing is configured**, which is the shape every other
//! networked binding here reaches for: `sutura_conformance::venue::refuse_a_declared_absence` fires
//! whenever `SUTURA_DEV_REQUIRE_TIER` is set, and `nix/postgres-tier.nix` sets it unconditionally for
//! `checks.nextest` - a fact about the Postgres tier being up, not about `BigQuery`. A fixture here
//! that answered absent for want of real cloud credentials would be refused by that check in the
//! one venue that matters most, `just validate`'s own sandbox. `Fixture::Standing` unconditionally
//! is the only honest shape left, and it is honest BECAUSE the transport is canned rather than
//! silently claiming a live one.
//!
//! # Leg declaration: `refuses_legs`
//!
//! `BigQueryWarehouse` leaves `Warehouse::EXECUTES_LEGS` at the domain default (`false`) -
//! `BigQueryWarehouse::render`'s `Executable::Leg` arm answers `BigQueryError::LegWithoutCombiner`
//! for every leg, transport untouched, exactly `sutura-exec-postgres`'s own shape and for the same
//! reason: a leg arriving here needs a combiner above it that nothing builds yet.
//!
//! # `PRICES_DRY_RUN`, exercised in the pack for the first time
//!
//! `BigQueryWarehouse` is the only adapter declaring [`Warehouse::PRICES_DRY_RUN`] `true`; every
//! other bound adapter declares `false`. [`Canned::validate`] answers a fixed, honestly-fake byte
//! estimate for every case, so `a_preflight_that_accepts_is_followed_by_an_answer`'s
//! `estimated_bytes.is_some() == W::PRICES_DRY_RUN` comparison is exercised here at `true` for the
//! first time in the pack rather than only in this adapter's own
//! `dry_run_estimate_agrees_with_its_declaration` cell, which runs against a fake transport told to
//! price and is not the pack.
//!
//! # What this file does not establish
//!
//! The three things `sutura-exec-duckdb`'s own binding lists - nothing about impersonation, nothing
//! about the rendered SQL being ACCEPTED anywhere (only that it renders the same way twice), and
//! nothing about the three hard cases `docs/adr/0012` names - plus the one this header already
//! spent most of its words on: nothing about a live endpoint at all.

// One `#[cfg(test)]` module holding the fixture and the binding, which is this workspace's shape for
// an integration test target: the strict lints exempt test code, and a fixture at file scope is not
// test code as far as clippy is concerned.
#[cfg(test)]
mod conformance {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Decimal128Array, Float64Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema, SchemaRef};

    use sutura_conformance::{Fixture, corpus};
    use sutura_domain::warehouse::estimate::EstimatedBytes;
    use sutura_domain::warehouse::{Accumulating, ParamValue, ResultBatches, RowSet, Value};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::transport::{
        DatasetAddress, DatasetId, DryRunEstimate, HeldTables, JobRequest, JobTransport, ListingTotal, ProjectId,
    };
    use sutura_sql::{Dialect, generate};

    /// The statement and its bound values, exactly as [`JobRequest`] carries them apart - the
    /// no-injection shape every real transport is handed, and the shape [`Canned`] keys on so a
    /// lookup cannot be fooled by two cases sharing rendered text with different bindings.
    type Key = (String, Vec<String>);

    fn key_of(statement: &str, params: &[ParamValue]) -> Key {
        (
            String::from(statement),
            params
                .iter()
                .map(|param| match *param {
                    ParamValue::Text(ref text) => text.clone(),
                    ParamValue::Date(date) => date.to_iso(),
                })
                .collect(),
        )
    }

    /// Whether this text is the shape a `NUMERIC`/`BIGNUMERIC` cell sends: an optional leading `-`,
    /// then digits, with at most one `.` - which is what tells `"9223372036854775808"` from
    /// `"north"` and from an ISO date, whose extra `-`s are never in a LEADING position.
    fn is_decimal_literal(text: &str) -> bool {
        let body = text.strip_prefix('-').unwrap_or(text);
        !body.is_empty() && body.matches('.').count() <= 1 && body.chars().all(|c| c.is_ascii_digit() || c == '.')
    }

    /// The one Arrow type a whole column of domain [`Value`]s round-trips through exactly.
    ///
    /// **A whole-column decision over every row, not the first non-null cell, and `docs/adr/0039`
    /// is why this fixture still needs one.** Arrow is columnar and a domain `Value` is a per-cell
    /// union, so a column that answers `Integer` for one row and a wide `Text` for another - which
    /// `wide-total-by-day` and its two siblings are exactly - has to be given ONE type, and a real
    /// `BigQuery` reports one `NUMERIC` for that column either way. `Decimal128(38, 0)` is the
    /// Arrow shape of that claim: the interior's decode widens a zero-scale decimal that fits an
    /// `i64` and leaves the rest as text, per cell, which is the behaviour the wire mapping this
    /// replaced had.
    ///
    /// `sutura_domain::warehouse::arrow::of_rows` is deliberately NOT used here even though it
    /// builds columns from rows: its inference is the narrow one a FAKE needs (all-`Integer`, or
    /// all-`Real`, else text), and it states that a mixed column becomes text. This fixture is
    /// claiming what a data system would have SENT, which is the one case that inference declines.
    fn kind_of(column: usize, rows: &[Vec<Value>]) -> DataType {
        let mut integer = false;
        let mut numeric_text = false;
        for value in rows.iter().filter_map(|row| row.get(column)) {
            match *value {
                Value::Null => {}
                Value::Real(_) => return DataType::Float64,
                Value::Integer(_) => integer = true,
                Value::Text(ref text) if is_decimal_literal(text) => numeric_text = true,
                Value::Text(_) => return DataType::Utf8,
            }
        }
        if numeric_text {
            DataType::Decimal128(DECIMAL_PRECISION, scale_of(column, rows))
        } else if integer {
            DataType::Int64
        } else {
            DataType::Utf8
        }
    }

    /// How many decimal places the widest numeric text in this column needs.
    ///
    /// **A whole-column scale, for the reason `kind_of` needs a whole-column TYPE**: one Arrow
    /// decimal column carries one scale, and an exact decimal rendered at the wrong one is a
    /// different number. `decimal-total-by-day` is the case that measured this - it expects
    /// `Text("11.50")`, so a column declared at scale 0 lost the fraction and answered null - while
    /// `wide-total-by-day` needs scale 0 so the interior's own decode widens a whole number back to
    /// `Integer`.
    fn scale_of(column: usize, rows: &[Vec<Value>]) -> i8 {
        let mut widest = 0_usize;
        for value in rows.iter().filter_map(|row| row.get(column)) {
            if let Value::Text(ref text) = *value
                && let Some((_whole, fraction)) = text.split_once('.')
            {
                widest = widest.max(fraction.len());
            }
        }
        i8::try_from(widest).unwrap_or(0)
    }

    /// One value as this column's decimal payload, at `scale`.
    ///
    /// Scaled by moving the point in TEXT rather than by multiplying a float, which is the whole
    /// point of an exact decimal: `11.50` at scale 2 is the payload `1150`, and reaching that
    /// through an `f64` is how a total that was exact stops being exact.
    fn payload_of(value: Option<&Value>, scale: i8) -> Option<i128> {
        let places = usize::try_from(scale).unwrap_or(0);
        match value {
            Some(&Value::Integer(number)) => {
                let mut text = number.to_string();
                text.push_str(&"0".repeat(places));
                text.parse::<i128>().ok()
            }
            Some(Value::Text(text)) => {
                let (whole, fraction) = text.split_once('.').unwrap_or((text.as_str(), ""));
                if fraction.len() > places {
                    return None;
                }
                let mut scaled = String::from(whole);
                scaled.push_str(fraction);
                scaled.push_str(&"0".repeat(places - fraction.len()));
                scaled.parse::<i128>().ok()
            }
            _ => None,
        }
    }

    /// The width this fixture declares every exact decimal at.
    ///
    /// The SCALE is per column - see `scale_of` - and the two halves of that split are both
    /// load-bearing: at scale 0 the interior's decode widens a whole number that fits an `i64` back
    /// to `Integer` (`wide-total-by-day`), and at a positive scale it renders the exact text
    /// (`decimal-total-by-day`). One constant for both would break one of the two.
    const DECIMAL_PRECISION: u8 = 38;

    /// One column, as the arrays a driver would hand back.
    ///
    /// The inverse of the interior's own `cell`, and it has to be: what this file proves is that the
    /// two agree. A value that does not match the column's declared type is a defect in `kind_of`
    /// above rather than a case to answer as null, so each arm says which type it expected.
    fn column_of(kind: &DataType, column: usize, rows: &[Vec<Value>]) -> ArrayRef {
        let cells = rows.iter().map(|row| row.get(column));
        match *kind {
            DataType::Float64 => Arc::new(Float64Array::from(
                cells
                    .map(|value| match value {
                        Some(&Value::Real(real)) => Some(real.get()),
                        _ => None,
                    })
                    .collect::<Vec<Option<f64>>>(),
            )),
            DataType::Int64 => Arc::new(Int64Array::from(
                cells
                    .map(|value| match value {
                        Some(&Value::Integer(number)) => Some(number),
                        _ => None,
                    })
                    .collect::<Vec<Option<i64>>>(),
            )),
            DataType::Decimal128(precision, scale) => {
                let payloads: Vec<Option<i128>> = cells.map(|value| payload_of(value, scale)).collect();
                Arc::new(
                    Decimal128Array::from(payloads)
                        .with_precision_and_scale(precision, scale)
                        .expect("a fixture decimal has a width and a scale"),
                )
            }
            _ => Arc::new(StringArray::from(
                cells
                    .map(|value| match value {
                        None | Some(&Value::Null) => None,
                        Some(other) => Some(other.render()),
                    })
                    .collect::<Vec<Option<String>>>(),
            )),
        }
    }

    /// A case's expected rows, as the Arrow batches a driver would have handed back for it.
    ///
    /// Built through `Accumulating` for the reason production is: `ResultBatches` has no other
    /// constructor, so a fixture cannot hand back a result the announced-schema guard would have
    /// refused.
    fn canned_rows(expected: &RowSet) -> ResultBatches {
        let mut fields = Vec::with_capacity(expected.columns().len());
        let mut arrays = Vec::with_capacity(expected.columns().len());
        for (index, label) in expected.columns().iter().enumerate() {
            let kind = kind_of(index, expected.rows());
            arrays.push(column_of(&kind, index, expected.rows()));
            fields.push(Field::new(label.as_str(), kind, true));
        }
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let mut accumulating = Accumulating::announcing(Arc::clone(&schema), expected.rows().len().max(1));
        if expected.rows().is_empty() {
            return accumulating.finish();
        }
        let batch = RecordBatch::try_new(schema, arrays).expect("a canned answer is rectangular");
        accumulating.push(batch).expect("a canned answer carries its own schema");
        accumulating.finish()
    }

    /// Never returned: every statement [`Canned`] is asked comes from a case it precomputed an
    /// answer for. Its own type rather than a shared one, because a lookup miss here is a defect in
    /// THIS fixture - a rendering change this file's own precompute did not follow - and not a
    /// question about what a real endpoint would have said.
    #[derive(Debug, thiserror::Error)]
    #[error("the conformance fixture rendered no case that matches this statement")]
    struct NoCannedAnswer;

    /// The fake transport this binding stands up unconditionally. See this file's own header for
    /// what a HELD behaviour over it does and does not establish.
    struct Canned {
        answers: BTreeMap<Key, ResultBatches>,
    }

    impl Canned {
        /// Renders every corpus case once, through the same function the adapter renders through,
        /// and remembers each one's answer under the exact request it will be asked for.
        fn from_corpus() -> Self {
            let mut answers = BTreeMap::new();
            for case in corpus::cases() {
                let rendered = generate(case.plan(), Dialect::BigQuery).expect("every corpus plan renders for BigQuery");
                let key = key_of(rendered.sql(), rendered.params());
                drop(answers.insert(key, canned_rows(case.expected())));
            }
            Self { answers }
        }
    }

    impl JobTransport for Canned {
        type Error = NoCannedAnswer;

        fn run(&self, request: &JobRequest<'_>) -> Result<ResultBatches, Self::Error> {
            let key = key_of(request.statement(), request.params());
            self.answers.get(&key).cloned().ok_or(NoCannedAnswer)
        }

        /// A fixed, honestly-fake byte estimate for every statement - see this file's header on
        /// `PRICES_DRY_RUN`. Not zero, so a caller reading `EstimatedBytes` as a real number would
        /// at least not read a free query.
        fn validate(&self, _request: &JobRequest<'_>) -> Result<DryRunEstimate, Self::Error> {
            Ok(Some(EstimatedBytes::parse(1024)))
        }

        /// Never asked: `execute_packs!` calls `execute`/`dry_run` only. An empty, unreported
        /// listing is the honest answer for a dataset this fake never declared anything about.
        fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
            Ok(HeldTables::of(BTreeSet::new(), ListingTotal::Unreported))
        }

        #[cfg(feature = "fixtures")]
        fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// `BigQueryWarehouse` over [`Canned`].
    ///
    /// **`Fixture::standing` unconditionally, and that is a claim rather than a wrapper** - see this
    /// file's header for why an always-canned transport is the honest way to make that claim for an
    /// adapter with no local tier.
    fn open() -> Fixture<BigQueryWarehouse<Canned>> {
        let warehouse = BigQueryWarehouse::new(
            corpus::source(),
            corpus::posture(),
            ProjectId::parse("conformance-fake").expect("a fixture project is a project"),
            DatasetId::parse("conformance").expect("a fixture dataset is a dataset"),
            Canned::from_corpus(),
        );
        Fixture::standing(warehouse)
    }

    // `refuses_legs`, because this adapter leaves `EXECUTES_LEGS` at its default - see this file's
    // header. The tag and the constant are torn apart by a `const` assertion inside the expansion,
    // so tagging it the other way does not build.
    //
    // The emitted names are `conformance::bigquery::<behaviour>`, which is what makes this adapter's
    // tier selectable on its own.
    sutura_conformance::execute_packs! {
        adapter: bigquery,
        warehouse: sutura_exec_bigquery::BigQueryWarehouse<crate::conformance::Canned>,
        open: crate::conformance::open,
        refuses_legs,
    }
}

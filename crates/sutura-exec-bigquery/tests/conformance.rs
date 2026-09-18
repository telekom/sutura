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
//! case's own [`sutura_conformance::corpus::Case::expected`] rows, converted to the wire's own
//! [`Cell`]/[`FieldType`] shapes. `run` looks the pair straight back up; a statement this pack never
//! rendered has no entry and the lookup fails loudly.
//!
//! **What that proves, stated so nobody reads this pack as a live-endpoint claim:** that this
//! adapter's OWN plumbing round-trips - the request it builds from a rendered plan, and the domain
//! `Value` a wire [`Cell`]/[`FieldType`] decodes back into, agree with what the corpus says an
//! answer to that plan looks like. **What it does NOT prove:** that a real `GoogleSQL` endpoint,
//! asked the rendered statement, returns those rows - nothing here executes SQL. That is
//! `tests/corpus.rs`'s claim, behind `wire`+`fixtures`, against a real dataset, and it is a
//! DIFFERENT and narrower thing than this file's `Fixture::Standing` claim is that it needed no
//! endpoint to make honestly.
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

    use sutura_conformance::{Fixture, corpus};
    use sutura_domain::warehouse::estimate::EstimatedBytes;
    use sutura_domain::warehouse::{ParamValue, RowSet, Value};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::transport::{
        Cell, DatasetAddress, DatasetId, DryRunEstimate, Field, FieldType, HeldTables, JobRequest, JobRows, JobTransport,
        ListingTotal, ProjectId,
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

    /// The one column type a domain [`Value`] round-trips through this adapter's own wire mapping
    /// exactly.
    ///
    /// **A whole-column decision over every row, not the first non-null cell** - `wide-total-by-day`
    /// and its two siblings are exactly the case that heuristic gets wrong: one day's total widens
    /// past `i64` and answers `Value::Text`, the other stays small and answers `Value::Integer`, and
    /// a real `BigQuery` reports ONE `NUMERIC` type for the whole column either way. `Value::Real`
    /// wins outright; a genuinely non-numeric `Text` (a dimension, a date) makes the column
    /// `String`; a numeric-looking `Text` anywhere makes it `Numeric` even where every OTHER row is
    /// a plain `Integer`, because that is the shape whose per-value fallback this proves.
    fn kind_of(column: usize, rows: &[Vec<Value>]) -> FieldType {
        let mut integer = false;
        let mut numeric_text = false;
        for value in rows.iter().filter_map(|row| row.get(column)) {
            match *value {
                Value::Null => {}
                Value::Real(_) => return FieldType::Float64,
                Value::Integer(_) => integer = true,
                Value::Text(ref text) if is_decimal_literal(text) => numeric_text = true,
                Value::Text(_) => return FieldType::String,
            }
        }
        if numeric_text {
            FieldType::Numeric
        } else if integer {
            FieldType::Int64
        } else {
            FieldType::String
        }
    }

    /// One cell, as the wire would send it - the inverse of `rowset::cell` in this crate's `src/`,
    /// and it has to be: what this file proves is that the two agree.
    fn cell_of(value: &Value) -> Cell {
        match *value {
            Value::Null => Cell::Null,
            Value::Integer(v) => Cell::Text(v.to_string()),
            Value::Real(v) => Cell::Text(v.get().to_string()),
            Value::Text(ref v) => Cell::Text(v.clone()),
        }
    }

    /// A case's expected rows, as the answer a wire transport would have sent for it.
    fn canned_rows(expected: &RowSet) -> JobRows {
        let kinds: Vec<FieldType> = (0..expected.columns().len())
            .map(|column| kind_of(column, expected.rows()))
            .collect();
        let fields: Vec<Field> = expected
            .columns()
            .iter()
            .zip(&kinds)
            .map(|(name, kind)| Field::of(name.clone(), kind.clone()))
            .collect();
        let rows: Vec<Vec<Cell>> = expected.rows().iter().map(|row| row.iter().map(cell_of).collect()).collect();
        let total = rows.len();
        JobRows::of(fields, rows, total)
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
        answers: BTreeMap<Key, JobRows>,
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

        fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
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

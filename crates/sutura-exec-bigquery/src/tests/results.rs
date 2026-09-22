//! Result mapping and refusal behavior under the fake transport.
//!
//! # MOST OF THIS FILE MOVED OR WENT WITH THE THING IT TESTED
//!
//! `docs/adr/0039` put the Arrow-to-`Value` mapping in
//! `sutura_domain::warehouse::arrow`, and the transport port now hands back Arrow rather than this
//! crate's own text cells. So:
//!
//! * The **value-mapping table** - every type this adapter maps, the whole-number decimal widening,
//!   the non-finite refusal, the unmapped-type refusal and the zero-row/all-null hole beside it -
//!   is `warehouse/arrow/tests.rs`, asserted once against the same `sutura-exec-duckdb` twin it was
//!   asserted against here. Keeping a copy would be two implementations of one agreement, which is
//!   what this change removes.
//! * A **declared number that did not parse** cannot happen any more. Those cells existed because
//!   the deleted HTTP transport received every value as a JSON string whatever its declared type
//!   was, so "declared INT64" and "parses as an integer" were two separate facts. There is no text
//!   to re-parse.
//! * A **ragged row** cannot happen either: an Arrow batch is columnar, so a row of the wrong width
//!   is not a value the type has.
//! * The **delivered-versus-reported** pair went with `jobs.query`'s paging. An ADBC read streams
//!   the whole result and `run` drains the reader, so completeness is the drain; the ADR records it.
//!
//! What is left here is what is still this adapter's own: the federated leg, the size-bound
//! delegation, and the row ceiling that replaced the page bound.

use super::{
    BigQueryError, Broken, Executable, Paged, Recording, Warehouse as _, impersonating_posture, leg_of, one_column, open, plan,
    shared_posture, test_deadline,
};

#[test]
fn a_federated_leg_is_submitted_as_the_asking_subject_at_this_sources_own_declared_account() {
    // **THE CELL `telekom/sutura#929`'s federation half asks for.** This arm used to answer
    // `LegWithoutCombiner` with the transport never touched, so `Warehouse::EXECUTES_LEGS` stayed at
    // its default and `sutura_app::federated`'s capability gate refused every `BigQuery` federation
    // before a credential existed. Three separate things have to hold for a leg to federate here,
    // and each one is a way this could be green while delivering nothing:
    //
    //  1. the constant, pinned BY VALUE - the gate reads `Warehouse::executes_legs()`, so a `false`
    //     here refuses the answer above this adapter and no assertion below would notice;
    //  2. the leg's statement REACHES THE TRANSPORT, and is `sutura_sql::generate_leg`'s own text
    //     for this dialect - the whole-plan renderer would have produced a grouped answer for the
    //     wrong question, and a hand-built one would be a second set of quoting decisions. This is
    //     also what puts a leg through `adbc::prepared`, the one funnel that sets
    //     `bigquery.query.max_bytes_billed`: `AdbcBigQuery::run` is the only caller of `connect`
    //     and `connect` the only caller of `prepared`, so a request that arrives here at all
    //     carries this source's configured ceiling;
    //  3. the leg carries BOTH halves of the subject's credential - the asker's own assertion AND
    //     the account this source declared for that asker (`#929` F3). A leg presenting the
    //     assertion with a dropped account runs every declared caller as the pool's own principal.
    let warehouse = open(Recording::empty(), impersonating_posture());
    // The INSTANCE method and not the constant, because that is what `answer_federated` reads -
    // `Warehouse::executes_legs` defaults to `EXECUTES_LEGS`, and the gate asks each leg's own
    // adapter instance so a closed enum over several kinds can answer per variant.
    assert!(
        warehouse.executes_legs(),
        "the capability gate in sutura_app reads this, and refuses the whole answer where it is false"
    );
    let leg = crate::tests::a_leg();
    let presented = leg_of(&impersonating_posture());
    drop(
        warehouse
            .execute(Executable::Leg(&leg), &presented, test_deadline())
            .expect("a leg renders and is submitted"),
    );
    let seen = warehouse.transport.seen.borrow();
    let [asked] = seen.as_slice() else {
        panic!("one leg is one job, and the transport saw {} of them", seen.len());
    };
    let rendered = sutura_sql::generate_leg(&leg, sutura_sql::Dialect::BigQuery).expect("the leg fixture renders");
    assert_eq!(
        asked.statement,
        rendered.sql(),
        "the leg reached the transport as something other than `generate_leg`'s own GoogleSQL"
    );
    assert_eq!(asked.params.len(), rendered.params().len(), "the values did not travel apart");
    assert_eq!(
        asked.subject.as_deref(),
        Some("an-exchanged-token-for-the-asker"),
        "the leg ran as somebody other than the asker"
    );
    assert_eq!(
        asked.impersonate.as_deref(),
        Some(super::fakes::A_DECLARED_ACCOUNT),
        "the account this source declared for the asker did not ride on the leg"
    );
}

#[test]
fn a_result_the_endpoint_would_not_return_at_once_is_a_size_bound_and_not_an_outage() {
    // THE defect the second bound exists for, at the adapter. `jobs.query` answers one page - as many
    // rows as fit the maximum permitted reply size - so a result UNDER the row cap can still be over
    // that, and both shapes that say so used to leave here as `BigQueryError` and reach a caller as
    // `503`: the status a dead endpoint produces, inviting a retry that returns the same page.
    //
    // Two shapes, and each is asked of the thing that knows. The page token is a fact about the wire
    // document, so the transport is asked - which is why the two failing transports below are
    // indistinguishable to `BigQueryError::Endpoint` and answer differently.
    let paged = open(Paged, shared_posture());
    let error = paged
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect_err("a paged result is not a result");
    assert!(
        paged.result_did_not_fit(&error),
        "a page of a larger result is a size bound: {error:?}"
    );

    // The control, and the reason this is not a test that says yes to everything: the same variant,
    // an error the adapter cannot tell from the one above, and a transport that does not claim the
    // bound. It must stay a failure.
    let broken = open(Broken, shared_posture());
    let refused = broken
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect_err("the endpoint said no");
    assert!(
        !broken.result_did_not_fit(&refused),
        "an endpoint that refused is not a result too large: {refused:?}"
    );
}

#[test]
fn an_unmapped_result_column_reaches_the_interior_rather_than_becoming_an_adapter_error() {
    // **What this asserts that the interior's own table does not: that this adapter is a
    // PASS-THROUGH.** The mapping and its refusals are `sutura_domain::warehouse::arrow`'s and are
    // tested there; what is this adapter's is that a column the domain maps no cell of leaves
    // `execute` as the driver's own column rather than as a `BigQueryError` - so the refusal names
    // the column and the Arrow type once, above the port, for every adapter at the same place.
    //
    // **This cell used to assert the opposite**, that `BigQueryError::Unreadable` wrapped the
    // interior's cause without flattening it. `docs/adr/0039` step 2's second half is what changed:
    // the port's currency is `ResultBatches`, so `execute` no longer decodes and there is no
    // adapter error left to carry a chain. `Self::rows` and that variant survive for the BOOT path,
    // which still reads an anchor's rows, and `verify_anchor`'s own cell is where that chain is
    // pinned now.
    //
    // A ZERO-ROW column, deliberately: it is the shape that used to be answered as a successful
    // empty result, because the cell mapping reads a null before it reads a type.
    let empty: arrow_array::ArrayRef = std::sync::Arc::new(arrow_array::Float32Array::from(Vec::<f32>::new()));
    let warehouse = open(Recording::answering(one_column(empty)), shared_posture());
    let answered = warehouse
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect("the driver's batch agrees with its own announced schema, so the port accepts it");
    let cause = answered
        .to_rows()
        .expect_err("an unmapped result column is refused above the port");
    match cause {
        sutura_domain::warehouse::UnreadableCell::UnsupportedType {
            ref column,
            ref arrow_type,
        } => {
            assert_eq!(column, "value");
            assert!(arrow_type.contains("Float32"), "{arrow_type}");
        }
        ref other => panic!("expected an unmapped type, got {other:?}"),
    }
}

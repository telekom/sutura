//! The node client against a loopback node over TLS: what crosses, what is refused before anything
//! is dialled, and how a node's closed code is read back.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{Presented, PrincipalName, Secret};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::remote::to_wire;
use sutura_domain::plan::{
    Executable, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanTerm, ResultLabel, StatementTables,
    range_filters,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::arrow::ResultBudget;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use crate::{NodeEndpoint, NodeError, NodeWarehouse};

mod fake;

use fake::{Node, Reply};

const TOKEN: &str = "exchanged-for-node-b";

fn source() -> SourceName {
    SourceName::parse("sales").expect("a source")
}

fn table() -> TableName {
    TableName::parse("orders").expect("a table")
}

fn column(raw: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(raw).expect("a column"))
}

/// One fact leg over June, its two range bounds bound as the parameters.
fn leg() -> LegPlan {
    let june = TimeRange::new(
        Date::parse("2026-06-01").expect("a date"),
        Date::parse("2026-07-01").expect("a date"),
    )
    .expect("June is a range");
    let mut params = Vec::new();
    let bounds = range_filters(june, &column("order_date"), &mut params);
    LegPlan::Fact {
        source: source(),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        tables: StatementTables::only(table()),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("order_date")),
        keys: Vec::new(),
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("amount_cents"),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(bounds.into(), params).expect("coherent"),
        range: june,
    }
}

/// One row under `labels`, every column an integer.
fn answer(labels: &[String]) -> RecordBatch {
    let fields: Vec<Field> = labels.iter().map(|label| Field::new(label, DataType::Int64, true)).collect();
    let columns: Vec<ArrayRef> = labels.iter().map(|_| Arc::new(Int64Array::from(vec![7])) as ArrayRef).collect();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).expect("a batch")
}

fn labels() -> Vec<String> {
    Executable::Leg(&leg()).result_labels()
}

fn client_trusting(node: &Node, anchor: tokio_rustls::rustls::pki_types::CertificateDer<'static>) -> NodeWarehouse {
    NodeWarehouse::open(
        source(),
        SourcePosture::ImpersonationAtSource,
        &NodeEndpoint::parse(&format!("https://localhost:{}", node.port)).expect("an https origin"),
        [anchor],
        ResultBudget::of_bytes(NonZeroUsize::new(1 << 20).expect("non-zero")),
    )
    .expect("a node source builds")
}

fn subject() -> Presented {
    Presented::SubjectToken {
        material: Secret::new(TOKEN),
    }
}

fn deadline() -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"))
}

fn run(warehouse: &NodeWarehouse, presented: &Presented) -> Result<sutura_domain::warehouse::ResultBatches, NodeError> {
    warehouse.execute(Executable::Leg(&leg()), presented, deadline())
}

#[test]
fn a_leg_crosses_as_one_exchange_carrying_its_bearer_and_its_values() {
    let node = Node::answering([Reply::Answer(vec![answer(&labels())])]);
    let warehouse = client_trusting(&node, node.anchor.clone());
    let answered = run(&warehouse, &subject()).expect("the node answers");
    assert_eq!(answered.rows(), 1);
    let [received] = <[_; 1]>::try_from(node.received()).ok().expect("one call per leg");
    assert_eq!(received.bearer.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
    assert!(received.timeout, "the deadline travels as grpc-timeout");
    assert_eq!(received.leg, to_wire(&leg()).expect("the leg sends"));
    assert_eq!(received.params.num_rows(), 1);
    assert_eq!(received.params.num_columns(), leg().params().len());
    let descriptor = serde_json::to_string(&received.leg).expect("a leg serializes");
    assert!(!descriptor.contains(TOKEN), "the token rides in metadata, never in the leg");
    assert!(warehouse.relays_to_a_node(), "a node source is another node, for decision 9");
    assert_eq!(<NodeWarehouse as sutura_domain::warehouse::Warehouse>::IMPERSONATION, ImpersonationCapability::PerSubjectCredential);
}

#[test]
fn a_node_source_sends_only_a_subject_token() {
    let node = Node::answering([]);
    let warehouse = client_trusting(&node, node.anchor.clone());
    let declared = SharedIdentityDeclared::of(AcknowledgementReason::parse("one identity for everybody").expect("a reason"));
    let name = PrincipalName::parse("analyst_a").expect("a principal");
    for presented in [
        Presented::SharedServiceUser { declared },
        Presented::SubjectPrincipal { name: name.clone() },
        Presented::SubjectSignIn {
            name,
            material: Secret::new(TOKEN),
        },
    ] {
        let refused = run(&warehouse, &presented).expect_err("only a subject's token is sent");
        assert!(matches!(refused, NodeError::Undeliverable { .. }), "{refused:?}");
    }
    assert!(node.received().is_empty(), "nothing was dialled");
}

#[test]
fn a_node_refusal_is_read_as_its_closed_code() {
    let named = source();
    let codes = [
        RefusalReason::RemoteLegNotCertified { source: named.clone() }.code(),
        RefusalReason::SourceRefused { source: named }.code(),
        "deadline_exceeded",
        "result_too_large",
        "budget_exhausted",
    ];
    let node = Node::answering(codes.map(|code| Reply::Refuse(tonic::Code::FailedPrecondition, code)));
    let warehouse = client_trusting(&node, node.anchor.clone());
    let read: Vec<[bool; 4]> = codes
        .iter()
        .map(|_| {
            let refused = run(&warehouse, &subject()).expect_err("the node refused");
            [
                warehouse.leg_not_certified(&refused),
                warehouse.source_refused(&refused),
                warehouse.deadline_exceeded(&refused),
                warehouse.result_did_not_fit(&refused),
            ]
        })
        .collect();
    assert_eq!(
        read,
        [
            [true, false, false, false],
            [false, true, false, false],
            [false, false, true, false],
            [false, false, false, true],
            [false, false, false, false],
        ]
    );
}

#[test]
fn an_answer_under_labels_the_leg_does_not_project_is_refused() {
    let node = Node::answering([Reply::Answer(vec![answer(&[String::from("other")])])]);
    let refused = run(&client_trusting(&node, node.anchor.clone()), &subject()).expect_err("the labels differ");
    assert!(matches!(refused, NodeError::Labels), "{refused:?}");
}

#[test]
fn a_node_whose_certificate_is_not_trusted_is_never_sent_the_leg() {
    let node = Node::answering([Reply::Answer(vec![answer(&labels())])]);
    let stranger = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a pair generates");
    let refused = run(&client_trusting(&node, stranger.cert.der().clone()), &subject()).expect_err("an untrusted node");
    assert!(matches!(refused, NodeError::Unreachable { .. }), "{refused:?}");
    assert!(node.received().is_empty(), "the leg never reached it");
}

#[test]
fn a_spent_deadline_dials_nothing() {
    let node = Node::answering([]);
    let spent = Deadline::opened_at(
        Instant::now().checked_sub(Duration::from_secs(10)).expect("ten seconds ago"),
        Budget::parse(Duration::from_secs(1)).expect("a budget"),
    );
    let refused = client_trusting(&node, node.anchor.clone())
        .execute(Executable::Leg(&leg()), &subject(), spent)
        .expect_err("no time left");
    assert!(matches!(refused, NodeError::DeadlineExceeded), "{refused:?}");
    assert!(node.received().is_empty());
}

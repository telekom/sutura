//! `run_sql` over a deployment the raw tool does not serve, through a real client: a refusal the
//! caller reads, never the misconfiguration failure a wiring defect gets.

use sutura_app::surface::LocalService;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Value, Warehouse};

use crate::testing;

/// The fixture warehouse under a name of its own, so two of them open together.
struct Named {
    source: SourceName,
    inner: sutura_app::Warehouses<testing::FakeWarehouse>,
    anchor: RowSet,
}

impl Named {
    fn open(name: &str) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source: SourceName::parse(name).expect("a test source is a source"),
            inner: testing::fake_warehouse(),
            anchor: RowSet::new(
                vec![String::from("revenue")],
                vec![vec![Value::Integer(testing::ANCHORED_VALUE)]],
            )
            .expect("a one-cell result is a result set"),
        })
    }

    fn fake(&self) -> &testing::FakeWarehouse {
        self.inner.each().next().expect("the fixture opens one warehouse").1
    }
}

impl Warehouse for Named {
    type Error = testing::Unreachable;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        self.fake().posture()
    }

    fn dry_run(
        &self,
        executable: Executable<'_>,
        presented: &sutura_domain::identity::Presented,
        deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        self.fake().dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &sutura_domain::identity::Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.fake().execute(executable, presented, deadline)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.anchor.clone()))
    }
}

type Service = LocalService<
    Named,
    std::sync::Arc<testing::CountingSink>,
    testing::GrantsTheSharedIdentity,
    sutura_domain::plan::RefusingCombiner,
>;

fn over(warehouses: sutura_app::Warehouses<Named>) -> Service {
    LocalService::start(
        &testing::FixedCatalog,
        warehouses,
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the fixture bundle validates")
}

async fn refusal_code(service: Service) -> (String, String) {
    let client = super::connected(service).await;
    let result = client
        .call_tool(super::raw(&serde_json::json!({ "statement": "select 1" })))
        .await
        .expect("the raw tool answers");
    assert_ne!(
        result.is_error,
        Some(true),
        "a refusal is a tool result, not a fault: {result:?}"
    );
    let structured = result
        .structured_content
        .as_ref()
        .expect("a refusal carries structured content");
    assert_eq!(structured["outcome"], "raw_refusal", "{structured}");
    let text = |key: &str| {
        structured[key]
            .as_str()
            .expect("a refusal names its code and detail")
            .to_owned()
    };
    let answered = (text("code"), text("detail"));
    drop(client.cancel().await);
    answered
}

#[tokio::test]
async fn run_sql_over_two_open_sources_is_a_refusal_naming_the_cause() {
    let both = Named::open("local")
        .merge(Named::open("other"))
        .expect("two distinct sources open together");
    let (code, detail) = refusal_code(over(both)).await;
    assert_eq!(code, "more_than_one_source_open");
    assert!(detail.contains("certified tools"), "{detail}");
}

#[tokio::test]
async fn run_sql_over_one_source_that_takes_no_raw_statement_is_a_refusal_naming_the_cause() {
    let (code, detail) = refusal_code(over(Named::open("local"))).await;
    assert_eq!(code, "no_source_accepts_raw_statements");
    assert!(detail.contains("tools.run_sql.enabled"), "{detail}");
}

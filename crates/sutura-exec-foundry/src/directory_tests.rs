//! The object-type directory a source resolves an API name through: the RID a statement names, a
//! name no read holds refused naming the table, reads merged rather than replaced, the startup
//! identity's reads kept apart, the exclusions, and a RID Foundry no longer finds read again. Over
//! fakes of the transport and of the list; no tenant is involved.

use std::collections::BTreeSet;
use std::sync::Arc;

use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::{ObjectTypeExclusions, QualifiedTable, ResourceId, SourceName};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::preflight::TablesPresent;
use sutura_domain::warehouse::{ResultBudget, Warehouse as _};

use super::{
    DirectoryFailed, FoundryError, FoundryTransport, FoundryWarehouse, Listed, ObjectTypeDirectory, OntologyIdentifier,
    Reference, ResourceIds, SqlRequest,
};

const AIRCRAFT: &str = "ri.ontology.main.object-type.00000000-0000-0000-0000-000000000005";
const AIRPORT: &str = "ri.ontology.main.object-type.00000000-0000-0000-0000-000000000010";
const SYSTEM: &str = "ri.ontology.main.object-type.00000000-0000-0000-0000-000000000092";

/// What the fake answers each statement with.
#[derive(Debug, thiserror::Error)]
enum Answer {
    #[error("not found")]
    NotFound,
}

/// Every statement it is handed; answers each with the next scripted outcome, then success.
/// `Debug` names nothing it holds, so a source's `Debug` is the directory's alone.
#[derive(Default)]
struct Statements {
    sent: parking_lot::Mutex<Vec<String>>,
    next: parking_lot::Mutex<Vec<Answer>>,
}

impl core::fmt::Debug for Statements {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Statements")
    }
}

impl FoundryTransport for Arc<Statements> {
    type Error = Answer;

    fn execute(&self, request: &SqlRequest, _token: &Secret, _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        let query = serde_json::to_value(request)
            .ok()
            .and_then(|body| body["query"].as_str().map(String::from))
            .unwrap_or_default();
        self.sent.lock().push(query);
        let mut next = self.next.lock();
        if next.is_empty() {
            Ok(Vec::new())
        } else {
            Err(next.remove(0))
        }
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }

    fn object_not_found(&self, error: &Self::Error) -> bool {
        matches!(error, Answer::NotFound)
    }
}

/// Answers each read with the next scripted list, and records the token each read carried.
#[derive(Debug)]
struct Listing {
    answers: parking_lot::Mutex<Vec<Script>>,
    tokens: parking_lot::Mutex<Vec<String>>,
}

/// One scripted read: the RIDs by API name, the names listed with no RID, the excluded RIDs.
type Script = (ResourceIds, BTreeSet<String>, BTreeSet<ResourceId>);

impl ObjectTypeDirectory for Arc<Listing> {
    fn listed(&self, token: &Secret, _deadline: Deadline) -> Result<Listed, DirectoryFailed> {
        #[expect(clippy::disallowed_methods, reason = "the cell asserts whose token listed")]
        let token = String::from(token.expose_secret());
        self.tokens.lock().push(token);
        let mut answers = self.answers.lock();
        let (ids, unusable, excluded) = if answers.len() > 1 {
            answers.remove(0)
        } else {
            answers[0].clone()
        };
        Ok(Listed::new(ids, unusable, excluded))
    }
}

fn rid(raw: &str) -> ResourceId {
    ResourceId::parse(raw).expect("a resource identifier")
}

fn ids(pairs: &[(&str, &str)]) -> Script {
    (
        pairs
            .iter()
            .map(|&(api_name, id)| (String::from(api_name), rid(id)))
            .collect(),
        BTreeSet::new(),
        BTreeSet::new(),
    )
}

/// A per-caller source over the fakes, the statements it sent and the reads it made.
type Fixture = (FoundryWarehouse<Arc<Statements>>, Arc<Statements>, Arc<Listing>);

fn source(answers: Vec<Script>) -> Fixture {
    let statements = Arc::new(Statements::default());
    let listing = Arc::new(Listing {
        answers: parking_lot::Mutex::new(answers),
        tokens: parking_lot::Mutex::new(Vec::new()),
    });
    let warehouse = FoundryWarehouse::each_caller(
        SourceName::parse("foundry").expect("the source parses"),
        OntologyIdentifier::parse("sales_ontology").expect("the ontology parses"),
        Arc::clone(&statements),
        ResultBudget::of_bytes(core::num::NonZeroUsize::MAX),
    )
    .with_directory(Arc::clone(&listing))
    .with_exclusions(ObjectTypeExclusions::default());
    (warehouse, statements, listing)
}

fn table(api_name: &str) -> QualifiedTable {
    QualifiedTable::parse(format!("`{api_name}`")).expect("a table")
}

fn caller(token: &str) -> Presented {
    Presented::SubjectToken {
        material: Secret::new(token),
    }
}

fn deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(core::time::Duration::from_secs(30)).expect("the fixture budget is non-zero"),
    )
}

fn probe(warehouse: &FoundryWarehouse<Arc<Statements>>, api_name: &str, token: &str) -> Result<(), FoundryError<Answer>> {
    warehouse.parse_object(&table(api_name), Reference::Resolved, &caller(token), deadline())
}

#[test]
fn an_object_type_is_queried_by_the_rid_the_list_read_under_the_callers_token_returns() {
    let (warehouse, statements, listing) = source(vec![ids(&[("Aircraft", AIRCRAFT)])]);
    probe(&warehouse, "Aircraft", "caller-a").expect("a listed object type resolves");
    let sent = statements.sent.lock().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(&format!("`{AIRCRAFT}`")), "{}", sent[0]);
    assert!(
        !sent[0].contains("`Aircraft`"),
        "the API name reached the statement: {}",
        sent[0]
    );
    assert_eq!(*listing.tokens.lock(), ["caller-a"]);

    warehouse
        .parse_object(&table("Aircraft"), Reference::AsWritten, &caller("caller-a"), deadline())
        .expect("the probe as written is sent");
    assert!(statements.sent.lock()[1].contains("`Aircraft`"));
    assert_eq!(listing.tokens.lock().len(), 1, "a probe as written reads no list");
}

#[test]
fn an_api_name_no_read_holds_is_refused_naming_the_table_and_nothing_is_sent() {
    let (warehouse, statements, _) = source(vec![ids(&[("Aircraft", AIRCRAFT)])]);
    let refused = probe(&warehouse, "Airport", "caller-a");
    assert!(
        matches!(&refused, Err(FoundryError::UnknownObjectType { table }) if table.as_str() == "Airport"),
        "{refused:?}"
    );
    assert!(
        warehouse.source_refused(&refused.expect_err("refused")),
        "the same refusal a hidden type gets"
    );
    assert!(
        statements.sent.lock().is_empty(),
        "a statement was sent for an unknown object type"
    );
}

#[test]
fn a_name_that_did_not_resolve_is_not_read_again_for_that_token_and_is_for_another() {
    let (warehouse, _, listing) = source(vec![ids(&[("Aircraft", AIRCRAFT)])]);
    for _ in 0..3 {
        assert!(matches!(
            probe(&warehouse, "Airport", "caller-a"),
            Err(FoundryError::UnknownObjectType { .. })
        ));
    }
    assert_eq!(*listing.tokens.lock(), ["caller-a"], "the same token read the list again");
    assert!(matches!(
        probe(&warehouse, "Airport", "caller-b"),
        Err(FoundryError::UnknownObjectType { .. })
    ));
    assert_eq!(
        *listing.tokens.lock(),
        ["caller-a", "caller-b"],
        "one token's unresolved name was held for another"
    );
}

#[test]
fn the_source_debug_shows_no_api_name_or_resource_identifier() {
    let (warehouse, _, _) = source(vec![ids(&[("Aircraft", AIRCRAFT)])]);
    probe(&warehouse, "Aircraft", "caller-a").expect("resolves");
    drop(probe(&warehouse, "Airport", "caller-a"));
    let shown = format!("{warehouse:?}");
    for leaked in ["Aircraft", "Airport", AIRCRAFT, "caller-a"] {
        assert!(!shown.contains(leaked), "{leaked} in {shown}");
    }
    assert!(shown.contains("names: 1"), "{shown}");
}

#[test]
fn a_narrower_read_adds_to_the_map_and_never_evicts_another_callers_entries() {
    let (warehouse, _, listing) = source(vec![ids(&[("Aircraft", AIRCRAFT)]), ids(&[("Airport", AIRPORT)])]);
    probe(&warehouse, "Aircraft", "caller-a").expect("caller a's read holds it");
    probe(&warehouse, "Airport", "caller-b").expect("a name the map lacks is read under the asking token");
    probe(&warehouse, "Aircraft", "caller-c").expect("caller b's narrower read did not evict it");
    probe(&warehouse, "Airport", "caller-a").expect("both are held");
    assert_eq!(*listing.tokens.lock(), ["caller-a", "caller-b"], "a held name was read again");
}

#[test]
fn an_anchors_reads_never_fill_the_map_questions_read() {
    let (warehouse, _, listing) = source(vec![ids(&[("Aircraft", AIRCRAFT)])]);
    let warehouse = warehouse.with_boot_identity(Secret::new("startup-identity"));
    // The anchor path is crate-private; its lookup is the same call with the boot map.
    let tables = [&table("Aircraft")];
    super::resolve::references::<Answer>(
        warehouse.directory.as_ref(),
        &warehouse.exclusions,
        super::Whose::Boot,
        &tables,
        &Secret::new("startup-identity"),
        deadline(),
    )
    .expect("the startup identity's read resolves it");
    probe(&warehouse, "Aircraft", "caller-a").expect("the question reads its own");
    assert_eq!(
        *listing.tokens.lock(),
        ["startup-identity", "caller-a"],
        "a question was answered from a read the startup identity made"
    );
}

#[test]
fn a_type_listed_without_a_rid_refuses_only_a_question_that_needs_it() {
    let mut script = ids(&[("Aircraft", AIRCRAFT)]);
    script.1.insert(String::from("Airport"));
    let (warehouse, _, _) = source(vec![script]);
    probe(&warehouse, "Aircraft", "caller-a").expect("the usable entry still resolves");
    let refused = probe(&warehouse, "Airport", "caller-a");
    assert!(
        matches!(&refused, Err(FoundryError::NoResourceId { table }) if table.as_str() == "Airport"),
        "{refused:?}"
    );
}

#[test]
fn an_excluded_type_is_refused_by_api_name_at_boot_and_by_rid_at_query_time() {
    let mut script = ids(&[("Aircraft", AIRCRAFT)]);
    script.2.insert(rid(SYSTEM));
    let (warehouse, statements, _) = source(vec![script]);
    let platform = table("com.palantir.system.Evolution");
    #[expect(clippy::disallowed_methods, reason = "the cell asks the pre-flight what boot asks it")]
    let preflight = warehouse
        .preflight(&BTreeSet::from([platform.clone(), table("Aircraft")]))
        .expect("the pre-flight asks nothing");
    assert!(
        matches!(&preflight, TablesPresent::AllBut(absent) if absent.named() == &BTreeSet::from([platform])),
        "{preflight:?}"
    );
    let refused = probe(&warehouse, "com.palantir.system.Evolution", "caller-a");
    assert!(matches!(refused, Err(FoundryError::ExcludedObjectType { .. })), "{refused:?}");
    let by_rid = warehouse.parse_object(
        &QualifiedTable::resource(rid(SYSTEM)),
        Reference::Resolved,
        &caller("caller-a"),
        deadline(),
    );
    assert!(matches!(by_rid, Err(FoundryError::ExcludedObjectType { .. })), "{by_rid:?}");
    assert!(statements.sent.lock().is_empty(), "an excluded type reached Foundry");
}

#[test]
fn a_rid_foundry_no_longer_finds_is_dropped_and_read_again_by_the_next_question() {
    let (warehouse, statements, listing) = source(vec![ids(&[("Aircraft", AIRCRAFT)]), ids(&[("Aircraft", AIRPORT)])]);
    statements.next.lock().push(Answer::NotFound);
    let stale = probe(&warehouse, "Aircraft", "caller-a");
    assert!(matches!(stale, Err(FoundryError::StaleObjectType { .. })), "{stale:?}");
    assert!(
        warehouse.source_refused(&stale.expect_err("stale")),
        "the same refusal a hidden type gets"
    );
    probe(&warehouse, "Aircraft", "caller-a").expect("the recreated type resolves again");
    assert_eq!(listing.tokens.lock().len(), 2, "the stale entry was not read again");
    let sent = statements.sent.lock().clone();
    assert!(
        sent[1].contains(&format!("`{AIRPORT}`")),
        "the new RID was not named: {}",
        sent[1]
    );
}

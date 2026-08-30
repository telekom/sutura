//! What both acceptance legs need from the environment, and how the adapter is opened over it.
//!
//! **One module rather than two copies, for `crates/sutura-app/tests/adapters/mod.rs`'s own reason:**
//! two copies of "open this data system over the developer's project" is two things to keep in step,
//! and the bound the [`bounds`] function sets is the one path in this repository that spends real
//! money. A second copy that drifted by a digit would be a leg that bills differently from the one a
//! reviewer read.
//!
//! In `tests/support/mod.rs` rather than `tests/support.rs` so cargo does not build it as a test
//! target of its own. It is shared by both legs, which is why **nothing in it is target-specific**:
//! `dead_code` is `deny` in the workspace lint table, so an item only one target used would fail the
//! build of the other. `SUTURA_BQ_TABLE` is therefore read by `acceptance.rs` and not here - the
//! corpus leg creates its own tables and has no use for it - while [`named`] is here, because both
//! legs reach it.
//!
//! # The three values, and where each comes from
//!
//! | Value | Where it comes from |
//! | --- | --- |
//! | the billing project | the CREDENTIAL's own `project_id` where it names one, else `SUTURA_BQ_BILLING_PROJECT` |
//! | the dataset | `SUTURA_BQ_DATASET` |
//! | the credential | `GOOGLE_APPLICATION_CREDENTIALS`, or the well-known application-default path |
//!
//! **Their names are here and their values are not, and will not be** - a project id is one of the
//! things this repository does not write down, and `.envrc` already sources a file under the user's
//! own configuration directory for exactly this class of value.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_exec_bigquery::BigQueryWarehouse;
use sutura_exec_bigquery::transport::{DatasetId, ProjectId};
use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

/// The adapter, wired to the endpoint - the type both legs execute through.
pub(crate) type Wired = BigQueryWarehouse<BigQueryWire<Credential>>;

/// What every job either leg submits is bounded by.
///
/// **The deadline is DERIVED from `server.request_timeout_seconds` rather than copied from it**,
/// which is the correction review forced on the smoke leg. Thirty seconds is the shipped default, and
/// thirty seconds is NOT what one call may spend: one answer calls the port twice and each call pays
/// connection setup on top of its own budget, so the number a caller wants is the share -
/// `QueryDeadline::within_request_timeout` does that arithmetic once, here and in whichever
/// composition root links this crate.
///
/// **1 GiB because these legs are the one path that spends real money.** A developer or a CI job
/// pointed at a partitioned table with years of history gets `bytesBilledLimitExceeded` from the
/// service, unbilled, instead of discovering the scan on an invoice. Nothing else in this repository
/// bounds bytes scanned - the row cap bounds rows RETURNED, not bytes read.
///
/// **What it costs the corpus leg is worth stating, because the number sounds generous next to a
/// 40 KB fixture set:** on-demand billing has a 10 MiB minimum per table referenced per query, so
/// what the corpus spends is set by the number of QUERIES and not by the size of the tables. The
/// ceiling is there for the case where somebody points this at a dataset that already holds
/// something large under one of the four fixture names.
pub(crate) fn bounds() -> JobBounds {
    JobBounds::of(
        QueryDeadline::within_request_timeout(30).expect("the shipped request timeout leaves a budget"),
        BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
    )
}

/// One variable, or a panic naming it and saying what it is for.
///
/// **It FAILS when a value is absent, and it used to SKIP - the reversal is the point of this
/// paragraph.** The skipping version printed `SKIPPED - ... is not set` and returned, and every test
/// then reported **PASS** with no project anywhere. That is a green nobody asked for, over exactly
/// the claim these legs exist to make.
///
/// **Why the compose tier's direction is right there and wrong here**, since it does skip: its cells
/// run inside `just test`, so failing would break the suite on every machine with no docker. These
/// tests are `#[ignore]`d, so the only way to reach one is to type `just bigquery-acceptance` - which
/// is already a statement of intent. A developer who asked for acceptance and got green ticks
/// against nothing has been told the opposite of the truth.
///
/// A panic rather than a `Result`, because a test's own precondition is not an outcome a test
/// reports on - and because the message is the whole product here: it has to name what a developer
/// has to set.
pub(crate) fn named(key: &str, what: &str) -> String {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) | Err(_) => panic!(
            "{key} is not set - it names {what}. This is an acceptance leg: it needs a real project. \
             See the header of the test file, and `docs/adr/0017`"
        ),
    }
}

/// The credential and the two resource names, or a panic saying exactly what is missing.
///
/// **`pub(crate)` fields rather than accessors, and the reason is a lint rather than taste.**
/// `dead_code` is `deny` here and a shared test module is compiled once per target, so an accessor
/// only one leg called would fail the build of the other. The fields are read by [`opened`] in this
/// module, which both legs call, and directly by the smoke leg where it builds a qualified path.
/// Nothing is being protected by a getter that a `pub(crate)` field in a test module would not
/// protect equally: these three values were parsed on the way in, by [`Self::required`].
pub(crate) struct Connection {
    pub(crate) billing_project: ProjectId,
    pub(crate) dataset: DatasetId,
    pub(crate) credentials: Credential,
}

impl Connection {
    /// Reads the environment, or panics naming what a developer has to set.
    pub(crate) fn required() -> Self {
        let agent = WireAgent::pinned(bounds());
        let file = CredentialFile::well_known().expect("this machine names a well-known credential location");
        let credentials = Credential::read(&file, agent).expect(
            "a credential file is readable - run `just gcloud-login`, or point \
             GOOGLE_APPLICATION_CREDENTIALS at a service-account key",
        );
        // Printed so a green run says WHICH identity produced it: the two kinds are two deployment
        // shapes, and an operator reading a log needs to know which one answered. A fixed word from a
        // closed match, never the file's own text.
        println!("bigquery-acceptance: credential kind {}", credentials.kind());

        // The key's own `project_id` first, then the variable, then a panic. A service-account key
        // carries it; an application-default login does not, so a developer on a laptop still sets the
        // variable and CI sets nothing. It also removes a second answer to *who pays* that could
        // disagree with the first - the same argument the wire makes for not reading
        // `quota_project_id`.
        let billing = credentials.project().cloned().unwrap_or_else(|| {
            named(
                "SUTURA_BQ_BILLING_PROJECT",
                "this credential names no project of its own, so the billing project has to be set",
            )
        });
        Self {
            billing_project: ProjectId::parse(billing).expect("a project id parses"),
            dataset: DatasetId::parse(named("SUTURA_BQ_DATASET", "the dataset an unqualified table resolves in"))
                .expect("a dataset id parses"),
            credentials,
        }
    }
}

/// The one acknowledgement both the posture and the presented leg are built from.
///
/// **One function rather than two literals**, because `Presented::agrees_with` compares the two
/// witnesses for equality - so two copies of this sentence that drifted by a character would be a
/// leg refused for a reason that has nothing to do with `BigQuery`.
fn declared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a developer's own application-default credential, reaching their own project")
            .expect("an acknowledgement is an acknowledgement"),
    )
}

/// The posture these legs run under, which is the only one this adapter can deliver.
///
/// A developer's own application-default credential, and equally a service-account key, IS one
/// identity for everybody who asks - so `SharedServiceUser` is the honest declaration and not a
/// placeholder. What that means for what a green run proves: *accepted, and correct for that
/// identity*, and nothing whatever about per-subject execution.
fn posture() -> SourcePosture {
    SourcePosture::SharedServiceUser { declared: declared() }
}

/// The credential one leg presents, read off [`posture`] so the two cannot drift.
pub(crate) fn presented() -> Presented {
    Presented::SharedServiceUser { declared: declared() }
}

/// The adapter, opened over the connection under the given source name, bounded as told.
///
/// **The composition, and it is a thing these legs are evidence for beside the round trip:** one
/// [`WireAgent`] carrying the bounds, one [`Credential`] behind the token port, one transport behind
/// the seam, one warehouse behind the domain's port. The agent is the pinned kind because it is the
/// only kind either half accepts, which is what makes the wire's claims properties of the types
/// rather than of a test file.
///
/// The source NAME is a parameter because the two legs need different ones: the smoke leg names its
/// own, and the corpus leg has to answer to the name the example catalog's models declare, since that
/// is what a plan selects a warehouse with.
///
/// **The BOUNDS are a parameter for a reason measured in CI rather than reasoned about.** [`bounds`]
/// derives its deadline from `server.request_timeout_seconds`, which is the budget for ANSWERING A
/// CALLER - and the corpus leg's fixture LOAD answers nobody. A run on 2026-08-31 failed with
/// `NotComplete` on an eight-row `CREATE OR REPLACE TABLE`: the endpoint had not finished the job
/// inside the request-derived share, which is around twelve seconds. So the caller decides, and the
/// corpus leg gives its loader a deadline of its own while keeping the money ceiling identical.
pub(crate) fn opened(source: SourceName, connection: Connection, bounds: JobBounds) -> Wired {
    BigQueryWarehouse::new(
        source,
        posture(),
        connection.billing_project,
        connection.dataset,
        BigQueryWire::new(WireAgent::pinned(bounds), connection.credentials),
    )
}

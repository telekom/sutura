//! Asking one certified question.
//!
//! # A refusal says so in the status, the code and the sentence
//!
//! The single most important thing about this handler, and it is the opposite of what this comment
//! said until now. `ToolOutcome::Refusal` is still a *result* and not an `Err` - that invariant is
//! the domain's and is untouched - but a caller is told about it three ways: an explicit status, the
//! stable `code` in the body, and a sentence that says what to do. `crate::wire::refusal` holds the
//! mapping, one exhaustive match, with the reason for each status written beside it.
//!
//! **The old argument was that a `4xx` invites a client library to retry, and that retrying a
//! governance decision until it succeeds is what the refusal exists to prevent.** The second half is
//! true. The first half is not, and it is checkable rather than arguable: no mainstream client
//! retries a `4xx` by default. `urllib3.util.Retry` - what `requests` mounts - documents
//! `status_forcelist` as "By default, this is disabled with `None`", so no status is retried at all
//! until somebody names one. `reqwest` 0.13's `retry` module documents its default as "to only retry
//! requests where an error or low-level protocol NACK is encountered that is known to be safe to
//! retry" - a transport condition, not a status. `axios` retries nothing on its own, and
//! `axios-retry` defaults to "a network error or a 5xx error on an idempotent request". Go's
//! `net/http` reference documents no status-driven retry anywhere. The statuses that *are* retried by
//! convention are `429` and `408`, and no refusal maps to either. `422` - where four of them land -
//! is documented the other way round: "Clients that receive a `422` response should expect that
//! repeating the request without modification will fail with the same error."
//!
//! **What the `200` actually cost is what nobody priced.** A refusal answered `200` is
//! indistinguishable from an answer to everything that reads a status and not a body: an ingress
//! log, a dashboard, an error-rate alert, a client's `raise_for_status()`, a generated client whose
//! success branch is `2xx`. A deployment refusing every question read as perfectly healthy. So the
//! statuses in this handler's documented responses are now about both kinds of thing - a refusal,
//! and the failures that are not one - and each entry says which codes reach it.
//!
//! # Why the call goes onto the blocking pool
//!
//! `Warehouse` is a synchronous port, and the in-process engine behind it drives its own
//! single-threaded runtime and blocks on it. Calling that from an `async` handler on a worker
//! thread panics - a runtime cannot be entered from within a runtime - and the panic happens at
//! collect time, deep inside the engine, which is a long way from the line that caused it.
//! `spawn_blocking` is not an optimisation here; it is the only correct way to call this port.
//!
//! The current span is carried across, so the lines the engine emits belong to the same request as
//! the lines this handler emits. That is `sutura_runtime::spawn_carrying_span` rather than three
//! lines written here, and `tokio::task::spawn_blocking` is banned in `clippy.toml` to keep the
//! next call site from having to remember: a pool thread has no current span, and a line emitted
//! outside the request's span is a line nothing ties to the request - which is invisible, because
//! the code compiles and the answer is right.
//!
//! # The slot, and exactly what it bounds
//!
//! **A slot is taken before the blocking task is spawned, and it is moved into that task.** Both
//! halves matter, and the second is the one that is easy to get wrong.
//!
//! The request timeout is a deadline on the *reply*. When it expires the caller is answered `408`
//! and this handler's future is dropped - and `tokio` documents that a started `spawn_blocking`
//! task cannot be aborted and that runtime shutdown waits for one. So the question keeps running
//! after the caller has been answered. Before the slot there was nothing bounding how many were
//! doing that: the blocking pool defaults to 512 threads with an unbounded queue, so a caller
//! asking questions that cost more than the timeout accumulated them at the rate limit and the only
//! real bound was memory - which also defeats the bounded shutdown this service advertises.
//!
//! What the slot fixes is the *count*. What it does not fix, and cannot:
//!
//! * **It does not cancel anything.** The `Warehouse` port is synchronous and carries no
//!   cancellation token, so a timed-out question runs to completion holding its slot. That is
//!   precisely why the slot is moved into the blocking task rather than held by this future: a slot
//!   released when the caller gives up would count *callers*, and the backlog would be unbounded
//!   again with a number in front of it that looked like a limit. The honest statement is that the
//!   backlog is now a number somebody chose instead of memory - not that a timeout cancels work.
//! * **It is not per-caller.** One caller can fill every slot and shed everybody else. Nothing here
//!   can tell two callers apart, because there is no identity to tell them apart by - see the crate
//!   documentation. The rate limiter bounds an address's *rate*; this bounds the deployment's
//!   *concurrency*.
//! * **It does not bound how long one question takes.** A single question that runs for an hour
//!   holds its slot for an hour, and no configuration here changes that.
//!
//! A panicking blocking task releases the slot by unwinding out of the closure, which is what
//! happens under a test profile. The shipped profiles set `panic = "abort"`, so there the process is
//! gone and the slot is moot.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use sutura_domain::query::Query;
use sutura_runtime::AtCapacity;

use crate::problem::Failure;
use crate::state::ServiceState;
use crate::surface::SurfaceFailure;
use crate::wire::{Outcome, OutcomeBody, QuestionBody};

/// The tag this route is grouped under in the generated document.
const TAG: &str = "query";

/// Answers one modelled question, or says why it will not.
#[utoipa::path(
    post,
    path = "/query",
    // The capability this operation IS - `sutura_app::Capability::AskMetric::id()`'s literal, written
    // out because a `#[utoipa::path]` attribute takes a literal. See the note on the catalog route.
    operation_id = "ask_metric",
    tag = TAG,
    request_body = QuestionBody,
    responses(
        (
            status = 200,
            description = "ANSWERED. Carries rows and the provenance of the definitions that \
                           produced them. This is the only status that means the question was \
                           answered: a refusal is a 4xx or a 5xx, listed below and carrying \
                           `outcome: refusal`.",
            body = OutcomeBody
        ),
        (status = 400, description = "The body is not a modelled question. The detail names the field.", body = crate::problem::ProblemBody),
        (status = 401, description = "No valid bearer token was presented.", body = crate::problem::ProblemBody),
        (
            status = 403,
            description = "THREE THINGS, and the body shape tells the first from the others - \
                           `outcome: refusal` for the first two, `code` with no `outcome` for the \
                           third.\n\n\
                           REFUSED ABOUT THE METRIC (`outcome: refusal`): the catalog does not \
                           permit this of this metric. `code` says which: \
                           `dimension_not_permitted` (the metric declares no such dimension), \
                           `dimension_not_filterable` (it can be grouped by and not filtered on), \
                           `dimension_value_not_allowed` (the value is outside the declared \
                           allowlist - the value itself is never echoed back). NOT a statement \
                           about your credential: no token widens a metric's dimension set.\n\n\
                           REFUSED ABOUT A CREDENTIAL AT THE DATA SYSTEM (`outcome: refusal`, \
                           `code: credential_unavailable`): the asking subject has no access at the \
                           data system this metric reads, and this deployment will not read it \
                           under its own identity instead. The missing grant is THERE and not here, \
                           so re-authenticating with this service changes nothing and neither does \
                           a narrower question.\n\n\
                           FAILED (`code: insufficient_scope`): your credential IS valid and does \
                           not carry the scope this operation requires; the detail names it. It \
                           says nothing about any metric - a caller who is granted the scope gets \
                           exactly the same rows anybody else would.",
            body = OutcomeBody
        ),
        (
            status = 404,
            description = "REFUSED - `outcome: refusal`, `code: metric_unknown`. This catalog \
                           snapshot defines no metric of that name. `GET /v1/catalog` lists the \
                           ones it does.",
            body = OutcomeBody
        ),
        (status = 408, description = "The request exceeded this service's time bound.", body = crate::problem::ProblemBody),
        (
            status = 409,
            description = "REFUSED - `outcome: refusal`, `code: federation_not_executable`. The \
                           question is answerable in principle and this build has no adapter that \
                           can execute one half of a question spanning two data systems, so it is \
                           refused rather than run partly.",
            body = OutcomeBody
        ),
        (
            status = 413,
            description = "TWO THINGS, and `code` is what tells them apart. `too_large`: the \
                           REQUEST body is larger than this service will read - that body is the \
                           failure shape. `result_too_large`: the ANSWER exceeded the row cap and \
                           was NOT truncated to fit - that body is `outcome: refusal`, and the \
                           detail names the cap and what to narrow.",
            body = OutcomeBody
        ),
        (
            status = 422,
            description = "REFUSED - `outcome: refusal`. The question is well formed and out of \
                           bounds. `code` says which bound: `time_range_too_long`, \
                           `too_many_dimensions`, `duplicate_dimension`, `grain_not_supported` \
                           (the metric exists; that grain is not defined for it), or \
                           `resources_exhausted` (answering needed more working memory than this \
                           deployment's ceiling, and it was refused rather than allowed to exhaust \
                           the process - narrow the period or group by fewer dimensions). \
                           Repeating the request unchanged will fail the same way; the detail \
                           carries the limit.",
            body = OutcomeBody
        ),
        (status = 429, description = "Too many requests from this address.", body = crate::problem::ProblemBody),
        (status = 500, description = "Something on our side went wrong. The body carries no detail.", body = crate::problem::ProblemBody),
        (
            status = 503,
            description = "Worth retrying, and `code` says which of four things happened. \
                           `unavailable`: the data system did not answer. `identity_unavailable`: \
                           the credential broker this deployment depends on did not answer, which \
                           is a different dependency being unwell and is diagnosed elsewhere. \
                           `at_capacity`: every \
                           execution slot was taken for the whole admission window, so this \
                           question was shed rather than queued - that response carries a \
                           `Retry-After` in seconds. `source_unavailable`: REFUSED - \
                           `outcome: refusal` - the data system the plan names could not be \
                           reached as the calling subject. No `Retry-After` on the other three: \
                           nothing here knows when a data system or an identity provider comes \
                           back, and a guessed number would be a promise.",
            body = crate::problem::ProblemBody
        ),
    )
)]
pub(crate) async fn ask(
    State(state): State<ServiceState>,
    // Leg 1's conclusion, when this deployment has leg 1. **An extractor and not a body field**, and
    // the difference is the whole control: `crate::inbound::VerifiedCaller` has one `pub(crate)`
    // constructor, called only after a signature check, implements no `Deserialize`, and is put into
    // the extensions by exactly one middleware. So this parameter cannot be reached by anything a
    // caller writes - see `crate::principal`, which explains why the check moved from the arity of a
    // function to a type.
    caller: Option<axum::Extension<crate::inbound::VerifiedCaller>>,
    body: Result<Json<QuestionBody>, JsonRejection>,
) -> Result<Outcome, Failure> {
    let Json(body) = body.map_err(|rejection| rejected(&rejection))?;
    let query = Query::try_from(body).map_err(|cause| Failure::NotAQuestion {
        detail: describe(&cause),
    })?;

    // WHICH question, onto the span, so every subsequent line of this request carries it.
    //
    // The two identifying fields go on the span and the two counts stay on the event, and the split
    // is not cosmetic: `metric` and `grain` are what the request IS, and they are what an operator
    // filters a log by - with `JsonStorageLayer` a field on the span appears on every line inside
    // it, including the answer, a refusal, and anything the data system says on the way. A count is
    // something that happened once and belongs where it happened.
    //
    // The fields are declared on the span in `crate::router::request_span`, as `Empty`, because
    // `tracing` cannot record a field a span was not opened with - and they are filled in here
    // because this is the first line at which the body has parsed and the values exist.
    let span = tracing::Span::current();
    span.record("metric", tracing::field::display(query.metric()));
    span.record("grain", tracing::field::display(query.grain()));

    // What was asked, before it is answered. Deliberately not the filter values: a rejected or
    // accepted value reflected into a log is a value that outlives the request, and the surface
    // already refuses to echo one back to the caller. The counts say how many there were, which is
    // what sizing a question needs and is not a value anybody wrote down.
    tracing::info!(
        dimensions = query.dimensions().len(),
        filters = query.filters().len(),
        "question received"
    );

    // Before the task is spawned, and not inside it: a slot acquired inside the blocking task would
    // be a thread already taken while waiting for permission to take one.
    let slot = state.admission().admit().await.map_err(|shed| refused(&shed))?;

    let surface = state.surface();
    // Derived from what this transport established, and from nothing the caller *sent*. Two answers:
    // a verified caller where leg 1 established one, and the deployment itself where it did not.
    // `established` takes no argument at all, and the other arm takes a value only a signature check
    // can produce - see `crate::principal`.
    let context = caller
        .as_ref()
        .map_or_else(crate::principal::established, |axum::Extension(verified)| {
            crate::principal::of_verified(verified)
        });
    // WHO asked, onto the span, so every line of this request is attributable. The label and not the
    // identifier: `established()` is a fixed word - `verified` or `deployment` - so it cannot raise
    // the log's cardinality or carry a subject into a field a caller chose. The audit record is where
    // the identifier goes, and `sutura_app::LocalService::answer` writes it.
    span.record("asker", context.chain().subject().established());
    // `spawn_carrying_span` rather than `tokio::task::spawn_blocking`, and the bare call is now on
    // the `disallowed-methods` list in `clippy.toml`: the pool thread has no current span, so a
    // bare spawn writes every line the engine emits outside this request. The three lines that fix
    // it were correct here and nothing made the NEXT call site write them - see
    // `sutura_runtime::blocking`.
    let joined = sutura_runtime::spawn_carrying_span(move || {
        // The audit record for this outcome is written INSIDE this call, before it returns - so it
        // is written on the blocking thread, inside the span this helper carries across, and it is
        // written whether or not the caller is still waiting for the response.
        let answered = surface.answer(&context, &query);
        // Explicitly, and here rather than at the top of the closure: the slot is released when the
        // WORK finishes, which is what makes the bound a bound on execution. Dropping it earlier
        // would let a second question start on top of this one.
        //
        // Inside the span as well, because the helper scopes the whole closure: a diagnostic
        // emitted while releasing is still attributable to this request.
        drop(slot);
        answered
    })
    .await;

    let outcome = match joined {
        Ok(answered) => answered.map_err(|failure| failed(&failure))?,
        Err(cause) => {
            // The blocking task panicked. The panic hook has already traced the payload and the
            // location; this says which request it took down with it.
            tracing::error!(error = %cause, "answering panicked");
            return Err(Failure::Internal);
        }
    };
    // One decision, one place: `wire::refusal` chooses the status, and nothing here restates it.
    //
    // NO LOG LINE FOR THE OUTCOME, and the `tracing::info!` per outcome that used to be here was
    // REPLACED rather than joined. `Surface::answer` writes one audit record per outcome, before it
    // returns, carrying the row count, the definition version and the refusal variant - every field
    // that line had except one - plus the principal chain, which is the thing that line's own doc
    // comment said it could not carry. Two channels saying nearly the same thing is a second place
    // for a field to be added to and forgotten.
    //
    // The one field that did not move is the HTTP STATUS, and it is not lost. The status is this
    // crate's and `sutura-app` cannot see it, so it could not travel on the record - and it does not
    // need to: `tower_http`'s response line already carries the status and the latency at `info`,
    // inside the same request span, configured in `crate::router`. The argument for putting the
    // status on the old line was that an operator correlating with an ingress log needs the number
    // that was actually sent, and the response line is where that number is reported.
    Ok(Outcome::from(&outcome))
}

/// Why the body did not become a `QuestionBody`.
///
/// **Two outcomes from one rejection type, and the split was found by a test rather than by
/// reading.** The body-limit layer causes the JSON extractor to reject with a length-limit error,
/// which is a `JsonRejection` like a malformed body is - so mapping every rejection to `400` made a
/// body over the bound indistinguishable from a body with a typo in it, and the documented `413`
/// was a status nothing produced. Branching on the rejection's own status rather than on its
/// variant keeps that true across an `axum` release that adds a variant.
fn rejected(rejection: &JsonRejection) -> Failure {
    if rejection.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
        return Failure::TooLarge;
    }
    Failure::NotAQuestion {
        // `body_text` and NOT the `#[source]` walk `describe` does. Each level of an `axum`
        // rejection's chain restates the whole message, so walking it produced the same sentence
        // three times in a row - observed in a response, not deduced. `body_text` is the one
        // sentence the rejection is designed to hand a caller, and it names the offending key for
        // an unknown field and the position for malformed JSON.
        detail: rejection.body_text(),
    }
}

/// Turns a shed question into the response, and says so once in the log.
///
/// `warn` and not `error`: shedding is the control working. It is also the line an operator sizes
/// from, so it carries the bound and the window the caller waited - which the response body
/// deliberately does not, because those numbers are this deployment's sizing.
fn refused(shed: &AtCapacity) -> Failure {
    tracing::warn!(
        max_concurrent_queries = shed.bound(),
        admission_timeout_seconds = shed.waited().as_secs(),
        "shed a question: every execution slot was taken for the whole admission window"
    );
    Failure::AtCapacity {
        retry_after_seconds: shed.waited().as_secs(),
    }
}

/// Maps a failure to a status, and logs the part the caller must not be told.
///
/// The split is the point. A data system that did not answer is a `503` and worth retrying; our own
/// bundle or generator being wrong is a `500` and is not. Neither response carries the message,
/// because a driver's complaint names a table, a column or a file.
fn failed(failure: &SurfaceFailure) -> Failure {
    // The chain is walked to text HERE, at the sink that writes it, and not inside the error. That
    // is the whole of the difference between a failure that can be inspected and one that has
    // already been turned into prose - see `crate::surface`.
    let chain = crate::surface::cause_chain(failure);
    match *failure {
        SurfaceFailure::Warehouse { ref cause } => {
            tracing::error!(error = %cause, chain = ?chain, "the data system did not answer");
            Failure::Unavailable
        }
        SurfaceFailure::Compile { ref cause } => {
            tracing::error!(error = %cause, chain = ?chain, "this deployment could not compile this question");
            Failure::Internal
        }
        // A different code from `Unavailable`, and `docs/adr/0014` asks for exactly that: a caller
        // told "the data system did not answer" against an authorization-server outage retries
        // successfully and learns the wrong thing about which dependency is unwell. Same status,
        // because both are worth retrying; different code, because they are diagnosed differently.
        SurfaceFailure::Broker { ref cause } => {
            tracing::error!(error = %cause, chain = ?chain, "the credential broker did not answer");
            Failure::IdentityUnavailable
        }
        // Our own wiring, so it carries nothing outward. An operator finds it in this line.
        SurfaceFailure::Miswired { ref cause } => {
            tracing::error!(error = %cause, chain = ?chain, "the credentials that came back do not fit this request");
            Failure::Internal
        }
    }
}

/// An error and its causes, on one line, for a caller.
///
/// `Display` on a `thiserror` enum prints the outermost message only, and for a malformed question
/// the outer message is the field and the cause is what was wrong with it - so both halves are
/// needed for the message to be actionable.
fn describe(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// What the admission bound does to a second question, through the assembled router.
///
/// Here rather than in `crate::harness` because the subject is this handler: the permit is acquired
/// on this line, before the blocking task is spawned, and moved into it. Driven through the real
/// router all the same, because a bound that a handler holds and a router does not install is the
/// failure mode the whole change is about.
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use sutura_config::{Environment, Settings, Sources};
    use tower::ServiceExt as _;

    use crate::state::ServiceState;
    use crate::surface::LocalService;
    use crate::testing::{bundle, catalog_of, sink, warehouse_that_can_be_held};

    /// A well formed question the fake will answer.
    const QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// A router over a warehouse whose answers can be held, and the switch that holds them.
    fn app(overlay: &str) -> (axum::Router, crate::testing::Held) {
        let settings =
            Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("the test settings load");
        let (engine, held) = warehouse_that_can_be_held();
        let service = LocalService::start(&catalog_of(bundle()), engine, sink(), crate::testing::broker(), 1 << 30)
            .expect("the test bundle validates");
        let router = crate::router(&crate::testing::state_over(Arc::new(service), settings)).expect("the test router assembles");
        (router, held)
    }

    /// TWO routers over ONE bound, composed the way a process serving two transports would be.
    ///
    /// **The instrument for `telekom/sutura#340`.** `ServiceState::new` used to derive its own
    /// `Admission` from the settings it was handed, so two states were two permit sets - two
    /// controls each reporting a limit the other can exceed, which is the shape
    /// `sutura_runtime::admission`'s module documentation calls not-a-bound. It takes the bound now,
    /// and what this hands back is the pair a reviewer has to be able to break: build a second
    /// bound instead of cloning this one and the test below goes green on the defect.
    ///
    /// Two states rather than an HTTP surface and an agent surface, because no crate in this
    /// workspace links both transports - `sutura-http` and `sutura-mcp` may not reach each other,
    /// and the two composition roots each link one. Two states are two independent TAKERS of the
    /// bound, which is the property under test; the transport they belong to is not.
    ///
    /// One service behind both, for the same reason a dual-transport root would share one: the
    /// resource the bound is about is the process's blocking pool and data system, not the router.
    fn two_states(overlay: &str) -> (axum::Router, axum::Router, crate::testing::Held, sutura_runtime::Admission) {
        let settings = || {
            Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("the test settings load")
        };
        // ONE call, and both states get a clone of it - every clone shares one permit set.
        let admission = sutura_runtime::Admission::from_settings(settings().runtime());
        let (engine, held) = warehouse_that_can_be_held();
        let service: Arc<dyn crate::surface::Surface> = Arc::new(
            LocalService::start(&catalog_of(bundle()), engine, sink(), crate::testing::broker(), 1 << 30)
                .expect("the test bundle validates"),
        );
        let assemble = |state: &ServiceState| crate::router(state).expect("the test router assembles");
        let first = assemble(&ServiceState::new(
            Arc::clone(&service),
            Arc::new(settings()),
            admission.clone(),
        ));
        let second = assemble(&ServiceState::new(
            Arc::clone(&service),
            Arc::new(settings()),
            admission.clone(),
        ));
        (first, second, held, admission)
    }

    /// One question, with the peer address `axum::serve` would have attached.
    ///
    /// Takes the router by value so the future borrows nothing and can be handed to `tokio::spawn`,
    /// which is what every test here needs: the point is two questions in flight at once.
    async fn ask_once(app: axum::Router) -> (StatusCode, String, Option<String>) {
        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/query")
            .header("content-type", "application/json")
            .body(Body::from(QUESTION))
            .expect("the test request is well formed");
        let peer: std::net::SocketAddr = "203.0.113.7:44444".parse().expect("a test peer address is an address");
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = app.oneshot(request).await.expect("the router is infallible as a service");
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .map(String::from);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the test response body is readable");
        (status, String::from_utf8_lossy(&bytes).into_owned(), retry_after)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_question_is_shed_rather_than_queued_when_the_bound_is_one() {
        // **The finding this exists for.** The request timeout drops the handler future; it does not
        // cancel a started `spawn_blocking` task, and `tokio` documents that such a task cannot be
        // aborted. So before the permit there was nothing bounding how many questions were running:
        // a caller could accumulate expensive ones that keep running after each `408`, exhaust the
        // blocking pool, and defeat the bounded shutdown this service advertises.
        //
        // A bound of one and a held warehouse is the whole shape of it. The first question takes the
        // only slot and does not return; the second must be SHED, and shed inside the admission
        // window rather than left waiting for the request timeout.
        let (app, held) = app(
            "runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\nserver:\n  request_timeout_seconds: 30\n",
        );
        held.arm();
        // Spawned first and given the runtime a turn, so the slot is actually taken before the
        // second question asks for one. Without the yield this races the spawn.
        let first = tokio::spawn(ask_once(app.clone()));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let began = Instant::now();
        let (status, body, retry_after) = ask_once(app.clone()).await;
        let waited = began.elapsed();

        // Released before the assertions, so a failing one does not also leave the blocking pool
        // holding the runtime open for the fake's own cap.
        held.release();
        let _first = first.await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(
            body.contains(r#""code":"at_capacity""#),
            "the shed request did not carry the documented body: {body}"
        );
        assert!(body.contains(r#""status":503"#), "{body}");
        assert_eq!(retry_after.as_deref(), Some("1"), "a shed request carries no Retry-After");
        // The admission timeout is what bounded the wait, not the request timeout thirty times it.
        assert!(waited >= Duration::from_secs(1), "shed before the window elapsed: {waited:?}");
        assert!(
            waited < Duration::from_secs(10),
            "the wait was not bounded by the window: {waited:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_slot_comes_back_when_the_work_finishes_rather_than_when_the_caller_gives_up() {
        // The other half, and the reason the permit is moved INTO the blocking task. A slot released
        // by the handler future would be handed back the moment a caller was answered `408`, while
        // the question it started was still running - so the bound would count callers rather than
        // work, and the backlog would be unbounded again with a number in front of it that looked
        // like a limit.
        //
        // Asserted the only way it can be from outside: hold the first question, watch the second be
        // shed, then release and watch a third be answered. A slot that never came back would shed
        // the third too.
        let (app, held) = app("runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\n");
        held.arm();
        let first = tokio::spawn(ask_once(app.clone()));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            ask_once(app.clone()).await.0,
            StatusCode::SERVICE_UNAVAILABLE,
            "the bound was not in effect, so this proves nothing about the release"
        );

        held.release();
        let (status, body, _) = first.await.expect("the first question's task ran");
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body, _) = ask_once(app.clone()).await;
        assert_eq!(status, StatusCode::OK, "the slot was never handed back: {body}");
        assert!(body.contains(r#""outcome":"answer""#), "{body}");
    }

    /// **`telekom/sutura#340`.** One bound handed to two states is ONE permit set, so a question
    /// admitted through one of them is a slot the other no longer has.
    ///
    /// This is the assertion that a second permit set fails, and it fails it on the number rather
    /// than on a name: with the bound at one, the first state's held question takes the only slot
    /// and the second state must SHED. A `ServiceState` that derived its own bound - which is what
    /// this crate did until #340 - answers the second question `200`, because it has a full permit
    /// set of its own that nothing else can see.
    ///
    /// **Non-vacuous by construction**: the third question, after the release, has to be ANSWERED
    /// through the same second router. Without that half a second router broken in any other way
    /// would satisfy the `503` and read as this property holding.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_bound_handed_to_two_states_is_one_permit_set() {
        let (first, second, held, admission) =
            two_states("runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\n");
        assert_eq!(admission.bound(), 1, "the overlay's bound did not reach either state");
        held.arm();
        // Through the FIRST state, and given the runtime a turn so the slot is actually taken.
        let running = tokio::spawn(ask_once(first));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            admission.free(),
            0,
            "the question in flight took no slot from the bound this test handed BOTH states, so a state built a permit set of its own"
        );

        // Through the SECOND. One permit set means there is nothing left for it.
        let (status, body, _) = ask_once(second.clone()).await;

        held.release();
        let (running_status, running_body, _) = running.await.expect("the held question's task ran");

        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "the second state admitted a question while the first state's bound was full, so the two hold two permit sets: {body}"
        );
        assert!(body.contains(r#""code":"at_capacity""#), "{body}");
        assert_eq!(running_status, StatusCode::OK, "{running_body}");
        // The other direction, so the `503` above is capacity and not a second router that answers
        // nothing: with the slot back, the SAME second router answers.
        let (status, body, _) = ask_once(second).await;
        assert_eq!(status, StatusCode::OK, "the second router answers nothing at all: {body}");
        assert!(body.contains(r#""outcome":"answer""#), "{body}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_configured_bound_is_the_number_of_questions_that_run_at_once() {
        // A bound above one, so the assertion is about the NUMBER rather than about the existence of
        // a permit set - the bug a bound of one cannot catch is an off-by-one that admits two.
        // Three slots, three held questions, and the fourth is shed.
        let (app, held) = app("runtime:\n  max_concurrent_queries: 3\n  admission_timeout_seconds: 1\n");
        held.arm();
        let mut running = Vec::new();
        for _ in 0_u8..3 {
            running.push(tokio::spawn(ask_once(app.clone())));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (status, body, _) = ask_once(app.clone()).await;
        held.release();
        for task in running {
            let _answered = task.await;
        }
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "a fourth question got a slot: {body}"
        );
    }
}

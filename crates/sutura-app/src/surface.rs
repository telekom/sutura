//! What a transport needs from this crate, with the ports' generic parameters erased.
//!
//! # Why this trait exists at all
//!
//! [`crate::answer`] is generic over a [`Warehouse`], and `Warehouse` has an associated error type.
//! A transport's request handler is a concrete function - an `axum` handler registered in a route
//! table by path, and named again by the macro that generates the interface description - so a
//! handler cannot be generic over the warehouse without the whole router becoming generic in it,
//! and the generated document becoming generic in it too.
//!
//! [`Surface`] is the seam: this crate's operations, with `W` gone - and with the audit sink's own
//! parameter gone for the same reason, since [`LocalService`] is generic in that too.
//!
//! # Why it is HERE and not in the transport that uses it
//!
//! **It used to be in `sutura-http`, and a review was right that that is the wrong crate.** The
//! module comment there said a second transport - an MCP one - would consume the same trait, which
//! is exactly the problem: it would have made one transport adapter depend on another, and the rule
//! this repository is built on is that nothing depends on an adapter. A driving port declared inside
//! an adapter is a port every other adapter has to reach through that one.
//!
//! **The tension with "a port arrives WITH its adapter", stated rather than dodged.** That rule
//! exists because a trait with no implementor is a guess at a signature, and `pub` hides the guess
//! from `dead_code`. [`LocalService`] moved with the trait, so the rule still holds: the port and
//! its only implementor are in one place, and neither is a guess. What the rule does not say is
//! *which* crate that place has to be.
//!
//! And there is a sharper reason it is not the transport's. A **driven** port - `Warehouse`,
//! `SemanticCatalog` - is dependency inversion: the interior declares what it needs, an adapter
//! outside implements it, and the trait has to sit inside the hexagon or the direction reverses. A
//! **driving** port inverts nothing. The caller is already outside and the implementation is already
//! the application, so there is no adapter for it to arrive with: [`LocalService`] is not an adapter
//! at all, it is this crate's own service with one generic parameter erased. It holds a
//! [`Validated`] bundle and a warehouse, and every method forwards. Putting that in a transport
//! crate made the application's interface the property of one of its callers.
//!
//! **Is the erasure an application concern or an HTTP one?** The *trigger* is an HTTP fact: a
//! handler is a concrete function. The *content* is not - `definitions`, `answer` and `run_sql` are
//! this crate's own operations, and [`LocalService::start`] is [`crate::verify_and_validate`] with
//! the catalog port consumed. Nothing in this file names a framework type, which is checkable
//! rather than asserted: `cargo xtask check-boundaries` fails on a framework anywhere in a tree it
//! governs, and this file added no dependency to this crate's manifest. A shape the application can
//! offer for free, that its callers all need, belongs with the application.
//!
//! **Why not delete the trait instead, under YAGNI?** That was the other option offered, and it does
//! not survive being tried. Deleting a one-implementation trait leaves the concrete
//! `LocalService<W>`, and then a transport either becomes generic in `W` - the router, its state and
//! the generated document with it, which is what this trait exists to prevent - or erases `W` inside
//! `LocalService`, which is the same trait under another name one crate lower. So the trait is not
//! speculative generality; it is the only shape that keeps a handler concrete with one transport,
//! let alone two. What WAS speculative is the sentence about MCP, and that sentence is what made the
//! location wrong rather than the trait. Deleting the trait would have answered a different finding.
//!
//! # Why the methods are synchronous
//!
//! Because [`Warehouse`] is. The port takes `&self` and returns a `Result`, and the engine behind
//! it drives its own single-threaded runtime and blocks on it. Calling that from inside an `async`
//! handler on a worker thread would panic - a runtime cannot be entered from within a runtime - so a
//! transport moves the call onto a blocking pool. Making this trait `async` would hide that
//! requirement behind a signature that looks like it had been dealt with.
//!
//! # Where the typed error goes
//!
//! Erasing the generic means the adapter's own error type cannot survive *as a named type*. It does
//! survive as an error: each variant of [`SurfaceFailure`] keeps the cause it was built from as an
//! owned `#[source]`, so `Error::source()` walks the whole chain and a caller that knows what
//! adapter is behind the port can still `downcast_ref` to it.
//!
//! **The previous shape was prose.** Both variants held a `String` message and a `Vec<String>` of
//! causes, walked to text at construction - so `source()` returned `None`, downcasting was
//! impossible, and the only thing a caller could do with the failure was print it. A vector of
//! display strings is a presentation of an error, not an error API. Flattening to text is still what
//! happens, but it happens at the *logging sink* - [`cause_chain`], called by whoever writes the
//! line - which is the one place where text is the point.
//!
//! `Box<dyn Error + Send + Sync>` and not `Box<dyn Error>`: a transport that answers on a blocking
//! pool sends the error back across a thread boundary. That is where the `W::Error: Send + Sync`
//! bound on the implementations below comes from. It is a bound on *this* type and not on the port,
//! so `sutura_domain::warehouse::Warehouse` is unchanged, and it is a `std` marker rather than a
//! framework type - a requirement a transport states, satisfied here.

use sutura_domain::audit::{AuditSink, CallRecord};
use sutura_domain::identity::{CredentialBroker, RequestContext};
use sutura_domain::pinned::{NotValidated, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::deadline::Deadline;

use crate::spend::SpendLedger;
use crate::warehouses::Warehouses;
use crate::{ServiceError, Validated, verify_and_validate};

/// The service, as a transport sees it.
///
/// `Send + Sync + 'static` because it is shared between connections and moved onto a blocking pool.
/// A transport holds it behind an `Arc` in its request state.
pub trait Surface: Send + Sync + 'static {
    /// The pinned bundle this process is serving.
    ///
    /// A borrow rather than a description, so the transport owns the wire shape and this trait does
    /// not have to change when the shape does. It is the *validated* bundle: there is no accessor
    /// here for an unvalidated one, because a [`Surface`] cannot be built from one.
    fn definitions(&self) -> &PinnedDefinitions;

    /// Answers one certified question, or says why it will not.
    ///
    /// A refusal comes back inside the `Ok` as [`ToolOutcome::Refusal`], never as an `Err`. That is
    /// the domain's invariant and this signature is where a transport inherits it: a caller cannot
    /// mistake "you may not ask that" for a transport hiccup and retry until something works.
    ///
    /// # The context, and why it is a second parameter rather than a field on the question
    ///
    /// `context` carries the principal chain the call is attributed to. It is separate from `query`
    /// because they come from different places and are trusted differently: the question is what the
    /// caller asked, and the chain is what the transport *established*. A caller that could state
    /// its own chain would be stating its own identity, so the two never share a shape - and
    /// `RequestContext` implements no `Deserialize`, which is what makes that structural rather than
    /// a rule somebody follows.
    ///
    /// The implementation writes one record per outcome, answer and refusal alike, **before this
    /// returns**. That ordering is the requirement rather than an optimisation: a record written
    /// after the response is the record a crash loses, and the call worth having a record of is the
    /// one that went wrong.
    ///
    /// # The deadline is opened by the transport, before this call
    ///
    /// `deadline` is one absolute [`Deadline`], opened at the instant the request arrived - before
    /// admission, so the wait for a concurrency slot sits inside the caller's own bound rather than
    /// adds to it. Taking it here, as a parameter rather than a field this trait's own state holds,
    /// is the same shape the working-set ceiling already uses: a transport cannot forget to open one
    /// because there is nowhere else for the value to come from. `docs/adr/0029` is the record; in
    /// this slice the deadline is carried through to the port and refused on when already spent, and
    /// nothing yet stops a data system mid-call with it.
    fn answer(&self, context: &RequestContext, query: &Query, deadline: Deadline) -> Result<ToolOutcome, SurfaceFailure>;

    /// Runs one literal statement against this deployment's configured source, or says why it will
    /// not - the raw SQL tool, `docs/adr/0013`.
    ///
    /// Same shape as [`Self::answer`] in every way that matters: a refusal comes back inside the
    /// `Ok` as [`sutura_domain::raw::RawOutcome::Refusal`], never as an `Err`, and the implementation
    /// writes one record per outcome before this returns. What differs is the vocabulary - see
    /// [`sutura_domain::raw`] for why it is not [`ToolOutcome`] wearing a second name.
    ///
    /// No `deadline` parameter: `docs/adr/0029`'s threading landed for `answer`/`execute_leg` only in
    /// the slice that added it, and this tool has no `LIMIT`-bearing plan for it to bound - the row
    /// cap and the connect-time `statement_timeout` are its only bounds today (`crates/
    /// sutura-exec-postgres/src/raw.rs`). Threading a deadline through this path too is future work,
    /// not decided here.
    fn run_sql(
        &self,
        context: &RequestContext,
        statement: &sutura_domain::raw::RawStatement,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure>;

    /// This replica's current spend headroom, or `None` where no per-replica ceiling is
    /// configured.
    ///
    /// **A query method, not a field on [`ToolOutcome`].** A headroom value is state of the
    /// ledger itself - true whether or not a question just ran, and unaffected by which question
    /// it was - so putting it on the outcome would mean every constructor of a caller-facing
    /// answer or refusal had to carry a number that has nothing to do with what was asked, for no
    /// reader of the wire body. A transport reads this once after a call to poll the ledger it
    /// already touched, never to answer a scrape: `docs/adr/0015`'s "a scrape must not make the
    /// service work" is why the metrics endpoint's own state never holds a [`Surface`] at all, so
    /// nothing here may be read from there.
    ///
    /// **Deployment-wide by construction, never per-subject** - see [`crate::spend::SpendLedger::headroom_bytes`]
    /// for why a value naming which subject is tight cannot exist on this port either: ADR-0015
    /// Decision 5 types a metric label as `&'static str`, so a `Subject` could not become one even
    /// if this trait tried to widen further.
    fn spend_headroom_bytes(&self) -> Option<u64>;
}

/// A typed error, owned, with its type erased and its `#[source]` chain intact.
///
/// `Send + Sync` because a transport may answer on a blocking pool, so a failure crosses a thread
/// boundary on the way back.
pub type ErasedCause = Box<dyn core::error::Error + Send + Sync + 'static>;

/// Something went wrong that is not a refusal.
///
/// Neither variant is something a caller can fix by asking differently, which is why neither is a
/// refusal: one is our own bundle or generator being wrong, and the other is the data system not
/// answering.
///
/// The variants are exhaustive and stay exhaustive - a transport chooses its status code from the
/// split - and each one carries the cause it was built from rather than a rendering of it.
///
/// **No audit record is written for either, and that is the limit on "every call is recorded".**
/// `sutura_domain::audit` records an *outcome* - an answer or a refusal - and neither of these is
/// one: a bundle that will not compile and a data system that did not answer are our own faults
/// rather than answers to a question. A transport logs them, with the cause chain, which is what
/// [`cause_chain`] is for. Widening the record to cover a failure means giving
/// `sutura_domain::audit::RecordedOutcome` a third variant, and that is a change to what a record
/// means rather than a field added to one.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceFailure {
    /// The pinned bundle would not compile this question, or the splitter built a two-source plan
    /// this workspace could not then assemble.
    ///
    /// **The message names neither, and that is deliberate since `telekom/sutura#338`.** The three
    /// sentences on this variant's path - this `Display`, the HTTP sink's log line and the MCP tool
    /// result a model reads - each blamed the bundle, which was true of the only cause this could
    /// carry and stopped being true when `sutura_semantic::CompileFailure` gained its second arm: an
    /// assembly failure is a defect in this workspace's own wiring, not a bundle that fails to hold
    /// what it names. The cause is kept as a `#[source]` and says which, so the sentence does not
    /// have to guess. **No sentence on this variant's path blames the pinned bundle**, and that is
    /// registered in `xtask`'s `ABSENCES` table rather than left to review - all three wordings are
    /// scanned for across every crate's library source.
    ///
    /// **The limit, next to the claim:** nothing drives this variant through either transport. No
    /// test builds a bundle that will not compile, or a plan that will not assemble, and asks for it
    /// over HTTP or MCP - so the `500` an assembly failure now gets and the sentence a caller reads
    /// with it are held by the code and by no cell. What IS measured is one layer in:
    /// `crates/sutura-app/tests/differential/federated.rs` sees an assembly failure as a failure
    /// rather than as a refusal.
    #[error("the question could not be compiled")]
    Compile {
        #[source]
        cause: ErasedCause,
    },
    #[error("the data system did not answer")]
    Warehouse {
        #[source]
        cause: ErasedCause,
    },
    /// The credential broker did not answer, so nothing could be executed as the asking subject.
    ///
    /// **Its own variant because the two outages are retried differently**, which `docs/adr/0014`
    /// states as a requirement rather than a preference: an authorization server that is down comes
    /// back, and a caller told the same sentence for both will retry a data-system outage the same
    /// way and learn nothing. A transport chooses a different code for it.
    #[error("the credential broker did not answer")]
    Broker {
        #[source]
        cause: ErasedCause,
    },
    /// Credentials came back that do not fit the request: a wiring defect on this side.
    ///
    /// Not a refusal - the question was fine - and not [`Self::Broker`] either, because a broker that
    /// answered and a broker that could not be reached are different things to whoever is paged. A
    /// caller can do nothing about it, so what it becomes on the wire is an internal failure.
    ///
    /// **What it covers grew, and the variant did not**, deliberately: the grant naming another
    /// subject, covering another source set, carrying a deadline that had passed, or a refusal naming
    /// a source nobody asked about are one thing to a transport - this deployment is wrong about its
    /// own identity wiring - and four things to whoever reads the log line, which is where the typed
    /// cause is. Splitting them here would ask each transport to pick a status code for a distinction
    /// that changes nothing a caller can do.
    #[error("the credentials that came back do not fit this request")]
    Miswired {
        #[source]
        cause: ErasedCause,
    },
}

/// Every cause beneath `error`, outermost first.
///
/// **A logging concern, and it lives here because this is where the erasure happens.** `Display` on
/// a `thiserror` enum prints the outermost message only, so a line that formatted the error would
/// say "the data system did not answer" and drop the driver's own complaint - which is the half that
/// names the table, the column or the file. This is called by whoever writes the line, not by
/// whoever builds the error, which is the difference between an error that can be inspected and one
/// that has already been turned into prose.
#[must_use]
pub fn cause_chain(error: &(dyn core::error::Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        chain.push(cause.to_string());
        cursor = cause.source();
    }
    chain
}

/// Why a service could not be started.
#[derive(Debug, thiserror::Error)]
pub enum ServiceNotStarted {
    /// A catalog adapter could not produce a bundle.
    #[error("the catalog could not be loaded")]
    Catalog {
        #[source]
        cause: ErasedCause,
    },
    /// The catalog contributions do not compose: two sources define one element, certify different
    /// versions, or one of them supplies a kind its declaration does not.
    #[error("the catalog contributions do not compose")]
    Composition {
        #[source]
        cause: crate::assemble::CompositionError,
    },
    /// The bundle loaded and an anchor did not reproduce the number its author certified, or could
    /// not be run at all.
    ///
    /// **This is the readiness gate, and it is a startup failure rather than a degraded mode.** A
    /// bundle whose anchors do not hold is a set of definitions that no longer computes the numbers
    /// somebody signed off on; serving it would answer questions with figures nobody certified.
    #[error("the pinned bundle is not fit to serve")]
    NotValidated {
        #[source]
        cause: NotValidated,
    },
}

/// The one implementation: a validated bundle, the data systems this process opened, and one audit
/// sink, behind the ports.
///
/// Holds the bundle as [`Validated`], which has no constructor other than one that executes every
/// anchor against a warehouse - so a [`LocalService`] that exists is one whose anchors held. That
/// is not a check this type performs; it is a type it could not otherwise have been built from.
///
/// **The sink is a constructor argument and not an `Option`.** A service cannot be started without
/// one, so "this deployment forgot to attach a sink" is not a state that exists - which is the
/// difference between a record that is always written and a record that is usually written. What the
/// sink then *does* with a record is the deployment's, and `sutura_domain::audit` states that limit
/// where the port is declared.
/// **It holds a [`Warehouses`] and not one warehouse, which is what makes more than one source
/// configurable.** A plan names one data system, so the registry is a lookup rather than a fan-out:
/// [`crate::answer`] selects the adapter the plan named and refuses `SourceUnavailable` when nothing
/// is registered under that name. The limit is stated where the type is - every entry is the same
/// adapter type `W`, so a deployment holds two file sources or two databases behind one adapter, and a
/// heterogeneous set is an architecture decision rather than a change here.
/// **And it holds the credential broker, which is what makes a question executable at all.** Every
/// answer mints once, for every source its plan reads, and `sutura_domain::warehouse::Warehouse`
/// has no signature that runs without the result - so a service with no broker is not a service
/// that answers as the process, it is a service that does not compile.
/// **It also holds the spend ledger, unbounded unless a composition root opts in.** `Self::start`
/// and `Self::start_composed` build one with [`SpendLedger::no_budget`] - today's behaviour, before
/// this counter existed - and [`Self::with_spend_ledger`] is how a root that read a configured
/// ceiling out of its settings replaces it. Not a constructor argument, unlike every other field
/// here: those are what a service cannot exist without, and an unbounded ledger is a real, working
/// default rather than an omission this type should refuse to start without.
pub struct LocalService<W, S, B> {
    definitions: Validated<PinnedDefinitions>,
    warehouses: Warehouses<W>,
    sink: S,
    broker: B,
    working_set_bytes: u64,
    spend_ledger: SpendLedger,
}

impl<W, S, B> LocalService<W, S, B>
where
    W: Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    S: AuditSink + Send + Sync + 'static,
    B: CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    /// Loads one catalog through its port, re-runs every anchor against `warehouse`, and returns a
    /// service only if all of them held.
    ///
    /// This is [`Self::start_composed`] over a single declared catalog - the metadata assembler is
    /// what makes several compose, and a single-catalog deployment is that function's one-entry
    /// case, so there is one code path to keep honest rather than a second one that happens to
    /// serve.
    pub fn start<C>(
        catalog: &C,
        warehouses: Warehouses<W>,
        sink: S,
        broker: B,
        working_set_bytes: u64,
    ) -> Result<Self, ServiceNotStarted>
    where
        C: SemanticCatalog,
        C::Error: Send + Sync,
    {
        Self::start_composed(core::slice::from_ref(catalog), warehouses, sink, broker, working_set_bytes)
    }

    /// Loads every declared catalog through its port, composes them into one bundle, re-runs every
    /// anchor against `warehouse`, and returns a service only if all of them held.
    ///
    /// Every port is consumed here, which is what lets a transport be transport-only: it never
    /// reads a catalog directory, never opens a data system and never decides where a record goes.
    /// The buttons the serve/schema each press are the same, which is what keeps "the bundle this
    /// validates is the bundle this serves" true for N sources rather than for one.
    ///
    /// `C::Error: Send + Sync` for the same reason `W::Error` is - the cause is kept, owned, and a
    /// startup failure is reported from wherever the composition root happens to be.
    pub fn start_composed<C>(
        catalogs: &[C],
        warehouses: Warehouses<W>,
        sink: S,
        broker: B,
        working_set_bytes: u64,
    ) -> Result<Self, ServiceNotStarted>
    where
        C: SemanticCatalog,
        C::Error: Send + Sync,
    {
        let mut bundles = Vec::with_capacity(catalogs.len());
        for catalog in catalogs {
            let pinned = catalog
                .load()
                .map_err(|cause| ServiceNotStarted::Catalog { cause: Box::new(cause) })?;
            bundles.push(pinned);
        }
        let pinned = crate::assemble::assemble(&bundles).map_err(|cause| ServiceNotStarted::Composition { cause })?;
        // The broker is NOT consulted here, and that is the boot path's whole shape: an anchor runs
        // through `Warehouse::verify_anchor`, which takes no credential because there is no caller to
        // mint one for. `docs/adr/0008` part 1 decides it, and `sutura_domain::warehouse` records
        // where this is narrower than that record asked for.
        let definitions = verify_and_validate(pinned, &warehouses).map_err(|cause| ServiceNotStarted::NotValidated { cause })?;
        Ok(Self {
            definitions,
            warehouses,
            sink,
            broker,
            working_set_bytes,
            spend_ledger: SpendLedger::no_budget(),
        })
    }

    /// Replaces the spend ledger, for a composition root that read a configured per-replica
    /// ceiling out of its settings.
    ///
    /// A setter rather than a constructor argument, so every existing caller of [`Self::start`] and
    /// [`Self::start_composed`] - most of which configure no ceiling at all - keeps its original
    /// argument list. `docs/adr/0030` is the record; `governance.per_replica_spend_ceiling` absent
    /// is the state every one of those callers is already in.
    #[must_use]
    pub fn with_spend_ledger(mut self, spend_ledger: SpendLedger) -> Self {
        self.spend_ledger = spend_ledger;
        self
    }
}

impl<W, S, B> Surface for LocalService<W, S, B>
where
    W: Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    S: AuditSink + Send + Sync + 'static,
    B: CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    fn definitions(&self) -> &PinnedDefinitions {
        self.definitions.get()
    }

    fn answer(&self, context: &RequestContext, query: &Query, deadline: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
        let answered = crate::answer(
            &self.definitions,
            query,
            context,
            &self.broker,
            &self.warehouses,
            self.working_set_bytes,
            deadline,
            &self.spend_ledger,
        )
        .map_err(|error| match error {
            // The generic parameter is what cannot survive; the VALUE does, boxed, with its own
            // `#[source]` chain under it.
            ServiceError::Compile { cause } => SurfaceFailure::Compile { cause: Box::new(cause) },
            ServiceError::Warehouse { cause } => SurfaceFailure::Warehouse { cause: Box::new(cause) },
            // Two brokers' worth of failure, kept apart on the way out for the reason the variants
            // give: an authorization server that is down and a broker that answered about the wrong
            // sources are not retried the same way.
            ServiceError::Broker { cause } => SurfaceFailure::Broker { cause: Box::new(cause) },
            ServiceError::Credentials { cause } => SurfaceFailure::Miswired { cause: Box::new(cause) },
            // The same arm, and deliberately: to a transport, "the broker's answer does not fit
            // the request" and "the leg does not fit the posture the adapter was opened with" are
            // one thing - this deployment is wrong about its own identity wiring, and a caller can
            // do nothing about either. The typed cause is what tells them apart in the log.
            ServiceError::Posture { cause } => SurfaceFailure::Miswired { cause: Box::new(cause) },
            // D19 + A4: only the combiner's OWN wiring defects reach here now -
            // `FederatedAnswerRefusal::of` classifies a deterministic combine failure (a non-finite
            // ratio, an ambiguous link) as a `RefusalReason` before `answer_federated` ever returns
            // this `Err`, because retrying either does not help. What is left really is a
            // data-system concern in the sense that matters to a transport: the question and the
            // caller were fine.
            ServiceError::Federated { cause } => SurfaceFailure::Warehouse { cause: Box::new(cause) },
        })?;
        // Here, and before the `Ok`. Not in the transport: a record the transport writes is a record
        // that exists only for the transports that remember to write one, and this is the one line
        // in the workspace where "before the outcome returns" is a property somebody can point at.
        //
        // Both outcomes reach it, because the `?` above is the only path that skips it - see the
        // limit stated on `SurfaceFailure` below.
        // The deadline the credentials this call ran under carried travels with the outcome for
        // exactly this line - `docs/adr/0008` fixes it as part of the record's content, and
        // `sutura_app::Answered` is what carries it here without putting it on the caller-facing
        // provenance. `None` is a question declined before the broker was asked.
        self.sink.record(&CallRecord::of(
            context.chain(),
            answered.outcome(),
            answered.executed_until(),
        ));
        Ok(answered.into_outcome())
    }

    fn run_sql(
        &self,
        context: &RequestContext,
        statement: &sutura_domain::raw::RawStatement,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
        let answered = crate::run_sql(context, statement, &self.broker, &self.warehouses).map_err(|error| match error {
            crate::RunSqlError::Broker { cause } => SurfaceFailure::Broker { cause: Box::new(cause) },
            crate::RunSqlError::Credentials { cause } => SurfaceFailure::Miswired { cause: Box::new(cause) },
            crate::RunSqlError::Posture { cause } => SurfaceFailure::Miswired { cause: Box::new(cause) },
            // A boot refusal is supposed to make this unreachable in a running deployment - see
            // `sutura_config`'s own refusal over `DeploymentIdentity` and the adapter's declared
            // `Warehouse::ACCEPTS_RAW_STATEMENTS`. Reported as a wiring defect rather than panicking,
            // because a port that CAN return this is a port whose contract says it might.
            crate::RunSqlError::NoAcceptingSource => SurfaceFailure::Miswired {
                cause: Box::new(crate::RunSqlError::<B::Error>::NoAcceptingSource),
            },
        })?;
        // Same ordering as `answer`: written before the `Ok`, both outcomes reaching it, so a raw
        // call is recorded exactly as reliably as a certified one.
        self.sink.record(&CallRecord::of_raw(
            context.chain(),
            statement,
            answered.outcome(),
            answered.executed_until(),
        ));
        Ok(answered.into_outcome())
    }

    fn spend_headroom_bytes(&self) -> Option<u64> {
        self.spend_ledger.headroom_bytes(std::time::Instant::now())
    }
}

impl<W, S, B> core::fmt::Debug for LocalService<W, S, B> {
    /// Hand-written because a warehouse adapter need not be `Debug`, and because printing a bundle
    /// into a log is a page of definitions for no benefit. The digest identifies it.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalService")
            .field("definition_version", &self.definitions.get().version())
            .field("definition_digest", &self.definitions.get().digest())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::Grain;
    use sutura_domain::pinned::NotValidated;
    use sutura_domain::pinned::view::ScopedView;
    use sutura_domain::query::Query;

    use super::{LocalService, ServiceNotStarted, Surface as _};
    use crate::tests::{asked_by_a_person, june, metric, shared, source};
    use crate::tests_support::{
        AuthoredWarehouse, DiscardingAuditSink, FixedBroker, FixedCatalog, RawCapableWarehouse, authored_bundle, bundle_over,
    };
    use crate::{Warehouses, verify_and_validate};

    #[test]
    fn an_authored_metric_does_not_boot_against_the_in_process_engine() {
        // The mechanism under test is `Warehouse::EXECUTES_AUTHORED_SQL` read inside
        // `verify_and_validate`, through the real composition path and the one adapter every shipped
        // binary links. Red on base by construction (`authored_bundle` cannot be built there); the
        // compiled mutation is `const EXECUTES_AUTHORED_SQL: bool = true;` on `DataFusionWarehouse`,
        // under which this test alone goes red.
        let catalog = FixedCatalog::of(authored_bundle(metric(), source()));
        let ceiling = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
        let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
            source(),
            shared(),
            sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
        )
        .expect("the in-process engine starts");
        let error = LocalService::start(
            &catalog,
            Warehouses::of(engine),
            DiscardingAuditSink,
            FixedBroker::GrantsShared,
            1 << 30,
        )
        .expect_err("the in-process engine cannot execute authored SQL");
        let ServiceNotStarted::NotValidated { cause } = error else {
            panic!("the authored computation must be the startup refusal: {error:?}");
        };
        assert_eq!(cause, NotValidated::AuthoredSqlNotExecutable { metric: metric() });
    }

    #[test]
    fn the_authored_sql_example_does_not_boot_against_the_in_process_engine() {
        // The other half of the fixture above, over the directory `examples/authored-sql` and the
        // README under it actually claim about: not a hand-built bundle, but
        // `LocalCatalog::load` through the real composition path `sutura query`, `sutura mcp` and
        // `sutura serve` all take. `LocalCatalog::capabilities()` is `everything()`
        // (`crates/sutura-catalog-local/src/lib.rs`), so composing this directory first requires the
        // example to carry every kind that declares: a second model, a relationship, a dimension
        // reached through it, a required filter, an anchor, and the four knowledge documents. Red
        // before those existed, at `CompositionError::Unfaithful` - a one-model, one-metric catalog
        // does not compose, which a review of this checkpoint found before the example carried
        // enough to reach the authored check at all. The compiled mutation is the same as the test
        // above.
        use std::path::Path;

        use sutura_catalog_local::LocalCatalog;
        use sutura_domain::pinned::DefinitionVersion;

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/authored-sql/catalog");
        let version = DefinitionVersion::parse("authored-sql-example").expect("a fixed version is a version");
        let catalog = LocalCatalog::new(source(), root, version);
        let ceiling = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
        let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
            source(),
            shared(),
            sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
        )
        .expect("the in-process engine starts");
        let error = LocalService::start(
            &catalog,
            Warehouses::of(engine),
            DiscardingAuditSink,
            FixedBroker::GrantsShared,
            1 << 30,
        )
        .expect_err("the example carries an authored metric the in-process engine cannot execute");
        let ServiceNotStarted::NotValidated { cause } = error else {
            panic!("the example must compose and then hit the authored-SQL refusal: {error:?}");
        };
        let metric = sutura_domain::model::MetricName::parse("order_value_spread").expect("the example metric name is a name");
        assert_eq!(cause, NotValidated::AuthoredSqlNotExecutable { metric });
    }

    #[test]
    fn an_adapter_declaring_authored_sql_support_passes_only_the_capability_gate() {
        // The other direction, so the gate above is a capability read and not an unconditional
        // refusal of the key. What the declaring fake does NOT get is an answer: the plan carries no
        // SQL, so the compiler names the metric rather than substituting a measure. No adapter this
        // workspace ships makes the declaration; the fake exists to hold the gate's shape.
        let pinned = authored_bundle(metric(), source());
        verify_and_validate(pinned.clone(), &Warehouses::of(AuthoredWarehouse::new(source(), shared())))
            .expect("the declaring fake passes startup because this bundle has no anchors");
        let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
        let error = sutura_semantic::compile(&question, &ScopedView::everything(&pinned))
            .expect_err("an authored computation has no representation in the semantic plan");
        match error {
            sutura_semantic::CompileFailure::AuthoredSqlNotPlanned { metric: failed } => assert_eq!(failed, metric()),
            other => panic!("the compiler must name the authored metric rather than substitute a measure: {other:?}"),
        }
    }

    /// `#666`'s review, finding 3: the constructor `CallRecord::of_raw` was tested directly, but
    /// nothing reached `LocalService::run_sql`'s own `self.sink.record(...)` call - a mutation that
    /// builds the record and never sinks it (`crates/sutura-app/src/surface.rs:449-454`) left the
    /// whole suite green. This calls `run_sql` through the real `Surface` implementation, over a
    /// sink that only a genuine `record` call can reach.
    #[test]
    fn run_sql_writes_one_record_per_outcome_carrying_the_statement_text() {
        use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};
        use sutura_domain::raw::RawStatement;

        /// `Send + Sync + 'static`, without `std::sync::Mutex` (`clippy.toml` disallows it) or a
        /// `tokio` dependency this crate does not have: a channel is `Sync` for a `Send` item and
        /// needs neither.
        struct RecordingSink {
            sender: std::sync::mpsc::Sender<String>,
        }

        impl AuditSink for RecordingSink {
            fn record(&self, record: &CallRecord<'_>) {
                let statement = record.statement().map(|text| text.as_str().to_owned());
                let line = match *record.outcome() {
                    RecordedOutcome::RawAnswered { rows, .. } => format!("raw_answered rows={rows} statement={statement:?}"),
                    RecordedOutcome::RawRefused { reason, .. } => {
                        format!("raw_refused reason={reason:?} statement={statement:?}")
                    }
                    RecordedOutcome::Answered { .. } | RecordedOutcome::Refused { .. } => String::from("certified"),
                };
                drop(self.sender.send(line));
            }
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        // An empty catalog: no metric, so no anchor `LocalService::start` would re-run against
        // this fake - `RawCapableWarehouse` answers only the raw path, deliberately, and boot must
        // not touch the certified one to reach it.
        let catalog = FixedCatalog::of(bundle_over(&[]));
        let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), 1);
        let service = LocalService::start(
            &catalog,
            Warehouses::of(warehouse),
            RecordingSink { sender },
            FixedBroker::GrantsShared,
            1 << 30,
        )
        .expect("an empty catalog with no anchors boots against any warehouse");

        let context = asked_by_a_person();
        let answered_statement = RawStatement::parse("select 1").expect("a test statement is a statement");
        drop(
            service
                .run_sql(&context, &answered_statement)
                .expect("the fake warehouse answers `select 1`"),
        );
        let refused_statement = RawStatement::parse("refuse me").expect("a test statement is a statement");
        drop(
            service
                .run_sql(&context, &refused_statement)
                .expect("a refusal is a result, not an `Err`"),
        );

        let lines: Vec<String> = receiver.try_iter().collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("raw_answered"), "{lines:?}");
        assert!(lines[0].contains("select 1"), "{lines:?}");
        assert!(lines[1].starts_with("raw_refused"), "{lines:?}");
        assert!(lines[1].contains("refuse me"), "{lines:?}");
    }
}

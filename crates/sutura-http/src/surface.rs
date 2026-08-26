//! What a transport needs from the service, with the ports' generic parameters erased.
//!
//! # Why this trait exists at all
//!
//! `sutura_app::answer` is generic over a [`Warehouse`], and `Warehouse` has an associated error
//! type. An `axum` handler is a concrete function registered in a route table, and
//! `utoipa_axum::routes!` names it by path - so a handler cannot be generic over the warehouse
//! without the whole router becoming generic in it, and the generated document becoming generic in
//! it too.
//!
//! [`Surface`] is the seam. It is a *driving* port - the direction a request arrives from, rather
//! than a dependency the domain inverts - and, like every port in this repository, it arrives with
//! its implementor: [`LocalService`] is in this file, is the only one, and does nothing but call
//! through to `sutura_app`. Nothing here re-implements a rule that lives inside the hexagon, and a
//! second transport - an MCP one, say - consumes this same trait rather than growing its own copy
//! of the wiring.
//!
//! # Why the methods are synchronous
//!
//! Because [`Warehouse`] is. The port takes `&self` and returns a `Result`, and the engine behind
//! it drives its own single-threaded runtime and blocks on it. Calling that from inside an `async`
//! handler on a worker thread would panic - a runtime cannot be entered from within a runtime - so
//! the handler moves the call onto the blocking pool. Making this trait `async` would hide that
//! requirement behind a signature that looks like it had been dealt with.
//!
//! # Where the typed error goes
//!
//! Erasing the generic means the adapter's own error type cannot survive as a type. It is walked to
//! text here - message plus the whole `#[source]` chain - which is the same trade `sutura-app`
//! makes at the same kind of boundary and for the same reason: the alternative is `Display` on the
//! outermost error, which prints one sentence and discards the part naming the table, the column or
//! the file.

use sutura_app::{ServiceError, Validated, verify_and_validate};
use sutura_domain::pinned::{NotValidated, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::Warehouse;

/// The service, as a transport sees it.
///
/// `Send + Sync + 'static` because it is shared between connections and moved onto the blocking
/// pool. Held behind an `Arc` in the request state.
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
    fn answer(&self, query: &Query) -> Result<ToolOutcome, SurfaceFailure>;
}

/// Something went wrong that is not a refusal.
///
/// Neither variant is something a caller can fix by asking differently, which is why neither is a
/// refusal: one is our own bundle or generator being wrong, and the other is the data system not
/// answering.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceFailure {
    #[error("the question could not be compiled against the pinned bundle: {message}")]
    Compile { message: String, chain: Vec<String> },
    #[error("the data system did not answer: {message}")]
    Warehouse { message: String, chain: Vec<String> },
}

impl SurfaceFailure {
    /// The `#[source]` chain, outermost first, for the log.
    #[inline]
    pub fn chain(&self) -> &[String] {
        match *self {
            Self::Compile { ref chain, .. } | Self::Warehouse { ref chain, .. } => chain,
        }
    }
}

/// Why a service could not be started.
#[derive(Debug, thiserror::Error)]
pub enum ServiceNotStarted {
    /// The catalog adapter could not produce a bundle.
    #[error("the catalog could not be loaded: {message}")]
    Catalog { message: String, chain: Vec<String> },
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

/// The one implementation: a validated bundle and one data system, behind the ports.
///
/// Holds the bundle as [`Validated`], which has no constructor other than one that executes every
/// anchor against a warehouse - so a [`LocalService`] that exists is one whose anchors held. That
/// is not a check this type performs; it is a type it could not otherwise have been built from.
pub struct LocalService<W> {
    definitions: Validated<PinnedDefinitions>,
    warehouse: W,
}

impl<W> LocalService<W>
where
    W: Warehouse + Send + Sync + 'static,
{
    /// Loads a catalog through its port, re-runs every anchor against `warehouse`, and returns a
    /// service only if all of them held.
    ///
    /// Both ports are consumed here and nowhere else in this crate: the transport never reads a
    /// catalog directory and never opens a data system, which is what keeps it transport-only.
    pub fn start<C>(catalog: &C, warehouse: W) -> Result<Self, ServiceNotStarted>
    where
        C: SemanticCatalog,
    {
        let pinned = catalog.load().map_err(|cause| {
            let (message, chain) = flatten(&cause);
            ServiceNotStarted::Catalog { message, chain }
        })?;
        let definitions = verify_and_validate(pinned, &warehouse).map_err(|cause| ServiceNotStarted::NotValidated { cause })?;
        Ok(Self { definitions, warehouse })
    }
}

impl<W> Surface for LocalService<W>
where
    W: Warehouse + Send + Sync + 'static,
{
    fn definitions(&self) -> &PinnedDefinitions {
        self.definitions.get()
    }

    fn answer(&self, query: &Query) -> Result<ToolOutcome, SurfaceFailure> {
        sutura_app::answer(&self.definitions, query, &self.warehouse).map_err(|error| match error {
            ServiceError::Compile { cause } => {
                let (message, chain) = flatten(&cause);
                SurfaceFailure::Compile { message, chain }
            }
            ServiceError::Warehouse { cause } => {
                let (message, chain) = flatten(&cause);
                SurfaceFailure::Warehouse { message, chain }
            }
        })
    }
}

impl<W> core::fmt::Debug for LocalService<W> {
    /// Hand-written because a warehouse adapter need not be `Debug`, and because printing a bundle
    /// into a log is a page of definitions for no benefit. The digest identifies it.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalService")
            .field("definition_version", &self.definitions.get().version())
            .field("definition_digest", &self.definitions.get().digest())
            .finish_non_exhaustive()
    }
}

/// A typed error, flattened at the boundary that erases its type.
///
/// The message and then every cause beneath it, outermost first. `Display` on a `thiserror` enum
/// prints only the outermost message, so formatting the error into a single sentence discards the
/// driver's own complaint - which is the half that names the table, the column or the file.
fn flatten(error: &dyn core::error::Error) -> (String, Vec<String>) {
    let mut chain = Vec::new();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        chain.push(cause.to_string());
        cursor = cause.source();
    }
    (error.to_string(), chain)
}

#[cfg(test)]
mod tests {
    use crate::testing::{FailingCatalog, FailingWarehouse, bundle, catalog_of, source};

    use super::{LocalService, ServiceNotStarted, Surface as _};

    #[test]
    fn a_catalog_that_cannot_be_read_keeps_its_whole_cause_chain() {
        // The reason `flatten` exists. Erasing the adapter's error type must not erase what it
        // said: an operator reading a failed boot needs the driver's own complaint, not "the
        // catalog could not be loaded".
        let error = LocalService::start(&FailingCatalog, FailingWarehouse::new(source()))
            .expect_err("a catalog that fails every read starts no service");
        let ServiceNotStarted::Catalog { ref message, ref chain } = error else {
            panic!("expected a catalog failure, got {error:?}");
        };
        assert_eq!(message, "the catalog directory could not be read");
        assert_eq!(chain, &vec![String::from("no such file: catalog/")]);
    }

    #[test]
    fn a_bundle_whose_anchors_cannot_run_starts_no_service() {
        // The readiness gate, at the only place it can be enforced: `Validated` has no other
        // constructor, so this is not a check that could be skipped by a caller who forgot it.
        let error = LocalService::start(&catalog_of(bundle()), FailingWarehouse::new(source()))
            .expect_err("a data system that answers nothing validates no bundle");
        assert!(matches!(error, ServiceNotStarted::NotValidated { .. }), "{error:?}");
    }

    #[test]
    fn a_service_that_started_serves_the_bundle_it_validated() {
        // The positive case, without which every assertion above is satisfied by refusing
        // everything.
        let service = LocalService::start(&catalog_of(bundle()), crate::testing::fake_warehouse())
            .expect("an anchored bundle over a warehouse that answers validates");
        assert_eq!(service.definitions().version().as_str(), "test-1");
        // The `Debug` impl names the bundle rather than printing it.
        let rendered = format!("{service:?}");
        assert!(rendered.contains("definition_digest"), "{rendered}");
    }
}

//! Which models one caller is shown.
//!
//! Asked of each declared source under that caller's own credential -
//! `docs/adr/20261125083027-catalog-visibility-per-caller.md`.
//!
//! **It narrows what a caller SEES, and it is not a data control.** The bundle stays the
//! deployment's; a question compiles against all of it, and a question on a hidden model reaches the
//! source, whose refusal under the caller's own token is what keeps the rows away.
//!
//! **It fails closed.** A broker that did not answer, a caller with no linked account, a source that
//! did not list, or a credential that does not fit the source is a [`SurfaceFailure`] - never the
//! unfiltered view. A broker that answered *no credential at this source* hides every model on it.
//!
//! **The cache is keyed by the whole [`PrincipalChain`]**, never a bare subject (the rule
//! `docs/adr/0031`'s second amendment records), holds only a listing every declared source answered,
//! and is bounded by [`TTL`] and [`MAX_CALLERS`]. A grant the source revokes stays visible for up
//! to [`TTL`] - a window of what is SHOWN, not of what is read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use sutura_domain::identity::{CredentialBroker, Presented, PrincipalChain, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::visibility::{CallerListing, HiddenModels, VisibleTables};
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::deadline::Deadline;

use crate::Warehouses;
use crate::surface::{ErasedCause, SurfaceFailure, broker_failure};

/// How long one caller's listing is reused.
pub const TTL: Duration = Duration::from_secs(60);

/// How many callers' listings are held at once. A full cache drops the expired entries, then the
/// oldest.
pub const MAX_CALLERS: usize = 1024;

type ErasedListing = Box<dyn Fn(&Presented, Deadline) -> Result<VisibleTables, ErasedCause> + Send + Sync>;

/// Each declared source's listing for one caller.
type Listings = BTreeMap<SourceName, VisibleTables>;

/// One caller's listings, every declared source's, and when they were read.
struct Listed {
    at: Instant,
    tables: Listings,
}

/// The sources a deployment narrows its catalog by, and the per-caller cache over them.
pub struct CatalogVisibility {
    listings: BTreeMap<SourceName, ErasedListing>,
    #[expect(
        clippy::disallowed_types,
        reason = "held only to read or store one entry, never across an await point - the argument `SpendLedger`'s std Mutex makes"
    )]
    cache: std::sync::Mutex<HashMap<PrincipalChain, Listed>>,
}

impl core::fmt::Debug for CatalogVisibility {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CatalogVisibility")
            .field("sources", &self.listings.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Default for CatalogVisibility {
    fn default() -> Self {
        Self::new()
    }
}

impl CatalogVisibility {
    #[must_use]
    pub fn new() -> Self {
        #[expect(clippy::disallowed_types, reason = "see the field's own note")]
        let cache = std::sync::Mutex::new(HashMap::new());
        Self {
            listings: BTreeMap::new(),
            cache,
        }
    }

    /// Narrows the models bound to `source` by what `listing` answers each caller.
    #[must_use]
    pub fn listing<L>(mut self, source: SourceName, listing: L) -> Self
    where
        L: CallerListing + Send + Sync + 'static,
    {
        self.listings.insert(
            source,
            Box::new(move |credential, deadline| {
                listing
                    .visible_to(credential, deadline)
                    .map_err(|cause| Box::new(cause) as ErasedCause)
            }),
        );
        self
    }

    /// The models `context`'s caller is not shown in `pinned`.
    ///
    /// # Errors
    ///
    /// [`SurfaceFailure::NotLinked`] or [`SurfaceFailure::Broker`] when the broker did not mint,
    /// [`SurfaceFailure::Miswired`] when what it minted does not fit the request or the source, and
    /// [`SurfaceFailure::Warehouse`] when a source did not list.
    pub(crate) fn hidden_from<W, B>(
        &self,
        broker: &B,
        warehouses: &Warehouses<W>,
        context: &RequestContext,
        pinned: &PinnedDefinitions,
        deadline: Deadline,
    ) -> Result<HiddenModels, SurfaceFailure>
    where
        W: Warehouse,
        B: CredentialBroker,
        B::Error: Send + Sync,
    {
        if self.listings.is_empty() {
            return Ok(HiddenModels::none());
        }
        let tables = match self.cached(context.chain()) {
            Some(tables) => tables,
            None => self.list(broker, warehouses, context, deadline)?,
        };
        let hidden = pinned
            .definitions()
            .models()
            .values()
            .filter(|model| {
                self.listings.contains_key(model.source())
                    && !tables.get(model.source()).is_some_and(|shown| shown.shows(model.table()))
            })
            .map(|model| model.name().clone())
            .collect::<BTreeSet<_>>();
        Ok(HiddenModels::of(hidden))
    }

    fn cached(&self, chain: &PrincipalChain) -> Option<Listings> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(chain)
            .filter(|listed| listed.at.elapsed() < TTL)
            .map(|listed| listed.tables.clone())
    }

    /// Every declared source's listing for this caller, cached only when each one was granted.
    fn list<W, B>(
        &self,
        broker: &B,
        warehouses: &Warehouses<W>,
        context: &RequestContext,
        deadline: Deadline,
    ) -> Result<Listings, SurfaceFailure>
    where
        W: Warehouse,
        B: CredentialBroker,
        B::Error: Send + Sync,
    {
        let mut tables = BTreeMap::new();
        let mut every_source_granted = true;
        for (source, listing) in &self.listings {
            let requested = SourceSet::of(source.clone());
            let minted = broker
                .mint(context, &requested)
                .map_err(|cause| broker_failure(Box::new(cause)))?;
            let agreed = minted
                .agreeing_with(context.chain().subject(), &requested, crate::now_in_unix_seconds())
                .map_err(|cause| SurfaceFailure::Miswired { cause: Box::new(cause) })?;
            let Ok(credentials) = crate::granted(agreed) else {
                // No credential for this caller at this source: none of its models is shown.
                every_source_granted = false;
                tables.insert(source.clone(), VisibleTables::default());
                continue;
            };
            let presented = credentials
                .presented_for(source)
                .map_err(|cause| SurfaceFailure::Miswired { cause: Box::new(cause) })?;
            let warehouse = warehouses.get(source).ok_or_else(|| SurfaceFailure::Miswired {
                cause: ErasedCause::from(format!("no source `{source}` is open to list for")),
            })?;
            presented
                .agrees_with(warehouse.posture(), source)
                .map_err(|cause| SurfaceFailure::Miswired { cause: Box::new(cause) })?;
            let shown = listing(presented, deadline).map_err(|cause| SurfaceFailure::Warehouse { cause })?;
            tables.insert(source.clone(), shown);
        }
        if every_source_granted {
            self.store(context.chain().clone(), tables.clone());
        }
        Ok(tables)
    }

    fn store(&self, chain: PrincipalChain, tables: Listings) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= MAX_CALLERS && !cache.contains_key(&chain) {
            cache.retain(|_, listed| listed.at.elapsed() < TTL);
            if cache.len() >= MAX_CALLERS
                && let Some(oldest) = cache
                    .iter()
                    .min_by_key(|&(_, listed)| listed.at)
                    .map(|(chain, _)| chain.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            chain,
            Listed {
                at: Instant::now(),
                tables,
            },
        );
    }
}

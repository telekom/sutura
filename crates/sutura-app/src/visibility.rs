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
//!
//! **A cache miss dials the identity provider and every declared source, so it is bounded three
//! ways.** The transports take an admission slot before narrowing, the one the question routes
//! take. Concurrent misses for one chain share ONE listing: the first lists, the rest wait for it
//! inside their own deadline and then read what it stored ([`Flight`]). A caller the broker
//! answered *not linked* is answered so again for [`REFUSED_TTL`] without asking anyone - keyed by
//! the same whole chain, so one caller's refusal is never another's. **The limit:** only *not
//! linked* is held; a source refusing the listing, or an outage, is asked again on the next miss.

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

/// How long a caller the broker answered *not linked* is answered so without asking again.
pub const REFUSED_TTL: Duration = Duration::from_secs(10);

type ErasedListing = Box<dyn Fn(&Presented, Deadline) -> Result<VisibleTables, ErasedCause> + Send + Sync>;

/// Each declared source's listing for one caller.
type Listings = BTreeMap<SourceName, VisibleTables>;

/// One caller's listings, every declared source's, and when they were read.
struct Listed {
    at: Instant,
    tables: Listings,
}

/// What the cache holds for one chain: its listings, or that the broker answered it *not linked*.
enum Held {
    Listed(Listed),
    NotLinked(Instant),
}

impl Held {
    fn fresh(&self) -> bool {
        match *self {
            Self::Listed(ref listed) => listed.at.elapsed() < TTL,
            Self::NotLinked(at) => at.elapsed() < REFUSED_TTL,
        }
    }

    const fn at(&self) -> Instant {
        match *self {
            Self::Listed(ref listed) => listed.at,
            Self::NotLinked(at) => at,
        }
    }
}

/// One listing in flight for a chain. Whoever started it lists; a concurrent miss for the same chain
/// waits for `landed`, inside its own deadline, and then reads the cache.
#[derive(Default)]
struct Flight {
    #[expect(clippy::disallowed_types, reason = "see `CatalogVisibility::cache`")]
    done: std::sync::Mutex<bool>,
    landed: std::sync::Condvar,
}

impl Flight {
    /// Whether the flight landed within `left`.
    fn landed_within(&self, left: Duration) -> bool {
        let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        *self
            .landed
            .wait_timeout_while(done, left, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }
}

/// The listing a request is the first for: lands its [`Flight`] however the listing ends.
struct Leading<'a> {
    visibility: &'a CatalogVisibility,
    chain: &'a PrincipalChain,
    flight: std::sync::Arc<Flight>,
}

impl Drop for Leading<'_> {
    fn drop(&mut self) {
        *self.flight.done.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.flight.landed.notify_all();
        self.visibility
            .flights
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(self.chain);
    }
}

/// The sources a deployment narrows its catalog by, and the per-caller cache over them.
pub struct CatalogVisibility {
    listings: BTreeMap<SourceName, ErasedListing>,
    #[expect(
        clippy::disallowed_types,
        reason = "held only to read or store one entry, never across an await point - the argument `SpendLedger`'s std Mutex makes"
    )]
    cache: std::sync::Mutex<HashMap<PrincipalChain, Held>>,
    /// The listings in flight, one per chain.
    #[expect(clippy::disallowed_types, reason = "see `cache`")]
    flights: std::sync::Mutex<Flights>,
}

/// The listings in flight, by chain.
type Flights = HashMap<PrincipalChain, std::sync::Arc<Flight>>;

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
        #[expect(clippy::disallowed_types, reason = "see the field's own note")]
        let flights = std::sync::Mutex::new(HashMap::new());
        Self {
            listings: BTreeMap::new(),
            cache,
            flights,
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
    /// [`SurfaceFailure::Warehouse`] when a source did not list, or another request's listing for the
    /// same chain did not land inside `deadline`.
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
        let tables = match self.cached(context.chain())? {
            Some(tables) => tables,
            None => self.list_once(broker, warehouses, context, deadline)?,
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

    /// Whether [`Self::hidden_from`] would dial for `chain` now: a source declared, and neither a
    /// listing nor a refusal held fresh for it.
    pub(crate) fn dials_for(&self, chain: &PrincipalChain) -> bool {
        !self.listings.is_empty() && matches!(self.cached(chain), Ok(None))
    }

    /// The chain's held listings, its held refusal, or `None` when nothing fresh is held.
    fn cached(&self, chain: &PrincipalChain) -> Result<Option<Listings>, SurfaceFailure> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let held = match cache.get(chain).filter(|held| held.fresh()) {
            Some(Held::Listed(listed)) => Some(listed.tables.clone()),
            Some(Held::NotLinked(_)) => {
                return Err(SurfaceFailure::NotLinked {
                    cause: ErasedCause::from("the broker answered this caller not linked moments ago"),
                });
            }
            None => None,
        };
        drop(cache);
        Ok(held)
    }

    /// [`Self::list`], once per chain at a time: a miss that finds another in flight for the same
    /// chain waits for it inside `deadline` and reads what it stored, and lists itself only when it
    /// stored nothing.
    fn list_once<W, B>(
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
        let chain = context.chain();
        let (flight, leads) = self.boarded(chain);
        if leads {
            let _leading = Leading {
                visibility: self,
                chain,
                flight,
            };
            return self.list(broker, warehouses, context, deadline);
        }
        if !flight.landed_within(deadline.remaining_at(Instant::now()).unwrap_or_default()) {
            return Err(SurfaceFailure::Warehouse {
                cause: ErasedCause::from("another request listing for this caller did not finish inside this request's deadline"),
            });
        }
        self.cached(chain)?
            .map_or_else(|| self.list(broker, warehouses, context, deadline), Ok)
    }

    /// The flight in progress for `chain`, or a new one this request leads - `true` when it leads.
    fn boarded(&self, chain: &PrincipalChain) -> (std::sync::Arc<Flight>, bool) {
        let mut flights = self.flights.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(flight) = flights.get(chain) {
            return (std::sync::Arc::clone(flight), false);
        }
        let flight = std::sync::Arc::new(Flight::default());
        flights.insert(chain.clone(), std::sync::Arc::clone(&flight));
        drop(flights);
        (flight, true)
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
            let minted = broker.mint(context, &requested).map_err(|cause| {
                let failure = broker_failure(Box::new(cause));
                if matches!(failure, SurfaceFailure::NotLinked { .. }) {
                    self.hold(context.chain().clone(), Held::NotLinked(Instant::now()));
                }
                failure
            })?;
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
            self.hold(
                context.chain().clone(),
                Held::Listed(Listed {
                    at: Instant::now(),
                    tables: tables.clone(),
                }),
            );
        }
        Ok(tables)
    }

    fn hold(&self, chain: PrincipalChain, held: Held) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= MAX_CALLERS && !cache.contains_key(&chain) {
            cache.retain(|_, held| held.fresh());
            if cache.len() >= MAX_CALLERS
                && let Some(oldest) = cache
                    .iter()
                    .min_by_key(|&(_, held)| held.at())
                    .map(|(chain, _)| chain.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(chain, held);
    }
}

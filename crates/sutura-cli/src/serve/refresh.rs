//! Re-reading a declared catalog on an interval and serving what it read, without a restart -
//! `github.com/telekom/sutura#975`, `#1326`.
//!
//! A `Refresher` owns the already-open catalogs and the served [`Surface`]. Each tick re-reads and
//! re-composes every catalog exactly as boot did (`crate::catalog::load_each`), then hands the
//! bundle to [`Surface::adopt`], which holds it to what boot held the first one to and stores it
//! whole or not at all. The interval is `catalogs[].refresh_seconds`, a deployment's own declaration,
//! so it is the DRIVING loop's argument rather than this module's own; `poll_once` does not know it.
//!
//! # What a refusal means, and what a change means
//!
//! A re-read that fails (the remote endpoint is down, a document no longer validates), or a bundle
//! that the surface refuses (an anchor no longer reproduces, a model names a table this process does
//! not hold), keeps the bundle already served and logs loudly rather than tearing anything down: an
//! answer keeps being computed from the last GOOD bundle. A bundle with the digest already served is
//! silent (`Outcome::Unchanged`); a different one is stored and logged by digest, never by content -
//! `docs/adr/0010`'s reasoning about a log line applies here too.
//!
//! # Which question sees which bundle
//!
//! `Surface::answer` takes one snapshot when a question starts and keeps it until it ends, so a
//! swap that lands mid-question changes nothing for it; the next question reads the new bundle.
//! Both transports answer through `Surface::answer`, and read the catalog through
//! `Surface::definitions`, so this module has no transport-specific half.

use std::sync::Arc;
use std::time::Duration;

use sutura_app::surface::{Adopted, Surface};
use sutura_domain::pinned::SemanticCatalog;
use sutura_runtime::Gauge;

use crate::catalog::{OpenedCatalogs, load_each};

/// What one look at a declared catalog decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Re-read successfully, and the surface kept the bundle it serves (`Adopted::Unchanged`).
    Unchanged,
    /// Re-read successfully, the digest changed, and the new bundle is what the next question reads.
    Rotated,
    /// The re-read failed, or the surface refused the bundle read. What was served before this look
    /// is still what the next question reads.
    Rejected,
}

/// The poll side: owns the already-open catalog (so it can re-read it), the surface the result is
/// handed to, and the `sutura_catalog_metrics` gauge a swap moves.
///
/// Generic in `K` for the reason `LocalService::start_composed` is: the catalog type is
/// monomorphised per declared KIND at the composition root, and this module does not add a second
/// way to erase it.
pub(crate) struct Refresher<K> {
    catalogs: Vec<K>,
    surface: Arc<dyn Surface>,
    coverage: Gauge,
}

impl<K> Refresher<K>
where
    K: SemanticCatalog,
{
    /// Builds a refresher around the SAME already-open catalogs boot opened and the SAME surface it
    /// serves - `Arc` because the transports and this poll are two owners of one service - and
    /// `coverage`, the served state's own `sutura_catalog_metrics` gauge.
    #[must_use]
    pub(crate) fn new(catalogs: Vec<K>, surface: Arc<dyn Surface>, coverage: Gauge) -> Self {
        Self {
            catalogs,
            surface,
            coverage,
        }
    }

    /// Re-reads every declared catalog, re-composes them, and offers the result to the surface.
    ///
    /// Blocking: the re-read does I/O and a changed bundle re-runs its anchors against the data
    /// systems. [`spawn`] runs it on the blocking pool.
    pub(crate) fn poll_once(&self) -> Outcome {
        let next = match load_each(&self.catalogs) {
            Ok(next) => next,
            Err(cause) => {
                tracing::error!(
                    error = %cause,
                    "a declared catalog's refresh failed to load; keeping the bundle already served"
                );
                return Outcome::Rejected;
            }
        };
        match self.surface.adopt(next) {
            Ok(Adopted::Unchanged) => Outcome::Unchanged,
            Ok(Adopted::Rotated { previous, digest }) => {
                self.coverage
                    .set(self.surface.definitions().definitions().metrics().len() as u64);
                tracing::info!(
                    previous_digest = previous.as_str(),
                    digest = digest.as_str(),
                    "a declared catalog refresh re-pinned the served bundle"
                );
                Outcome::Rotated
            }
            Err(cause) => {
                tracing::error!(
                    error = %cause,
                    causes = ?sutura_app::surface::cause_chain(&cause),
                    "a refreshed bundle was refused; keeping the bundle already served"
                );
                Outcome::Rejected
            }
        }
    }
}

/// The shortest declared `refresh_seconds` among the entries this build actually opened.
///
/// **Shortest, not first, and not an average.** Several catalogs of one kind are already composed
/// into one bundle by `load_each`, so there is one bundle to re-pin regardless of how many
/// entries declared an interval - the shortest is the one that makes every declared interval hold
/// (an entry that asked for one minute is never left waiting five just because a second entry in
/// the same deployment asked for less).
pub(crate) fn shortest_declared_interval(declared: &sutura_config::Catalogs) -> Option<Duration> {
    declared
        .each()
        .filter_map(sutura_config::CatalogSettings::refresh_seconds)
        .min()
        .map(Duration::from_secs)
}

/// Starts the refresh poll for whichever catalogs this build opened, if any entry declared
/// `refresh_seconds` and a tokio runtime is already running.
///
/// **No runtime means it does not poll**, the same shape `crate::rotation::drive_rotation`
/// documents for the outbound-material rotation: call this from `serve_until_stopped`, where
/// `runtime.block_on` has already entered one, never from the synchronous boot section above it.
///
/// Takes the opened catalogs BY VALUE: the poll is their last holder, so nothing is cloned for it -
/// and a catalog holding a credential (the `rdbms` reader's connection string) need not be `Clone`
/// at all.
pub(crate) fn drive(catalogs: OpenedCatalogs, surface: Arc<dyn Surface>, coverage: Gauge, interval: Option<Duration>) {
    let Some(interval) = interval else {
        return;
    };
    if tokio::runtime::Handle::try_current().is_err() {
        tracing::warn!("catalogs[].refresh_seconds is declared, but no tokio runtime is running; it will not be polled");
        return;
    }
    tracing::info!(
        interval_seconds = interval.as_secs(),
        "this deployment's catalog will be polled and re-pinned on a declared interval"
    );
    match catalogs {
        OpenedCatalogs::Markdown(catalogs) => spawn(catalogs, surface, coverage, interval),
        #[cfg(feature = "datahub")]
        OpenedCatalogs::Datahub(catalogs) => spawn(catalogs, surface, coverage, interval),
        OpenedCatalogs::Okf(catalogs) => spawn(catalogs, surface, coverage, interval),
        #[cfg(feature = "openmetadata")]
        OpenedCatalogs::Openmetadata(catalogs) => spawn(catalogs, surface, coverage, interval),
        OpenedCatalogs::DataContract(catalogs) => spawn(catalogs, surface, coverage, interval),
        #[cfg(feature = "rdbms")]
        OpenedCatalogs::Rdbms(catalogs) => spawn(catalogs, surface, coverage, interval),
    }
}

/// One kind's worth of the poll loop: polls on `interval`, each poll on the blocking pool.
///
/// The refresher is moved into the blocking task and handed back with its outcome, so nothing is
/// shared or cloned to get a `'static` closure. A panic inside a poll ends the loop, loudly: a
/// refresher that may have died half-way is not one to keep calling.
fn spawn<K>(catalogs: Vec<K>, surface: Arc<dyn Surface>, coverage: Gauge, interval: Duration)
where
    K: SemanticCatalog + Send + Sync + 'static,
    K::Error: Send + Sync,
{
    let mut refresher = Refresher::new(catalogs, surface, coverage);
    drop(tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            match sutura_runtime::spawn_carrying_span(move || {
                let outcome = refresher.poll_once();
                (refresher, outcome)
            })
            .await
            {
                Ok((polled, _outcome)) => refresher = polled,
                Err(cause) => {
                    tracing::error!(error = %cause, "the catalog refresh panicked; it will not be polled again");
                    return;
                }
            }
        }
    }));
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
    use sutura_domain::catalog::{Audience, Definitions, Description, InconsistentDefinitions, Metric, Model};
    use sutura_domain::definitions::NotDigestible;
    use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, InvalidIdentifier, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{
        CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
    };

    use sutura_config::{CatalogKind as ConfiguredKind, CatalogSettings, Catalogs};

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, PoisonError, RwLock};

    use sutura_app::surface::{Adopted, ErasedCause, NotAdopted, Surface, SurfaceFailure};
    use sutura_domain::identity::RequestContext;
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::warehouse::deadline::Deadline;

    use super::{Outcome, Refresher, shortest_declared_interval};

    /// What this test's fake catalog can be told to answer next.
    #[derive(Clone)]
    enum Answer {
        Bundle(Vec<&'static str>),
        Refused,
    }

    /// Why the fake refused - the only variant this module's error type needs, since the fake
    /// never produces the domain's own composition failures.
    #[derive(Debug, thiserror::Error)]
    #[error("the fake reader was told to refuse this read")]
    struct FakeRefused;

    impl From<InconsistentDefinitions> for FakeRefused {
        fn from(_: InconsistentDefinitions) -> Self {
            Self
        }
    }
    impl From<InconsistentKnowledge> for FakeRefused {
        fn from(_: InconsistentKnowledge) -> Self {
            Self
        }
    }
    impl From<NotDigestible> for FakeRefused {
        fn from(_: NotDigestible) -> Self {
            Self
        }
    }
    impl From<InvalidIdentifier> for FakeRefused {
        fn from(_: InvalidIdentifier) -> Self {
            Self
        }
    }

    /// The read side of this test's fake: a cell the TEST changes between calls, and never across
    /// a thread - `Refresher::poll_once` runs on the test's own thread, never through
    /// `super::spawn`, so `Rc<RefCell<_>>` is correct here and `Arc<Mutex<_>>` would be the wrong
    /// primitive for a single-threaded fake rather than a stricter one.
    type Cell = Rc<RefCell<Answer>>;

    /// A catalog whose `load()` reads a shared cell the TEST changes between calls - the "fake
    /// reader" the acceptance asks for: not a fixed corpus, because this property needs the SAME
    /// catalog to answer differently across two reads.
    #[derive(Clone)]
    struct FakeCatalog {
        name: SourceName,
        version: DefinitionVersion,
        next: Cell,
    }

    impl SemanticCatalog for FakeCatalog {
        type Error = FakeRefused;

        const KIND: CatalogKind = CatalogKind::Declaring;

        fn capabilities() -> MetadataCapabilities {
            MetadataCapabilities::of(
                DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains])
                    .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
                KnowledgeCapabilities::none(),
            )
        }

        fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
            let columns = match &*self.next.borrow() {
                Answer::Refused => return Err(FakeRefused),
                Answer::Bundle(columns) => columns.clone(),
            };
            let model = Model::new(
                ModelName::parse("fake_model")?,
                self.name.clone(),
                TableName::parse("fake_table")?,
                columns
                    .iter()
                    .copied()
                    .map(ColumnName::parse)
                    .collect::<Result<BTreeSet<_>, _>>()?,
                Description::parse("A fake catalog for the refresher's own tests.").map_err(|_cause| FakeRefused)?,
            );
            // One metric per column, so the number of metrics served is the number of columns read.
            let metrics = columns
                .iter()
                .copied()
                .map(|column| {
                    Metric::new(
                        MetricName::parse(column)?,
                        ModelName::parse("fake_model")?,
                        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                            Aggregate::Sum,
                            ColumnName::parse(column)?,
                        ))),
                        Vec::new(),
                        ColumnName::parse(column)?,
                        BTreeSet::from([Grain::Month]),
                        Vec::new(),
                        None,
                        Description::default(),
                        Audience::Open,
                    )
                    .map_err(|_cause| FakeRefused)
                })
                .collect::<Result<Vec<_>, FakeRefused>>()?;
            let definitions = Definitions::assemble(vec![model], Vec::new(), metrics)?;
            let capabilities = MetadataCapabilities::of(
                DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains])
                    .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
                KnowledgeCapabilities::none(),
            );
            Ok(PinnedDefinitions::pin(
                self.version.clone(),
                definitions,
                Knowledge::none(),
                ContributionManifest::single(self.name.clone(), Contribution::of(capabilities)),
            )?)
        }
    }

    fn fake(initial: Vec<&'static str>) -> (FakeCatalog, Cell) {
        let next = Rc::new(RefCell::new(Answer::Bundle(initial)));
        (
            FakeCatalog {
                name: SourceName::parse("fake").expect("a test source is a source"),
                version: DefinitionVersion::parse("fake-1").expect("a test version is a version"),
                next: Rc::clone(&next),
            },
            next,
        )
    }

    fn refreshing(catalog: FakeCatalog, surface: &Arc<FakeSurface>, coverage: sutura_runtime::Gauge) -> Refresher<FakeCatalog> {
        Refresher::new(vec![catalog], Arc::<FakeSurface>::clone(surface), coverage)
    }

    /// The gauge a refresher moves, and the registry that renders it.
    pub(crate) fn coverage() -> (sutura_runtime::Gauge, sutura_runtime::Registry) {
        let mut builder = sutura_runtime::RegistryBuilder::default();
        let gauge = builder.gauge("sutura_catalog_metrics");
        (gauge, builder.build())
    }

    fn set(cell: &Cell, answer: Answer) {
        *cell.borrow_mut() = answer;
    }

    /// **Shortest, not first, and not an average** - the doc comment's own claim, otherwise
    /// unexercised: a `.min()` weakened to `.max()` would leave the second entry's declared 30s
    /// waiting for the first entry's declared 300s instead.
    #[test]
    fn the_declared_interval_is_the_shortest_across_catalogs_not_the_first() {
        let first = CatalogSettings::parse(
            SourceName::parse("first").expect("a test source is a source"),
            ConfiguredKind::Markdown,
            std::path::PathBuf::from("catalog"),
            std::path::PathBuf::from("data"),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .expect("a complete markdown entry parses")
        .with_refresh_seconds(Some(300))
        .expect("a positive interval is usable");
        let second = CatalogSettings::parse(
            SourceName::parse("second").expect("a test source is a source"),
            ConfiguredKind::Markdown,
            std::path::PathBuf::from("catalog"),
            std::path::PathBuf::from("data"),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .expect("a complete markdown entry parses")
        .with_refresh_seconds(Some(30))
        .expect("a positive interval is usable");
        let declared = Catalogs::parse(vec![first, second]).expect("two distinctly named catalogs are valid");

        assert_eq!(
            shortest_declared_interval(&declared),
            Some(std::time::Duration::from_secs(30))
        );
    }

    /// A served bundle that stores whatever differs from it, and counts what it was offered - the
    /// surface as the refresher sees it. What `LocalService` adds to this (the checks, and which
    /// snapshot a question reads) is held by its own cells in `sutura-app`.
    #[expect(
        clippy::disallowed_types,
        reason = "a test fake behind an Arc: the lock only clones or stores one `Arc`, never across an await point, the license `sutura_mcp`'s RecordingSurface already holds"
    )]
    pub(crate) struct FakeSurface {
        served: RwLock<Arc<PinnedDefinitions>>,
        offered: AtomicUsize,
        refuses: bool,
    }

    impl FakeSurface {
        #[expect(clippy::disallowed_types, reason = "see the struct's own note")]
        pub(crate) fn serving(initial: PinnedDefinitions, refuses: bool) -> Arc<Self> {
            Arc::new(Self {
                served: RwLock::new(Arc::new(initial)),
                offered: AtomicUsize::new(0),
                refuses,
            })
        }

        pub(crate) fn digest(&self) -> String {
            self.definitions().digest().as_str().to_owned()
        }
    }

    impl Surface for FakeSurface {
        fn definitions(&self) -> Arc<PinnedDefinitions> {
            Arc::clone(&self.served.read().unwrap_or_else(PoisonError::into_inner))
        }

        fn adopt(&self, next: PinnedDefinitions) -> Result<Adopted, NotAdopted> {
            self.offered.fetch_add(1, Ordering::SeqCst);
            if self.refuses {
                return Err(NotAdopted::Preflight {
                    cause: ErasedCause::from("the fake refuses every bundle"),
                });
            }
            let previous = self.definitions().digest().clone();
            if next.digest() == &previous {
                return Ok(Adopted::Unchanged);
            }
            let digest = next.digest().clone();
            *self.served.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(next);
            Ok(Adopted::Rotated { previous, digest })
        }

        fn answer(&self, _: &RequestContext, _: &Query, _: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
            Err(SurfaceFailure::Compile {
                cause: Box::new(FakeRefused),
            })
        }

        fn run_sql(
            &self,
            _: &RequestContext,
            _: &sutura_domain::raw::RawStatement,
            _: Deadline,
        ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
            Err(SurfaceFailure::Compile {
                cause: Box::new(FakeRefused),
            })
        }

        fn spend_headroom_bytes(&self) -> Option<u64> {
            None
        }

        fn spent_bytes_total(&self) -> Option<u64> {
            None
        }
    }

    #[test]
    fn an_unchanged_read_is_silent_and_the_surface_keeps_its_bundle() {
        let (catalog, _next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let surface = FakeSurface::serving(initial, false);
        let before = surface.digest();
        let refresher = refreshing(catalog, &surface, coverage().0);

        assert_eq!(refresher.poll_once(), Outcome::Unchanged);
        assert_eq!(surface.digest(), before);
    }

    /// **The change cell.** Column set changes -> content changes -> digest changes -> the surface
    /// serves the new bundle.
    #[test]
    fn a_changed_read_is_what_the_surface_serves_next() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let surface = FakeSurface::serving(initial, false);
        let before = surface.digest();
        let (gauge, registry) = coverage();
        let refresher = refreshing(catalog, &surface, gauge);
        assert!(
            registry.render().contains("sutura_catalog_metrics 0"),
            "{}",
            registry.render()
        );

        set(&next, Answer::Bundle(vec!["a", "b"]));
        assert_eq!(refresher.poll_once(), Outcome::Rotated);
        assert_ne!(
            surface.digest(),
            before,
            "a genuinely different bundle must be served under a new digest"
        );
        assert_eq!(surface.definitions().definitions().metrics().len(), 2);
        assert!(
            registry.render().contains("sutura_catalog_metrics 2"),
            "the coverage gauge follows the bundle now served: {}",
            registry.render()
        );

        // The identical content is silent on the next tick - it produces the same digest again.
        assert_eq!(refresher.poll_once(), Outcome::Unchanged);
    }

    /// **The failed-read cell.** A catalog that cannot be read is never offered to the surface, so
    /// what is served does not move - and keeps not moving across repeated failures.
    #[test]
    fn a_failed_read_never_reaches_the_surface_and_the_old_bundle_serves() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let surface = FakeSurface::serving(initial, false);
        let before = surface.digest();
        let refresher = refreshing(catalog, &surface, coverage().0);

        set(&next, Answer::Refused);
        assert_eq!(refresher.poll_once(), Outcome::Rejected);
        assert_eq!(refresher.poll_once(), Outcome::Rejected);

        assert_eq!(
            surface.offered.load(Ordering::SeqCst),
            0,
            "a bundle that did not load is not offered"
        );
        assert_eq!(surface.digest(), before);
    }

    /// **The refused-bundle cell.** A bundle the surface will not store is a failed refresh: the
    /// bundle already served stays served.
    #[test]
    fn a_bundle_the_surface_refuses_keeps_the_old_one_served() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let surface = FakeSurface::serving(initial, true);
        let before = surface.digest();
        let refresher = refreshing(catalog, &surface, coverage().0);

        set(&next, Answer::Bundle(vec!["a", "b"]));
        assert_eq!(refresher.poll_once(), Outcome::Rejected);

        assert_eq!(
            surface.offered.load(Ordering::SeqCst),
            1,
            "the bundle was offered and refused"
        );
        assert_eq!(surface.digest(), before);
    }
}

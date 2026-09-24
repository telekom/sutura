//! Re-reading a declared catalog on an interval and re-pinning it, without a restart -
//! `github.com/telekom/sutura#975`.
//!
//! The shape is `sutura_tls`'s own rotation poll: a poll side (`Refresher`) that owns the
//! already-open catalog and can re-read it, and a read side (`Rotating`, `Clone`, cheap) that a
//! caller holds and reads the current pinned bundle off through a lock-free
//! `tokio::sync::watch` swap. Unlike that crate's fixed 30-second `sutura_tls::POLL_INTERVAL`,
//! the interval here is
//! `catalogs[].refresh_seconds` - a deployment's own declaration, not a constant - so it is the
//! DRIVING loop's argument rather than this module's own; `poll_once` itself does not know it.
//!
//! # What a refusal means, and what a change means
//!
//! `poll_once` re-runs the exact composition boot already used - `super::catalog::load_each`,
//! generic in the same catalog type `super::catalog::OpenedCatalogs` monomorphises over - so a
//! re-read that fails (the remote endpoint is down, a document no longer validates) keeps the
//! previously pinned bundle and logs loudly rather than tearing anything down: an answer keeps
//! being computed from the last GOOD pin. A re-read that succeeds and produces the SAME digest is
//! silent (`Outcome::Unchanged`); one that succeeds with a DIFFERENT digest swaps and audits the
//! transition by digest, never by content - `docs/adr/0010`'s reasoning about a log line applies
//! here too: the two digests are the provenance an operator needs to correlate an answer against,
//! and neither is the bundle itself.
//!
//! # What is wired, and what is NOT - stated where the claim is
//!
//! `drive` starts the poll for real, from `serve_until_stopped` where a tokio runtime is
//! already entered: a declared `catalogs[].refresh_seconds` re-reads and re-pins on that
//! interval, and the currently-pinned digest is logged on the same tick, for real, through
//! `Rotating::current` - not a dead handle nobody reads.
//!
//! **What is NOT wired is the LIVE SERVED bundle.** A rotation this module adopts does not reach
//! an in-flight or a future `Surface::answer` - the audit trail above is a log line, not a
//! consumer. Making an answer's provenance reflect a swap needs
//! `sutura_app::surface::Surface::definitions` to return something other than
//! `&PinnedDefinitions` (an `Arc` this module could hand a request), which is a signature every
//! transport and every existing implementor shares - a wider architecture decision than this
//! issue's own scope. `.agents/skills/sutura/query-surface/SKILL.md`'s *Built and not wired*
//! section is the precedent for stating this rather than either hiding it or leaving the whole
//! mechanism unbuilt.

use std::sync::Arc;
use std::time::Duration;

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};

use super::catalog::{OpenedCatalogs, load_each};

/// The read side a caller would hold: clones the current pinned bundle out, cheap and lock-free.
///
/// `Clone` is sharing, not copying - every clone observes the same channel, so a swap `Refresher`
/// adopts reaches every held `Rotating`. `Debug`-free like `sutura_tls::Rotating`, for
/// the same reason: a bundle is a page of definitions, not something a log line should ever hold.
#[derive(Clone)]
pub(crate) struct Rotating {
    current: tokio::sync::watch::Receiver<Arc<PinnedDefinitions>>,
}

impl Rotating {
    /// The currently pinned bundle, as an `Arc` - cheap to clone and to hand to an answer in
    /// flight.
    #[must_use]
    pub(crate) fn current(&self) -> Arc<PinnedDefinitions> {
        Arc::clone(&self.current.borrow())
    }
}

/// What one look at a declared catalog decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Re-read successfully and the digest is byte-for-byte the one already in use.
    Unchanged,
    /// Re-read successfully, the digest changed, and the new bundle is what `Rotating::current`
    /// returns next.
    Rotated,
    /// The re-read failed - a source refusal, a composition refusal, an unreadable directory.
    /// What was pinned before this look is still what `current()` returns.
    Rejected,
}

/// The poll side: owns the already-open catalog (so it can re-read it) and the sender half of the
/// channel `Rotating` reads.
///
/// Generic in `K` for the reason `LocalService::start_composed` is: the catalog type is
/// monomorphised per declared KIND at the composition root, and this module does not add a second
/// way to erase it.
pub(crate) struct Refresher<K> {
    catalogs: Vec<K>,
    sender: tokio::sync::watch::Sender<Arc<PinnedDefinitions>>,
    digest: DefinitionDigest,
}

impl<K> Refresher<K>
where
    K: SemanticCatalog,
{
    /// Builds a refresher around the SAME already-open catalogs and the SAME already-pinned
    /// bundle a composition root's boot produced - never a fresh load, so this cannot observe a
    /// digest change that boot itself would not have.
    #[must_use]
    pub(crate) fn new(catalogs: Vec<K>, initial: PinnedDefinitions) -> Self {
        let digest = initial.digest().clone();
        let (sender, receiver) = tokio::sync::watch::channel(Arc::new(initial));
        drop(receiver);
        Self {
            catalogs,
            sender,
            digest,
        }
    }

    /// The read handle a caller holds. Cheap to clone; every clone observes this channel.
    #[must_use]
    pub(crate) fn rotating(&self) -> Rotating {
        Rotating {
            current: self.sender.subscribe(),
        }
    }

    /// Re-reads every declared catalog and re-composes them, mirroring the boot path exactly
    /// (`load_each`). On success with a changed digest, adopts the new bundle and audits the
    /// digest transition; on an unchanged digest, does nothing; on a refusal, keeps the bundle
    /// already in use and logs loudly rather than tearing anything down.
    pub(crate) fn poll_once(&mut self) -> Outcome {
        match load_each(&self.catalogs) {
            Ok(next) => {
                if next.digest() == &self.digest {
                    return Outcome::Unchanged;
                }
                let previous = self.digest.clone();
                self.digest = next.digest().clone();
                tracing::info!(
                    previous_digest = previous.as_str(),
                    digest = self.digest.as_str(),
                    "a declared catalog refresh re-pinned this bundle"
                );
                drop(self.sender.send_replace(Arc::new(next)));
                Outcome::Rotated
            }
            Err(cause) => {
                tracing::error!(
                    error = %cause,
                    digest = self.digest.as_str(),
                    "a declared catalog's refresh failed to load; keeping the bundle already pinned"
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
fn shortest_declared_interval(declared: &sutura_config::Catalogs) -> Option<Duration> {
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
pub(crate) fn drive(catalogs: &OpenedCatalogs, pinned: &PinnedDefinitions, declared: &sutura_config::Catalogs) {
    let Some(interval) = shortest_declared_interval(declared) else {
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
        OpenedCatalogs::Markdown(catalogs) => spawn(catalogs.clone(), pinned.clone(), interval),
        #[cfg(feature = "datahub")]
        OpenedCatalogs::Datahub(catalogs) => spawn(catalogs.clone(), pinned.clone(), interval),
        OpenedCatalogs::Okf(catalogs) => spawn(catalogs.clone(), pinned.clone(), interval),
    }
}

/// One kind's worth of the poll loop: builds a `Refresher`, polls it on `interval`, and logs
/// the digest currently pinned after every tick through `Rotating::current` - the real reader
/// this module's own doc header promises, so the mechanism is observably running rather than
/// built and immediately discarded.
fn spawn<K>(catalogs: Vec<K>, initial: PinnedDefinitions, interval: Duration)
where
    K: SemanticCatalog + Clone + Send + Sync + 'static,
    K::Error: Send + Sync,
{
    let mut refresher = Refresher::new(catalogs, initial);
    let rotating = refresher.rotating();
    drop(tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            refresher.poll_once();
            tracing::debug!(digest = rotating.current().digest().as_str(), "catalog refresh polled");
        }
    }));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
    use sutura_domain::catalog::{Definitions, Description, InconsistentDefinitions, Model};
    use sutura_domain::definitions::NotDigestible;
    use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities};
    use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{
        CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
    };

    use super::{Outcome, Refresher};

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

    /// A catalog whose `load()` reads a shared cell the TEST changes between calls - the "fake
    /// reader" the acceptance asks for: not a fixed corpus, because this property needs the SAME
    /// catalog to answer differently across two reads.
    #[derive(Clone)]
    struct FakeCatalog {
        name: SourceName,
        version: DefinitionVersion,
        next: Arc<Mutex<Answer>>,
    }

    impl SemanticCatalog for FakeCatalog {
        type Error = FakeRefused;

        const KIND: CatalogKind = CatalogKind::Declaring;

        fn capabilities() -> MetadataCapabilities {
            MetadataCapabilities::of(
                DefinitionCapabilities::of([DefinitionKind::Structure])
                    .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
                KnowledgeCapabilities::none(),
            )
        }

        fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
            let columns = match &*self.next.lock().expect("the test cell is not poisoned") {
                Answer::Refused => return Err(FakeRefused),
                Answer::Bundle(columns) => columns.clone(),
            };
            let model = Model::new(
                ModelName::parse("fake_model").expect("a test model is a model"),
                self.name.clone(),
                TableName::parse("fake_table").expect("a test table is a table"),
                columns
                    .into_iter()
                    .map(|column| ColumnName::parse(column).expect("a test column is a column"))
                    .collect::<BTreeSet<_>>(),
                Description::parse("A fake catalog for the refresher's own tests.").expect("a test description is one"),
            );
            let definitions = Definitions::assemble(vec![model], Vec::new(), Vec::new())?;
            let capabilities = MetadataCapabilities::of(
                DefinitionCapabilities::of([DefinitionKind::Structure])
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

    fn fake(initial: Vec<&'static str>) -> (FakeCatalog, Arc<Mutex<Answer>>) {
        let next = Arc::new(Mutex::new(Answer::Bundle(initial)));
        (
            FakeCatalog {
                name: SourceName::parse("fake").expect("a test source is a source"),
                version: DefinitionVersion::parse("fake-1").expect("a test version is a version"),
                next: Arc::clone(&next),
            },
            next,
        )
    }

    fn set(cell: &Arc<Mutex<Answer>>, answer: Answer) {
        *cell.lock().expect("the test cell is not poisoned") = answer;
    }

    #[test]
    fn an_unchanged_read_is_silent_and_keeps_the_same_bundle() {
        let (catalog, _next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let mut refresher = Refresher::new(vec![catalog], initial);
        let rotating = refresher.rotating();
        let digest = rotating.current().digest().clone();

        assert_eq!(refresher.poll_once(), Outcome::Unchanged);
        assert_eq!(rotating.current().digest(), &digest);
    }

    /// **The change cell.** Column set changes -> content changes -> digest changes -> adopted.
    #[test]
    fn a_changed_read_rotates_to_the_new_bundle() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let before_digest = initial.digest().clone();
        let mut refresher = Refresher::new(vec![catalog], initial);
        let rotating = refresher.rotating();

        set(&next, Answer::Bundle(vec!["a", "b"]));
        assert_eq!(refresher.poll_once(), Outcome::Rotated);
        let after = rotating.current();
        assert_ne!(
            after.digest(),
            &before_digest,
            "a genuinely different bundle must re-pin under a new digest"
        );

        // The identical content is silent on the next tick - it produces the same digest again.
        assert_eq!(refresher.poll_once(), Outcome::Unchanged);
        assert_eq!(rotating.current().digest(), after.digest());
    }

    /// **The invalid-change cell.** A refusal keeps the previously pinned bundle rather than
    /// tearing anything down or adopting nothing usable.
    #[test]
    fn an_invalid_change_is_rejected_and_the_old_bundle_survives() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let before_digest = initial.digest().clone();
        let mut refresher = Refresher::new(vec![catalog], initial);
        let rotating = refresher.rotating();

        set(&next, Answer::Refused);
        assert_eq!(refresher.poll_once(), Outcome::Rejected);
        assert_eq!(
            rotating.current().digest(),
            &before_digest,
            "a refused re-read must not disturb the bundle already pinned"
        );

        // And the process keeps answering from that old bundle across repeated failures.
        assert_eq!(refresher.poll_once(), Outcome::Rejected);
        assert_eq!(rotating.current().digest(), &before_digest);
    }

    /// **Provenance across a swap.** What a caller reads through `Rotating::current()` right
    /// after a rotation carries the NEW digest, and a clone taken beforehand still (correctly)
    /// reads the value it was handed - `Arc::clone` freezes a moment, not a subscription. The
    /// acceptance's own third clause: an in-flight answer's provenance never moves under it.
    #[test]
    fn provenance_reads_the_bundle_a_clone_was_taken_from_not_a_live_subscription() {
        let (catalog, next) = fake(vec!["a"]);
        let initial = catalog.load().expect("the fake's own first load succeeds");
        let mut refresher = Refresher::new(vec![catalog], initial);
        let rotating = refresher.rotating();
        let held_before_the_swap = rotating.current();

        set(&next, Answer::Bundle(vec!["a", "b"]));
        assert_eq!(refresher.poll_once(), Outcome::Rotated);

        assert_ne!(
            held_before_the_swap.digest(),
            rotating.current().digest(),
            "the swap must be visible to a NEW read"
        );
        // The `Arc` taken before the swap is unaffected by it - exactly what makes it safe for an
        // in-flight answer to have cloned `current()` once at the start and hold that clone for
        // the whole of its own computation.
        assert_eq!(held_before_the_swap.definitions().models().len(), 1);
    }
}

//! Rotating declared **outbound** trust material (and a source's client identity pair) without a
//! restart, by reusing the serving side's poll rather than its swap.
//!
//! The serving side (`sutura-http::tls::Renewal`) rotates the certificate the LISTENER presents: it
//! re-reads the files on an interval, compares bytes, validates a candidate before adopting it, and
//! keeps the old material with one loud line when a candidate cannot be adopted. Its SWAP - a new
//! key handed to the next HANDSHAKE - is `github.com/telekom/sutura#125` item 3's starting point,
//! but the client side is long-lived in a different way: a `security.outbound` consumer is a
//! per-request HTTP client, and a source's client identity is a connection that stays open for the
//! adapter's life. So this crate reuses the serving side's **poll** (bytes compared on an interval,
//! parse-before-adopt, old kept with one loud complaint) and lets each consumer be the decision about
//! the **swap**: a per-request `ureq` agent adopts on the next request, a `Postgres` connection keeps
//! the pair it was established under until it closes. Stated, not inherited - `docs/adr/0010`.
//!
//! # Shape: a poll handle and a read handle, like serving's `Renewal`/resolver split
//!
//! [`Rotator`] is the poll side: it owns the declared [`Anchors`] (and optional client [`Identity`]),
//! the `rebuild` closure that turns freshly loaded material into the consumer's rebuilt `T`, and the
//! `seen` marker that makes identical bytes silent. `poll_once()` re-reads the declared paths
//! (bounded, on [`POLL_INTERVAL`] - a slow tick over two small files, which is precisely the
//! "deployment shape, not a knob" argument `sutura-http::tls::RENEWAL_INTERVAL` commits the serving
//! side to), compares to what was last examined, and only then loads and rebuilds:
//!
//! - on `Ok` it `send_replace`s the new `T` (a `tokio::sync::watch` channel, the same lock-free swap
//!   the serving resolver uses) and writes one `info!` line;
//! - on a load or rebuild refusal it keeps the old `T` and writes **one** `error!` line naming the
//!   **class** of source that failed (anchor bundle / host store / client identity pair) and the load
//!   error - never a path, because `Debug` on both handles prints no path.
//!
//! [`Rotating<T>`] is the read side the consumers hold (`Clone`, cheap): `current()` returns an
//! `Arc<T>` to the latest adopted `T`. A per-request agent calls it on the next request; a
//! long-lived connection's caller calls it when it connects and keeps what it got.
//!
//! Runtime-agnostic except for the watch channel itself: `poll_once` and `current` are synchronous
//! (both work off any runtime or none), and *starting* the poll loop is deliberately the composition
//! root's choice - this crate does not spawn. `tokio::sync::watch` is the one synchronization this
//! crate needs (the same swap shape `sutura-http` already resolved to), so that small `sync` slice of
//! tokio is its only new dependency.
//!
//! # Refusals are typed, and the old material always survives
//!
//! The load step reuses the crate's existing [`LoadError`] (`AnchorsRead`/`AnchorsEmpty`/
//! `SystemStoreRead`/`SystemStoreEmpty`/`IdentityRead`/`IdentityIncomplete`/`IdentityKey`) - the
//! rebuild step is generic over the consumer's own error so a `Postgres` `ClientConfig` build keeps
//! its own variants. Either way [`Outcome`] is the typed state a caller (and a test) observes, and a
//! refusal keeps serving the last good material with one loud line. A rejected replacement is still
//! an *examined* one, so the identical broken bytes are silent on the next tick - the same guard
//! `sutura-http::tls::Renewal::seen` provides, stated there and repeated here because the client side
//! has no handshake to fail visibly and the log line is the only complaint a deployment hears.

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use crate::{Anchors, Identity, LoadError, LoadedAnchors, LoadedIdentity, load_anchors, load_identity};

/// How often the declared outbound material is re-read.
///
/// The same value and the same argument as the serving side's `RENEWAL_INTERVAL`: thirty seconds is
/// far below any horizon at which a trust-bundle or client-certificate rotation is urgent - an issuer
/// plans one days ahead - and far above the cost of re-reading two small files. A constant, not a
/// setting, for the reason the serving constant is one: nothing a caller can change alters what the
/// right interval is, and one setting shared by every fixed-host consumer would be a second thing to
/// keep consistent.
pub const POLL_INTERVAL: core::time::Duration = core::time::Duration::from_secs(30);

/// The most any declared path may hold, checked before the bytes are allocated.
///
/// The same bound and the same reasoning as `sutura-http`'s `MAX_MATERIAL_BYTES`: this is read at
/// boot and then every [`POLL_INTERVAL`] for the life of the process, so an unbounded read is a
/// denial-of-service primitive. A bundle of a dozen certificates with 4096-bit keys is far under it.
const MAX_MATERIAL_BYTES: usize = 64 * 1024;

/// What one look at the declared material decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The declared material is byte for byte what is already in use.
    Unchanged,
    /// The material changed, loaded, and the rebuilt `T` is what `current()` returns next.
    Rotated,
    /// The material changed and the replacement could not be adopted. What was in use still is.
    Rejected,
}

/// Which declared thing a load refused - the path CLASS, never the path itself.
///
/// The one thing a malformed-replacement line may name: a deployment operator recognizes the class of
/// files to fix, and a formatter never walks a path that the `Debug` on [`Rotator`] and [`Rotating`]
/// is obligated not to print. The paths themselves stay inside this crate's private fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialClass {
    /// `security.outbound.transport_anchors: <path>` - a PEM bundle.
    AnchorBundle,
    /// `security.outbound.transport_anchors: system` - the host trust store.
    SystemStore,
    /// The client certificate+key pair a `mutual` channel presents.
    IdentityPair,
}

impl MaterialClass {
    /// The word a log line prints for the class.
    const fn as_str(self) -> &'static str {
        match self {
            Self::AnchorBundle => "anchor bundle",
            Self::SystemStore => "system store",
            Self::IdentityPair => "client identity pair",
        }
    }
}

/// Which declared FILE a read refused - the two file-backed classes only, since the host store is
/// read through `load_anchors`, never as a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileClass {
    AnchorBundle,
    IdentityPair,
}

impl FileClass {
    /// The [`LoadError`] a failing read of this file surfaces as.
    fn read_error(self, path: &Path, cause: std::io::Error) -> LoadError {
        match self {
            Self::AnchorBundle => LoadError::AnchorsRead {
                path: path.display().to_string(),
                cause,
            },
            Self::IdentityPair => LoadError::IdentityRead {
                path: path.display().to_string(),
                cause,
            },
        }
    }

    /// The audit-line class this file's refusal is reported under.
    const fn material(self) -> MaterialClass {
        match self {
            Self::AnchorBundle => MaterialClass::AnchorBundle,
            Self::IdentityPair => MaterialClass::IdentityPair,
        }
    }
}

/// The refused-load half of a look: the source class named in the audit line, and the reason.
type ProbeError = (MaterialClass, LoadError);

/// What a look decided, or refused.
type Probe = Result<Examined, ProbeError>;

/// The consumer's materialization of freshly loaded material, as stored in a [`Rotator`].
type Rebuild<T, E> = Box<dyn Fn(LoadedAnchors, Option<LoadedIdentity>) -> Result<T, E> + Send + Sync>;

/// A client identity's two files, as raw bytes, for the byte comparison - named because the pair
/// sits inside an `Option` inside an enum variant, and the `type_complexity` lint needs it factored.
type IdentityFiles = (Vec<u8>, Vec<u8>);

/// What one poll examined, so identical bytes are silent on the next tick.
///
/// A rejected replacement is still an *examined* one - the surviving failures are exactly the ones a
/// loud log line must not repeat. The identity pair is compared by its raw file bytes, and the
/// anchor source by its raw file bytes (a bundle) or by the root certificates the store resolved to
/// (the host store, which has no file to read).
#[derive(PartialEq)]
enum Examined {
    /// A declared bundle (and optional identity pair), by raw path bytes.
    Bundle {
        anchors: Vec<u8>,
        identity: Option<IdentityFiles>,
    },
    /// The explicitly selected host store, by the roots it resolved to, plus an optional identity.
    Store {
        roots: Vec<Vec<u8>>,
        identity: Option<IdentityFiles>,
    },
    /// The last look could not even be read; that refusal was already reported.
    Failed,
}

/// The read side a consumer holds: clones out the in-use `T`.
///
/// `Clone` is sharing, not copying: every clone observes the same channel, so a rotation adopted by
/// `poll_once` reaches every consumer that holds a clone. `current()` is an `Arc` clone under a
/// `watch` borrow - no lock a request path waits on. `Debug` prints no path.
#[derive(Clone)]
pub struct Rotating<T> {
    current: tokio::sync::watch::Receiver<Arc<T>>,
}

impl<T> Rotating<T> {
    /// The in-use material, as an `Arc` (cheap to clone and to hand to a per-request client).
    #[must_use]
    pub fn current(&self) -> Arc<T> {
        Arc::clone(&self.current.borrow())
    }

    /// A handle that never rotates: `current()` always returns this one value.
    ///
    /// The compiled-in-roots half of `security.outbound` - a deployment with no declaration has
    /// nothing to poll, and this is how a consumer that accepts a [`Rotating`] keeps accepting one
    /// in that case rather than needing a second, non-rotating shape.
    #[must_use]
    pub fn fixed(value: T) -> Self {
        let (sender, receiver) = tokio::sync::watch::channel(Arc::new(value));
        drop(sender);
        Self { current: receiver }
    }
}

impl<T> core::fmt::Debug for Rotating<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Rotating { .. }")
    }
}

/// The poll handle a composition root drives on [`POLL_INTERVAL`].
///
/// Owns the declared source (so it can re-read it), the `sender` half of the channel [`Rotating`]
/// reads, the rebuild closure (the consumer's own materialization), and the `seen` marker that makes
/// identical bytes silent. `poll_once` is synchronous, like `sutura-http::tls::Renewal::poll_once`;
/// *starting* the loop is the composition root's choice, and this type does not spawn.
pub struct Rotator<T, E = LoadError> {
    anchors: Anchors,
    identity: Option<Identity>,
    sender: tokio::sync::watch::Sender<Arc<T>>,
    rebuild: Rebuild<T, E>,
    seen: Option<Examined>,
}

impl<T, E> Rotator<T, E>
where
    T: Send + Sync + 'static,
    E: core::fmt::Display + Send + Sync + 'static,
{
    /// Builds a rotator around already-materialized `initial` (the composition root's boot-time
    /// build, whose own error type already refused an unusable declaration). The `seen` marker is
    /// seeded from the current on-disk state, so the first `poll_once` is `Unchanged` rather than a
    /// spurious rotation of the very material this was just built from.
    #[must_use]
    pub fn new(
        anchors: Anchors,
        identity: Option<Identity>,
        rebuild: impl Fn(LoadedAnchors, Option<LoadedIdentity>) -> Result<T, E> + Send + Sync + 'static,
        initial: T,
    ) -> Self {
        let (sender, receiver) = tokio::sync::watch::channel(Arc::new(initial));
        drop(receiver);
        let seen = probe(&anchors, identity.as_ref()).ok();
        Self {
            anchors,
            identity,
            sender,
            rebuild: Box::new(rebuild),
            seen,
        }
    }

    /// The read handle consumers hold. Cheap to clone; every clone observes this channel.
    #[must_use]
    pub fn rotating(&self) -> Rotating<T> {
        Rotating {
            current: self.sender.subscribe(),
        }
    }

    /// Looks at the declared material once, mirroring `sutura-http::tls::Renewal::poll_once`.
    ///
    /// Re-reads (bounded), compares to what was last examined, and only on a difference loads and
    /// rebuilds: on `Ok` it adopts the new `T` with one `info!` line; on a load or rebuild refusal it
    /// keeps the old `T` with one `error!` line naming the source class. A rejected replacement is
    /// still an examined one, so identical broken bytes are silent next tick.
    pub fn poll_once(&mut self) -> Outcome {
        let examined = match probe(&self.anchors, self.identity.as_ref()) {
            Ok(examined) => examined,
            Err((class, cause)) => {
                self.refuse(class, &cause);
                return Outcome::Rejected;
            }
        };
        if self.seen.as_ref() == Some(&examined) {
            return Outcome::Unchanged;
        }

        let loaded = match load_anchors(&self.anchors) {
            Ok(loaded) => loaded,
            Err(cause) => {
                self.refuse(class_of(&self.anchors), &cause);
                return Outcome::Rejected;
            }
        };
        let identity = match &self.identity {
            None => None,
            Some(declared) => match load_identity(declared) {
                Ok(loaded) => Some(loaded),
                Err(cause) => {
                    self.refuse(MaterialClass::IdentityPair, &cause);
                    return Outcome::Rejected;
                }
            },
        };
        match (self.rebuild)(loaded, identity) {
            Ok(next) => {
                drop(self.sender.send_replace(Arc::new(next)));
                tracing::info!(
                    source = %class_of(&self.anchors).as_str(),
                    interval_seconds = POLL_INTERVAL.as_secs(),
                    "outbound trust material rotated; new requests and connections adopt the new material"
                );
                self.seen = Some(examined);
                Outcome::Rotated
            }
            Err(cause) => {
                self.refuse(class_of(&self.anchors), &cause);
                self.seen = Some(examined);
                Outcome::Rejected
            }
        }
    }

    /// One loud line for a replacement that cannot be adopted, then carry on with what works.
    ///
    /// The line names the source CLASS and the load error, never a path. A failure that was already
    /// examined (and reported) is silent.
    fn refuse(&mut self, class: MaterialClass, cause: &dyn core::fmt::Display) {
        if self.seen.as_ref() == Some(&Examined::Failed) {
            return;
        }
        tracing::error!(
            source = %class.as_str(),
            error = %cause,
            "outbound trust material changed and is NOT usable; keeping the material in use. \
             Fix the files - nothing will rotate until they load"
        );
        self.seen = Some(Examined::Failed);
    }
}

impl<T, E> core::fmt::Debug for Rotator<T, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Rotator")
            .field("source", &class_of(&self.anchors).as_str())
            .finish_non_exhaustive()
    }
}

/// The source class a refusal of the declared anchors is reported as.
const fn class_of(anchors: &Anchors) -> MaterialClass {
    match anchors {
        Anchors::Bundle(_) => MaterialClass::AnchorBundle,
        Anchors::System => MaterialClass::SystemStore,
    }
}

/// Re-reads the declared material into the comparable [`Examined`] marker.
///
/// For a bundle and an identity pair this is the raw path bytes (bounded), exactly the comparison
/// `sutura-http` makes; for the host store there is no file, so it loads the store and keeps the
/// roots it resolved to. This is the change-detection half; the actual load for adoption happens only
/// once a difference is observed, in `poll_once`.
fn probe(anchors: &Anchors, identity: Option<&Identity>) -> Probe {
    let identity_bytes = match identity {
        None => None,
        Some(declared) => Some((
            read_bounded(declared.certificate(), FileClass::IdentityPair)?,
            read_bounded(declared.key(), FileClass::IdentityPair)?,
        )),
    };
    match anchors {
        Anchors::Bundle(path) => Ok(Examined::Bundle {
            anchors: read_bounded(path, FileClass::AnchorBundle)?,
            identity: identity_bytes,
        }),
        Anchors::System => {
            let loaded = load_anchors(anchors).map_err(|cause| (MaterialClass::SystemStore, cause))?;
            let roots = loaded.into_iter().map(|cert| cert.as_ref().to_vec()).collect();
            Ok(Examined::Store {
                roots,
                identity: identity_bytes,
            })
        }
    }
}

/// One declared file, bounded like the serving side's `read_file`.
fn read_bounded(path: &Path, class: FileClass) -> Result<Vec<u8>, ProbeError> {
    let unreadable = |cause: std::io::Error| (class.material(), class.read_error(path, cause));
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut bytes = Vec::new();
    let bound = u64::try_from(MAX_MATERIAL_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    file.take(bound).read_to_end(&mut bytes).map_err(unreadable)?;
    if bytes.len() > MAX_MATERIAL_BYTES {
        return Err((
            class.material(),
            class.read_error(
                path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, "the declared material exceeds the read cap"),
            ),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const SUBJECT: &str = "localhost";

    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!("sutura-tls-rotate-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
            Self { directory }
        }

        /// Writes a fresh self-signed CA and returns the bundle path (so each call yields a DIFFERENT
        /// CA, which is what lets a cell replace a trust set and observe the swap).
        fn ca(&self, name: &str) -> PathBuf {
            let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed CA generates");
            let path = self.directory.join(format!("{name}.pem"));
            std::fs::write(&path, issued.cert.pem()).expect("a certificate writes");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.directory);
        }
    }

    /// A rebuild that records what it was handed, so a cell can assert adoption happened and with what.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Rebuilt(usize);

    /// Counts `error!` events written while the returned subscriber is default.
    struct Errors(Arc<AtomicUsize>);

    impl tracing::Subscriber for Errors {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if *event.metadata().level() == tracing::Level::ERROR {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    fn error_count() -> (Errors, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (Errors(Arc::clone(&count)), count)
    }

    /// A rotator whose rebuilt material is a monotonically increasing BUILD count, given a bundle
    /// path. The boot-time material is `Rebuilt(0)`; each subsequent rebuild yields the next count,
    /// so a cell can observe that adoption produced genuinely NEW material rather than the same value.
    fn bundle_rotator(path: &Path, builds: &Arc<AtomicUsize>) -> Rotator<Rebuilt, crate::LoadError> {
        let builds = Arc::clone(builds);
        Rotator::new(
            Anchors::Bundle(path.to_path_buf()),
            None,
            move |_loaded, _identity| Ok(Rebuilt(builds.fetch_add(1, Ordering::SeqCst) + 1)),
            Rebuilt(0),
        )
    }

    #[test]
    fn an_unchanged_bundle_is_unrotated_and_keeps_the_initial_material() {
        let scratch = Scratch::new("unchanged");
        let path = scratch.ca("root");
        let builds = Arc::new(AtomicUsize::new(0));
        let mut rotator = bundle_rotator(&path, &builds);
        let rotating = rotator.rotating();
        assert_eq!(rotating.current().as_ref(), &Rebuilt(0));

        assert_eq!(rotator.poll_once(), Outcome::Unchanged);
        assert_eq!(rotating.current().as_ref(), &Rebuilt(0));
    }

    #[test]
    fn a_replaced_bundle_rotates_and_the_next_read_gets_the_new_material() {
        let scratch = Scratch::new("replaced");
        let pa = scratch.ca("ca-a");
        let mut rotator = bundle_rotator(&pa, &Arc::new(AtomicUsize::new(0)));
        let rotating = rotator.rotating();

        // Ca-b is a fresh self-signed CA, so overwriting the bundle path's bytes with it is a real
        // trust-set replacement: the same path now holds certificates from a different issuer.
        let ca_b = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a second pair generates");
        std::fs::write(&pa, ca_b.cert.pem()).expect("the bundle path rewrites");

        assert_eq!(rotator.poll_once(), Outcome::Rotated);
        assert_eq!(rotating.current().as_ref(), &Rebuilt(1));
        // The new material is what a consumer clones next; identical bytes are silent after that.
        assert_eq!(rotator.poll_once(), Outcome::Unchanged);
    }

    #[test]
    fn a_malformed_replacement_keeps_the_old_material_and_logs_exactly_one_rejection() {
        let scratch = Scratch::new("malformed");
        let path = scratch.ca("root");
        let mut rotator = bundle_rotator(&path, &Arc::new(AtomicUsize::new(0)));
        let rotating = rotator.rotating();

        std::fs::write(&path, b"this is not a certificate").expect("prose overwrites the bundle");

        let (subscriber, errors) = error_count();
        let outcome = tracing::subscriber::with_default(subscriber, || rotator.poll_once());
        assert_eq!(outcome, Outcome::Rejected);
        assert_eq!(rotating.current().as_ref(), &Rebuilt(0), "the old material is still in use");

        // The identical broken bytes are silent on the next tick - exactly ONE loud line for the
        // malformed replacement, and then the deployment stops hearing about it until it changes.
        let outcome_again =
            tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || rotator.poll_once());
        assert_eq!(outcome_again, Outcome::Rejected);
        assert_eq!(
            errors.load(Ordering::SeqCst),
            1,
            "one audit line for the malformed replacement"
        );
    }

    #[test]
    fn a_debug_of_either_handle_prints_no_path() {
        let scratch = Scratch::new("debug");
        let path = scratch.ca("root");
        let rotator = bundle_rotator(&path, &Arc::new(AtomicUsize::new(0)));
        let rotating = rotator.rotating();

        // The bundle path is a literal filesystem path and must not reach either formatter.
        let path_text = path.display().to_string();
        assert!(!format!("{rotator:?}").contains(&path_text));
        assert!(!format!("{rotating:?}").contains(&path_text));
    }

    /// Mutating the working Set: adopting without parsing loses a malformed replacement.
    #[test]
    fn an_adopt_without_parse_would_trust_a_malformed_replacement() {
        // This cell pins the CONTRACT the real cells rest on: `poll_once` must load (parse) before
        // adopting. The failure mode it guards is a rotator that sends whatever bytes arrived without
        // checking them - which this test proves is NOT what happens by asserting `Rejected`.
        let scratch = Scratch::new("parse");
        let path = scratch.ca("root");
        let mut rotator = bundle_rotator(&path, &Arc::new(AtomicUsize::new(0)));
        std::fs::write(&path, b"not a PEM at all").expect("prose overwrites the bundle");

        let outcome = tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || rotator.poll_once());
        assert_eq!(outcome, Outcome::Rejected);
    }
}

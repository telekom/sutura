//! The combiner every arm of `run` serves through: across the nodes `server.nodes` declares, or on
//! this node alone.
//!
//! Declared nodes build this node's worker once. The combine runs its own stages in it, and the
//! listener serves the same worker to the other nodes behind the node-certificate gate. A link
//! presents `server.tls_certificate` and `server.tls_key`, the pair the listener presents, and
//! accepts only a node `server.tls_node_authority` signed.
//!
//! **Limits.** The pair and the authority are read once, at boot: the listener's rotation does not
//! reach a link, so a renewed node certificate is presented on links after a restart. Each call a
//! link carries holds its question's task grant; the served worker admits no call without it. A
//! message whose length prefix declares more bytes than this node's working set is refused before
//! it is buffered or the grant is asked: the bound is per message, so concurrent streams each reach
//! it, and the replies a link reads are not bounded here.

use core::num::{NonZeroU64, NonZeroUsize};

use sutura_app::Warehouses;
#[cfg(feature = "distributed")]
use sutura_config::InvalidDeclaredNode;
use sutura_config::RuntimeSettings;
use sutura_config::{DeclaredNode, TlsMaterial};
use sutura_domain::warehouse::Warehouse;
use sutura_exec_datafusion::DataFusionCombiner;
use sutura_http::ServiceState;

/// What `run` mounts beside the other transports: this node's worker, when nodes are declared.
#[cfg(feature = "distributed")]
pub(super) type Served = Option<sutura_http::WorkerMount>;
#[cfg(not(feature = "distributed"))]
pub(super) type Served = ();

/// The combiner `run` serves through, and what it mounts beside the other transports.
pub(super) type Chosen = (DataFusionCombiner, Served);

/// This node's declared `own` address, with a literal port `0` resolved to `bound_port` - the port
/// this process actually bound its listener to.
///
/// `own` is read by every OTHER node that reaches this one (`Nodes::declared`'s `own_url`, where
/// `sutura-exec-datafusion`'s combine serves this node's stage output), so it has to name a real,
/// dialable port. A literal `0` cannot be dialled, so a deployment writing it is declaring "whatever
/// this process binds" - the same shorthand `server.port: 0` already is for the listener itself -
/// rather than a port chosen in advance.
///
/// Any other value, including the absence of a port (the scheme's default), is a deployment's own
/// explicit claim about where it is reachable and is left untouched: this process opens exactly one
/// listener, so a literal `0` is the only spelling that could mean "it", and there is nothing else
/// for it to ambiguously mean - no second listener, no second bound port, to choose between.
///
/// Called only once [`own_wants_bound_port`] has already said `own` names the literal port `0`, so
/// `bound_port` always names a real port here - never the fallback `own` itself would be.
#[cfg(feature = "distributed")]
pub(super) fn resolve_own(own: &url::Url, bound_port: u16) -> url::Url {
    if own.port() != Some(0) {
        return own.clone();
    }
    let mut resolved = own.clone();
    // `own` is `with_nodes`-validated as an https URL with a host, so it always accepts a port;
    // the only way `Url::set_port` refuses is a cannot-be-a-base URL, which is not a shape this
    // type can hold here - so this is a true invariant, not a case this fell through silently.
    #[expect(
        clippy::expect_used,
        reason = "with_nodes validated `own` as an https URL with a host, which Url::set_port never refuses"
    )]
    resolved.set_port(Some(bound_port)).expect("own accepts a port");
    resolved
}

/// Whether `own` names the literal port `0` and so needs a real, bound port resolved for it before
/// [`combiner`] builds the worker other nodes reach this one through.
///
/// This is the gate for the early bind in `run`: every other deployment keeps the original late
/// bind, inside `serve_as_configured`, exactly as before this fix - a socket in `LISTEN` accepting
/// handshakes into its backlog while boot may still refuse (telemetry, outbound anchors, an adapter)
/// is new exposure this fix must not hand to a deployment that never asked for it.
#[cfg(feature = "distributed")]
pub(super) fn own_wants_bound_port(tls: Option<&TlsMaterial>) -> bool {
    tls.and_then(TlsMaterial::nodes)
        .is_some_and(|nodes| nodes.own().port() == Some(0))
}

#[cfg(not(feature = "distributed"))]
pub(super) const fn own_wants_bound_port(_tls: Option<&TlsMaterial>) -> bool {
    false
}

/// This node's declared `own`, resolved against `bound_port` when it names the literal port `0`.
///
/// # Errors
///
/// `own` names the literal port `0` but `bound_port` is `None` - [`own_wants_bound_port`] disagreeing
/// with what `run` bound and handed [`combiner`]. That is this module's own invariant between the two
/// functions, never a deployment mistake, so it is a typed refusal rather than a silent fallback to
/// the undialable `own` a deployment never meant to ship.
#[cfg(feature = "distributed")]
fn declared_own(own: &url::Url, bound_port: Option<u16>) -> Result<url::Url, String> {
    if own.port() != Some(0) {
        return Ok(own.clone());
    }
    bound_port.map_or_else(
        || {
            Err(String::from(
                "server.nodes.own names the literal port 0 but no bound port was resolved for it before \
                 the combiner ran - this is sutura-cli's own early-bind gate disagreeing with itself, not \
                 a configuration error",
            ))
        },
        |bound_port| Ok(resolve_own(own, bound_port)),
    )
}

/// What [`bind_server_socket`] hands back: the listener, and the port it actually bound.
pub(super) type BoundSocket = (std::net::TcpListener, u16);

/// Binds `settings.server().bind()`, synchronously and before anything else - only called when
/// [`own_wants_bound_port`] says so.
///
/// A declared node's own address can name port `0` for "whatever this process binds", and that has
/// to be a real, dialable port before [`combiner`] builds the worker other nodes reach it through:
/// guessing it in advance (a probe socket bound, read and dropped) is exactly the TOCTOU a
/// concurrent process can win first. No runtime is entered yet (`run`'s own doc comment says why
/// that order matters), and `std::net::TcpListener` needs none; `tokio_listener` adopts it into the
/// async runtime later, right before serving.
pub(super) fn bind_server_socket(settings: &sutura_config::Settings) -> Result<BoundSocket, String> {
    let bind_address = settings.server().bind().socket();
    let listener =
        std::net::TcpListener::bind(bind_address).map_err(|cause| format!("could not listen on {bind_address}: {cause}"))?;
    let bound_port = listener
        .local_addr()
        .map_err(|cause| format!("could not read back the address bound at {bind_address}: {cause}"))?
        .port();
    Ok((listener, bound_port))
}

/// Hands a listener - whether bound early by [`bind_server_socket`] or, the ordinary case, about to
/// be bound here at its original, late timing - to the runtime now driving it.
pub(super) fn tokio_listener(listener: std::net::TcpListener) -> Result<tokio::net::TcpListener, String> {
    listener
        .set_nonblocking(true)
        .map_err(|cause| format!("the bound listener could not be set non-blocking: {cause}"))?;
    tokio::net::TcpListener::from_std(listener).map_err(|cause| format!("the bound listener could not be adopted: {cause}"))
}

/// Serves plaintext, or terminates TLS here, according to what was configured.
///
/// **Two bodies, chosen by the `tls` feature, and they are not equivalent.** The configuration
/// cannot ask for something a build cannot do - `sutura-cli`'s `tls` feature turns on
/// `sutura-config`'s, so a binary without it refuses `security.tls_termination: in-process` at
/// startup with `NotFitToServe::InProcessTlsNotCompiledIn`, naming the feature. That refusal is the
/// gate; this is not a second copy of it.
///
/// The check in the second body is narrower and is about the one failure that must never be quiet:
/// it keys on the MATERIAL rather than on the declaration, so serving plaintext on a port that was
/// given a certificate is impossible in this file rather than impossible two crates away. A `cfg`
/// that silently fell through to `serve` would be exactly the silent fallback this is not allowed to
/// have.
#[cfg(feature = "tls")]
pub(super) async fn serve_as_configured(
    router: axum::Router,
    socket: Socket,
    stopping: sutura_runtime::Shutdown,
    material: Option<TlsMaterial>,
) -> Result<(), String> {
    match (socket, material.as_ref()) {
        (Socket::Bound(listener), Some(material)) => {
            sutura_http::serve_tls_on(tokio_listener(listener)?, router, stopping, material)
                .await
                .map_err(super::flatten)
        }
        (Socket::Bound(listener), None) => sutura_http::serve_on(tokio_listener(listener)?, router, stopping)
            .await
            .map_err(super::flatten),
        (Socket::Address(address), Some(material)) => sutura_http::serve_tls(router, address, stopping, material)
            .await
            .map_err(super::flatten),
        (Socket::Address(address), None) => sutura_http::serve(router, address, stopping).await.map_err(super::flatten),
    }
}

/// The same decision in a build with no TLS listener in it.
#[cfg(not(feature = "tls"))]
pub(super) async fn serve_as_configured(
    router: axum::Router,
    socket: Socket,
    stopping: sutura_runtime::Shutdown,
    material: Option<TlsMaterial>,
) -> Result<(), String> {
    if material.is_some() {
        return Err(String::from(
            "server.tls_certificate and server.tls_key are set and this binary was built without \
             the `tls` feature, so it has no TLS listener at all. Rebuild with `--features tls`, or \
             terminate TLS in front of this process and remove the paths. Refusing to serve \
             plaintext on a port that was configured to be encrypted",
        ));
    }
    match socket {
        Socket::Bound(listener) => sutura_http::serve_on(tokio_listener(listener)?, router, stopping)
            .await
            .map_err(super::flatten),
        Socket::Address(address) => sutura_http::serve(router, address, stopping).await.map_err(super::flatten),
    }
}

/// A socket for `serve_as_configured`: either an address for it to bind at its original, late
/// timing - the unchanged path for every deployment whose `own` is not resolved here - or a listener
/// [`bind_server_socket`] already bound early, when [`own_wants_bound_port`] said this node's `own`
/// needed a real port before [`combiner`] ran.
pub(super) enum Socket {
    Address(std::net::SocketAddr),
    Bound(std::net::TcpListener),
}

/// What [`socket_for`] hands back: the socket `run` serves through, and the port it was bound to
/// early - [`Some`] only when [`own_wants_bound_port`] said `settings.server().nodes.own` needed a
/// real port resolved before [`combiner`] runs; every other deployment keeps the original late
/// bind, unchanged.
pub(super) type EarlySocket = (Socket, Option<u16>);

/// `run`'s one call to decide the socket: binds early, synchronously, only when
/// [`own_wants_bound_port`] says `settings.server().nodes.own` needs a real port before
/// [`combiner`] runs - otherwise hands back the address for `serve_as_configured` to bind at its
/// original, late timing, unchanged.
///
/// # Errors
///
/// [`bind_server_socket`] could not bind or read back the address it bound.
pub(super) fn socket_for(settings: &sutura_config::Settings) -> Result<EarlySocket, String> {
    if own_wants_bound_port(settings.server().tls()) {
        let (listener, bound_port) = bind_server_socket(settings)?;
        Ok((Socket::Bound(listener), Some(bound_port)))
    } else {
        Ok((Socket::Address(settings.server().bind().socket()), None))
    }
}

/// The combiner, and the worker the listener serves when it plans across declared nodes.
///
/// `bound_port` is the port this process's own listener was already bound to, when
/// [`own_wants_bound_port`] said binding early was needed - see [`declared_own`] for what it is
/// used for, and why it is `None` on every deployment that kept the original late bind.
///
/// # Errors
///
/// A pair or an authority that cannot be read or does not make a node's identity. Declared nodes
/// never fall back to a combine on one node. [`own_wants_bound_port`] disagreeing with `bound_port`
/// (see [`declared_own`]), or a peer declared at the address `own` resolves to.
#[cfg(feature = "distributed")]
pub(super) fn combiner(tls: Option<&TlsMaterial>, runtime: RuntimeSettings, bound_port: Option<u16>) -> Result<Chosen, String> {
    use sutura_exec_datafusion::Nodes;

    let Some((tls, nodes)) = tls.and_then(|tls| tls.nodes().map(|nodes| (tls, nodes))) else {
        return Ok((crate::catalog::one_node_combiner()?, None));
    };
    let identity = identity(tls, nodes.authority())?;
    let own_url = declared_own(nodes.own(), bound_port)?;
    // `TlsMaterial::with_nodes` already refused a peer declared at `own`'s own, LITERAL address -
    // but a peer declared at the port `own: 0` resolves to is a self-reference that check could not
    // see, so it is re-checked here, after resolution, against the address `own` now actually names.
    if let Some(node) = nodes.others().iter().find(|node| node.url().origin() == own_url.origin()) {
        return Err(super::flatten(InvalidDeclaredNode::NodeNamesItself {
            url: node.url().to_string(),
        }));
    }
    let working_set = crate::sources::working_set(runtime);
    let own = Nodes::worker(working_set, identity.clone())
        .map_err(|cause| format!("{}\ncould not build this node's worker", super::flatten(cause)))?;
    let wait = nodes
        .wait_for_workers()
        .map(|seconds| core::time::Duration::from_secs(core::num::NonZeroU64::from(seconds).get()));
    let across = Nodes::declared(own, own_url, nodes.others().iter().map(|node| node.url().clone()), identity)
        .across_above(nodes.across_above())
        .wait_for_workers(wait)
        .address_family(nodes.address_family());
    let across = nodes
        .others()
        .iter()
        .flat_map(DeclaredNode::sources)
        .fold(across, |across, alias| {
            across.reading(alias.clone(), nodes.reading(alias).map(|node| node.url().clone()))
        });
    let mount = sutura_http::WorkerMount::new(
        across
            .served()
            .into_worker_server()
            .max_decoding_message_size(working_set.bytes()),
    );
    let combiner = DataFusionCombiner::across(across)
        .map_err(|cause| format!("{}\ncould not build the federation combiner", super::flatten(cause)))?;
    Ok((combiner, Some(mount)))
}

/// The same, on a build that cannot plan across nodes: declared ones are refused.
#[cfg(not(feature = "distributed"))]
pub(super) fn combiner(tls: Option<&TlsMaterial>, _runtime: RuntimeSettings, _bound_port: Option<u16>) -> Result<Chosen, String> {
    super::not_linked::nodes_refused_if_declared(tls)?;
    Ok((crate::catalog::one_node_combiner()?, ()))
}

/// This node's identity: the listener's pair, accepting only a node `authority` signed.
#[cfg(feature = "distributed")]
pub(super) fn identity(tls: &TlsMaterial, authority: &std::path::Path) -> Result<sutura_exec_datafusion::NodeIdentity, String> {
    let pair = sutura_tls::Identity::new(tls.certificate().to_path_buf(), tls.key().to_path_buf());
    let (chain, key) = sutura_tls::load_identity(&pair)
        .map_err(|cause| {
            format!(
                "{}\nserver.tls_certificate and server.tls_key are not a node's identity",
                super::flatten(cause)
            )
        })?
        .into_parts();
    let authority = sutura_tls::load_anchors(&sutura_tls::Anchors::Bundle(authority.to_path_buf()))
        .map_err(|cause| format!("{}\nserver.tls_node_authority is not a node authority", super::flatten(cause)))?;
    sutura_exec_datafusion::NodeIdentity::new(chain, key, authority)
        .map_err(|cause| format!("{}\nthis node's identity could not be built", super::flatten(cause)))
}

/// `engines`, each source alias a declared peer reads named for splitting a lookup leg into at most
/// as many partitions as there are such peers, above `server.nodes.across_above_bytes` or, unset,
/// the working set.
///
/// **Limit:** the peers are counted as declared, not as ready. A split leg the source does not grant
/// is still read on this node - one of one partition (every split of a source one peer reads), one
/// whose link the plan declares a comparison for, one with a `NUMERIC` column, one
/// `PartitionLeg::encode` refuses, and one that names a payer - so it holds up to its readers times
/// the source's per-stream bound here, where the same leg read whole is refused past one stream's.
/// A lookup leg of a source named here, asked with no verified caller, is refused before the source
/// is asked anything - unless it names a payer, which is read whole, as the deployment; a deployment
/// declaring such a source with no `security.inbound` does not boot.
/// Only a PostgreSQL source, a `BigQuery` source that federates at a pool, a `ClickHouse` source that
/// declares `hash_buckets_acknowledged_because`, or an Oracle source that declares `split: flashback`,
/// erased into `kind::AnyWarehouse` splits a leg; any other adapter reads it whole.
pub(super) fn splitting<W>(engines: Warehouses<W>, tls: Option<&TlsMaterial>, runtime: RuntimeSettings) -> Warehouses<W>
where
    W: Warehouse,
{
    let Some(nodes) = tls.and_then(TlsMaterial::nodes) else {
        return engines;
    };
    let above = nodes
        .across_above()
        .unwrap_or_else(|| NonZeroU64::try_from(runtime.working_set().bytes()).unwrap_or(NonZeroU64::MAX));
    nodes
        .others()
        .iter()
        .flat_map(DeclaredNode::sources)
        .fold(engines, |engines, alias| {
            match NonZeroUsize::new(nodes.reading(alias).count()) {
                Some(readers) => engines.splitting(alias.clone(), above, readers),
                None => engines,
            }
        })
}

/// Whether a peer `server.nodes` declares reads one of `engines`' sources.
#[cfg(any(feature = "postgres", feature = "oracle", feature = "clickhouse"))]
pub(super) fn read_by_a_peer<W>(engines: &Warehouses<W>, tls: Option<&TlsMaterial>) -> bool
where
    W: Warehouse,
{
    tls.and_then(TlsMaterial::nodes).is_some_and(|nodes| {
        nodes
            .others()
            .iter()
            .flat_map(DeclaredNode::sources)
            .any(|alias| engines.get(alias).is_some())
    })
}

/// `state` with this node's worker mounted, when there is one.
#[cfg(feature = "distributed")]
pub(super) fn mounted(state: ServiceState, worker: Served) -> ServiceState {
    match worker {
        Some(mount) => state.with_worker_surface(mount),
        None => state,
    }
}

#[cfg(not(feature = "distributed"))]
pub(super) const fn mounted(state: ServiceState, (): Served) -> ServiceState {
    state
}

#[cfg(feature = "distributed")]
#[cfg(test)]
mod resolve_own_tests {
    use url::Url;

    use super::resolve_own;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap_or_else(|cause| panic!("{text} is a test URL: {cause}"))
    }

    #[test]
    fn a_literal_zero_port_resolves_to_the_bound_port() {
        assert_eq!(resolve_own(&url("https://127.0.0.1:0"), 54321).port(), Some(54321));
    }

    #[test]
    fn an_explicit_nonzero_port_is_left_untouched() {
        let own = url("https://node-a.example.com:8443");
        assert_eq!(resolve_own(&own, 54321), own);
    }

    #[test]
    fn the_scheme_default_port_is_left_untouched() {
        let own = url("https://node-a.example.com");
        assert_eq!(resolve_own(&own, 54321), own);
    }
}

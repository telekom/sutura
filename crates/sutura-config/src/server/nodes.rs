//! The nodes a combine is planned across, and - `#828` - the role and source aliases each
//! declared peer reads.
//!
//! **What [`DeclaredNodes::reading`] does not do.** It answers "which declared peers named this
//! source alias", read from this node's own configuration. Its count sizes a lookup leg's split at
//! that source, and the planner places a split leg's partitions only on these peers, but no served
//! question builds one yet - every partition is still read on this node - and readiness is
//! unaffected by it: a node that failed to resolve or is unreachable is still
//! *declared*, so this accessor is a configuration fact, not a liveness one. Readiness stays exactly
//! where it already was, with resolving the node.

use std::net::IpAddr;
use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;
use sutura_domain::plan::AddressFamily;
use url::{Host, Url};

use super::{ServerRole, TlsMaterial};

/// One peer's url, role spelling and source aliases, as [`TlsMaterial::with_nodes`] takes them.
///
/// Borrowed primitives because `crate::raw`'s `RawDeclaredNode` is private and cannot name a
/// public signature, and a borrow costs the caller no clone.
///
/// Named fields rather than a positional tuple: `(&str, &str, &[String])` let `url` and `role`
/// swap silently, since both are bare strings and nothing at the call site would catch it.
///
/// Fields are private even though nothing here is validated yet - a struct literal would still
/// let a caller build one with the fields transposed, the exact mix-up the named fields exist to
/// catch. [`DeclaredNodeInput::new`] is the only way in.
#[derive(Debug, Clone, Copy)]
pub struct DeclaredNodeInput<'a> {
    url: &'a str,
    role: &'a str,
    sources: &'a [String],
}

impl<'a> DeclaredNodeInput<'a> {
    /// A peer's address, its role spelling and the source aliases it reads, as read off the
    /// configuration.
    pub const fn new(url: &'a str, role: &'a str, sources: &'a [String]) -> Self {
        Self { url, role, sources }
    }
}

/// One peer this node's configuration declares: its address, what it is for, and - for a
/// `reading-worker` - the source aliases it reads.
///
/// `sources` is empty for a `worker` peer: `#828`'s decision 7 is that a combining worker holds no
/// source credential, so a `worker` entry naming one is refused at
/// [`TlsMaterial::with_nodes`] rather than carried here as a fact nothing then uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredNode {
    url: Url,
    role: ServerRole,
    sources: Vec<SourceName>,
}

impl DeclaredNode {
    /// The address this peer is reached at.
    #[inline]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// What this peer is for.
    #[inline]
    pub const fn role(&self) -> ServerRole {
        self.role
    }

    /// The source aliases this peer reads. Empty for every `worker` peer.
    #[inline]
    pub fn sources(&self) -> &[SourceName] {
        &self.sources
    }
}

/// The nodes a combine is planned across: this node's address as the others reach it, and theirs.
///
/// Every address is `https`. A link is mutual TLS whatever its URL says, so an `http` address would
/// read as a plaintext link that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredNodes {
    own: Url,
    others: Vec<DeclaredNode>,
    authority: PathBuf,
    across_above: Option<NonZeroU64>,
    wait_for_workers: Option<NonZeroU32>,
    address_family: AddressFamily,
}

impl DeclaredNodes {
    /// This node's address, as the others reach it.
    #[inline]
    pub const fn own(&self) -> &Url {
        &self.own
    }

    #[inline]
    pub fn others(&self) -> &[DeclaredNode] {
        &self.others
    }

    /// The declared peers that read `alias`, in declaration order.
    ///
    /// Counted to size a lookup leg's split at `alias`, and the only nodes a split leg's partitions
    /// are placed on when its source grants it. Readiness still comes from resolving the node, not
    /// from appearing here.
    pub fn reading<'a>(&'a self, alias: &SourceName) -> impl Iterator<Item = &'a DeclaredNode> {
        self.others.iter().filter(move |node| node.sources.contains(alias))
    }

    /// The authority every node's certificate is checked against, the same `tls_node_authority`.
    #[inline]
    pub fn authority(&self) -> &Path {
        &self.authority
    }

    /// The leg bytes above which a question reading several data systems is planned across before
    /// it is tried on this node, the question's own ceiling when undeclared.
    #[inline]
    pub const fn across_above(&self) -> Option<NonZeroU64> {
        self.across_above
    }

    /// Seconds a question that goes across waits for the other nodes it wants, none when undeclared.
    #[inline]
    pub const fn wait_for_workers(&self) -> Option<NonZeroU32> {
        self.wait_for_workers
    }

    /// The family of address a peer declared by a host is dialled at, [`AddressFamily::Auto`] when
    /// undeclared.
    #[inline]
    pub const fn address_family(&self) -> AddressFamily {
        self.address_family
    }
}

/// Why the paths, or a declared peer's address, are not usable TLS material.
///
/// What a peer declares about itself - its role, its source aliases, whether it repeats another
/// peer or names this node - is not a TLS problem and is refused as [`InvalidDeclaredNode`]
/// instead; this type stays the certificate pair, the node authority and the bare address shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTlsMaterial {
    /// One half was given and the other was not.
    ///
    /// Refused rather than half-configured: a certificate with no key cannot start a listener, and
    /// the silent alternative is a service that either falls back to plaintext or fails at the
    /// first handshake.
    #[error("{given} is set and {missing} is not - TLS needs both a certificate chain and its private key")]
    OnlyOneHalf { given: &'static str, missing: &'static str },
    /// A path was given as an empty string, which is not a path.
    #[error("{name} is empty - remove the key to disable TLS rather than setting it to nothing")]
    EmptyPath { name: &'static str },
    /// A node authority was given with no certificate and key: a listener that does not terminate
    /// TLS cannot ask a peer for a certificate.
    #[error("server.tls_node_authority is set and server.tls_certificate and server.tls_key are not")]
    AuthorityWithoutPair,
    /// Nodes were declared with no node authority: the listener would admit any client as a node,
    /// and a peer would be dialled with nothing to check its certificate against.
    #[error("server.nodes is set and server.tls_node_authority is not")]
    NodesWithoutAuthority,
    #[error("`{found}` in server.nodes is not an https URL with a host")]
    NotANodeAddress { found: String },
    /// A node address carried a path, query, fragment or userinfo beyond the bare origin - none of
    /// those reach anything at a node address, and carrying one would invite two spellings of the
    /// same node to be read as different peers. A fragment naming an IP address is the one extra kept:
    /// it is where the node is dialled, while its certificate is still checked against the host.
    #[error(
        "`{found}` in server.nodes is not a bare https origin - drop the path, query, userinfo or a fragment that is not an IP address"
    )]
    NodeAddressHasExtra { found: String },
}

/// Why a declared peer itself - not the TLS pair or the node authority - is not usable: its role,
/// its source aliases, or its address repeating another peer's or this node's own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDeclaredNode {
    /// A peer's `role` was not `worker` or `reading-worker` - the only two a peer may declare,
    /// unlike `server.role`'s own third spelling `serving`.
    #[error("`{found}` is not a peer role for `{url}` in server.nodes - use `worker` or `reading-worker`")]
    UnknownPeerRole { url: String, found: String },
    /// A peer's declared source alias is not a valid identifier at all - the shape check, not the
    /// cross-field one that checks it against this deployment's own `sources:` section.
    #[error("`{found}` is not a source alias for `{url}` in server.nodes")]
    InvalidSourceAlias { url: String, found: String },
    /// A `reading-worker` peer named no source: it would read nothing, which is not what the role
    /// is for.
    #[error("`{url}` in server.nodes is a reading-worker and names no source")]
    ReadingWorkerNamesNoSource { url: String },
    /// A `worker` peer named a source: `#828`'s decision 7 is that a combining worker holds no
    /// source credential, so this fact would describe an access the peer never uses.
    #[error("`{url}` in server.nodes is a worker and names a source - a worker reads nothing")]
    WorkerNamesSources { url: String },
    /// The same URL was declared twice among the peers.
    #[error("`{url}` is declared twice in server.nodes")]
    DuplicateNodeUrl { url: String },
    /// A peer entry named this node's own address.
    #[error("`{url}` in server.nodes is this node's own address, named under `own`")]
    NodeNamesItself { url: String },
    /// A peer's address is an IP literal of the other family than `server.nodes.address_family`,
    /// so it could never be dialled.
    #[error("`{url}` in server.nodes is an IP address of the other family than server.nodes.address_family `{family}`")]
    AddressOfOtherFamily { url: String, family: AddressFamily },
}

/// Why [`TlsMaterial::with_nodes`] refused.
///
/// Either half of what it checks - the TLS-material shape of a peer's address, or what the peer
/// declares about itself - can be wrong independently, so the call site routes each to its own
/// [`crate::settings::SettingsError`] variant rather than one catch-all carrying both.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeclaredNodeRefusal {
    #[error(transparent)]
    Tls(#[from] InvalidTlsMaterial),
    #[error(transparent)]
    Node(#[from] InvalidDeclaredNode),
}

impl TlsMaterial {
    /// Reads the pair, and the authority a peer node's certificate is checked against, or nothing.
    ///
    /// `None` for all three is "no in-process TLS", which is the default and is not an error.
    pub fn parse(
        certificate: Option<&str>,
        key: Option<&str>,
        node_authority: Option<&str>,
    ) -> Result<Option<Self>, InvalidTlsMaterial> {
        let node_authority = match super::trimmed(node_authority) {
            None => None,
            Some("") => {
                return Err(InvalidTlsMaterial::EmptyPath {
                    name: "server.tls_node_authority",
                });
            }
            Some(authority) => Some(PathBuf::from(authority)),
        };
        match (super::trimmed(certificate), super::trimmed(key)) {
            (None, None) if node_authority.is_some() => Err(InvalidTlsMaterial::AuthorityWithoutPair),
            (None, None) => Ok(None),
            (Some(certificate), None) => {
                if certificate.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath {
                        name: "server.tls_certificate",
                    });
                }
                Err(InvalidTlsMaterial::OnlyOneHalf {
                    given: "server.tls_certificate",
                    missing: "server.tls_key",
                })
            }
            (None, Some(key)) => {
                if key.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath { name: "server.tls_key" });
                }
                Err(InvalidTlsMaterial::OnlyOneHalf {
                    given: "server.tls_key",
                    missing: "server.tls_certificate",
                })
            }
            (Some(certificate), Some(key)) => {
                if certificate.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath {
                        name: "server.tls_certificate",
                    });
                }
                if key.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath { name: "server.tls_key" });
                }
                Ok(Some(Self {
                    certificate: PathBuf::from(certificate),
                    key: PathBuf::from(key),
                    node_authority,
                    nodes: None,
                }))
            }
        }
    }

    /// The certificate chain, in PEM.
    #[inline]
    pub fn certificate(&self) -> &Path {
        &self.certificate
    }

    /// The private key, in PEM.
    #[inline]
    pub fn key(&self) -> &Path {
        &self.key
    }

    /// The authority a peer node's client certificate is checked against, in PEM, if this listener
    /// asks for one.
    #[inline]
    pub fn node_authority(&self) -> Option<&Path> {
        self.node_authority.as_deref()
    }

    /// The same material, planning a combine across `own` and `others` - each peer's URL, role
    /// spelling (`worker` or `reading-worker`) and the source aliases it reads. Refused without a
    /// node authority.
    ///
    /// Takes primitive types rather than a richer peer type: `crate::raw` is a private module, so a
    /// public signature cannot name its `RawDeclaredNode` without a private-type-in-public-interface
    /// error, and this crate has no other already-public peer shape to offer instead.
    pub fn with_nodes(self, own: &str, others: &[DeclaredNodeInput]) -> Result<Self, DeclaredNodeRefusal> {
        let Some(authority) = self.node_authority.clone() else {
            return Err(InvalidTlsMaterial::NodesWithoutAuthority.into());
        };
        let own_url = node_address(own)?;
        let mut parsed: Vec<DeclaredNode> = Vec::with_capacity(others.len());
        for &DeclaredNodeInput { url, role, sources } in others {
            let url = node_address(url)?;
            if url.origin() == own_url.origin() {
                return Err(InvalidDeclaredNode::NodeNamesItself { url: url.to_string() }.into());
            }
            if parsed.iter().any(|node| node.url.origin() == url.origin()) {
                return Err(InvalidDeclaredNode::DuplicateNodeUrl { url: url.to_string() }.into());
            }
            let role = parse_peer_role(url.as_str(), role)?;
            let sources = sources
                .iter()
                .map(|alias| parse_peer_source(url.as_str(), alias))
                .collect::<Result<Vec<_>, _>>()?;
            // `parse_peer_role` refuses `ServerRole::Serving` above, so only these two reach here.
            if role == ServerRole::ReadingWorker && sources.is_empty() {
                return Err(InvalidDeclaredNode::ReadingWorkerNamesNoSource { url: url.to_string() }.into());
            }
            if role == ServerRole::Worker && !sources.is_empty() {
                return Err(InvalidDeclaredNode::WorkerNamesSources { url: url.to_string() }.into());
            }
            parsed.push(DeclaredNode { url, role, sources });
        }
        let nodes = DeclaredNodes {
            own: own_url,
            others: parsed,
            authority,
            across_above: None,
            wait_for_workers: None,
            address_family: AddressFamily::Auto,
        };
        Ok(Self {
            nodes: Some(nodes),
            ..self
        })
    }

    /// The same material, planning a question whose legs hold more than `bytes` across the declared
    /// nodes before it is tried on this one.
    #[must_use]
    pub const fn across_above(mut self, bytes: Option<NonZeroU64>) -> Self {
        if let Some(nodes) = self.nodes.as_mut() {
            nodes.across_above = bytes;
        }
        self
    }

    /// The same material, a question that goes across waiting up to `seconds` for the other nodes
    /// it wants.
    #[must_use]
    pub const fn wait_for_workers(mut self, seconds: Option<NonZeroU32>) -> Self {
        if let Some(nodes) = self.nodes.as_mut() {
            nodes.wait_for_workers = seconds;
        }
        self
    }

    /// The same material, a peer declared by a host dialled at `family` addresses.
    ///
    /// Without declared nodes there is no peer to dial, and this is a no-op.
    ///
    /// # Errors
    ///
    /// A peer whose address, or the `#<ip>` fragment it is dialled at, is an IP literal `family` does
    /// not dial. `own` is how the others reach this node and is not read.
    pub fn address_family(mut self, family: AddressFamily) -> Result<Self, InvalidDeclaredNode> {
        if let Some(nodes) = self.nodes.as_mut() {
            if let Some(node) = nodes.others.iter().find(|node| !dials(family, &node.url)) {
                return Err(InvalidDeclaredNode::AddressOfOtherFamily {
                    url: node.url.to_string(),
                    family,
                });
            }
            nodes.address_family = family;
        }
        Ok(self)
    }

    /// The nodes a combine is planned across, if any were declared.
    #[inline]
    pub const fn nodes(&self) -> Option<&DeclaredNodes> {
        self.nodes.as_ref()
    }
}

/// Whether `family` dials `url`: a host name is resolved to it, and an IP literal in the host or the
/// fragment must be of it.
fn dials(family: AddressFamily, url: &Url) -> bool {
    let host = match url.host() {
        Some(Host::Ipv4(address)) => family.admits(IpAddr::V4(address)),
        Some(Host::Ipv6(address)) => family.admits(IpAddr::V6(address)),
        Some(Host::Domain(_)) | None => true,
    };
    host && url
        .fragment()
        .and_then(|address| address.parse::<IpAddr>().ok())
        .is_none_or(|address| family.admits(address))
}

/// A peer's role: only `worker` and `reading-worker` are declarable for another node -
/// `server.role`'s third spelling, `serving`, names what a node is to its own callers, not what it
/// is to a peer declaring it.
/// `ServerRole::parse` already trims before matching, so this does not trim a second time - the
/// refusal reports `raw` exactly as the operator wrote it, trailing whitespace included.
fn parse_peer_role(url: &str, raw: &str) -> Result<ServerRole, InvalidDeclaredNode> {
    match ServerRole::parse(raw) {
        Ok(ServerRole::Worker) => Ok(ServerRole::Worker),
        Ok(ServerRole::ReadingWorker) => Ok(ServerRole::ReadingWorker),
        Ok(ServerRole::Serving) | Err(_) => Err(InvalidDeclaredNode::UnknownPeerRole {
            url: String::from(url),
            found: String::from(raw),
        }),
    }
}

fn parse_peer_source(url: &str, raw: &str) -> Result<SourceName, InvalidDeclaredNode> {
    SourceName::parse(raw).map_err(|_bad_identifier| InvalidDeclaredNode::InvalidSourceAlias {
        url: String::from(url),
        found: String::from(raw),
    })
}

fn node_address(written: &str) -> Result<Url, InvalidTlsMaterial> {
    match Url::parse(written.trim()) {
        Ok(url) if url.scheme() == "https" && url.has_host() => {
            let bare_path = url.path().is_empty() || url.path() == "/";
            if !bare_path
                || url.query().is_some()
                || url.fragment().is_some_and(|address| address.parse::<IpAddr>().is_err())
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(InvalidTlsMaterial::NodeAddressHasExtra {
                    found: String::from(written),
                });
            }
            Ok(url)
        }
        Ok(_) | Err(_) => Err(InvalidTlsMaterial::NotANodeAddress {
            found: String::from(written),
        }),
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use super::{DeclaredNodeInput, DeclaredNodeRefusal, InvalidDeclaredNode, InvalidTlsMaterial};
    use crate::server::TlsMaterial;

    fn declared(sources: &[String]) -> Vec<DeclaredNodeInput<'_>> {
        vec![DeclaredNodeInput {
            url: "https://b.example:8443",
            role: "reading-worker",
            sources,
        }]
    }

    fn material_with_authority() -> TlsMaterial {
        TlsMaterial::parse(Some("/tls/chain.pem"), Some("/tls/key.pem"), Some("/tls/nodes.pem"))
            .expect("a pair and an authority parse")
            .expect("a pair is material")
    }

    #[test]
    fn declared_nodes_need_the_node_authority_and_https_addresses() {
        let pair = TlsMaterial::parse(Some("/tls/chain.pem"), Some("/tls/key.pem"), None)
            .expect("a pair parses")
            .expect("a pair is material");
        let others = [DeclaredNodeInput {
            url: "https://b.example:8443",
            role: "worker",
            sources: [].as_slice(),
        }];
        assert_eq!(
            pair.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Tls(InvalidTlsMaterial::NodesWithoutAuthority))
        );
        let material = material_with_authority();
        assert_eq!(
            material.clone().with_nodes("http://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Tls(InvalidTlsMaterial::NotANodeAddress {
                found: String::from("http://a.example:8443")
            }))
        );
        let declared = material
            .with_nodes(" https://a.example:8443 ", &others)
            .expect("https addresses under an authority are nodes");
        let nodes = declared.nodes().expect("the nodes ride with the material");
        assert_eq!(nodes.own().as_str(), "https://a.example:8443/");
        assert_eq!(nodes.others().len(), 1);
        assert_eq!(nodes.others()[0].url().as_str(), "https://b.example:8443/");
    }

    #[test]
    fn a_reading_worker_peer_naming_no_source_is_refused() {
        let material = material_with_authority();
        let others = declared(&[]);
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::ReadingWorkerNamesNoSource {
                url: String::from("https://b.example:8443/")
            }))
        );
    }

    #[test]
    fn a_worker_peer_naming_a_source_is_refused() {
        let material = material_with_authority();
        let sources = [String::from("orders")];
        let others = [DeclaredNodeInput {
            url: "https://b.example:8443",
            role: "worker",
            sources: sources.as_slice(),
        }];
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::WorkerNamesSources {
                url: String::from("https://b.example:8443/")
            }))
        );
    }

    #[test]
    fn the_same_peer_url_declared_twice_is_refused() {
        let material = material_with_authority();
        let others = [
            DeclaredNodeInput {
                url: "https://b.example:8443",
                role: "worker",
                sources: [].as_slice(),
            },
            DeclaredNodeInput {
                url: "https://b.example:8443",
                role: "worker",
                sources: [].as_slice(),
            },
        ];
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::DuplicateNodeUrl {
                url: String::from("https://b.example:8443/")
            }))
        );
    }

    #[test]
    fn a_peer_url_differing_only_by_case_default_port_or_trailing_slash_is_the_same_node() {
        let material = material_with_authority();
        let others = [
            DeclaredNodeInput {
                url: "https://B.EXAMPLE:443",
                role: "worker",
                sources: [].as_slice(),
            },
            DeclaredNodeInput {
                url: "https://b.example/",
                role: "worker",
                sources: [].as_slice(),
            },
        ];
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::DuplicateNodeUrl {
                url: String::from("https://b.example/")
            }))
        );
    }

    #[test]
    fn a_peer_address_with_a_path_query_fragment_or_userinfo_is_refused() {
        let material = material_with_authority();
        for written in [
            "https://b.example:8443/workers",
            "https://b.example:8443?role=worker",
            "https://b.example:8443#node",
            "https://user:pw@b.example:8443",
        ] {
            let others = [DeclaredNodeInput {
                url: written,
                role: "worker",
                sources: [].as_slice(),
            }];
            assert_eq!(
                material.clone().with_nodes("https://a.example:8443", &others),
                Err(DeclaredNodeRefusal::Tls(InvalidTlsMaterial::NodeAddressHasExtra {
                    found: String::from(written)
                }))
            );
        }
    }

    #[test]
    fn a_peer_naming_this_nodes_own_address_is_refused() {
        let material = material_with_authority();
        let others = [DeclaredNodeInput {
            url: "https://a.example:8443",
            role: "worker",
            sources: [].as_slice(),
        }];
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::NodeNamesItself {
                url: String::from("https://a.example:8443/")
            }))
        );
    }

    #[test]
    fn an_unknown_peer_role_is_refused_including_serving() {
        let material = material_with_authority();
        for spelling in ["serving", "combiner", ""] {
            let others = [DeclaredNodeInput {
                url: "https://b.example:8443",
                role: spelling,
                sources: [].as_slice(),
            }];
            assert_eq!(
                material.clone().with_nodes("https://a.example:8443", &others),
                Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::UnknownPeerRole {
                    url: String::from("https://b.example:8443/"),
                    found: String::from(spelling),
                }))
            );
        }
    }

    #[test]
    fn a_malformed_source_alias_is_refused_naming_it() {
        let material = material_with_authority();
        let sources = [String::from("not an alias")];
        let others = declared(&sources);
        assert_eq!(
            material.with_nodes("https://a.example:8443", &others),
            Err(DeclaredNodeRefusal::Node(InvalidDeclaredNode::InvalidSourceAlias {
                url: String::from("https://b.example:8443/"),
                found: String::from("not an alias"),
            }))
        );
    }

    #[test]
    fn declared_nodes_know_which_peers_read_a_source() {
        let material = material_with_authority();
        let sources = [String::from("orders")];
        let others = [
            DeclaredNodeInput {
                url: "https://b.example:8443",
                role: "reading-worker",
                sources: sources.as_slice(),
            },
            DeclaredNodeInput {
                url: "https://c.example:8443",
                role: "worker",
                sources: [].as_slice(),
            },
        ];
        let declared = material
            .with_nodes("https://a.example:8443", &others)
            .expect("a valid mixed declaration");
        let nodes = declared.nodes().expect("the nodes ride with the material");
        let orders = SourceName::parse("orders").expect("orders is a valid alias");
        let readers: Vec<_> = nodes.reading(&orders).map(|node| node.url().as_str()).collect();
        assert_eq!(readers, vec!["https://b.example:8443/"]);
        let unread = SourceName::parse("unread").expect("unread is a valid alias");
        assert_eq!(nodes.reading(&unread).count(), 0);
    }
}

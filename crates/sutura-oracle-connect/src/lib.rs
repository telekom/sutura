#![forbid(unsafe_code)]
//! The Oracle connector `sutura-exec-oracle` dials a source through and `sutura-catalog-rdbms`'s
//! Oracle reader dials its documentation schema through: one EZCONNECT address, one TLS posture,
//! one dial bound and one driver configuration, so the two dials cannot drift apart. It cannot live
//! in the exec adapter, because a catalog adapter may not reach `sutura-sql` and the exec adapter
//! renders through it.
//!
//! # Limits
//!
//! - **TLS verifies against the declared anchors only.** [`Channel::Verified`] hands the driver
//!   PEM certificates that REPLACE its bundled public certificate authorities. The driver builds
//!   its own `rustls::ClientConfig`, so a host store and a client certificate have nothing here to
//!   reach.
//! - **The bound covers the TCP connect only.** The driver reads the TLS and TNS handshakes with no
//!   timeout, so a listener that accepts and then never answers is not bounded by [`DIAL_DEADLINE`].
//! - **A listener's redirect is refused**: the driver is told not to follow one, so the connection
//!   stays on the declared address.

pub mod oracle_trace;

/// How long the TCP connect to the listener may take before the dial is refused.
///
/// The bound covers the connect only: a listener that accepts and then never answers is not bounded
/// by it, because the driver reads its handshake with no timeout.
pub const DIAL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// How the connection to the listener is secured.
#[derive(Debug, Clone, Copy)]
pub enum Channel<'pem> {
    /// No transport security. No composition root builds this; the adapter's own unit cells do.
    Plaintext,
    /// TLS, verified against these PEM certificates and no others: they replace the driver's bundled
    /// public certificate authorities rather than adding to them.
    Verified { anchors_pem: &'pem str },
}

/// Where one connection goes and how it is secured: an EZCONNECT `host:port/service_name`.
#[derive(Debug, Clone, Copy)]
pub struct Dial<'dial> {
    host: &'dial str,
    port: u16,
    service_name: &'dial str,
    channel: Channel<'dial>,
    deadline: std::time::Duration,
}

/// Why a [`Dial`] has no driver configuration.
#[derive(Debug, thiserror::Error)]
pub enum DialError {
    /// The driver's packet trace is switched on - see [`crate::oracle_trace::refuse`].
    #[error(transparent)]
    PacketTraceOn(#[from] crate::oracle_trace::PacketTraceOn),
    /// The dial's anchors or address are refused - see [`Dial::prepare`].
    #[error(transparent)]
    Prepare(#[from] PrepareError),
}

/// Why a [`Dial`] cannot be prepared.
#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    /// The declared anchors are not PEM certificates the driver can verify a server against.
    #[error("the declared trust anchors are not usable certificates: {0}")]
    TrustAnchors(oracledb::Error),
    /// The driver cannot parse the address.
    #[error("the driver cannot parse the address: {0}")]
    Address(oracledb::Error),
}

/// A [`Dial`] whose address and anchors the driver has parsed, kept so that each connect clones
/// them instead of parsing them again.
#[derive(Clone)]
pub struct PreparedDial(oracledb::Config);

impl core::fmt::Debug for PreparedDial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PreparedDial").finish_non_exhaustive()
    }
}

impl PreparedDial {
    /// The driver's configuration for one connect, with no credential.
    ///
    /// # Errors
    ///
    /// [`crate::oracle_trace::PacketTraceOn`] while the driver's packet trace is switched on.
    pub fn config(&self) -> Result<oracledb::Config, crate::oracle_trace::PacketTraceOn> {
        crate::oracle_trace::refuse()?;
        Ok(self.0.clone())
    }
}

impl<'dial> Dial<'dial> {
    /// A dial bounded by [`DIAL_DEADLINE`].
    #[must_use]
    pub const fn new(host: &'dial str, port: u16, service_name: &'dial str, channel: Channel<'dial>) -> Self {
        Self {
            host,
            port,
            service_name,
            channel,
            deadline: DIAL_DEADLINE,
        }
    }

    /// The bound on the TCP connect.
    #[must_use]
    pub const fn deadline(&self) -> std::time::Duration {
        self.deadline
    }

    /// The same dial with its TCP connect bounded by `deadline` instead.
    #[must_use]
    pub const fn within(self, deadline: std::time::Duration) -> Self {
        Self { deadline, ..self }
    }

    /// The driver's configuration for this dial, with no credential: its address, its channel and
    /// anchors, the dial bound, and a refused redirect.
    ///
    /// # Errors
    ///
    /// [`DialError::PacketTraceOn`] while the driver's packet trace is switched on, before anything
    /// else; otherwise [`DialError::Prepare`] for what [`Dial::prepare`] refuses.
    pub fn config(self) -> Result<oracledb::Config, DialError> {
        crate::oracle_trace::refuse()?;
        Ok(self.prepare()?.0)
    }

    /// The same configuration as [`Dial::config`], parsed once and asking the packet-trace refusal
    /// at each [`PreparedDial::config`] instead.
    ///
    /// # Errors
    ///
    /// [`PrepareError::TrustAnchors`] for anchors that are not usable certificates, and
    /// [`PrepareError::Address`] for an address the driver cannot parse.
    pub fn prepare(self) -> Result<PreparedDial, PrepareError> {
        let address = ezconnect(self.host, self.port, self.service_name);
        let connect_string = match self.channel {
            Channel::Plaintext => address,
            Channel::Verified { .. } => format!("tcps://{address}"),
        };
        let config = oracledb::Config::default()
            .set_connect_string(&connect_string)
            .map_err(PrepareError::Address)?
            .set_follow_redirects(false)
            .set_transport_connect_timeout(Some(self.deadline));
        match self.channel {
            Channel::Plaintext => Ok(PreparedDial(config)),
            Channel::Verified { anchors_pem } => config
                .set_trust_anchors_pem(anchors_pem)
                .map(PreparedDial)
                .map_err(PrepareError::TrustAnchors),
        }
    }
}

/// An EZCONNECT `host:port/service_name`, with an IPv6 literal in brackets.
///
/// The driver's EZCONNECT parser reads a host as a bracketed literal or a run of name characters,
/// so an unbracketed `::1:1521/FREEPDB1` is not an EZCONNECT string to it at all: it falls through
/// to a `tnsnames.ora` alias lookup and fails - or, where a configuration directory is set in the
/// environment, resolves whatever that file names. Bracketing is decided on a parsed address, not
/// on the presence of a `:`, so a name is never wrapped.
#[must_use]
pub fn ezconnect(host: &str, port: u16, service_name: &str) -> String {
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}/{service_name}")
    } else {
        format!("{host}:{port}/{service_name}")
    }
}

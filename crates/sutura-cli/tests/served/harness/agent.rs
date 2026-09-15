//! The agent-surface settings builder, split out under the same `max-lines` reason `harness.rs`'s
//! own header names for `postgres`/`keycloak`/`reading`: this file is `#[cfg(feature = "agent")]`
//! on the whole module so a build without the feature parses none of it.
//!
//! `Served::mcp` stays in `harness.rs`'s own `impl Served` block rather than moving here -
//! `clippy::multiple_inherent_impl` is `-D warnings` under this workspace's lints and refuses a
//! second `impl Served` in a sibling file, so only free functions split out.

use std::path::Path;

use sutura_dev::issuer::MockIssuer;

use super::{SINGLE_USER, deployment};

/// The bind every AGENT-SURFACED served cell starts (loopback, kernel port, surface on); the `agent` feature must be compiled in for the mount to exist.
pub(crate) const AGENT_LOOPBACK: &str = "  host: \"127.0.0.1\"\n  port: 0\n  agent_surface:\n    enabled: true\n";

/// The leg-1 deployment WITH the agent surface turned on - the shape every agent-surface cell that expects `/mcp` to answer starts from.
pub(crate) fn settings_with_agent_surface(example: &Path, issuer: &MockIssuer, key_set: &Path) -> String {
    deployment(
        example,
        AGENT_LOOPBACK,
        &format!(
            "{SINGLE_USER}  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
             authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n",
            issuer.audience(),
            issuer.issuer(),
            key_set.display(),
        ),
    )
}

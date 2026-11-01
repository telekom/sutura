//! The `operator` section, which `sutura-operator` reads and a served deployment does not.
//!
//! The operator reads the same layered sources as `sutura serve`, so one directory and one set of
//! `SUTURA__*` variables configure both. The whole tree is still read, and an unknown key anywhere
//! still refuses; only this section, `security.metrics_token` and `server.tls_node_authority` are
//! parsed, so a tree that
//! [`crate::NotFitToServe`] refuses - no access token, an off-host bind - still configures the
//! operator.

use std::path::{Path, PathBuf};

use sutura_domain::identity::Secret;

use crate::security::{AccessToken, InvalidAccessToken};
use crate::settings::{SettingsLoadError, Sources, read};

/// What `sutura-operator` starts with.
#[derive(Debug)]
pub struct OperatorSettings {
    api_group: ApiGroup,
    metrics_token: Option<Secret>,
    node_authority: Option<PathBuf>,
    workers: Option<DeploymentName>,
    serving: Option<DeploymentName>,
}

impl OperatorSettings {
    /// Reads the layered sources and parses the `operator` section.
    pub fn load(sources: &Sources) -> Result<Self, OperatorSettingsError> {
        let (raw, _layers) = read(sources)?;
        let api_group = ApiGroup::parse(&raw.operator.api_group).map_err(|cause| OperatorSettingsError::ApiGroup { cause })?;
        let metrics_token = match raw.security.metrics_token.as_deref() {
            None | Some("") => None,
            Some(value) => {
                AccessToken::parse(value).map_err(|cause| OperatorSettingsError::MetricsToken { cause })?;
                Some(Secret::new(value))
            }
        };
        let node_authority = match raw.server.tls_node_authority.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(path) => Some(PathBuf::from(path)),
        };
        let workers = match raw.operator.workers.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(name) => Some(DeploymentName::parse(name).map_err(|cause| OperatorSettingsError::Workers { cause })?),
        };
        let serving = match raw.operator.serving.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(name) => Some(DeploymentName::parse(name).map_err(|cause| OperatorSettingsError::Serving { cause })?),
        };
        Ok(Self {
            api_group,
            metrics_token,
            node_authority,
            workers,
            serving,
        })
    }

    /// `operator.api_group`.
    #[inline]
    pub const fn api_group(&self) -> &ApiGroup {
        &self.api_group
    }

    /// `security.metrics_token`, which the operator presents to each pod's `/metrics` as `serve`
    /// requires it there.
    #[inline]
    pub const fn metrics_token(&self) -> Option<&Secret> {
        self.metrics_token.as_ref()
    }

    /// `server.tls_node_authority`, which each pod's certificate is checked against.
    #[inline]
    pub fn node_authority(&self) -> Option<&Path> {
        self.node_authority.as_deref()
    }

    /// `operator.workers`, the only Deployment the operator may scale. Unset is not validated here:
    /// `sutura-operator run` refuses to start without it (`ClusterError::NoWorkersName`).
    #[inline]
    pub const fn workers(&self) -> Option<&DeploymentName> {
        self.workers.as_ref()
    }

    /// `operator.serving`, the only Deployment the operator may read demand from. Unset is not
    /// validated here: `sutura-operator run` refuses to start without it (`ClusterError::NoServingName`).
    #[inline]
    pub const fn serving(&self) -> Option<&DeploymentName> {
        self.serving.as_ref()
    }
}

/// Why the operator's settings did not load.
#[derive(Debug, thiserror::Error)]
pub enum OperatorSettingsError {
    #[error(transparent)]
    Load(#[from] SettingsLoadError),
    #[error("`operator.api_group` is not a Kubernetes API group")]
    ApiGroup {
        #[source]
        cause: InvalidApiGroup,
    },
    #[error("`security.metrics_token` is not usable as a token")]
    MetricsToken {
        #[source]
        cause: InvalidAccessToken,
    },
    #[error("`operator.workers` is not a Deployment name")]
    Workers {
        #[source]
        cause: InvalidDeploymentName,
    },
    #[error("`operator.serving` is not a Deployment name")]
    Serving {
        #[source]
        cause: InvalidDeploymentName,
    },
}

/// The API group the worker-pool resource is served under
/// (`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`).
///
/// What the Kubernetes API server requires of a custom resource's group: a lowercase RFC 1123 DNS
/// subdomain of at most 253 characters, with at least one dot. **Not checked here**, and refused by
/// the API server instead: a group longer than the definition's name allows once the resource's
/// plural is prefixed, and a group under `k8s.io` or `kubernetes.io`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiGroup(String);

impl ApiGroup {
    const MAX_LEN: usize = 253;

    /// Parses `raw`, refusing what the API server would refuse for its form.
    pub fn parse(raw: &str) -> Result<Self, InvalidApiGroup> {
        if raw.len() > Self::MAX_LEN || !raw.split('.').all(is_label) {
            return Err(InvalidApiGroup::NotSubdomain { group: raw.to_owned() });
        }
        if !raw.contains('.') {
            return Err(InvalidApiGroup::NoDot { group: raw.to_owned() });
        }
        Ok(Self(raw.to_owned()))
    }

    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// One label of an RFC 1123 subdomain: `[a-z0-9]([-a-z0-9]*[a-z0-9])?`.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Why a string is not an [`ApiGroup`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidApiGroup {
    #[error(
        "`{group}` is not a lowercase DNS subdomain: dot-separated labels of `a-z`, `0-9` and `-`, each \
         beginning and ending with a letter or digit, 253 characters at most"
    )]
    NotSubdomain { group: String },
    #[error("`{group}` has no dot, and a Kubernetes API group is a domain with at least one")]
    NoDot { group: String },
}

/// A Deployment's name.
///
/// As `SuturaWorkerPool.spec.workers`/`.serving` require: a lowercase RFC 1123 subdomain of at most
/// 253 characters, the same form `DEPLOYMENT_NAME` checks in `sutura-operator` (duplicated here
/// rather than shared, since `sutura-config` does not depend on that crate).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentName(String);

impl DeploymentName {
    const MAX_LEN: usize = 253;

    /// Parses `raw`, refusing what the API server's schema would refuse for its form.
    pub fn parse(raw: &str) -> Result<Self, InvalidDeploymentName> {
        if raw.len() > Self::MAX_LEN || !raw.split('.').all(is_label) {
            return Err(InvalidDeploymentName::NotSubdomain { name: raw.to_owned() });
        }
        Ok(Self(raw.to_owned()))
    }

    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Why a string is not a [`DeploymentName`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDeploymentName {
    #[error(
        "`{name}` is not a lowercase DNS subdomain: dot-separated labels of `a-z`, `0-9` and `-`, each \
         beginning and ending with a letter or digit, 253 characters at most"
    )]
    NotSubdomain { name: String },
}

#[cfg(test)]
mod tests {
    use super::{ApiGroup, InvalidApiGroup};

    #[test]
    fn a_group_without_a_dot_is_refused() {
        assert_eq!(
            ApiGroup::parse("sutura"),
            Err(InvalidApiGroup::NoDot {
                group: String::from("sutura")
            })
        );
    }
}

/// Apart from `tests`, so neither file names every `InvalidApiGroup` variant (`check-refusal-coverage`).
#[cfg(test)]
mod at_load;

/// `operator.workers`/`.serving` coverage, kept out of `at_load` so reverting this file (`DeploymentName`,
/// `OperatorSettingsError::Workers`/`::Serving`) at causality's base run orphans it rather than leaving it
/// held at a shape the base tree cannot build.
#[cfg(test)]
mod deployment_names;

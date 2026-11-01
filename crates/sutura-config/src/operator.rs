//! The `operator` section, which `sutura-operator` reads and a served deployment does not.
//!
//! The operator reads the same layered sources as `sutura serve`, so one directory and one set of
//! `SUTURA__*` variables configure both. The whole tree is still read, and an unknown key anywhere
//! still refuses; only this section and `security.metrics_token` are parsed, so a tree that
//! [`crate::NotFitToServe`] refuses - no access token, an off-host bind - still configures the
//! operator.

use sutura_domain::identity::Secret;

use crate::security::{AccessToken, InvalidAccessToken};
use crate::settings::{SettingsLoadError, Sources, read};

/// What `sutura-operator` starts with.
#[derive(Debug)]
pub struct OperatorSettings {
    api_group: ApiGroup,
    metrics_token: Option<Secret>,
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
        Ok(Self {
            api_group,
            metrics_token,
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

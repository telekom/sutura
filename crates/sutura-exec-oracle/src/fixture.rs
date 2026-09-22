//! The fixture tier's credential: **configured, or refused by name.**
//!
//! Behind the default-off `fixtures` feature - `sutura-exec-postgres::fixture`'s own reason, one
//! size smaller: nothing a release publishes links this crate at all (see the workspace manifest's
//! member-list entry), so the feature is about keeping a `pub fn` that reads `std::env::var` out of
//! the default cargo feature set rather than about hiding a shipped artefact's credential.
//!
//! **`SUTURA_DEV_USER`/`SUTURA_DEV_PASSWORD`, not a third-tier name.** `compose.services.yaml`'s
//! `oracle` service reuses the SAME `x-fixture-credentials` anchor every compose-only service in
//! that file shares - `clickhouse` included - because this tier, unlike Postgres, is provisioned by
//! that file rather than by a nix-native script. `sutura_exec_postgres::fixture` deliberately does
//! NOT share that name, for the opposite reason: nothing in `compose.services.yaml` provisions a
//! Postgres, so a shared name there would have implied a coupling that did not exist. Here the
//! coupling is real - the compose block IS the definition - so sharing the name is the honest
//! spelling rather than a third naming scheme nobody chose.

use sutura_domain::identity::Secret;

/// Why there is no fixture credential to connect with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnconfiguredFixture {
    /// The variable is not in the environment at all.
    #[error(
        "{0} is unset, so this worktree has no Oracle fixture credential. Bring the tier up with \
         `just dev-up-oracle`, which starts `compose.services.yaml`'s `oracle` profile under the \
         fixture credential every compose-only service in that file shares."
    )]
    Unset(&'static str),
    /// The variable is set to nothing - a shell script exporting `""` is the ordinary way to reach
    /// this, and read as a credential it is a login attempt as the empty user.
    #[error("{0} is set to an empty value, which is not a credential.")]
    Blank(&'static str),
}

/// The fixture tier's credential.
///
/// No `Display`, no `PartialEq`, no public field: the password is a
/// [`sutura_domain::identity::Secret`], which has neither, and the two names leave this crate only
/// as arguments to `crate::OracleWarehouse::local_config`.
#[derive(Debug)]
pub struct FixtureCredential {
    user: String,
    password: Secret,
}

impl FixtureCredential {
    /// This process's credential, as `compose.services.yaml`'s anchors export it.
    pub fn from_env() -> Result<Self, UnconfiguredFixture> {
        Self::parse(|name| std::env::var(name).ok())
    }

    /// The parse, over a LOOKUP rather than over the process environment - `std::env::set_var` is
    /// `unsafe` in Rust 2024 and this crate's root forbids `unsafe_code`, so a test exercising
    /// the real environment could not be written at all.
    pub(crate) fn parse(lookup: impl Fn(&'static str) -> Option<String>) -> Result<Self, UnconfiguredFixture> {
        let present = |name: &'static str| match lookup(name) {
            None => Err(UnconfiguredFixture::Unset(name)),
            Some(value) if value.trim().is_empty() => Err(UnconfiguredFixture::Blank(name)),
            Some(value) => Ok(value),
        };
        Ok(Self {
            user: present("SUTURA_DEV_USER")?,
            password: Secret::new(present("SUTURA_DEV_PASSWORD")?),
        })
    }

    /// The application schema user - `compose.services.yaml`'s `APP_USER`.
    #[inline]
    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    /// The password, still wrapped. Exposed at exactly one call site - the connection config.
    #[inline]
    pub(crate) const fn password(&self) -> &Secret {
        &self.password
    }
}

#[cfg(test)]
mod tests {
    use super::{FixtureCredential, UnconfiguredFixture};

    #[test]
    fn an_absent_variable_is_refused_and_named() {
        let refusal = FixtureCredential::parse(|name| (name != "SUTURA_DEV_PASSWORD").then(|| String::from("x")));
        assert_eq!(refusal.err(), Some(UnconfiguredFixture::Unset("SUTURA_DEV_PASSWORD")));
    }

    #[test]
    fn a_blank_variable_is_refused_and_named() {
        let refusal = FixtureCredential::parse(|name| Some(String::from(if name == "SUTURA_DEV_USER" { "   " } else { "x" })));
        assert_eq!(refusal.err(), Some(UnconfiguredFixture::Blank("SUTURA_DEV_USER")));
    }

    #[test]
    fn both_present_parses() {
        let credential = FixtureCredential::parse(|name| Some(String::from(name))).expect("both present values parse");
        assert_eq!(credential.user(), "SUTURA_DEV_USER");
    }

    #[test]
    fn debug_does_not_leak_the_password() {
        let credential = FixtureCredential::parse(|name| {
            Some(String::from(if name == "SUTURA_DEV_PASSWORD" {
                "hunter2-do-not-log-me"
            } else {
                "sutura"
            }))
        })
        .expect("both present values parse");
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("hunter2"), "Debug leaked the password: {rendered}");
    }
}

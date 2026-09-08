//! The fixture tier's credential: **configured, or refused by name.**
//!
//! # What this replaces, and why the old shape was wrong in a way its strength cannot fix
//!
//! `local_config` used to read `SUTURA_DEV_USER` / `SUTURA_DEV_PASSWORD` / `SUTURA_DEV_DB` through
//! an `unwrap_or_else(|_| "sutura")` fallback, in a `pub fn` that was neither `#[cfg(test)]` nor
//! feature-gated. The reasoning beside the compose file - *these are fixtures, nothing outside the
//! host can reach them, every port is published ephemerally* - is sound, and it is sound **about a
//! container.** It does not reach a function whose `host` and `port` are PARAMETERS: this code
//! cannot know it is talking to an ephemeral local server, so the property *"that credential only
//! ever reaches one"* was held by the function's name and by the discipline of its callers.
//! `AGENTS.md`: invariants are held by a type, a lint, a hook or a gate, never by recall.
//!
//! And the doc comment inverted the default - *"so a host that objects to a weak default can change
//! one value"* makes hardening **opt-in**. That inversion is the defect, more than the strength of
//! the password: a weak default a caller must object to is the opposite of secure by design.
//!
//! # The shape now
//!
//! [`crate::fixture::FixtureCredential`] is the only way to hold one, its three values are private, and
//! `FixtureCredential::parse` is the only way in. So *unconfigured* is not a value
//! `PostgresWarehouse::local_config` can be handed - it is unrepresentable rather than rejected -
//! and there is no branch left for a fallback to live in.
//!
//! # Where the values come from, because nothing set them before
//!
//! **The provisioner publishes them, and it is not the compose file.** There is no Postgres
//! service in `compose.services.yaml` at all - this tier is nixpkgs' `postgresql_18`, started by
//! `nix/postgres-tier.nix`, which now generates a password per worktree and prints the three
//! exports from `sutura-postgres-tier credentials`. `nix/with-tier.sh` evaluates them exactly where
//! it already exports `SUTURA_DEV_REQUIRE_TIER`, so *the server is there* and *the client knows how
//! to log in* cannot come apart, and `checks.postgres-tier` drives that subcommand's two answers.
//!
//! The variables are `SUTURA_POSTGRES_TIER_*` rather than the old `SUTURA_DEV_*`: the names were
//! shared with the compose fixture credential while nothing in that file provisions a Postgres, and
//! a shared name is what made a coupling look real. The compose anchors keep their own defaults and
//! their own argument.
//!
//! # Two limits, stated with the claim
//!
//! - **A unix socket under this tier authenticates by `trust`.** `nix/postgres-tier.nix` runs
//!   `initdb` with no `--auth`, so the password is not what admits a client to *that* server. What
//!   the refusal buys is that no OTHER host is offered a guessable one, which is the half the old
//!   `pub fn` could not hold.
//! - **The role name and the database name are still `sutura`.** They are names the provisioner
//!   CHOOSES and publishes, not values a client guesses when nothing is set - which is the
//!   distinction this module is about. Only the password is generated.

use sutura_domain::identity::Secret;

/// One of the three values the Postgres fixture tier publishes into the environment.
///
/// An enum rather than a `&'static str`, because a refusal has to carry WHICH value was missing as
/// data: a caller that read the variable's name out of the message would be matching on prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixtureVariable {
    /// The login role the tier created.
    User,
    /// That role's password, generated per worktree by `nix/postgres-tier.nix`.
    Password,
    /// The database the tier created and owns.
    Database,
}

impl FixtureVariable {
    /// Every variable, in the order `FixtureCredential::parse` reads them.
    ///
    /// Exists so a test - and a message - can walk the set rather than restate it, which is how the
    /// third one comes to be missed.
    pub const ALL: [Self; 3] = [Self::User, Self::Password, Self::Database];

    /// The environment variable's name, spelled once.
    ///
    /// One definition, because a refusal that tells somebody to set a name the reader spells
    /// differently is a fix that does not work and looks like it should.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::User => "SUTURA_POSTGRES_TIER_USER",
            Self::Password => "SUTURA_POSTGRES_TIER_PASSWORD",
            Self::Database => "SUTURA_POSTGRES_TIER_DB",
        }
    }
}

impl core::fmt::Display for FixtureVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Why there is no fixture credential to connect with.
///
/// **Two variants, and no third that carries a substitute.** The failure is that this process was
/// not told the credential, and the only honest answers to it are *say which variable* and *stop* -
/// so there is nowhere in this type for a default to be returned from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnconfiguredFixture {
    /// The variable is not in the environment at all.
    #[error(
        "{0} is unset, so this worktree has no Postgres fixture credential. The tier publishes one: \
         run the suite with `just test`, which brings the tier up and exports what \
         `just postgres-tier credentials` prints."
    )]
    Unset(FixtureVariable),
    /// The variable is set to nothing, which a shell script exporting `""` is the ordinary way to
    /// reach. Read as a credential it is a login attempt as the empty user, so it is refused here
    /// rather than at the server.
    #[error(
        "{0} is set to an empty value, which is not a credential. Re-provision the tier - \
         `just postgres-tier stop` then `just test` - so it publishes one."
    )]
    Blank(FixtureVariable),
}

/// The fixture tier's credential.
///
/// Exists only if all three values were present, so nothing downstream asks again. No `Display`,
/// no `PartialEq`, no public field and no public accessor: the password is a
/// [`sutura_domain::identity::Secret`], which has neither `Display` nor `==`, and the two names
/// leave this crate only as arguments to `PostgresWarehouse::local_config`.
#[derive(Debug)]
pub struct FixtureCredential {
    user: String,
    password: Secret,
    database: String,
}

impl FixtureCredential {
    /// This process's credential, as the tier exported it.
    ///
    /// The production entry point, and a one-line adapter over `FixtureCredential::parse` on
    /// purpose - see that function for why the seam is there.
    pub fn from_env() -> Result<Self, UnconfiguredFixture> {
        Self::parse(|variable| std::env::var(variable.name()).ok())
    }

    /// The parse, over a LOOKUP rather than over the process environment.
    ///
    /// The seam is not decoration: `std::env::set_var` is `unsafe` in Rust 2024 and this workspace
    /// sets `unsafe_code = "forbid"`, so a test that reached for the real environment could not be
    /// written at all - the same wall `crates/sutura-cli/src/audit.rs` documents. The refusals below
    /// are therefore provokable, which is the whole reason to have typed them.
    ///
    /// **The order is the order [`FixtureVariable::ALL`] gives**, so the first thing a reader is
    /// told to set is the first thing the tier exports, rather than whichever field the struct
    /// literal happened to evaluate first.
    pub(crate) fn parse(lookup: impl Fn(FixtureVariable) -> Option<String>) -> Result<Self, UnconfiguredFixture> {
        let present = |variable: FixtureVariable| match lookup(variable) {
            None => Err(UnconfiguredFixture::Unset(variable)),
            // Trimmed rather than compared to `""`: a value exported as `" "` is the same absence
            // with harder-to-read evidence.
            Some(value) if value.trim().is_empty() => Err(UnconfiguredFixture::Blank(variable)),
            Some(value) => Ok(value),
        };
        Ok(Self {
            user: present(FixtureVariable::User)?,
            password: Secret::new(present(FixtureVariable::Password)?),
            database: present(FixtureVariable::Database)?,
        })
    }

    /// The login role.
    #[inline]
    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    /// The database.
    #[inline]
    pub(crate) fn database(&self) -> &str {
        &self.database
    }

    /// The password, still wrapped. Exposed at exactly one call site - the connection config -
    /// where the value itself is the payload.
    #[inline]
    pub(crate) const fn password(&self) -> &Secret {
        &self.password
    }
}

#[cfg(test)]
mod tests {
    use super::{FixtureCredential, FixtureVariable, UnconfiguredFixture};

    /// A lookup that answers for every variable except the one being withheld.
    fn all_but(withheld: FixtureVariable) -> impl Fn(FixtureVariable) -> Option<String> {
        move |variable| (variable != withheld).then(|| String::from("configured"))
    }

    /// A lookup that answers with whitespace for one variable and a value for the rest.
    fn blank_for(blank: FixtureVariable) -> impl Fn(FixtureVariable) -> Option<String> {
        move |variable| Some(String::from(if variable == blank { "   " } else { "configured" }))
    }

    /// Every variable is refused BY NAME when it is absent - all three, driven off
    /// [`FixtureVariable::ALL`] so a fourth value cannot arrive untested.
    ///
    /// This is the test the old shape could not have: there, an absent variable produced
    /// `sutura` and an `Ok`.
    #[test]
    fn an_absent_variable_is_refused_and_named() {
        for withheld in FixtureVariable::ALL {
            let refusal = FixtureCredential::parse(all_but(withheld));
            assert_eq!(
                refusal.err(),
                Some(UnconfiguredFixture::Unset(withheld)),
                "{withheld} unset must refuse, naming {withheld}"
            );
        }
    }

    /// And an EMPTY value is refused too, separately, because `export FOO=` is how a shell script
    /// supplies nothing while looking like it supplied something.
    #[test]
    fn a_blank_variable_is_refused_and_named() {
        for blank in FixtureVariable::ALL {
            let refusal = FixtureCredential::parse(blank_for(blank));
            assert_eq!(
                refusal.err(),
                Some(UnconfiguredFixture::Blank(blank)),
                "{blank} blank must refuse, naming {blank}"
            );
        }
    }

    /// The refusal SAYS the variable's name, not just carries it: the typed field is for a
    /// caller and the rendering is for the person who has to fix it.
    #[test]
    fn the_message_names_the_variable_and_a_task_that_supplies_it() {
        for withheld in FixtureVariable::ALL {
            let rendered = FixtureCredential::parse(all_but(withheld))
                .err()
                .map(|refusal| refusal.to_string())
                .expect("an absent variable refuses");
            assert!(rendered.contains(withheld.name()), "the refusal did not name it: {rendered}");
            assert!(
                rendered.contains("just test"),
                "the refusal named no way to supply it: {rendered}"
            );
        }
    }

    /// All three present is the only way a credential comes into existence, and it carries what it
    /// was given.
    #[test]
    fn all_three_present_parses() {
        let credential = FixtureCredential::parse(|variable| Some(String::from(variable.name())))
            .expect("three present values are a credential");
        assert_eq!(credential.user(), FixtureVariable::User.name());
        assert_eq!(credential.database(), FixtureVariable::Database.name());
    }

    /// The password does not reach a log through the credential's own `Debug`.
    ///
    /// A PROPERTY rather than an exact string - the rendering is `secrecy`'s, and pinning it byte
    /// for byte would fail a harmless upstream rewording while proving nothing extra.
    #[test]
    fn debug_does_not_leak_the_password() {
        let credential = FixtureCredential::parse(|variable| {
            Some(String::from(match variable {
                FixtureVariable::Password => "hunter2-do-not-log-me",
                FixtureVariable::User | FixtureVariable::Database => "sutura",
            }))
        })
        .expect("three present values are a credential");
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("hunter2"), "Debug leaked the password: {rendered}");
        assert!(rendered.contains("REDACTED"), "Debug said nothing was withheld: {rendered}");
    }

    /// The three names are distinct, and each one is namespaced to THIS tier.
    ///
    /// The second half is the assertion that matters: the old names were `SUTURA_DEV_USER` and
    /// `SUTURA_DEV_PASSWORD`, which `compose.services.yaml` reads for its own fixture credential
    /// with its own defaults. Sharing them is what made two unrelated tiers look coupled.
    #[test]
    fn each_variable_is_named_for_this_tier_alone() {
        let mut seen = std::collections::BTreeSet::new();
        for variable in FixtureVariable::ALL {
            assert!(
                variable.name().starts_with("SUTURA_POSTGRES_TIER_"),
                "{variable} is not namespaced to this tier"
            );
            assert!(seen.insert(variable.name()), "{variable} repeats another variable's name");
        }
        assert_eq!(seen.len(), FixtureVariable::ALL.len());
    }
}

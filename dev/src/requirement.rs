//! Whether an absent service tier is a skip or a failure - one definition, read by both halves.
//!
//! The decision has two call sites and they are on opposite sides of the tier:
//!
//! * **Provisioning** asks it when there is no container runtime to bring services UP with.
//! * **A harness** asks it when there is nothing provisioned to CONNECT to.
//!
//! It lived in `xtask` while there was only the first, and it moved here when the second arrived.
//! Two copies of a fail-open/fail-closed decision is the shape that drifts: the copies are edited
//! months apart, one of them stops matching the documentation, and the direction a wrong answer
//! costs the most is the one that silently flipped.
//!
//! **Neither direction is the default, and what a wrong answer costs decides it.** On a developer
//! machine a false failure blocks a contributor who is not touching services - docker is a host
//! dependency this repository deliberately does not pin with nix. In CI this tier is the only thing
//! standing behind a network adapter, so a run that skipped it would report green having tested
//! nothing.
//!
//! **The limit, stated with the claim:** "CI" here is an environment variable, so a check that runs
//! inside a nix sandbox is NOT in CI by this definition - the sandbox scrubs the environment, and it
//! has neither a network nor a docker socket to provision with in any case. What that means in
//! practice is that `checks.nextest` skips the docker-gated tests loudly rather than failing, and
//! the required direction is reached by a runner that invokes a task directly, or by anybody who
//! sets the variable below.

/// The variable that overrides the machine class, in **both** directions.
///
/// Named once, here, because a message that tells somebody to set it and a read that spells it
/// differently is a fix that does not work and looks like it should.
pub const FORCE: &str = "SUTURA_DEV_REQUIRE_DOCKER";

/// Whether a missing tier is fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// A missing tier FAILS. The CI direction: a green run that quietly tested nothing is the
    /// failure the whole tier exists to prevent.
    Required,
    /// A missing tier SKIPS, loudly, naming what did not run. The developer-machine direction.
    Optional,
}

impl Requirement {
    /// The direction this process is running under, read from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        decide(std::env::var("CI").ok().as_deref(), std::env::var(FORCE).ok().as_deref())
    }

    /// Is a missing tier fatal here?
    #[must_use]
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

/// The decision, over the two values rather than over the environment, so it is testable.
///
/// The forced value wins over the machine class, in both directions: a developer who exports
/// `SUTURA_DEV_REQUIRE_DOCKER=1` wants the CI behaviour, and a runner that exports `0` means it.
#[must_use]
pub fn decide(ci: Option<&str>, forced: Option<&str>) -> Requirement {
    if let Some(value) = forced {
        return if truthy(value) {
            Requirement::Required
        } else {
            Requirement::Optional
        };
    }
    if ci.is_some_and(truthy) {
        return Requirement::Required;
    }
    Requirement::Optional
}

/// GitHub Actions sets `CI=true`; a developer who exports `CI=0` means it.
fn truthy(value: &str) -> bool {
    !matches!(value.trim().to_lowercase().as_str(), "" | "0" | "false" | "no")
}

#[cfg(test)]
mod tests {
    use super::{Requirement, decide, truthy};

    #[test]
    fn an_absent_tier_skips_locally_and_fails_in_ci() {
        // Both directions, because a fail-open/fail-closed decision with only one side tested is
        // half a decision - and this is the function that decides it for provisioning AND for the
        // harness, so a regression here is silent on both sides at once.
        assert_eq!(decide(None, None), Requirement::Optional);
        assert_eq!(decide(Some("true"), None), Requirement::Required);
    }

    #[test]
    fn the_flag_overrides_the_machine_class_in_both_directions() {
        assert_eq!(decide(None, Some("1")), Requirement::Required);
        assert_eq!(decide(Some("true"), Some("0")), Requirement::Optional);
    }

    #[test]
    fn an_empty_or_negative_ci_variable_is_not_ci() {
        for value in ["", " ", "0", "false", "no", "FALSE"] {
            assert!(!truthy(value), "`{value}` read as CI");
            assert_eq!(decide(Some(value), None), Requirement::Optional, "`{value}`");
        }
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(truthy(value), "`{value}` read as not-CI");
        }
    }
}

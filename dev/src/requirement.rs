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
//! **Neither direction is the default, and what a wrong answer costs decides it.** A false failure
//! blocks a contributor who is not touching services - docker is a host dependency this repository
//! deliberately does not pin with nix. A false pass reports green having tested nothing, which is
//! the failure the whole tier exists to prevent.
//!
//! **So the signal is "somebody provisioned a tier here", and it is NOT the `CI` variable.** That
//! distinction was learned rather than designed: this module first read `CI`, on the reasoning that
//! CI is where a silent skip costs most. The reasoning was right and the signal was wrong. No CI job
//! provisions this tier - the nix sandbox has neither a network nor a docker socket, and the workflow
//! job that runs the suite never brings the services up - so `CI=true` made a missing tier fatal in
//! the one place its absence is expected, and it failed on the first push of the branch that added
//! it, in a step that had tested nothing needing docker.
//!
//! Only the job that provisions the tier knows that it did. So that job opts in by setting the
//! variable below and gets the fail-closed direction; everything else skips loudly and names what did
//! not run. **The limit, stated with the claim:** nothing here verifies that a job setting the
//! variable really did provision anything - it is a declaration, and a job that lies about it gets
//! the failure it asked for.

/// The variable that overrides the machine class, in **both** directions.
///
/// Named once, here, because a message that tells somebody to set it and a read that spells it
/// differently is a fix that does not work and looks like it should.
pub const FORCE: &str = "SUTURA_DEV_REQUIRE_DOCKER";

/// Whether a missing tier is fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// A missing tier FAILS. What a job that has PROVISIONED the tier asks for by setting
    /// [`FORCE`]: there, a green run that quietly tested nothing is the failure the whole tier
    /// exists to prevent.
    ///
    /// **Not implied by `CI`.** No CI job provisions the tier today, so keying on that variable made
    /// a missing tier fatal in the one place it is expected - see [`decide`].
    Required,
    /// A missing tier SKIPS, loudly, naming what did not run. The developer-machine direction.
    Optional,
}

impl Requirement {
    /// The direction this process is running under, read from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        decide(std::env::var(FORCE).ok().as_deref())
    }

    /// Is a missing tier fatal here?
    #[must_use]
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

/// The decision, over the value rather than over the environment, so it is testable.
///
/// **One parameter, and it used to be two.** The other was `CI`, and it is gone rather than ignored:
/// a parameter a function does not read is a parameter a caller believes in. See the module header for
/// why that signal was the wrong one.
#[must_use]
pub fn decide(forced: Option<&str>) -> Requirement {
    if let Some(value) = forced {
        return if truthy(value) {
            Requirement::Required
        } else {
            Requirement::Optional
        };
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
    fn an_absent_tier_skips_unless_a_job_says_it_provisioned_one() {
        // Both directions, because a fail-open/fail-closed decision with only one side tested is
        // half a decision - and this is the function that decides it for provisioning AND for the
        // harness, so a regression here is silent on both sides at once.
        assert_eq!(decide(None), Requirement::Optional);
        assert_eq!(decide(Some("1")), Requirement::Required);
    }

    // **There is deliberately no test that `CI` cannot make an absent tier fatal**, and the absence
    // is the point rather than an omission. That regression is real - keying on `CI` failed the
    // harness test in the `Test causality` step on this branch's first push, because no CI job
    // provisions the tier - and it is now **unrepresentable instead of checked**: [`decide`] has no
    // parameter the variable could arrive through, and [`Requirement::from_env`] does not read it.
    //
    // A test written for it would have to either loop over CI spellings while calling a function that
    // cannot see them - which asserts nothing while reading as coverage, the exact shape this
    // repository treats as worse than no test - or manipulate the process environment, which is racy
    // across a threaded test runner. The compiler holds this one.

    #[test]
    fn the_flag_overrides_the_machine_class_in_both_directions() {
        assert_eq!(decide(Some("1")), Requirement::Required);
        assert_eq!(decide(Some("0")), Requirement::Optional);
    }

    #[test]
    fn an_empty_or_negative_forced_value_does_not_require_a_tier() {
        for value in ["", " ", "0", "false", "no", "FALSE"] {
            assert!(!truthy(value), "`{value}` read as CI");
            // `truthy` still decides what a FORCED value means, which is the reading that survives.
            assert_eq!(decide(Some(value)), Requirement::Optional, "`{value}`");
        }
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(truthy(value), "`{value}` read as not-CI");
        }
    }
}

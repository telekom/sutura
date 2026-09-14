//! Whether a wall-clock assertion runs under CI's own margin, or a wider one for a shared machine.
//!
//! `telekom/sutura#140`: a deadline cell asserting `elapsed < budget + a_few_hundred_ms` reddened
//! three times in one evening, on branches that could not have touched the code it exercises - the
//! host was running several other lanes' builds at once. The margin itself is not wrong on an
//! isolated runner; it is wrong on a machine sharing its cores with work this repository's own
//! multi-lane workflow expects. So the number this module hands back depends on which of those two
//! machines is asking, and **not on how busy the machine actually is right now** - a bound keyed on
//! measured load gives the same tree a different verdict on different runs, which is worse than a
//! flaky cell because it stops being reproducible at all.
//!
//! **CI wins unconditionally, and everything unrecognised is CI too.** [`decide`] checks
//! `GITHUB_ACTIONS` first and returns [`Tolerance::Strict`] the moment it reads `"true"` - the exact
//! spelling `crates/sutura-exec-bigquery/tests/corpus.rs`'s own `ci_run_id` cell already holds
//! (`GITHUB_ACTIONS=false was read as CI` is asserted there as a failure, so `"true"` is the only
//! reading, not a truthy list). Only past that check does [`RELAXED`] get read, and an absent or
//! unrecognised environment - not exactly one of those two branches - falls through to
//! [`Tolerance::Strict`] as well: a venue this module has not been taught about is safer strict than
//! silently lenient.
//!
//! **`RELAXED` cannot reach the nix sandbox, and that is what makes the fallback do the work.**
//! `checks.nextest` in `flake.nix` builds this crate's tests inside a `nix build` derivation, which
//! is a pure evaluation with no `--impure` anywhere `nix build .#checks.*.nextest` is invoked in
//! this repository (`.github/workflows/ci.yml`, `justfile`, `nix/run-gate.sh` all read) - so
//! `builtins.getEnv` there always answers `""` and there is no way to thread the invoking shell's
//! `GITHUB_ACTIONS` into that build even when it IS a real GitHub Actions runner. The derivation
//! declares no [`RELAXED`] either, so a test running inside it sees neither variable and lands on
//! [`Tolerance::Strict`] by the same default this module already needs for an unrecognised venue -
//! which is exactly the leg the reported reds happened in. Only `justfile`'s `test` and `causality`
//! recipes export [`RELAXED`], because only the ordinary dev shell they run in is the crowded one:
//! `sutura_dev::requirement`'s own header states the parallel rule for a different variable - "only
//! the thing that provisions ... knows that it did", so it opts in, and nothing else does.

use std::time::Duration;

/// Set by `justfile`'s `test` and `causality` recipes, and nothing else - naming both callers
/// rather than leaving a reader to grep for a third.
pub const RELAXED: &str = "SUTURA_DEV_RELAXED_TOLERANCE";

/// Which wall-clock margin a deadline assertion should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tolerance {
    /// CI's own margin - an isolated runner, one job, no other lane's build sharing its cores.
    Strict,
    /// A wider margin for a shared machine, chosen so the assertion still fails a deadline that
    /// stopped enforcing entirely, and read - never measured - so the same tree gives the same
    /// verdict every time it runs here.
    Relaxed,
}

impl Tolerance {
    /// The venue this process is running in, read once.
    #[must_use]
    pub fn from_env() -> Self {
        decide(
            std::env::var("GITHUB_ACTIONS").ok().as_deref(),
            std::env::var(RELAXED).ok().as_deref(),
        )
    }

    /// Picks `strict` or `relaxed` for this venue, and announces the second choice on stderr - the
    /// `SKIPPED`-in-the-first-column convention `sutura_dev::provisioned` already uses for the same
    /// reason: a developer reading a passing run must not mistake a relaxed margin for CI's own.
    #[must_use]
    pub fn ceiling(self, strict: Duration, relaxed: Duration) -> Duration {
        match self {
            Self::Strict => strict,
            Self::Relaxed => {
                eprintln!(
                    "RELAXED - this cell's wall-clock ceiling is {relaxed:?} here, not the {strict:?} CI enforces \
                     ({RELAXED} is set)"
                );
                relaxed
            }
        }
    }
}

/// The decision, over the two values rather than over the environment.
///
/// Testable without mutating the process environment - the same reason
/// `sutura_dev::requirement::decide` takes a value rather than reading `std::env` itself.
///
/// **`GITHUB_ACTIONS` is checked before `RELAXED`, and wins.** A developer's shell exporting the
/// relaxed opt-in for a local run must not be able to widen CI's own margin if it were ever run
/// there directly - belt-and-suspenders over the fact that CI here only ever reaches these tests
/// through the nix sandbox, which sets neither variable.
#[must_use]
pub fn decide(github_actions: Option<&str>, relaxed_opt_in: Option<&str>) -> Tolerance {
    if github_actions == Some("true") {
        return Tolerance::Strict;
    }
    if relaxed_opt_in.is_some_and(|value| !crate::requirement::NOT_REQUIRED.contains(&value.trim().to_lowercase().as_str())) {
        return Tolerance::Relaxed;
    }
    Tolerance::Strict
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Tolerance, decide};

    #[test]
    fn an_unset_environment_is_strict() {
        assert_eq!(decide(None, None), Tolerance::Strict);
    }

    #[test]
    fn the_opt_in_alone_relaxes_it() {
        assert_eq!(decide(None, Some("1")), Tolerance::Relaxed);
    }

    /// **CI wins even carrying the opt-in** - the belt-and-suspenders case the module doc names: a
    /// stale `RELAXED` export left over from a local shell must not widen CI's own margin.
    #[test]
    fn ci_overrides_the_opt_in() {
        assert_eq!(decide(Some("true"), Some("1")), Tolerance::Strict);
    }

    /// `GITHUB_ACTIONS=false was read as CI` is the failure `corpus.rs`'s own cell names for the
    /// same exact-match reading; this is that same predicate misreading the OTHER direction: a
    /// falsy `GITHUB_ACTIONS` must not suppress a real opt-in either, since only the literal
    /// `"true"` means CI.
    #[test]
    fn a_falsy_ci_flag_does_not_shadow_the_opt_in() {
        assert_eq!(decide(Some("false"), Some("1")), Tolerance::Relaxed);
    }

    #[test]
    fn an_unrecognised_opt_in_value_stays_strict() {
        for value in ["", "0", "false", "no", "  ", "FALSE"] {
            assert_eq!(decide(None, Some(value)), Tolerance::Strict, "`{value}`");
        }
    }

    #[test]
    fn the_relaxed_ceiling_is_wider_and_the_strict_one_is_unchanged() {
        let strict = Duration::from_millis(500);
        let relaxed = Duration::from_millis(1500);
        assert_eq!(Tolerance::Strict.ceiling(strict, relaxed), strict);
        assert_eq!(Tolerance::Relaxed.ceiling(strict, relaxed), relaxed);
    }
}

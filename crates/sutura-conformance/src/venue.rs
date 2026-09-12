//! Whether this environment can stand a fixture up, and what a DECLARED absence costs.
//!
//! Split out of the crate root because that file reached the 1000-line ceiling `cargo xtask
//! max-lines` holds, and the rule this repository applies to a threshold lint applies to itself:
//! split the file rather than raise the number. The split is by TASK rather than by size - every
//! item here answers *is the thing this adapter needs even here*, and nothing here knows what a
//! behaviour is.
//!
//! # The distinction the whole module exists for
//!
//! [`crate::Outcome::Declined`] is a typed statement about the ADAPTER - *this adapter cannot do
//! that*. An absent tier is a statement about the VENUE. Collapsing the two would make a green run
//! over an absent Postgres indistinguishable from a green run against one, which is the failure
//! mode the packs were built against, so they are reported under different words and decided at
//! different levels: a declination comes out of a pack that RAN, and an absence stops the pack
//! running.
//!
//! # Which venue may skip is not this crate's decision, and a DECLARED absence is not free
//!
//! `sutura_dev::requirement` decides skip-or-fail once for every harness in this repository, from
//! [`REQUIRE_TIER`], and only the thing that provisioned a tier sets it - so an honest fixture in
//! such a venue never reports an absence at all, because `sutura_dev::provisioned::here` has
//! already failed the run. What CAN reach here is a fixture that answered [`Fixture::Absent`]
//! without asking, and [`absence_is_impossible`] is what makes that cost something.

/// An adapter's fixture, or the reason this venue could not stand one up.
///
/// **The type every binding's `open` path returns, and it is the mechanism rather than a
/// convention.** [`crate::execute_packs`] used to call `open` for a `W`, so an adapter whose data system
/// may not be reachable here had exactly one option - panic in its fixture - and therefore could
/// not be bound at all: `sutura-exec-postgres` was registered in the golden matrix and carried the
/// one declared exemption in `cargo xtask check-conformance-bindings` for precisely that reason
/// (`telekom/sutura#348`).
///
/// **What the return type buys, stated exactly, because the sentence that stood here read wider
/// than the mechanism.** It forces a VALUE, not a question: `Fixture::standing(connect().unwrap())`
/// asks nothing and PANICS, which is loud and fail-closed; `Fixture::Absent(Missing::tier(s, &".."))`
/// asks nothing and is silent in the two venues named in this module's header. So what a binding
/// cannot do is leave the two cases unconsidered - a fixture returning `W` does not compile - and
/// what it can still do is answer either one dishonestly. That is one line, in a file whose whole
/// content is a fixture and a declaration, and the diff is where it is read.
///
/// # Why this is not an [`crate::Outcome`], which is the distinction the design turns on
///
/// [`crate::Outcome::Declined`] is a statement about the ADAPTER - *this adapter cannot do that*, carrying
/// a typed [`crate::Declination`]. An absent tier is a statement about the ENVIRONMENT. Collapsing the two
/// would make a green run over an absent Postgres indistinguishable from a green run against one,
/// which is the failure mode the packs were built against. So the two are reported under different
/// words ([`crate::hold`] prints `DECLINED`, [`crate::not_here`] prints `NOT RUN`) and decided at different
/// levels: a declination comes out of a pack that RAN, and an absence stops the pack running.
///
/// # What it does NOT establish
///
/// See this module's header: nothing here can tell an absence that was DISCOVERED from one that was
/// merely declared, and the reason the harness cannot is a dependency rule that has its own gate.
pub enum Fixture<W> {
    /// It stood up. What an in-process or in-memory adapter always answers.
    Standing(W),
    /// The environment this adapter needs is not here, so nothing was asked of it.
    Absent(Missing),
}

impl<W> Fixture<W> {
    /// It stood up.
    ///
    /// Named rather than left to the variant, so an in-process binding's last line reads as the
    /// answer it is and the two answers are spelled at the same length.
    #[inline]
    pub const fn standing(warehouse: W) -> Self {
        Self::Standing(warehouse)
    }

    /// The reason it did not, where it did not.
    #[inline]
    pub const fn missing(&self) -> Option<&Missing> {
        match *self {
            Self::Standing(_) => None,
            Self::Absent(ref why) => Some(why),
        }
    }
}

/// Why this venue could not stand a fixture up. **About the environment, never about the adapter.**
///
/// Typed rather than a message, for the reason every refusal in this workspace is: a reader that
/// matched on the text would be depending on the text. One variant today - a second arrives with
/// the first adapter whose absence is not a tier, and cloud state a run cannot create is the shape
/// that asks for it. It arrives WITH that adapter rather than ahead of it, because a variant
/// nothing constructs is a claim nothing provokes, and this crate has paid for one of those already
/// ([`crate::Fault::EmptyCorpus`], which needed a seam before it was reachable at all).
#[derive(Debug, Clone, thiserror::Error)]
pub enum Missing {
    /// A service this adapter reaches over a socket, which nothing has provisioned here.
    #[error("no `{service}` is provisioned in this venue, so nothing was asked of this adapter - {diagnostic}")]
    Tier {
        /// The service the provisioner was asked for.
        service: String,
        /// The provisioner's own diagnostic, which names the worktree, the file it read and the
        /// task that starts the tier.
        diagnostic: String,
    },
}

impl Missing {
    /// A tier this venue has not provisioned, carrying the provisioner's own diagnostic.
    ///
    /// The diagnostic arrives as a `Display` rather than as a `String`, so what reaches a reader is
    /// the sentence the provisioner wrote rather than one a binding composed beside it. That
    /// remedy is derived per venue and three checks hold it; a binding restating it would be a
    /// fourth copy with no mechanism.
    #[must_use]
    pub fn tier(service: &str, diagnostic: &impl core::fmt::Display) -> Self {
        Self::Tier {
            service: String::from(service),
            diagnostic: diagnostic.to_string(),
        }
    }
}

/// The variable a provisioner sets when it has brought a tier up, spelled here as well.
///
/// **`sutura_dev::requirement::FORCE`'s name, duplicated, and the duplication is PINNED rather than
/// hoped about.** This crate may not take `sutura-dev` through a normal dependency -
/// `xtask/src/boundaries/harness.rs` holds it to `sutura-domain` alone - so the name and its
/// truthiness are spelled twice, and two statements about one fact can disagree.
/// `tests/bound.rs`'s `the_requirement_this_harness_reads_is_the_one_the_provisioner_writes` is the
/// mechanism that keeps them equal: it takes `sutura-dev` as a DEV-dependency, which that gate
/// permits by design (what may not happen is a pack BODY compiled against something, and a pack
/// body is `src/`), and compares both halves against `FORCE` and `requirement::decide`.
pub const REQUIRE_TIER: &str = "SUTURA_DEV_REQUIRE_TIER";

/// What this venue declared about tiers, read from the environment.
///
/// **The only environment read in this crate, and everything below it takes the VALUE.** That is
/// what makes the reporters testable at all, and it was measured rather than reasoned about: with
/// the read inside `not_here` and `census`, `just validate` refused two of THIS crate's own cells -
/// `checks.nextest` provisions the Postgres tier and sets the variable, and a fake absence in a
/// fake venue is indistinguishable from a fabricated one. `unsafe_code` is `forbid` across this
/// workspace and `std::env::set_var` is unsafe on Rust 2024, so no test can turn it off either. So
/// the macro reads it once per cell and hands it down, which is also the shape
/// `sutura_dev::requirement::decide` chose for the same reason.
#[must_use]
pub fn declared_here() -> Option<String> {
    std::env::var(REQUIRE_TIER).ok()
}

/// Whether an absent tier is a failure here, decided over the VALUE rather than the environment.
///
/// Over the value for the reason `sutura_dev::requirement::decide` is: an environment read is not
/// testable across a threaded runner, and this is the half a test has to be able to compare.
///
/// **The falsy spellings are a COPY and the owner is `sutura_dev::requirement::NOT_REQUIRED`**,
/// because that crate cannot be reached from here through a normal dependency. The copy is not
/// held by the eye: `tests/bound.rs` iterates the owner's list, so a spelling added there fails
/// this crate's own cell until this line agrees. Review found the version before that - a fixed
/// array of eleven values chosen HERE - and named the scenario: add `"off"`, the obvious next
/// spelling for a variable people set by hand, and `SUTURA_DEV_REQUIRE_TIER=off` means *optional*
/// to `provisioned::here`, which skips, and *required* here, which then refuses the absence that
/// skip produced.
#[must_use]
pub fn a_tier_is_required(forced: Option<&str>) -> bool {
    forced.is_some_and(|value| !matches!(value.trim().to_lowercase().as_str(), "" | "0" | "false" | "no"))
}

/// Whether a DECLARED absence is a defect here rather than a skip.
///
/// **Pure, over the value, because the alternative is not available and would be wrong anyway.**
/// `unsafe_code` is `forbid` across this workspace and `std::env::set_var` is `unsafe` on Rust
/// 2024, so a test cannot manipulate the environment here at all - and
/// `sutura_dev::requirement`'s own tests refuse to do it for the second reason, which is that it
/// races across a threaded runner. So the decision is a value every caller passes down from
/// [`declared_here`], which is what lets `tests/bound.rs` provoke the refusal end to end, message
/// included, in both endings and in either direction.
///
/// **An exhaustive `match` and not a `matches!`, and the difference is the whole of this claim.**
/// [`REQUIRE_TIER`] is a statement about TIERS, so the variant that arrives for cloud state a run
/// cannot create has to decide its own direction - and a `matches!` gave it one by omission:
/// `false`, silently, with `cargo check --all-features` exit 0. That is this branch's own hole
/// reopened one adapter later and inside the venue this crate says is closed - a fixture answering
/// `Absent(Cloud)` without asking anything, in `checks.nextest`, which sets the variable. Measured
/// with the refusal absent: every test passed, and the only tell was printed lines nobody diffs.
///
/// With the `match` a new variant does not compile until somebody writes its arm, so the fail-open
/// direction cannot be chosen by not looking. **No test asserts that and none can** - a compile
/// error is not an outcome libtest has - so the evidence is the mutation, re-taken on 2026-09-06:
/// adding a `Missing::Cloud` variant made `just lint` fail with
/// `E0004` - a pattern for the new variant not covered - at this arm, where the same mutation
/// against the `matches!` version was exit 0.
#[must_use]
pub fn absence_is_impossible(missing: &Missing, forced: Option<&str>) -> bool {
    match *missing {
        Missing::Tier { .. } => a_tier_is_required(forced),
    }
}

/// The refusal both absent endings share: a DECLARED absence where a venue provisioned a tier.
///
/// **One function rather than one per caller, because it is one decision.** [`crate::not_here`] ends a
/// behaviour cell and [`crate::census`] ends the binding's own cell, and a refusal written in only one of
/// them left the other green - measured at 6 of 7 failing before this was factored out, with the
/// binding's own census the cell that passed over a fabricated absence.
///
/// `what` is the behaviour's name, or what the census is, so a failure says which cell refused.
pub(crate) fn refuse_a_declared_absence(adapter: &str, what: &str, missing: &Missing, declared: Option<&str>) {
    // **The refusal that makes a DECLARED absence cost something.** [`Fixture`] is a value a
    // binding fills in and this crate cannot see a socket, so a fixture that answered `Absent`
    // without looking would take its whole tier quiet and green - measured, with the tier UP and
    // this variable set before this existed: everything passed, and the only tell was printed lines.
    // What closes it is the one fact a venue does publish: only the thing that provisioned a tier
    // sets `REQUIRE_TIER`, so where it is set an absent tier is impossible and a fixture claiming
    // one is the defect. An honest fixture never reaches here in that venue anyway -
    // `sutura_dev::provisioned::here` has already failed the run.
    assert!(
        !absence_is_impossible(missing, declared),
        "conformance {adapter}: {what} - this venue set {REQUIRE_TIER}, so it provisioned a tier \
         and an absent one is not possible here: {missing}. A fixture reporting an absence in this \
         venue has not asked the provisioner - only the thing that brought a tier up sets that \
         variable, and `sutura_dev::provisioned::here` fails the run itself where a tier it looked \
         for is genuinely gone"
    );
}

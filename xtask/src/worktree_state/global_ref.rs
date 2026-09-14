//! A repository-GLOBAL git ref is not a worktree's own state.
//!
//! `github.com/telekom/sutura#405` is about state a second checkout on the same machine also
//! reaches. `super::scan` reads the filesystem half of that. This is the git half, and it is one
//! instance: **`refs/stash` is one ref per repository, and a linked worktree does not get its
//! own.** Two agents in two worktrees of this tree push onto one stack, so a pop takes whichever
//! entry is on top - measured 2026-09-13, one lane's pop taking a sibling's entry. The remedy is a
//! patch file under the worktree, which is state the worktree does own.
//!
//! # What this holds, and the larger half it does not
//!
//! It holds that **no file in scope instructs the operation.** That is the whole of what a
//! repository can hold here, and claiming more would itself be the defect this repository cares
//! about: git has no hook on a stash - `reference-transaction` fires inside the repository, after
//! the fact, not on a reader typing a command - so nothing in this tree reaches an agent's shell.
//! **The measured collision came from a command typed at a prompt, and this rule would not have
//! stopped it.** What it stops is this repository being the thing that told them to, which is the
//! half that was held by recall and had a live violation: a doc comment on
//! `crates/sutura-sql/tests/adversarial_findings.rs`'s seventh finding handed a reader the shared
//! stack as the way to reproduce an abort, and it carried a bare `cargo` line into the bargain.
//!
//! # Scope is `super::scan::language_of`'s, not a second opinion
//!
//! Same predicate, same census pass, so this arm adds no walk, no count and no anchor - a number
//! nobody prints cannot be wrong, and the anchor set already names one subject per arm of the one
//! predicate both rules share. **Its consequence, stated rather than implied:** markdown is out,
//! so `.agents/` prose may name the operation in order to warn about it, and only first-party
//! `.rs` plus `nix/*.sh` and `nix/*-tier.nix` are judged.
//!
//! # And it reads text, not intent
//!
//! A line mentioning the operation reads the same as a line instructing it. That direction is the
//! safe one for a rule whose scope excludes the prose that would want to discuss it, and the
//! needle is assembled from parts for the reason [`instructed`] gives.
//!
//! **The other direction is the one to distrust: one literal, so anything that is not that literal
//! passes.** A paraphrase, a second space, or the same operation reached through a flag this needle
//! does not spell are all invisible here, and no widening fixes that - a matcher over intent is not
//! a thing a text scan has. So this rule catches the sentence somebody actually wrote once, and is
//! worth exactly that; `telekom/sutura#670` is the same shape one level out, where a register over
//! prose is defeated by rewording and, per that issue, does not reach `.rs` at all. That second
//! half is why this arm lives here rather than as an entry in that register - the doc comment it
//! refuses was in a file the register does not select. Read the selector for today's answer rather
//! than a copy of it here, which would rot the moment #670 is closed.

use super::scan;

/// The needle, assembled from parts the way `super::scan::rust_root` assembles its own.
///
/// This file states the rule, so a plain literal here would make the rule's own reasoning its
/// first violation - and an exemption list is a second thing to keep true. The cost is that
/// grepping this tree for the phrase does not find the gate that refuses it; the module name and
/// this header are what a reader finds instead.
fn instructed() -> String {
    format!("{} {}", "git", concat!("st", "ash"))
}

/// How many instructions `text` OFFERS, by its own expression over the whole text.
///
/// The other side of a conservation law, and the reason this arm has one at all: it prints no
/// count, so deleting its one line in [`super::run`] would leave the rule reaching nothing with
/// every number in the verdict still agreeing. `super::Inspected`'s constructor refuses a mismatch,
/// which is the shape `scan::offered` and `scan::takings` already form. **What it catches is a
/// NARROWED loop - a dropped `extend`, a `take(n)` - and not a wrong needle**, because both sides
/// read the same needle; that half is held by this module's own tests instead.
///
/// **A removal of the wiring itself is held by a LINT rather than by this law**, and that is worth
/// knowing because no floor could do it: the takings arm fails closed on an empty scan because this
/// workspace really holds dozens, while a CORRECT tree offers zero instructions, so the same device
/// would refuse every clean run. Measured - deleting the `extend` in [`super::run`] does not
/// compile: `error: function instructed_lines is never used`, from the workspace's deny-level
/// `dead_code`. What stays outside every mechanism is deleting the arm and this module together,
/// which is a visible diff rather than a quiet one.
pub(super) fn offered(text: &str) -> usize {
    text.matches(&instructed()).count()
}

/// The 1-based line of every instruction in `text`, one entry per OCCURRENCE so that
/// [`offered`] counts the same unit this returns.
pub(super) fn instructed_lines(text: &str) -> Vec<usize> {
    let needle = instructed();
    text.match_indices(&needle).map(|(at, _)| scan::line_of(text, at)).collect()
}

#[cfg(test)]
mod tests {
    use super::{instructed_lines, offered};

    /// The fixture spells the needle from DIFFERENT parts than [`super::instructed`] does, so the
    /// two spellings have to agree - and neither leaves the contiguous phrase in this file, which
    /// is what `this_rules_own_reasoning_is_not_its_first_violation` reads.
    #[test]
    fn an_instruction_to_operate_on_the_repositorys_shared_stack_is_found() {
        let text = format!(
            "fine\n/// Reproduce it: `{} && just test`.\nalso fine\n",
            concat!("git ", "stash")
        );
        assert_eq!(instructed_lines(&text), vec![2], "the line a reader would open");
    }

    /// [`offered`] is the other side of [`super::super::Inspected::of`]'s conservation law, and
    /// every caller in `worktree_state.rs`'s own tests passes it a literal that already agrees
    /// with `instructed.len()` - so a narrowed `offered` (`.count().saturating_mul(0)`, reads
    /// kept) never disagreed with anything there. This calls it directly over text carrying two
    /// occurrences, which a narrowing cannot pass by accident.
    #[test]
    fn offered_counts_by_its_own_expression_not_by_agreement_with_a_caller() {
        let text = format!("one: {stash}\ntwo: {stash}\n", stash = concat!("git ", "stash"));
        assert_eq!(
            offered(&text),
            2,
            "two occurrences, counted independently of instructed_lines"
        );
    }

    #[test]
    fn a_file_that_instructs_nothing_offers_no_line() {
        assert_eq!(
            instructed_lines("keep the revert in a patch file under this worktree\n"),
            Vec::<usize>::new()
        );
    }

    /// The assembly in [`super::instructed`] earns its keep or this file is its own violation.
    #[test]
    fn this_rules_own_reasoning_is_not_its_first_violation() {
        assert!(
            instructed_lines(include_str!("global_ref.rs")).is_empty(),
            "the needle is assembled from parts precisely so this holds"
        );
    }
}

//! The tree every registered hygiene gate has to refuse, and the argument for its shape.
//!
//! `github.com/telekom/sutura#371`. The defect that issue collects is a gate whose HELPERS are
//! tested while `run` and the exit code it produces are driven by nothing, so a mutation on the
//! verdict path leaves the suite green. Measured on this tree before the falsifier existed: 2 of
//! 31 hygiene gates had a test driving the registered entry point's verdict, and 7 of 31 answered
//! `ok` over a tree with none of their subjects. The test that uses this lives in
//! `crate::tests`, next to the `TASKS` table it reads.
//!
//! **Both root markers are load-bearing.** [`crate::repo::root`] identifies a root by `flake.nix`
//! AND `Cargo.toml`; without them its walk falls through to `CARGO_MANIFEST_DIR`'s parent and
//! every gate reads this repository, which is the one tree that makes the assertion vacuous.
//!
//! **The seeded files are for the gates whose subject is a KIND OF FILE**, where absence is a
//! legitimate pass: a tree with no text file genuinely has no over-long file and no CRLF, so only a
//! violation falsifies them. Measured - without the seed, `max-lines`, `line-endings` and
//! `text-hygiene` all answer `ok` here, and every other gate is falsified by the bare root alone.
//! No extension here is `.rs`, deliberately: `check-expect-thresholds` anchors its floor on
//! `xtask/src/main.rs` (see `threshold_expect`), which no falsifier tree can contain, so a
//! foreign `.rs` would no longer buy it a satisfied floor - the absent anchor still refuses it
//! (`Refusal::NotJudged`). And `check-worktree-state` is the reason `nix/shared-scratch.sh`
//! exists rather than a Rust file.
//!
//! **A seed is what makes a refusal come from a gate's OWN RULE rather than from a missing input,
//! and only five of the gates here manage that.** Measured on `d26814e2` plus this commit by
//! running every registered hygiene gate inside a reconstruction of this tree and classifying its
//! first line: **23 refuse on an absent or unreadable input, 9 on an empty-scan floor, and 5 on
//! their own rule** - `max-lines`, `line-endings` and `text-hygiene` off the seeded text files,
//! `check-worktree-state` off the shell script, and `check-nix-platform` off `nix/platform.nix`.
//!
//! **THOSE THREE NUMBERS ARE A MEASUREMENT AND NOTHING EXECUTES THEM, which is the residue
//! `telekom/sutura#371` names.** The sentence they replace said 20 / 8 / 3 over 31 gates and was
//! wrong in every figure by the time it was read: all 37 gates refuse, so the test below stayed
//! green for the whole time the split was stale. A gate that stops refusing on its own rule and
//! starts refusing on a missing input is a weaker gate and moves nothing here. `AGENTS.md` prefers
//! a check to a sentence and this is still a sentence: classifying a reason mechanically means
//! capturing each gate's own output in-process, which is a design question rather than a one-liner.
//! **So treat the split as of its commit and re-measure rather than citing it.**
//!
//! `telekom/sutura#405` asks its own gate to be one of them, so the shell script below carries a
//! real violation - an unkeyed path under the machine's temporary root - and
//! `check-worktree-state` refuses it by name, with a line number, having found and adjudicated one
//! taking. The file is a `.sh` under `nix/` because that is a scope no other gate in the sweep
//! reads for content, so it falsifies exactly one gate.
//!
//! **Why this module exists at all, rather than sitting in `main.rs`:** that file hit the
//! 1000-line cap on the merge of two branches that both grew it, and `sutura/gates` says to move
//! the HARNESS and keep every `#[test]` where it is. It carries a test of its own for the reason
//! that same page gives: a file adding no `#[test]` is revertible, so the base tree would delete
//! it while `main.rs` - held for its tests - still declared the module, and `E0583` would turn a
//! real causal verdict into `INCONCLUSIVE`.
//!
//! What the mechanism does NOT hold is the `sutura/invariants` row's third column; the short form
//! is that a refusal says nothing about WHY.

use std::path::PathBuf;

/// A repository root that is not this repository, seeded to be adversarial to every gate.
///
/// Keyed on the process id and REMOVED FIRST, because a pid is reusable: a directory left behind
/// by an earlier run would otherwise seed files this tree never declared into a scan whose whole
/// point is that the tree is known. That is not hypothetical - `telekom/sutura#328` is a sibling
/// test failing exactly that way on inherited state.
pub(crate) fn falsifier_tree() -> PathBuf {
    let root = std::env::temp_dir().join(format!("sutura-falsifier-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("a scratch root");
    std::fs::write(root.join("flake.nix"), "{ }\n").expect("the first root marker");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("the second root marker");
    // Over the 1000-line cap and clean in every other way, so `max-lines` is the only gate this
    // file is about.
    let mut over_long = String::new();
    for _ in 0..1001_u16 {
        over_long.push_str("a line\n");
    }
    std::fs::write(root.join("over-long.txt"), over_long).expect("the over-long file");
    // CRLF for `line-endings`; the trailing space and the missing final newline for
    // `text-hygiene`, whose rules are neither of the other two.
    let malformed = "a line with a trailing space \r\nand no final newline";
    std::fs::write(root.join("carriage-return.txt"), malformed).expect("the malformed text file");
    // A gate script writing to a path every checkout on the machine reaches, which is
    // `check-worktree-state`'s subject and `telekom/sutura#405`'s instance 2 in shape. Clean in
    // every other way - LF, a final newline, no trailing space, well under the line cap - so this
    // file falsifies one gate and tells the other three nothing.
    std::fs::create_dir_all(root.join("nix")).expect("the shell scope");
    // A platform predicate read off the deprecated `stdenv` alias, which is `check-nix-platform`'s
    // own rule. Seeded rather than left to the bare root FOR THE REASON THIS MODULE'S HEADER
    // GIVES: `flake.nix` is a `.nix` file, so that gate's scan is neither empty nor missing its
    // anchor here, and without a violation its refusal would have to come from an input it could
    // not read - which is the weaker of the two and the residue `telekom/sutura#371` names. Clean
    // in every other way - LF, a final newline, no trailing space, four lines - so it falsifies
    // exactly one gate and tells the others nothing.
    std::fs::write(
        root.join("nix/platform.nix"),
        "{ pkgs }:\n{\n  buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];\n}\n",
    )
    .expect("the deprecated platform predicate");
    std::fs::write(
        root.join("nix/shared-scratch.sh"),
        "#!/usr/bin/env bash\nlog=\"/tmp/sutura-gate-state.log\"\necho hi >\"$log\"\n",
    )
    .expect("the unkeyed shared path");
    root
}

#[cfg(test)]
mod tests {
    use super::falsifier_tree;

    #[test]
    fn a_stale_tree_from_an_earlier_run_is_not_inherited() {
        // THE PROPERTY THE CONSTRUCTOR'S FIRST LINE EXISTS FOR, and the one a reader would not
        // think to check: the directory name carries a process id, process ids are reused, and a
        // run that panicked before its cleanup leaves the tree behind. Inheriting it would put a
        // file no gate's verdict is about into a tree whose whole argument is that its contents
        // are known - which is how `telekom/sutura#328`'s sibling flake behaves, measured.
        let root = falsifier_tree();
        std::fs::write(root.join("left-behind.md"), "from a previous run\n").expect("a stray file");

        let again = falsifier_tree();
        let stray = again.join("left-behind.md").exists();
        let seeded = again.join("over-long.txt").exists();
        drop(std::fs::remove_dir_all(&again));

        assert!(
            !stray,
            "the constructor inherited a file an earlier run left in {}",
            again.display()
        );
        assert!(seeded, "the constructor removed the tree and did not seed it again");
    }
}

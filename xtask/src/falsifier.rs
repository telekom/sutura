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
//! No extension here is `.rs`, deliberately: `check-expect-thresholds` scans Rust source, so a
//! `.rs` file would satisfy its floor while telling it nothing about this tree - and
//! `check-worktree-state` is the reason `nix/shared-scratch.sh` exists rather than a Rust file.
//!
//! **A seed is what makes a refusal come from a gate's OWN RULE rather than from a missing input,
//! and only three of the gates here manage that.** Over this tree 20 refuse on an absent or
//! unreadable input and 8 on an empty-scan floor; `telekom/sutura#405` asks its own gate to be one
//! of the three, so the shell script below carries a real violation - an unkeyed path under the
//! machine's temporary root - and `check-worktree-state` refuses it by name, with a line number,
//! having found and adjudicated one taking. The file is a `.sh` under `nix/` because that is a
//! scope no other gate in the sweep reads for content, so it falsifies exactly one gate.
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

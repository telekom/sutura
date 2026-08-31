# Can every file's licence be answered without reading prose?
#
# ITS OWN FILE for the reason `nix/oci.nix`, `nix/mimalloc.nix` and `nix/auditable.nix` give:
# `flake.nix` is under the same 1000-line cap `cargo xtask max-lines` enforces on everything else,
# and it reached 1014 with this inline. What moves is the DERIVATION; the `checks.reuse` and
# `apps.reuse` declarations stay in `flake.nix`, because `xtask/src/pins.rs` and
# `xtask/src/workflows.rs` scan that file textually for them and both fail closed on finding none.
#
# WHAT THIS IS FOR. `reuse lint` reads `REUSE.toml` and any per-file SPDX headers and answers, per
# file, which licence governs it. `LICENSE` and `VENDOR.md` already say what governs what, and both
# are prose - unreadable by a tool, and unavailable to somebody who received one file rather than
# the repository. This is the mechanical form of the same statement.
#
# **WHAT IT DOES NOT CATCH, and this is not a small caveat - it is the direct consequence of the
# shape `REUSE.toml` is written in.** That file opens with a `path = "**"` catch-all, so EVERY file
# resolves to Apache-2.0 unless a later block narrows it. A new file therefore cannot be
# unattributed, which means this check cannot fail on one - and more importantly, **a newly vendored
# third-party file is silently attributed to us** until somebody adds a block for it. An earlier
# version of this comment claimed the opposite - that the check "stops a new file being attributed
# by nobody" - and that was false in the way this repository treats as a defect in itself.
#
# WHAT IT DOES CATCH, verified by breaking each one rather than by reading the manual: a licence
# referenced with no text under `LICENSES/` (removing `LICENSES/MIT.txt` reports every mirrored
# skill under `# MISSING LICENSES`), a licence text nothing references, a deprecated SPDX
# identifier, and a malformed expression. So it holds the DECLARATION complete and well formed, and
# it does not police what the declaration says.
#
# No cheap sound mechanism closes the remaining half, and it is worth saying why rather than leaving
# a TODO. The obvious one - every path `VENDOR.md` names must have its own block - is WRONG: of the
# ten paths that file lists, most are recorded as *"Rewritten, not copied"*, so Apache-2.0 is the
# correct answer for them and a gate demanding a foreign licence would fail on correct code. Only
# the mirrored skills and the two vendored allocator trees are true copies. Telling those apart is
# what `VENDOR.md` is for, and it is a review question.
{ pkgs, src }:

{
  # The tool, so `apps.reuse` and this check cannot resolve to two different versions.
  tool = pkgs.reuse;

  # NOT A CRANE DERIVATION, and it is the only check in this repository that is not. It runs no
  # cargo and reads no Rust: hanging it off `ciArgs` would make a seconds-long text check wait for
  # the whole dependency closure, and a gate that waits for DataFusion to compile is a gate somebody
  # eventually moves out of the inner loop. `runCommand` keeps it cheap enough that its cost is
  # never the argument.
  #
  # The WHOLE tree, and here that is load-bearing rather than a detail: this gate's entire job is to
  # judge EVERY file, and crane's filter keeps only Cargo inputs - so against the filtered source it
  # would pass having checked a fraction of the tree. There is no `.git` in the sandbox, which is
  # fine because `reuse` walks the filesystem; and the flake source is git-derived, so an untracked
  # file is invisible to it. That last part cuts both ways and is worth knowing: a file added and
  # not staged is not checked here, which is the same `git add -N` caveat every nix check has.
  check = pkgs.runCommand "sutura-reuse"
    {
      inherit src;
      nativeBuildInputs = [ pkgs.reuse ];
    } ''
    cd $src
    reuse lint
    touch $out
  '';
}

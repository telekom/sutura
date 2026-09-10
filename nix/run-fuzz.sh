#!/usr/bin/env bash
# The fuzz runner. `just fuzz` and `just fuzz-smoke` both come through here.
#
# ITS OWN FILE for `nix/lint-workflows.sh`'s first reason and its second one: two callers rather
# than two copies, and the justfile is within a few lines of the 1000-line cap `cargo xtask
# max-lines` enforces - which the inline version could cross on its own.
#
# The target list is DERIVED from `fuzz/fuzz_targets/*.rs` rather than written here, so a target
# arriving or disappearing changes what runs without anybody editing a list. `cargo xtask
# check-fuzz` is what holds that derivation against the manifest, the seeds and the workflow.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

targets=()
for file in fuzz/fuzz_targets/*.rs; do
  targets+=("$(basename "$file" .rs)")
done
if [ "${#targets[@]}" -eq 0 ]; then
  echo "run-fuzz: no target in fuzz/fuzz_targets - refusing to report a green run over nothing" >&2
  exit 1
fi

mode="${1:-run}"
seconds="${2:-300}"
only="${3:-}"

if [ -n "$only" ]; then
  targets=("$only")
fi

# `cargo fuzz` builds with one codegen unit by default, and that is the single largest cost in a
# cold harness build: 448s cold for `sql_expression` on a developer machine (re-run 436s) against
# 276s at sixteen - 172s / 38% - measured 2026-09-10.
#
# WHY THE FLAG AND NOT `fuzz/Cargo.toml`'s `[profile.release]`, which is where a reader reaches
# first: that route is inert, and silently so. cargo-fuzz composes a RUSTFLAGS of its own ending in
# `-Ccodegen-units=<n>`, and cargo appends RUSTFLAGS to the rustc line AFTER every flag it derives
# from the profile - both were read off a verbose build's own rustc line, the profile's value early
# and cargo-fuzz's late. rustc takes the last, so a manifest value is emitted and then overridden.
#
# Appending to RUSTFLAGS in `nix/fuzz.nix` does work, for the mirror-image reason: the
# environment's copy lands after cargo-fuzz's own, so the pair reads `-Ccodegen-units=1
# -Ccodegen-units=16` and the second wins. But it wins by ORDERING inside a composition cargo-fuzz
# does not document, and a bump that reversed it would put the build time back with nothing going
# red. The flag is the documented surface, emits one token instead of two, and would fail the build
# loudly rather than quietly if it were ever withdrawn.
#
# All three were read the cheap way, without fuzzing anything: cargo-fuzz prints the RUSTFLAGS and
# the cargo line it composed in its own build-failure message, so naming a target that does not
# exist shows what a real run would have used.
#
# THE COST IS NOT MEASURED. Sixteen units means less cross-unit optimisation, so some fuzzing
# throughput is expected to be traded for the build time - cargo-fuzz's own help for this flag says
# "faster fuzz builds at the cost of somewhat slower fuzz runs". This tree has no trustworthy
# exec/s pair to put a figure on it: the baseline arm of the only comparison run so far was
# CPU-starved by a concurrent test run, so the number is still owed.
codegen_units="--codegen-units=16"

for target in "${targets[@]}"; do
  case "$mode" in
    smoke)
      # `-runs=0` loads the corpus, executes every seed once, and exits. No mutation, so the
      # verdict is a function of the committed files: this is the regression half of fuzzing, and
      # the only half that belongs anywhere near a gate.
      echo "run-fuzz: replaying the committed seeds for $target"
      nix run .#fuzz -- run "$codegen_units" "$target" "fuzz/seeds/$target" -- -runs=0
      ;;
    run)
      echo "run-fuzz: fuzzing $target for ${seconds}s"
      dict=(-dict="fuzz/dictionaries/$target.dict")
      # A dictionary is an optimisation, not a requirement: libFuzzer refuses to start on a
      # `-dict=` naming no file, so a target without one runs without one rather than failing.
      if [ ! -f "fuzz/dictionaries/$target.dict" ]; then
        dict=()
      fi
      # Two corpus directories: the first is libFuzzer's WRITABLE working corpus, which is
      # gitignored because it grows by thousands of generated files, and the second is the tracked
      # seed set it reads and never writes. `fuzz/.gitignore` argues the split.
      mkdir -p "fuzz/corpus/$target"
      nix run .#fuzz -- run "$codegen_units" "$target" "fuzz/corpus/$target" "fuzz/seeds/$target" -- \
        -max_total_time="$seconds" -max_len=8192 -print_final_stats=1 "${dict[@]}"
      ;;
    *)
      echo "run-fuzz: unknown mode '$mode' (expected 'run' or 'smoke')" >&2
      exit 2
      ;;
  esac
done

#!/usr/bin/env bash
# The fuzz runner. `just fuzz` and `just fuzz-smoke` both come through here.
#
# ITS OWN FILE for `nix/lint-workflows.sh`'s first reason and its second one: two callers rather
# than two copies, and the justfile is 12 lines under the 1000-line cap `cargo xtask max-lines`
# enforces - which the inline version would have crossed on its own.
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

for target in "${targets[@]}"; do
  case "$mode" in
    smoke)
      # `-runs=0` loads the corpus, executes every seed once, and exits. No mutation, so the
      # verdict is a function of the committed files: this is the regression half of fuzzing, and
      # the only half that belongs anywhere near a gate.
      echo "run-fuzz: replaying the committed seeds for $target"
      # The TRACKED seeds only, and `-runs=0` executes each once and exits without mutating - so
      # the verdict is a function of files somebody read, and nothing is written back.
      nix run .#fuzz -- run "$target" "fuzz/seeds/$target" -- -runs=0
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
      nix run .#fuzz -- run "$target" "fuzz/corpus/$target" "fuzz/seeds/$target" -- \
        -max_total_time="$seconds" -max_len=8192 -print_final_stats=1 "${dict[@]}"
      ;;
    *)
      echo "run-fuzz: unknown mode '$mode' (expected 'run' or 'smoke')" >&2
      exit 2
      ;;
  esac
done

#!/usr/bin/env bash
# ASSERT that a link check's output is an executable for the CPU the triple names.
#
# **`file` is a READOUT, not an assertion, and both link steps used it as one.** Measured:
# `file /nonexistent-path-xyz` prints ``cannot open `/nonexistent-path-xyz'`` and exits **0**. So a
# step whose only check beside `nix build` is `file` stays green when the executable it names is
# absent or misnamed - and the feature-probe step reads that name out of a manifest, which is
# exactly the field a rename would change. The comment that stood above the shipped step -
# *"Proves the arch, not just the exit code: a linker that silently produced a host binary would
# pass a bare build"* - was true of the bare build and equally untrue of the `file` beside it,
# because nothing compared `file`'s answer to anything.
#
# Two things are asserted here, and the readout is still printed because a human reading the log
# wants the whole line:
#
#   * the path EXISTS and is executable. `nix build` succeeding says the derivation built, not that
#     it installed a `bin/` entry under the name the caller expects.
#   * the CPU `file` reports is the CPU the triple names - the claim the old comment made.
#
# **Two spellings per family, because the object format decides which one `file` uses:** ELF says
# `ARM aarch64` and `x86-64`, Mach-O says `arm64` (or `arm64e`) and `x86_64`. Found by running the
# happy path before trusting it - the first version of this script accepted the ELF spellings only
# and went RED on a correct host binary, which is the failure mode a gate must not have.
#
# **What is deliberately NOT asserted, so nobody reads more into a green than is here.** The libc
# half: `statically linked`, `static-pie linked` and `dynamically linked` vary with linker, target
# and `file` version in ways nobody here has measured, and a gate that fails on a correct tree gets
# disabled - so a musl target linked dynamically passes this. And a Mach-O UNIVERSAL binary names
# every architecture it carries, so it would satisfy either family; no build in this repository
# produces one, and cargo cannot without `lipo`.
#
# A shared script rather than two copies for the reason `nix/lint-workflows.sh` and
# `nix/run-gate.sh` are: `ci.yml` is against a live 1000-line cap, and a second copy of an
# assertion is a second thing to keep true. `check-workflows` follows `nix/*.sh`, so a flake
# reference moved in here stays in that gate's sight.
#
# Usage: assert-linked.sh <rust-triple> <path-to-executable>
set -euo pipefail

triple="${1:?usage: assert-linked.sh <rust-triple> <path>}"
path="${2:?usage: assert-linked.sh <rust-triple> <path>}"

case "$triple" in
  aarch64-*) want='ARM aarch64|arm64' ;;
  x86_64-*) want='x86-64|x86_64' ;;
  # FAIL CLOSED. A triple nobody has written an expectation for is not a pass: adding one is a
  # line here, and passing without asserting anything is how a link check stops being one.
  *)
    echo "::error::${triple}: nix/assert-linked.sh has no expected architecture for this triple. Add one - a link check that cannot say what it expected has not checked the link."
    exit 1
    ;;
esac

if [ ! -x "$path" ]; then
  echo "::error::${triple}: ${path} is not an executable file, so nothing was installed under the name this step was told to look for. \`file\` alone prints \`cannot open\` here and exits zero."
  exit 1
fi

reported="$(file "$path")"
echo "$reported"
if [[ ! $reported =~ $want ]]; then
  echo "::error::${triple}: ${path} reports none of ${want}, so the link produced a binary for another CPU. \`file\` said: ${reported}"
  exit 1
fi

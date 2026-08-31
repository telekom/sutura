# The shipped binary carries its own dependency list.
#
# ITS OWN FILE for the reason `nix/oci.nix` and `nix/mimalloc.nix` give: `flake.nix` is under the
# same 1000-line cap `cargo xtask max-lines` enforces on everything else, and it reached 982 with
# this inline. Nothing that a text-scanning gate reads moves - `apps.<name>`, the `packages = `
# block and the `checks = {` block all stay in `flake.nix`; what moves is two helpers those call.
#
# WHAT THIS DOES. `cargo auditable build` compiles exactly what `cargo build` compiles and adds one
# section to the ELF: the crate names and versions the binary was actually built from, compressed.
# `syft` reads it back out with its `cargo-auditable-binary-cataloger`, so the image SBOM the
# release path already generates becomes a real dependency inventory rather than a list of three
# files.
#
# WHY EMBEDDED RATHER THAN A SIDECAR, and it is this repository's own rule applied to an SBOM. A
# document generated beside the binary is a CHECKED claim about it: the two can drift, and nothing
# in either one says they have. A section inside the binary cannot drift from the binary - *prefer
# unrepresentable to checked*, which is what `Secret` and `TimeRange` are, pointed at provenance
# instead of at a value.
#
# It also closes the gap the alternative could not. `cargo cyclonedx` reads the RESOLVED graph, and
# the resolved graph is not this binary's graph: `sutura-cli` links the engine only, while
# `libduckdb-sys` and the BigQuery wire put `ureq`, rustls and `ring` in the resolve for a binary
# that links none of them - `deny.toml` has carried that argument for longer than this file has. A
# workspace-wide document would name all of them and be wrong in the direction that matters, which
# is overstating what ships.
#
# WHAT IT DOES NOT DO, because the section is a list of crates and not a claim about them: it says
# which versions were compiled in, and it says nothing about whether any of them has an advisory
# against it. That is `cargo-deny` against the RustSec database, and its verdict is a CI run rather
# than a property of the artifact.
{ pkgs }:

{
  # THE PROFILE FLAG IS COMPUTED HERE rather than through crane's `cargoWithProfile`, and that is a
  # correctness point rather than a preference. That helper inserts the profile after the FIRST word
  # of the command - it is written for `cargo build`, so `cargoWithProfile auditable build` emits
  # `cargo auditable --release build`, which is not a command. The profile is a parameter of the
  # callers anyway, so it is spelled out.
  #
  # `--release` rather than `--profile release` for that one profile, matching what crane's helper
  # does, so a build log reads the same either way.
  buildCommand = profile:
    if profile == "release"
    then "cargo auditable build --release"
    else "cargo auditable build --profile ${profile}";

  # ON THE FINAL BUILD ONLY, never on the args that feed `buildDepsOnly`. Two reasons, and the
  # second is the expensive one to get wrong.
  #
  # `buildDepsOnly` has to stay byte-identical to what the checks share, and it is derived from
  # those same args - so a `nativeBuildInputs` entry added there changes that derivation's hash and
  # every check stops reusing one dependency closure. `flake.nix`'s existing note on
  # `cargoExtraArgs` says the same thing about the same attrset.
  #
  # And it does not need to be there: `cargo auditable` works by setting `RUSTC_WORKSPACE_WRAPPER`,
  # which by definition applies to WORKSPACE members and not to registry dependencies. So the
  # dependency artifacts built without it are still the artifacts this build wants.
  #
  # From `pkgs` and never a cross package set: this is a tool that RUNS during the build, and
  # `strictDeps = true` on both callers makes that distinction load-bearing rather than stylistic.
  toolFor = args: {
    nativeBuildInputs = args.nativeBuildInputs ++ [ pkgs.cargo-auditable ];
  };
}

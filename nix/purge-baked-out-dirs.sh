# shellcheck shell=bash
# Regenerate build script output that hard-coded the directory it was generated in.
#
# THE FAILURE, measured on `aarch64-darwin` against an unmodified tree - `just validate` exiting 1
# at `checks.nextest` BEFORE a single test ran, which made the one command this repo calls
# verification unusable on this platform:
#
#   error: #[derive(RustEmbed)] folder '/nix/var/nix/builds/nix-74462-1743377963/source/target/ci/
#          build/utoipa-swagger-ui-cf31195d9c893f1b/out/swagger-ui-5.17.14/dist/' does not exist
#     --> /nix/var/nix/builds/nix-47928-46264783/source/target/ci/build/...-cf31195d9c893f1b/out/embed.rs:4:1
#   error[E0599]: no associated function or constant named `get` found for struct `SwaggerUiDist`
#   error: could not compile `utoipa-swagger-ui` (lib) due to 2 previous errors
#
# Read the two roots: `nix-74462-...` is where `sutura-deps` built, `nix-47928-...` is where
# `checks.nextest` ran. `utoipa-swagger-ui`'s build script unzips its VENDORED asset bundle into
# `$OUT_DIR` and writes a `rust-embed` `#[folder = "..."]` naming that directory as an absolute
# literal. Every check decompresses `sutura-deps` for its `target/`, so the literal arrives naming
# the DEPENDENCY derivation's build directory. A linux build root is `/build` for every derivation,
# so it happens to resolve; a darwin build root is `/nix/var/nix/builds/nix-<pid>-<random>/`,
# unique per derivation, so it resolves nowhere and the derive expands to a struct with no `Embed`
# impl. Nothing is wrong with the tree or the tests - only with a path that outlived its directory.
#
# WHY A REGENERATION AND NOT AN AGREEMENT. `flake.nix`'s `wholeTree` gives every check's source
# root the same NAME so the roots agree, and says at that binding that a name cannot make two
# darwin build roots agree. This is what makes agreement unnecessary rather than cheaper: whatever
# baked a build root is regenerated where it now sits, so no consumer has to sit where the closure
# was built. It fetches nothing - the assets are the crate's own vendored bundle either way.
#
# WHY THIS IS A MECHANISM AND NOT A CRATE NAME. `root-output` is cargo's OWN record of the absolute
# `OUT_DIR` a build script ran with, so "have these artifacts moved" is read rather than assumed,
# and the next crate that bakes a path is caught without editing this file. The mismatch ALONE is
# true of every build script in a relocated `target/` - purging on that would rebuild the closure
# and lose the reuse these checks exist on - so a directory is purged only when what it generated
# names it. The original failure investigation measured **1 of 138** build script output directories,
# `utoipa-swagger-ui`, found in 1.8 s. The cost per consumer is that one build script rerun plus
# one crate; nothing downstream of it is a dependency, so nothing else recompiles.
#
# WHAT IT DOES NOT COVER.
# A generated file naming some OTHER absolute directory - the source root, a sibling crate's
# `OUT_DIR` - is invisible here, and so is a path written into a compiled artifact rather than into
# the bytes of the output directory. The search also excludes the sibling `$unitDir/output`, but
# retained directive bytes are not necessarily the values Cargo gives rustc. Cargo 1.98 reads the
# previous `OUT_DIR` from `root-output` and replaces that literal with the current directory in
# parsed `cargo:` and `cargo::` directive values; see `prev_build_output` and `BuildOutput::parse`:
# https://github.com/rust-lang/cargo/blob/rust-1.98.0/src/cargo/core/compiler/custom_build.rs
# An independent offline probe on aarch64-darwin moved a target and changed only its consumer:
# both forms of `rustc-env` and native `rustc-link-search` reached rustc with the new directory,
# both generated-file byte checks passed, and the build script still had run only once. The saved
# `root-output` and `output` bytes stayed unchanged, naming the now-absent old directory. This is
# not proof of native-library linking, differently spelled paths or other Cargo versions.
#
# Read-only scans on 2026-09-08 covered all 100 / 106 records in two warmed targets. In each target,
# searching `output` too added 12 matching records across five crates to the two generated-content matches;
# broad text matches also included logs and metadata, not just compiler directives. The first
# baseline / widened scan took 3.24 / 3.36 s: detection cost, NOT the cost of rebuilding those crates.
# No widening is justified by the tested literal directives. The existing four-unit actual-script
# fixture proves the `output`-only unit SURVIVES; it tests this detector, not Cargo's relocation.
#
# NO TOP-LEVEL `set`: this text is INLINED, into every artifact-inheriting derivation's `preBuild`
# and into `cargoWarmStart`, which every warm-start app expands, so a `set -e` here would change
# the shell that runs the rest of the build. The work is a subshell function - `()` and not `{}` -
# so its strictness stops at its own closing paren. Portable flags only, for the same reason: in a
# check this runs under the sandbox's GNU tools and in the app under whatever the host ships.

suturaPurgeBakedOutDirs() (
  set -euo pipefail

  targetDir="${CARGO_TARGET_DIR:-target}"
  case "$targetDir" in
    /*) ;;
    *) targetDir="$PWD/$targetDir" ;;
  esac

  records=""
  if [ -d "$targetDir" ]; then
    records="$(find "$targetDir" -type f -name root-output)"
  fi

  # COLLECTED BEFORE ANYTHING IS DELETED. One crate owns two unit directories - the compiled build
  # script and the run that produced `out/` - and the purge takes both, so a streaming read would
  # hand `find` a directory that no longer exists.
  purged=0
  while IFS= read -r record; do
    if [ -z "$record" ]; then continue; fi

    unitDir="${record%/root-output}"
    outDir="$unitDir/out"
    if [ ! -d "$outDir" ]; then continue; fi

    ranIn="$(cat "$record")"
    # Same directory: this derivation is where the script ran, so nothing it wrote can be stale.
    if [ "$ranIn" = "$outDir" ]; then continue; fi
    # Moved - but only content that hard-coded the old directory is unusable.
    if ! grep -qrF -- "$ranIn" "$outDir"; then continue; fi

    unit="${unitDir##*/}"
    crate="${unit%-*}"
    if [ -z "$crate" ] || [ "$crate" = "$unit" ]; then continue; fi

    buildDir="${unitDir%/*}"
    profileDir="${buildDir%/*}"
    printf 'purge-baked-out-dirs: %s baked %s into what it generated\n' "$crate" "$ranIn"
    # Both unit directories and every fingerprint for the crate: the build script has to RERUN
    # (that is what rewrites the path) and the library has to be recompiled against what it wrote.
    rm -rf -- "${buildDir:?}/$crate"-* "${profileDir:?}/.fingerprint/$crate"-*
    purged=$((purged + 1))
  done <<<"$records"

  printf 'purge-baked-out-dirs: %s inherited build script output(s) regenerated here\n' "$purged"
)

suturaPurgeBakedOutDirs

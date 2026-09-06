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
# names it. Measured over the current closure: **1 of 138** build script output directories,
# `utoipa-swagger-ui`, found in 1.8 s. The cost per consumer is that one build script rerun plus
# one crate; nothing downstream of it is a dependency, so nothing else recompiles.
#
# WHAT IT DOES NOT COVER, and the third of these is narrower than this paragraph used to admit.
# A generated file naming some OTHER absolute directory - the source root, a sibling crate's
# `OUT_DIR` - is invisible here, and so is a path written into a compiled artifact rather than into
# the bytes of the output directory. And the search is `$unitDir/out` alone, so `$unitDir/output` -
# cargo's record of the `cargo::` directives the build script PRINTED, a sibling of `out/` rather
# than a file in it - is not read at all: an absolute `$OUT_DIR` in a `rustc-link-search` or a
# `rustc-env` there survives the unpack unregenerated. Widening to it is not free and the cost is
# UNMEASURED - a build script that publishes its own output directory as a link path names it in
# `output` as a matter of course, so purging on that record could reach a large share of the
# closure's 138 build scripts and give back the reuse these checks exist on. Stated rather than
# taken, because an unmeasured widening of a purge is the more expensive of the two mistakes; all
# three shapes fail the same loud way this did.
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

# What a BARE cargo needs before it can build this workspace, as shell lines for a flake app.
#
# The checks need none of this: crane puts the libraries in `buildInputs`, the nix builder sets
# the linker search path from them, and its hooks arrange the vendored registry and the inherited
# artifacts. An app is a plain shell script OUTSIDE any build sandbox, so it inherits nothing and
# has to say all of it itself - and every time one of these lines was missing, the app failed in
# a way that read as a broken toolchain rather than as a missing export.
#
# ITS OWN FILE for the reason `nix/api-docs.nix` gives: flake.nix sat at exactly the 1000-line
# limit `cargo xtask max-lines` enforces. `apps.<name>` stays in flake.nix, because
# `xtask/src/pins.rs` and `xtask/src/workflows.rs` scan that file for those declarations and both
# fail closed on finding none.
{ pkgs, duckdb, cargoArtifacts, cargoVendorDir }:

let
  # What cargo needs to LINK this workspace, outside a build sandbox, as shell lines.
  #
  # The checks do not need this: crane puts `duckdb.package` in `buildInputs` and the nix
  # builder sets the linker search path from it. An app is a plain shell script outside
  # any build sandbox, so it inherits nothing and has to say so itself.
  #
  # Two separate omissions, found one after the other, both in `apps.causality`:
  #
  #   - It exported only `PATH`, and `--all-features` pulls `sutura-exec-duckdb`, which
  #     links `-lduckdb`: `ld.lld: error: unable to find library -lduckdb`. Only visible
  #     after a disk fix let the gate run far enough to reach the linker, which is why a
  #     pre-existing gap looked like a new regression.
  #   - `.cargo/config.toml` sets `linker = "clang"` with `-fuse-ld=lld` and neither was on
  #     PATH. The dev shell's `runtimeInputs` comment says precisely what that looks like -
  #     "every build script fails with linker `clang` not found" - and the apps never got
  #     the same treatment. It PASSED in CI and failed locally, which is the wrong way
  #     round: `ubuntu-latest` ships clang, so the gate was depending on ambient tooling
  #     in the one place this flake exists to make ambient tooling irrelevant.
  #
  # One binding, so the next app that shells out to cargo cannot omit half of it.
  cargoLinkEnv = ''
    export PATH="${pkgs.clang}/bin:${pkgs.lld}/bin:$PATH"
    export DUCKDB_LIB_DIR="${duckdb.env.DUCKDB_LIB_DIR}"
    export DUCKDB_INCLUDE_DIR="${duckdb.env.DUCKDB_INCLUDE_DIR}"
    export LD_LIBRARY_PATH="${duckdb.env.LD_LIBRARY_PATH}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    # The third omission, and the one that only shows up where an app is easiest to try: the
    # cranelift backend is INHERITED from the dev shell, and the pinned STABLE cargo these apps
    # carry rejects it before running anything - "feature codegen-backend is required", because
    # cargo VALIDATES every profile named in the environment even when --profile ci is what was
    # asked for. nix/api-docs.nix unsets the same two variables for the same reason and says so
    # at length. Reproduced by running `nix run .#causality` from inside the dev shell, which is
    # how a developer would first try it; CI never sees it, because CI has no dev shell.
    unset CARGO_PROFILE_DEV_CODEGEN_BACKEND CARGO_UNSTABLE_CODEGEN_BACKEND
  '';

  # Shell lines that let a BARE cargo reuse the dependency closure the checks build.
  #
  # WHY: `apps.causality` shells out to `cargo nextest`, and a bare cargo cannot read
  # /nix/store. On the run that prompted this, `Test causality` took 12m16s, of which 9m48s
  # was the head leg compiling the whole closure a SECOND time - `checks.nextest` had
  # compiled the same closure minutes earlier in the same job. `magic-nix-cache` caches the
  # store, which is exactly what a bare cargo does not read, and `ci.yml` caches no
  # `target/`. So the artifacts existed and the tool could not see them.
  #
  # TWO HALVES, AND EITHER ALONE DOES NOTHING. Unpacking the artifacts without the vendored
  # registry recompiles from `proc-macro2` onward: they were built against crane's vendored
  # sources and a bare cargo resolves out of `~/.cargo`, so every fingerprint differs.
  # MEASURED both ways: artifacts alone rebuilt the closure; artifacts plus `CARGO_HOME`
  # finished in 17.83 s, compiling 24 crates - our 14 plus the 10 the deps derivation does
  # not carry, which are the feature-gated duckdb/rustls stack. Those 10 are cheap and are
  # deliberately not chased: widening the deps build to `--all-features` would change what
  # every check builds.
  #
  # THE SAME DERIVATIONS THE CHECKS USE, as interpolations rather than copied paths. flake.nix
  # passes `ciArtifacts` - what `clippy`, `nextest`, `doctest`, `hygiene`, `crap` and `api-docs`
  # each hand crane as `cargoArtifacts` - and `craneLib.vendorCargoDeps ciArgs`, the vendor
  # directory crane computes for that same argument set. Naming any other derivation would warm a
  # closure the cache does not already hold, which is the whole cost this removes.
  #
  # BOTH DIRECTORIES ARE WRITABLE COPIES under `target/`. A store path is read-only, and
  # crane's own hook APPENDS to `$CARGO_HOME/config.toml`; cargo also writes into the
  # target directory. `target/` and not `$TMPDIR`, so a `cargo clean` takes them.
  #
  # THE TARGET DIRECTORY IS NAMED IN TWO PLACES, and that is the weak seam here.
  # `xtask/src/causality.rs` computes `<root>/target/causality-target` for its own two runs
  # and reads no environment variable for it, so warming any other directory would silently
  # do nothing at all - the reuse would simply not happen, with no error. Change one and
  # change the other. `cargo xtask check-warm-start` now fails when they disagree, reading each
  # side through its mechanism - what this exports, and the whole `join` chain over there -
  # rather than by finding the literal string in both files.
  #
  # The unpack is crane's `inheritCargoArtifacts` line verbatim, and it is STAMPED with the
  # derivation that produced it: 756 MB of decompression is worth paying once per closure
  # and not once per run.
  cargoWarmStart = ''
    export PATH="${pkgs.zstd}/bin:${pkgs.gnutar}/bin:$PATH"
    warmRoot="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
    warmTarget="$warmRoot/target/causality-target"

    export CARGO_HOME="$warmRoot/target/causality-cargo-home"
    mkdir -p "$CARGO_HOME" "$warmTarget"
    # Written, not appended: a second run would otherwise stack duplicate source blocks.
    install -m 644 "${cargoVendorDir}/config.toml" "$CARGO_HOME/config.toml"

    if [ "$(cat "$warmTarget/.sutura-warm-start" 2>/dev/null)" != "${cargoArtifacts}" ]; then
      echo "warm start: unpacking ${cargoArtifacts} into $warmTarget"
      zstd -d "${cargoArtifacts}/target.tar.zst" --stdout | tar -x -C "$warmTarget"
      printf '%s' "${cargoArtifacts}" > "$warmTarget/.sutura-warm-start"
    fi
    # The app's own `cargo run -p xtask` compiles here too, rather than into a second
    # directory that would have to build xtask's dependencies from scratch.
    export CARGO_TARGET_DIR="$warmTarget"

    # THE ONE ARTIFACT THE UNPACK CANNOT BE TRUSTED WITH, and it is here rather than in the app
    # that first hit it because it is a property of THIS unpack and not of that gate.
    # `utoipa-swagger-ui`'s build script embeds an ABSOLUTE `OUT_DIR` path into the rust-embed
    # `#[folder]` attribute it generates (`<target>/ci/build/utoipa-swagger-ui-*/out/embed.rs`).
    # The closure above was produced in a sandbox whose source root is `/build/source`, so any
    # consumer that then RECOMPILES the crate - a different feature set, or a `cargo check` unit,
    # which is a different unit from the `cargo build` one the closure carries - reuses that stale
    # `embed.rs` and fails with `#[derive(RustEmbed)] folder ... does not exist`. Purging the
    # crate's build output makes `build.rs` rerun and regenerate it against the current root.
    #
    # `apps.causality` carried these lines inline and `apps.bigquery-acceptance` did not, which is
    # the shape of a fix that only the app that was measured has: both warm the same closure and
    # both compile `sutura-http`, so both were exposed and one was patched.
    rm -rf -- "$warmTarget/ci/build/utoipa-swagger-ui-"* \
              "$warmTarget/ci/.fingerprint/utoipa-swagger-ui-"* \
              2>/dev/null || true
  '';
in
{
  inherit cargoLinkEnv cargoWarmStart;
}

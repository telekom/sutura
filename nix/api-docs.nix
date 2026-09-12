# `nix run .#api-docs` - the writer for the committed API reference pages.
#
# ITS OWN FILE because flake.nix sat at exactly the 1000-line limit `cargo xtask max-lines`
# enforces, so the next edit to it failed the gate. `nix/duckdb.nix`, `nix/crap.nix` and
# `nix/toolchains.nix` set the precedent for a nix/ module; the seam is chosen so that nothing
# a text-scanning gate reads out of flake.nix moves. `xtask/src/pins.rs` scans it for
# `apps.<name>` and `xtask/src/workflows.rs` for `apps.<name>`, `packages = ` and `checks = {`,
# and both FAIL CLOSED on finding none - so the `apps.api-docs` declaration stays there and only
# the package it points at lives here.
#
# NOT imported by devenv.nix, unlike the three modules above: the whole point of this one is that
# `just api` reaches it with nothing but nix, from outside any dev shell.
{ pkgs, toolchain, duckdb }:

# WRITES the committed API pages. `checks.api-docs` is the gate that fails when they fall
# behind; this is the fix it names, and the two must agree byte for byte, so both get their
# tools from here: `toolchain` above and a stdlib interpreter out of nixpkgs.
#
# An app rather than a check, because a check cannot write to the source tree - the point
# of this one is to leave the regenerated file in the worktree for review. It exists because
# `just api` called a BARE `cargo` and a BARE `pixi`, so it only worked where a dev shell
# was already active - the drift this flake exists to remove, and it bit. The recipe is now
# `nix run .#api-docs` and needs nothing but nix.
#
# `python3` and not pixi's, matching `checks.api-docs`: the generator imports json,
# pathlib, re and sys and nothing else. If it ever grows a third-party import, this and the
# check must become a pixi environment together or the gate disagrees with its own fix.
#
# Relative paths, so it must run at the repository root. Asserted rather than assumed - a
# silent miss writes nothing and reports success, the same failure shape as `repo::root()`
# returning the wrong directory.
pkgs.writeShellApplication {
  name = "sutura-api-docs";
  # clang and lld because `.cargo/config.toml` selects them as the linker, and this app runs
  # outside the dev shell that would otherwise have them. Without them every build script
  # in the tree fails with "linker `clang` not found", which reads like a broken toolchain.
  runtimeInputs = [ pkgs.python3 pkgs.clang pkgs.lld duckdb.package ];
  text = ''
    if [ ! -f flake.nix ] || [ ! -f Cargo.toml ]; then
      echo "run this from the repository root: it resolves docs/ and target/ relatively" >&2
      exit 1
    fi
    # The toolchain's `cargo doc` child, reached exactly as `checks.api-docs` reaches it,
    # with a target directory of its own so it cannot invalidate the dev shell's `target/`.
    export PATH="${toolchain}/bin:$PATH"
    export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-target}/api-docs"
    # The cranelift backend is INHERITED when this app is run from inside the dev shell,
    # and it cannot build this tree: `utoipa-swagger-ui`'s build script unzips its vendored
    # asset bundle, and the CRC32 in `zip` uses `llvm.x86.pclmulqdq.256`, which cranelift
    # does not implement - so the build script aborts with SIGABRT and the whole run dies
    # after writing six of nine pages. Nothing here needs a fast codegen backend: this app
    # emits rustdoc JSON and runs a Python renderer over it.
    unset CARGO_PROFILE_DEV_CODEGEN_BACKEND CARGO_UNSTABLE_CODEGEN_BACKEND
    # `--all-features` reaches the adapters, and one of them links libduckdb. This app runs
    # OUTSIDE the dev shell - that is the point of it - so the three variables have to be
    # here too, from the same nix/duckdb.nix the shell and the checks read.
    export DUCKDB_LIB_DIR="${duckdb.env.DUCKDB_LIB_DIR}"
    export DUCKDB_INCLUDE_DIR="${duckdb.env.DUCKDB_INCLUDE_DIR}"
    export LD_LIBRARY_PATH="${duckdb.env.LD_LIBRARY_PATH}"

    # DERIVED, not listed. `checks.api-docs` reads the library crates out of `cargo
    # metadata`, so a hardcoded list here is a list that goes stale silently: the gate would
    # ask for a page this writer never generates, and the fix it names would not produce it.
    # This is the same query, so the two cannot disagree.
    libs=$(cargo metadata --format-version 1 --no-deps | python3 -c '
    import json, sys
    meta = json.load(sys.stdin)
    names = sorted(
        p["name"]
        for p in meta["packages"]
        if any("lib" in t["kind"] for t in p["targets"])
    )
    print(" ".join(names))
    ')
    if [ -z "$libs" ]; then
      echo "no library crates found: cargo metadata returned none" >&2
      exit 1
    fi
    # ONE invocation for every member, where this was 19 serial `cargo rustdoc` calls. Two things
    # change and both matter: cargo schedules the units across the host instead of the loop
    # serialising them, and features resolve ONCE over the whole workspace - the same
    # `--workspace --all-features` resolution `sutura-deps` is built under, so `checks.api-docs`
    # reuses those artifacts instead of recompiling every feature-gated dependency per package.
    # `--no-deps` keeps it to the members; without it this documents the entire closure.
    # `--profile ci`: cargo's default `dev` optimises the closure at `opt-level = 3`.
    #
    # `--document-private-items`: rustdoc runs NO link-resolution pass over an item it is not
    # documenting, so `broken_intra_doc_links` - `forbid` in the root manifest - reports nothing
    # about a private module's doc comments without it. It does not change a page - the generator
    # keeps `public` and `default` visibility only.
    #
    # The flags travel in the ENVIRONMENT because `cargo doc` documents many units and so takes no
    # trailing rustdoc arguments. `xtask/src/api_docs/writer.rs` holds this assignment AND the
    # three selection arguments above equal to `checks.api-docs`' own: a difference on one side
    # only means the fix this writer IS cannot see what the gate refused, or renders pages from a
    # feature resolution the gate never judged.
    export RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items"
    cargo doc -q --no-deps --workspace --all-features --profile ci
    # rustdoc names its JSON after the crate's Rust identifier, so a package with a
    # hyphen becomes a file with an underscore.
    # The target directory variable, and not a literal `target/`: a developer who redirects the target
    # directory - onto a faster volume, say - would otherwise get a "no such file" from
    # the generator rather than the pages they asked for.
    jsons=()
    for lib in $libs; do
      echo "api-docs: $lib"
      jsons+=("''${CARGO_TARGET_DIR:-target}/doc/$(printf '%s' "$lib" | tr - _).json")
    done
    # ONE call over every input. The generator accepts many and defaults its output directory to
    # docs/api, which is what this app is for.
    python3 docs/.tools/rustdoc_to_markdown.py "''${jsons[@]}"
  '';
}

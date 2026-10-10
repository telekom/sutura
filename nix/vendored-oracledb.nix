# THE VENDORED ORACLE DRIVER'S OWN UNIT TESTS, for the modules this repository patched
# (`VENDOR.md`'s `vendor/rust-oracledb/**` row). The workspace excludes the vendored crate, so
# `checks.nextest` never compiles its `#[cfg(test)]` modules. This builds them the way upstream
# does: the crate's own manifest and default features, and none of this workspace's lints,
# `.cargo/config.toml` or profiles - `src` is the vendored directory alone. The versions it builds
# against are the root `Cargo.lock`'s, so the tests run over the dependencies the binary links.
#
# **IT GOES AWAY WITH THE VENDORED COPY.** When that row's removal condition is met and the driver
# returns to the registry, delete this file, its `checks` line in `flake.nix` and its `just ci`
# entry.
#
# COST: 422 s cold at `cores = 4`, `max-jobs = 1`, most of it the `aws-lc-sys` C build that
# rustls' default features pull in; a cache hit otherwise, since its inputs are this directory,
# the root lock and the toolchain.
#
# `[dev-dependencies]` is dropped from the copied manifest. Its one entry, `rstest`, serves the
# integration tests under `tests/`, which need a live database and are not run here, and the root
# lock does not carry it, so the network-less sandbox cannot resolve it. A dev-dependency a unit
# test does need fails this build to compile; it cannot pass it.
#
# **A LITERAL FLOOR**, `adbc-driver-bigquery`'s reason: libtest calls a run whose filters match
# nothing `ok`, so a renamed module would pass here over zero tests. `floor` is the number of tests
# in `modules` when this was written; a patch that adds one moves nothing, one that drops a test or
# a module fails here until somebody lowers it.
#
# `noArtifacts` is `flake.nix`'s `inheritedArtifacts null`: nothing to reuse, and the one pairing
# `cargo xtask check-warm-start` accepts for a derivation that takes `cargoArtifacts`.
{ craneLib, cargoVendorDir, buildInputs, noArtifacts }:
let
  # The patched modules that carry tests. Patch (4)'s `messages/connect.rs` has none:
  # `crates/sutura-exec-oracle/tests/dial.rs` drives it through a fake listener instead.
  modules = [ "config::base::" "transport::" "metadata::" "messages::execute::" "response::error_info::" "ora_type::timestamp::" "external_auth::" ];
  floor = 15;
in
craneLib.mkCargoDerivation (noArtifacts // {
  pname = "vendored-oracledb-tests";
  src = ../vendor/rust-oracledb;
  cargoLock = ../Cargo.lock;
  inherit cargoVendorDir buildInputs;
  strictDeps = true;
  doCheck = false;
  doInstallCargoArtifacts = false;
  postPatch = ''
    awk '/^\[/ { skip = ($0 == "[dev-dependencies]") } !skip' Cargo.toml > Cargo.toml.next
    mv Cargo.toml.next Cargo.toml
  '';
  buildPhaseCargoCommand = ''
    cargo test --offline --lib -- ${builtins.concatStringsSep " " modules} > test.log 2>&1 \
      || { cat test.log; exit 1; }
    cat test.log
    passed=$(sed -n 's/^test result: ok\. \([0-9]*\) passed.*/\1/p' test.log)
    test "''${passed:-0}" -ge ${toString floor} \
      || { echo "ran ''${passed:-0} vendored oracledb test(s) and this check declares ${toString floor}" >&2; exit 1; }
  '';
  installPhaseCommand = ''
    mkdir -p $out
    cp test.log $out/
  '';
})

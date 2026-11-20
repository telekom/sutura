# THE VENDORED CLICKHOUSE DRIVER'S OWN UNIT TESTS (`VENDOR.md`'s `vendor/adbc_clickhouse/**` row).
# The workspace excludes the vendored crate, so `checks.nextest` never compiles its `#[cfg(test)]`
# modules, and `nix/clickhouse-adbc.nix` keeps `doCheck = false`: it builds once per release triple,
# cross-compiled, where a test binary cannot run, and the crate's `tests/it` cells need a ClickHouse
# server. This runs the library's unit cells on the HOST only, `--locked` against the same lock and
# the same features the shipped build uses.
#
# The cells serve loopback listeners of their own (the timeout cells stall a connection on purpose),
# hence `__darwinAllowLocalNetworking`. They need no ClickHouse server.
#
# **A LITERAL FLOOR**, `vendored-oracledb`'s reason: libtest calls a run whose filters match nothing
# `ok`, so a renamed module would pass here over zero tests. `floor` is the number of unit cells when
# this was written; a patch that adds one moves nothing, one that drops one fails here until
# somebody lowers it.
#
# **IT GOES AWAY WITH THE VENDORED COPY.** When that row's removal condition is met, delete this
# file, its `checks` line in `flake.nix`, its `just ci` entry and its CI step.
#
# `noArtifacts` is `flake.nix`'s `inheritedArtifacts null`: nothing to reuse, and the one pairing
# `cargo xtask check-warm-start` accepts for a derivation that takes `cargoArtifacts`.
{ pkgs, crane, noArtifacts }:
let
  craneLib = (crane.mkLib pkgs).overrideToolchain
    (p: p.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml);
  # The tree and its lock side by side, as `nix/clickhouse-adbc.nix` lays them out.
  src = pkgs.runCommand "adbc_clickhouse-test-src" { } ''
    cp -r ${../vendor/adbc_clickhouse} $out
    chmod -R u+w $out
    cp ${./clickhouse-adbc.Cargo.lock} $out/Cargo.lock
  '';
  # `floor`: the `test result: ok. N passed` line of `cargo test --lib` over these features, 35 on
  # 2026-11-20.
  floor = 35;
in
craneLib.mkCargoDerivation (noArtifacts // {
  pname = "vendored-clickhouse-adbc-tests";
  version = "0.1.1"; # `vendor/adbc_clickhouse`, provenance in `VENDOR.md`
  inherit src;
  cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./clickhouse-adbc.Cargo.lock; };
  strictDeps = true;
  doCheck = false;
  doInstallCargoArtifacts = false;
  __darwinAllowLocalNetworking = true;
  # The `native-roots` feature reads the system store when a client is built, and the sandbox points
  # `SSL_CERT_FILE` at a file that is not there, which panics every cell that builds a client.
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  # The features `nix/clickhouse-adbc.nix` builds the shipped driver with.
  buildPhaseCargoCommand = ''
    cargo test --locked --offline --lib --features ffi,rustls-tls-ring,rustls-tls-native-roots > test.log 2>&1 \
      || { cat test.log; exit 1; }
    cat test.log
    passed=$(sed -n 's/^test result: ok\. \([0-9]*\) passed.*/\1/p' test.log)
    test "''${passed:-0}" -ge ${toString floor} \
      || { echo "ran ''${passed:-0} vendored ClickHouse driver test(s) and this check declares ${toString floor}" >&2; exit 1; }
  '';
  installPhaseCommand = ''
    mkdir -p $out
    cp test.log $out/
  '';
})

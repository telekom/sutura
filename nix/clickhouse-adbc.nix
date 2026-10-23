# The ADBC ClickHouse driver (`ClickHouse/adbc_clickhouse`, MIT OR Apache-2.0), built per release
# triple from the vendored tree `vendor/adbc_clickhouse`, the way `nix/bigquery-adbc.nix` and
# `nix/postgres-adbc.nix` build theirs (telekom/sutura#1002). `VENDOR.md` records the pin, each
# patch and its removal condition.
#
# **Built with the workspace's own pinned toolchain** (`devco/rust-toolchain-nightly.toml`), from
# the lock `nix/clickhouse-adbc.Cargo.lock`, which the vendored tree does not carry (the workspace
# excludes it, and a lock there would sit under the upstream copyright annotation). The lock is
# committed so the vendor directory is a pure function of repository files and the build holds no
# import-from-derivation; a manifest edit without this file moving fails `--locked`.
#
# **The features are `ffi`, `rustls-tls-ring` and `rustls-tls-native-roots`, and the choice is the
# point.** `ring` is the one TLS provider the shipped binary already links, so no second one
# (`aws-lc`) comes in with a C build of its own. The system root store is what `sutura-tls` falls back
# to when a source declares no bundle, so a declared `clickhouse.client.tls_root_certs` replaces it
# in the driver as the declared bundle replaces it in sutura. A root-store feature is not optional:
# the `clickhouse` crate refuses to compile a provider without one.
#
# Output, the two shapes the other drivers have where they apply:
# `lib/libadbc_driver_clickhouse.{so,dylib}` for a deployment that mounts a driver - not on a musl
# triple, where a `cdylib` of a statically linked target is not a thing, and a static artefact has
# no dynamic loader in any case - and `lib/libadbc_driver_clickhouse.a` for an artefact to link in
# (every non-darwin triple).
#
# **The archive is ONE relocatable object exporting ONE symbol, `AdbcClickhouseInit`.** A Rust
# `staticlib` carries its own copy of `std`, `compiler_builtins`, an allocator shim, `ring` and the
# whole dependency graph, with their symbols global - linked beside a Rust binary built from the
# same compiler, that is a duplicate-definition lottery over names like `rust_eh_personality`
# and `__rust_alloc`. `ld -r` merges the archive, and `objcopy --keep-global-symbol` makes every
# other defined symbol local, so the final link sees the driver's entrypoint and the undefined
# names a C library answers, and nothing else. That also drops `AdbcDriverInit`, the generic
# fallback name `adbc_ffi::export_driver!` defines and the BigQuery Go archive defines too.
# `postBuild` proves both: `nm` refuses an archive whose defined globals are anything but the one
# entrypoint, and a probe links against it - `-static` on musl, the link a static artefact makes.
#
# What this does not establish: that the driver LOADS or RUNS. The probe is linked, never executed
# (the build is cross); `crates/sutura-adbc/tests/linked.rs` is what runs it.
#
# `pkgs` is the package set for `crossSystemName`; `noArtifacts` is `flake.nix`'s
# `inheritedArtifacts null`: nothing to reuse, and the one pairing `cargo xtask check-warm-start`
# accepts for a derivation that takes `cargoArtifacts`.
{ pkgs, crane, noArtifacts, crossSystemName }:
let
  inherit (pkgs.stdenv) hostPlatform;
  isMusl = hostPlatform.isMusl;
  isDarwin = hostPlatform.isDarwin;
  target = hostPlatform.rust.rustcTarget;
  craneLib = (crane.mkLib pkgs).overrideToolchain
    (p: p.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml);
  src = pkgs.buildPackages.runCommand "adbc_clickhouse-src" { } ''
    cp -r ${../vendor/adbc_clickhouse} $out
    chmod -R u+w $out
    cp ${./clickhouse-adbc.Cargo.lock} $out/Cargo.lock
  '';
  crateTypes = pkgs.lib.concatMapStringsSep " " (t: "--crate-type ${t}")
    (pkgs.lib.optional (!isDarwin) "staticlib" ++ pkgs.lib.optional (!isMusl) "cdylib");
in
craneLib.mkCargoDerivation (noArtifacts // {
  pname = "adbc-driver-clickhouse";
  version = "0.1.1"; # `vendor/adbc_clickhouse`, provenance in `VENDOR.md`
  inherit src;
  cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./clickhouse-adbc.Cargo.lock; };
  strictDeps = true;
  doCheck = false;
  doInstallCargoArtifacts = false;
  CARGO_BUILD_TARGET = target;
  nativeBuildInputs = [ pkgs.buildPackages.removeReferencesTo ];

  buildPhaseCargoCommand = ''
    cargo rustc --locked --release --lib --features ffi,rustls-tls-ring,rustls-tls-native-roots ${crateTypes}
  '' + pkgs.lib.optionalString (!isDarwin) ''
    $LD -r --whole-archive target/${target}/release/libadbc_clickhouse.a -o merged.o
    $OBJCOPY --keep-global-symbol=AdbcClickhouseInit merged.o
    rm -f libadbc_driver_clickhouse.a
    $AR rcs libadbc_driver_clickhouse.a merged.o
    syms="$($NM -g --defined-only libadbc_driver_clickhouse.a | grep -E ' [A-Za-z] ' || true)"
    test "$(wc -l <<<"$syms")" -eq 1 && grep -qE ' T AdbcClickhouseInit$' <<<"$syms" \
      || { echo "the ClickHouse archive must define exactly AdbcClickhouseInit, it defines:" >&2; echo "$syms" >&2; exit 1; }
    printf 'int AdbcClickhouseInit(int, void *, void *);\nint main(void) { return AdbcClickhouseInit(0, 0, 0); }\n' > probe.c
    $CC ${pkgs.lib.optionalString isMusl "-static"} probe.c libadbc_driver_clickhouse.a -lm -lpthread -ldl -o probe
  '';

  installPhaseCommand = if isDarwin then ''
    install -Dm755 target/${target}/release/libadbc_clickhouse.dylib $out/lib/libadbc_driver_clickhouse.dylib
  '' else ''
    install -Dm644 libadbc_driver_clickhouse.a $out/lib/libadbc_driver_clickhouse.a
  '' + pkgs.lib.optionalString (!isMusl && !isDarwin) ''
    install -Dm755 target/${target}/release/libadbc_clickhouse.so $out/lib/libadbc_driver_clickhouse.so
  '';
  passthru.triple = crossSystemName;
})

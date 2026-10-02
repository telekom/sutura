# The ADBC PostgreSQL driver (`apache/arrow-adbc` `c/driver/postgresql`, Apache-2.0), self-built
# from source per release triple, the same way and for the same reason as `nix/bigquery-adbc.nix`
# (telekom/sutura#913): nixpkgs does not package it, and a hermetic release needs one driver per
# triple from the flake lock - including the two static musl ones.
#
# The driver is a subdirectory of the `c/` CMake project, so that project is configured with
# `ADBC_DRIVER_POSTGRESQL` on; shared, static and the vendored `fmt`/`nanoarrow` are upstream
# defaults. Its one external dependency is nixpkgs `libpq`, found through `pkg-config`.
#
# **`ADBC_DEFINE_COMMON_ENTRYPOINTS=OFF`, so the driver exports ONE C symbol of the ADBC API:
# `AdbcDriverPostgresqlInit`.** With upstream's default ON it also defines `AdbcDriverInit` and the
# whole `AdbcDatabaseNew`... C API, 55 names the BigQuery Go archive defines too (measured with `nm`
# on the x86_64-musl pair) - so the two could never share one link. The mounted `.so` loses nothing:
# `adbc_driver_manager` derives `AdbcDriverPostgresqlInit` from the file name when no entrypoint
# is passed.
#
# Output, the same two shapes as the BigQuery driver:
# `lib/libadbc_driver_postgresql.so` for a deployment that mounts a driver, and
# `lib/libadbc_driver_postgresql.a` for an artefact to link in. **Upstream's installed static
# archive is NOT self-contained** - with the vendored copies on, `adbc_driver_common`,
# `adbc_driver_framework`, `nanoarrow` and `fmt` are built but never installed - so the archive
# here is those five merged into one.
#
# **On a musl triple `lib/` also carries the archive's static link set**, because a static binary
# has nothing else to resolve `libpq` against: `libpq.a`, `libpgcommon.a`, `libpgport.a` and
# OpenSSL's `libssl.a`/`libcrypto.a` (`docs/adr/0018`'s Thirteenth amendment says why OpenSSL and
# not rustls). That libpq is built without GSSAPI and without libcurl (OAuth) - the `.so` keeps
# both - and `postBuild` links a static probe against exactly that set, so a member missing from
# it fails the build here rather than in a Rust link.
#
# What this does not establish: that the driver LOADS or RUNS. The probe is linked, never
# executed (the build is cross); `crates/sutura-adbc/tests/linked.rs` is what runs it.
#
# **On darwin it is the mounted driver alone** - `lib/libadbc_driver_postgresql.dylib`, built natively
# for a developer host's `just test` and `checks.nextest`. No release triple is darwin, so nothing
# links an archive there and none is built.
#
# `src` must be the repository root; `pkgs` is the cross package set for `crossSystemName`.
{ pkgs, src, crossSystemName }:
let
  isMusl = pkgs.stdenv.hostPlatform.isMusl;
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  # The libpq a static link resolves against. GSSAPI and libcurl off, because both would pull a
  # further static closure (krb5, curl and its TLS) for an auth method sutura never declares.
  staticLibpq = (pkgs.libpq.override {
    gssSupport = false;
    curlSupport = false;
    openssl = staticOpenssl;
  }).overrideAttrs { dontDisableStatic = true; };
  staticOpenssl = pkgs.openssl.override { static = true; };
  staticLibs = [
    "${staticLibpq.dev}/lib/libpq.a"
    "${staticLibpq.dev}/lib/libpgcommon.a"
    "${staticLibpq.dev}/lib/libpgport.a"
    "${staticOpenssl.out}/lib/libssl.a"
    "${staticOpenssl.out}/lib/libcrypto.a"
  ];
in
pkgs.stdenv.mkDerivation {
  pname = "adbc-driver-postgresql";
  version = "1.12.0"; # provenance is the flake-locked `arrow-adbc-src` tag apache-arrow-adbc-24
  inherit src;
  cmakeDir = "../c";
  nativeBuildInputs = [ pkgs.buildPackages.cmake pkgs.buildPackages.pkg-config ];
  buildInputs = [ pkgs.libpq ];
  cmakeFlags = [ "-DADBC_DRIVER_POSTGRESQL=ON" "-DADBC_DEFINE_COMMON_ENTRYPOINTS=OFF" ];

  postBuild = pkgs.lib.optionalString (!isDarwin) ''
    $AR -M <<EOF
    create libadbc_driver_postgresql-merged.a
    addlib driver/postgresql/libadbc_driver_postgresql.a
    addlib driver/common/libadbc_driver_common.a
    addlib driver/framework/libadbc_driver_framework.a
    addlib vendor/nanoarrow/libnanoarrow.a
    addlib vendor/fmt/libfmt.a
    save
    end
    EOF
    printf 'int AdbcDriverPostgresqlInit(int, void *, void *);\nint main(void) { return AdbcDriverPostgresqlInit(0, 0, 0); }\n' > probe.c
    $CC -c probe.c -o probe.o
    $CXX probe.o libadbc_driver_postgresql-merged.a -lpq -o probe
  '' + pkgs.lib.optionalString isMusl ''
    $CXX -static probe.o libadbc_driver_postgresql-merged.a -Wl,--start-group ${builtins.concatStringsSep " " staticLibs} -Wl,--end-group -o probe-static
  '';
  installPhase = if isDarwin then ''
    runHook preInstall
    install -Dm755 driver/postgresql/libadbc_driver_postgresql.dylib $out/lib/libadbc_driver_postgresql.dylib
    runHook postInstall
  '' else ''
    runHook preInstall
    install -Dm755 driver/postgresql/libadbc_driver_postgresql.so $out/lib/libadbc_driver_postgresql.so
    install -Dm644 libadbc_driver_postgresql-merged.a $out/lib/libadbc_driver_postgresql.a
  '' + pkgs.lib.optionalString isMusl ''
    install -m644 -t $out/lib ${builtins.concatStringsSep " " staticLibs}
  '' + ''
    runHook postInstall
  '';
  passthru.triple = crossSystemName;
}

# The ADBC PostgreSQL driver (`apache/arrow-adbc` `c/driver/postgresql`, Apache-2.0), self-built
# from source per release triple, the same way and for the same reason as `nix/bigquery-adbc.nix`
# (telekom/sutura#913): nixpkgs does not package it, and a hermetic release needs one driver per
# triple from the flake lock - including the two static musl ones.
#
# The driver is a subdirectory of the `c/` CMake project, so that project is configured with
# `ADBC_DRIVER_POSTGRESQL` on; shared, static and the vendored `fmt`/`nanoarrow` are upstream
# defaults. Its one external dependency is nixpkgs `libpq`, found through `pkg-config`.
#
# Output, the same two shapes as the BigQuery driver, both exporting `AdbcDriverInit`:
# `lib/libadbc_driver_postgresql.so` for a deployment that mounts a driver, and
# `lib/libadbc_driver_postgresql.a` for a published artefact to link in. **Upstream's installed
# static archive is NOT self-contained** - with the vendored copies on, `adbc_driver_common`,
# `adbc_driver_framework`, `nanoarrow` and `fmt` are built but never installed - so the archive
# here is those five merged into one, and `libpq` stays outside it. `postBuild` links a probe
# against the merged archive plus `-lpq`, so an archive that misses a member fails the build.
#
# What this does not establish: that the driver LOADS or RUNS. The probe is linked, never
# executed (the build is cross), and nothing in Rust reads this output yet.
#
# `src` must be the repository root; `pkgs` is the cross package set for `crossSystemName`.
{ pkgs, src, crossSystemName }:
pkgs.stdenv.mkDerivation {
  pname = "adbc-driver-postgresql";
  version = "1.12.0"; # provenance is the flake-locked `arrow-adbc-src` tag apache-arrow-adbc-24
  inherit src;
  cmakeDir = "../c";
  nativeBuildInputs = [ pkgs.buildPackages.cmake pkgs.buildPackages.pkg-config ];
  buildInputs = [ pkgs.libpq ];
  cmakeFlags = [ "-DADBC_DRIVER_POSTGRESQL=ON" ];

  postBuild = ''
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
    printf 'int AdbcDriverInit(int, void *, void *);\nint main(void) { return AdbcDriverInit(0, 0, 0); }\n' > probe.c
    $CC -c probe.c -o probe.o
    $CXX probe.o libadbc_driver_postgresql-merged.a -lpq -o probe
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 driver/postgresql/libadbc_driver_postgresql.so $out/lib/libadbc_driver_postgresql.so
    install -Dm644 libadbc_driver_postgresql-merged.a $out/lib/libadbc_driver_postgresql.a
    runHook postInstall
  '';
  passthru.triple = crossSystemName;
}

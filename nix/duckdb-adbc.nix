# The DuckDB ADBC driver as ONE static archive per release triple - the third linked driver
# (`crates/sutura-adbc/build.rs`), on `nix/postgres-adbc.nix`'s pattern.
#
# **No driver source of its own: it is nixpkgs' `duckdb`, cross-built.** DuckDB compiles its ADBC
# entrypoint, `duckdb_adbc_init`, into the engine library itself, so the DuckDB the dev shell
# mounts for `sutura-exec-duckdb` (`nix/duckdb.nix`) IS the driver - one version, one source
# hash, and no second DuckDB in the build. That name is the only C-linkage ADBC symbol it defines:
# no `AdbcDriverInit`, and no `AdbcDriverDuckdbInit` for a driver manager to derive from a file
# name, which is why the mounted route passes the entrypoint explicitly (`src/lib.rs`).
#
# **The installed `libduckdb_static.a` is NOT self-contained.** nixpkgs' `lib` output carries it
# beside the vendored third-party archives (`libduckdb_re2.a`, `libduckdb_zstd.a`, ...), the
# in-tree extensions it is configured with and the generated loader that references them. So the
# archive here is all of those merged into one - except `libdummy_static_extension_loader.a`, the
# loader for a build with no linked extensions, which defines the generated loader's names again.
#
# **`postBuild` proves it, on every triple:** a probe calling `duckdb_adbc_init` links against the
# merged archive alone - and on a musl triple with `-static`, the link a static musl binary has to
# make. Then `nm` refuses an archive that defines any `Adbc*` C name: the BigQuery archive defines
# the generic ADBC C API, and a second copy would make the linker pick one.
#
# What this does not establish: that the driver LOADS or RUNS. The probe is linked, never executed
# (the build is cross); `crates/sutura-adbc/tests/linked.rs` is what runs it.
#
# `pkgs` is the cross package set for `crossSystemName`.
{ pkgs, crossSystemName }:
let
  isMusl = pkgs.stdenv.hostPlatform.isMusl;
  duckdb = pkgs.duckdb;
in
pkgs.stdenv.mkDerivation {
  pname = "adbc-driver-duckdb";
  inherit (duckdb) version;
  dontUnpack = true;
  buildPhase = ''
    runHook preBuild
    {
      echo "create libduckdb_adbc.a"
      for a in ${duckdb.lib}/lib/*.a; do
        [ "''${a##*/}" = libdummy_static_extension_loader.a ] || echo "addlib $a"
      done
      echo save
      echo end
    } | $AR -M
    printf 'extern "C" int duckdb_adbc_init(int, void *, void *);\nint main() { return duckdb_adbc_init(0, 0, 0); }\n' > probe.cc
    $CXX ${pkgs.lib.optionalString isMusl "-static"} probe.cc libduckdb_adbc.a -o probe
    syms="$($NM -g --defined-only libduckdb_adbc.a)"
    grep -qE ' T duckdb_adbc_init$' <<<"$syms"
    if grep -E ' [A-Z] Adbc' <<<"$syms"; then
      echo "the DuckDB archive defines a generic ADBC C name (above) another linked driver also defines" >&2
      exit 1
    fi
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm644 libduckdb_adbc.a $out/lib/libduckdb_adbc.a
    runHook postInstall
  '';
  passthru.triple = crossSystemName;
}

# The ADBC BigQuery driver (`adbc-drivers/bigquery`, Apache-2.0), self-built from
# source so that every release triple - including the two static musl ones - gets
# a hermetic, reproducible native driver (telekom/sutura#913).
#
# Why self-built rather than `dbc install bigquery`: this repository's release
# artefacts are hermetic (AGENTS.md) and cross-built to four triples, two of
# which are static musl. A prebuilt driver `.so` from the Columnar CDN is a
# per-platform binary that has no aarch64-musl artefact and is non-reproducible;
# building from source gives one `.so` per triple, all from the flake lock.
#
# Why Go: `adbc-drivers/bigquery` ships C# and Go drivers; neither has a native
# Rust counterpart selected by `adbc_core`/`adbc_driver_manager`. Go is the
# clean cross-compilable one (CGO + c-shared to linux-musl/aarch64).
#
# The driver advertises a go floor above what nixpkgs' default `go` provides;
# override `buildGoModule` with `go_1_27` for this derivation only.
#
# The c-shared facade lives in `go/pkg` and is gated behind the `driverlib` build
# tag, which is how upstream's `adbc-make` builds it; we pass `-tags driverlib`.
#
# Modules are fetched into a per-triple Go-modules fixed-output derivation; each
# triple's FOD is a separate derivation, but all four fetch the same resolved
# module set (libc/arch-independent on linux), so they share one `vendorHash`.
# That hash was measured from a real aarch64-musl build, not guessed.
#
# Output: `$out/lib/libadbc_driver_bigquery.so` - the ADBC v1 C ABI entrypoint
# `AdbcDriverInit` that `adbc_driver_manager` dlopens.
#
# `src` must be the repository's `go/` directory (the Go module root).
{ pkgs, src, crossSystemName, vendorHash }:
let
  # `pkgs` here is the cross package set the caller chose for this release triple.
  # The driver advertises a go newer than nixpkgs' default; pin `go_1_27` here so
  # the driver and its toolchain cannot drift apart from this file's pin.
  buildGoModule = pkgs.buildGoModule.override { go = pkgs.buildPackages.go_1_27; };
in
buildGoModule {
  pname = "adbc-driver-bigquery";
  version = "1.13.0"; # provenance is the flake-locked `bigquery-adbc-src` tag go/v1.13.0
  inherit src vendorHash;
  proxyVendor = true;

  # The go.mod go-floor is relaxed at the source by the caller (see
  # nix/bigquery-adbc-drivers.nix), before this derivation is realised, so both
  # the module-vendoring FOD and the build see a floor this nixpkgs accepts.

  # Dependency licence reporting (telekom/sutura#913 item 7). nixpkgs' go-licenses
  # 2.0.1 cannot classify this module graph - it resolves `github.com/google/uuid`
  # as a stdlib import (`uuid: ... not in std` under its bundled go 1.26.7) no
  # matter GOFLAGS/GO111MODULE/PATH - so this records the real licence texts of
  # every pinned module straight from the populated module cache, which is what
  # the FOD downloaded and what the .so was built against. Hermetic, grep-able.
  buildPhase = ''
    runHook preBuild
    go build -tags driverlib -buildmode=c-shared -o libadbc_driver_bigquery.so ./pkg
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 libadbc_driver_bigquery.so $out/lib/libadbc_driver_bigquery.so
    # Record, machine-greppable beside the binary, every pinned module and its
    # real licence text. `go build` above already extracted the exact import
    # closure into $GOMODCACHE, so walk those module dirs (<path>@<version>,
    # skipping the zip/lib `cache/download` mirror) - no further go invocation,
    # which would force re-resolving the whole go.mod requires graph.
    cache="$(go env GOMODCACHE)"
    : > "$out/lib/DRIVER-MODULES.txt"
    find "$cache" -type d -name '*@*' ! -path '*/cache/download/*' | sort | while read -r moddir; do
      current="''${moddir#''$cache/}"   # full module path@version, e.g. cloud.google.com/go/auth@v0.23.2
      for lic in "$moddir"/LICENSE* "$moddir"/LICENCE* "$moddir"/COPYING*; do
        [ -f "$lic" ] || continue
        echo "$current ''${lic##*/}" >> "$out/lib/DRIVER-MODULES.txt"
        install -Dm644 "$lic" "$out/share/licenses/$current/''${lic##*/}"
      done
    done
    runHook postInstall
  '';
  passthru.triple = crossSystemName;
}

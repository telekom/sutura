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
# The driver requires Go >= 1.27.1, which nixpkgs' default `go` does not yet
# provide, so we override `buildGoModule` with `go_1_27` for this derivation only.
#
# The c-shared facade lives in `go/pkg` and is gated behind the `driverlib` build
# tag, which is how upstream's `adbc-make` builds it; we pass `-tags driverlib`.
#
# Modules are fetched once into a fixed-output derivation keyed by the target
# triple; `vendorHash` below is that FOD's sha256. A hash marked
# `# TODO: capture on first CI build` was not exercised in this environment; each
# remaining triple yields its real hash on the first build after merge (the nix
# error prints the expected value, exactly as this one was obtained).
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
  version = "0.0.0"; # provenance is the flake-locked `bigquery-adbc-src` rev
  inherit src vendorHash;
  proxyVendor = true;
  subPackages = [ "pkg" ];

  buildPhase = ''
    runHook preBuild
    # The driver declares `go 1.27.1`, but this nixpkgs pins go_1_27 = 1.27.0.
    # 1.27.1 is a patch release, so the directive is a floor: relax it to the
    # toolchain we actually build with rather than pulling a second nixpkgs.
    sed -i 's#^go 1\.27\.[0-9]*$#go 1.27#' go.mod
    go build -tags driverlib -buildmode=c-shared -o libadbc_driver_bigquery.so ./pkg
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 libadbc_driver_bigquery.so $out/lib/libadbc_driver_bigquery.so
    runHook postInstall
  '';
  passthru.triple = crossSystemName;
}

# The ADBC BigQuery driver, built per release triple (telekom/sutura#913).
#
# This exposes `libadbc_driver_bigquery.so` for each of the four release triples
# as flake packages, so a CI venue can build them. They are NOT yet wired into
# the shipped closures (that is the follow-on), so nothing serves them today.
#
# The Rust `adbc_driver_manager` consumption, the dlopen-vs-static-link decision
# for the two musl triples, and the provisioned live acceptance leg are the
# follow-on PR. For the two static-musl artefacts in particular, `dlopen` of this
# dynamic `.so` is impossible (a static musl binary has no dynamic loader), so
# "adopt on musl" means a follow-on that switches to `-buildmode=c-archive` and
# `ManagedDriver::load_static` - the `.so` this PR builds is not the artefact a
# static-musl `sutura` loads.
#
# The one vendor hash covers all four triples: each triple realises a separate
# Go-modules fixed-output derivation, but they all fetch the same resolved module
# set (independent of libc/arch on linux), so they agree on one sha256. This was
# measured, not asserted.
{ pkgs, bigqueryAdbcGoSource, buildDriver }:
# `buildDriver` is `import ./bigquery-adbc.nix`; `pkgs` is the HOST package set.
let
  # The driver advertises a go-1.27.1 floor that this nixpkgs' go_1_27 (1.27.0)
  # rejects, both in the module-vendoring FOD and the build. Relax it at the
  # source so both see it; recorded in VENDOR.md.
  patchedGoSrc = pkgs.runCommand "adbc-driver-bigquery-go-src-relaxed" { } ''
    cp -rL ${bigqueryAdbcGoSource} $out
    chmod -R u+w $out
    sed -i 's#^go 1\.[0-9.]*$#go 1.27#' $out/go.mod
    sed -i '/^toolchain /d' $out/go.mod
  '';
  driver = { triple, crossPkgs }:
    {
      "adbc-driver-bigquery-${triple}" = buildDriver {
        pkgs = crossPkgs;
        src = patchedGoSrc;
        crossSystemName = triple;
        vendorHash = "sha256-EijrXEBhQvaJR0k9jrGawDDELAOKeRVuPjCe9UnXNQI=";
      };
    };
in
# Every triple is cross-built uniformly through `pkgsCross`, exactly as
# `nix/shipped.nix` does - none is the host aliased to itself, because a driver
# named for a release triple must actually target it.
driver { triple = "x86_64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.gnu64; }
// driver { triple = "aarch64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform; }
// driver { triple = "aarch64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl; }
// driver { triple = "x86_64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.musl64; }

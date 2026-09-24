# The ADBC BigQuery driver, built per release triple (telekom/sutura#913).
#
# This exposes both driver shapes for each of the four release triples as flake
# packages: `lib/libadbc_driver_bigquery.so` for a deployment that mounts one, and
# `lib/libadbc_driver_bigquery.a` - the `c-archive` - which `nix/shipped.nix` links
# into the published artefact for every triple that has one.
#
# **The archive is what makes the musl artefacts usable, and it is the whole reason
# it exists** (`telekom/sutura#929`'s sixth finding, `docs/adr/0018`'s tenth
# amendment). A static musl binary has no dynamic loader, so `dlopen` of the `.so`
# is impossible there; `crates/sutura-exec-bigquery/src/adbc/linked.rs` declares
# the archive's `AdbcDriverInit` and `ManagedDriver::load_static` opens it. The two
# gnu triples take the same route, so there is one mechanism rather than two and
# neither is the untested half of the other.
#
# What no derivation here establishes is that a driver RUNS.
# `nix/bigquery-driver-check.sh` executes both release binaries on x86_64-linux and
# reads what they say; this only builds.
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

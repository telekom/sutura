# The jscpd derivation (issue #474), referenced from flake.nix rather than defined there:
# flake.nix is at the `cargo xtask max-lines` cap, and the apps/checks/declarations that two
# xtask gates scan textually must stay in it - so this module holds what they POINT AT.
#
# WHY CRANE FROM THE UPSTREAM `rust/` WORKSPACE AND NOT buildNpmPackage: the npm `jscpd`
# tarball wraps prebuilt platform binaries behind a Node loader, and buildNpmPackage would
# fetch Node deps at build time. The upstream repo builds its `rust/` workspace into a
# self-contained native binary (no Node runtime), which is what a fresh clone reaches
# through the pinned `jscpd-src` flake input - the provisioning decision issue #474 settled
# and the acceptance criterion names.
#
# THE BINARY IS `jscpd`/`cpd`; one crate declares both bins. nixpkgs carries no jscpd
# package, so the derivation is authored here. The version is whatever `jscpd-src` (a pinned
# flake input) points at; keeping it a flake input rather than a fetchurl inside this file is
# what makes the source come from flake.lock.
#
# LIMIT, NEXT TO THE CLAIM: this derivation makes the binary exist. It does not decide what
# duplication trips the gate - the thresholds and allowlist are `check-jscpd`'s, in xtask,
# so the version and the policy do not drift apart here.
{
  pkgs,
  craneLib,
  # The artifacts get the purge-baked-out-dirs sweep (`purge-baked-out-dirs.sh`) the SAME way
  # `nix/shipped.nix` receives it: `check-warm-start` pairs every `cargoArtifacts` taking with
  # that sweep, so a separate `cargoArtifacts` binding here would leak an unconsumed build root
  # into a recycled closure. This module inlines it via `inheritedArtifacts` like the flake's own
  # checks do.
  inheritedArtifacts,
  src,
}:
let
  # crane's filter keeps Cargo inputs; the workspace reads two non-Cargo classes at compile
  # time - cpd-reporter's `#[template(path = ...)]` over templates/report.html, and every
  # crate's `#[doc = include_str!("../README.md")]`. Both must ride along (the same clause the
  # upstream flake uses for the first), or the build fails with a file-lost error.
  rustSrc = pkgs.lib.cleanSourceWith {
    src = src + "/rust";
    filter = path: type:
      (craneLib.filterCargoSources path type)
      || (pkgs.lib.hasSuffix ".html" path && pkgs.lib.hasInfix "/templates/" path)
      || (pkgs.lib.baseNameOf path == "README.md");
  };

  commonArgs = {
    src = rustSrc;
    pname = "jscpd";
    # `craneLib.crateNameFromCargoToml` reads the version from the crate that declares the
    # bins, so this cannot drift from upstream's Cargo.toml.
    version = (craneLib.crateNameFromCargoToml {
      cargoToml = src + "/rust/crates/cpd/Cargo.toml";
    }).version;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
  };
in
# `doCheck = false` ON THE DEPS DERIVATION, AND THE MEASUREMENT IS WHY. crane defaults it to
# `true` there so that dev-dependencies get cached, and that default costs a THIRD cargo pass:
# `cargo check`, then `cargo build`, then `cargo test --no-run`. Counted off this derivation's
# own build log, before and after, on the same host and the same pinned source:
#
# |             | cargo passes | `Compiling` | `Checking` |
# | ----------- | ------------ | ----------- | ---------- |
# | crane's default | check --all-targets, build, test --no-run | 231 | 73 |
# | `doCheck = false` | check, build | **137** | 73 |
#
# So the test pass was a second codegen of the entire 94-crate build closure, and NOTHING in this
# repository can use it: the consumer below sets `doCheck = false`, so `jscpd`'s own tests never
# build and never run here. `Checking` does not move - dropping `--all-targets` from the check
# pass changes nothing measurable, and saying so is cheaper than someone re-deriving it. CI's
# `Structural gates` step reported 229 on linux against 231 here, so the shape holds on both.
#
# LIMIT, NEXT TO THE CLAIM: this removes compilation, not wall clock that can be claimed. The
# `ci` job's measured run-to-run spread is 772-1343 s, so a saving of this size is inside the
# noise; the honest witness is the `Compiling` count in the build log, never the duration.
#
# HELD BY `cargo xtask check-warm-start`, not by this comment: an inline `buildDepsOnly` - one
# whose artifacts are constructed inside the very expression that consumes them, as here - must
# AGREE with that consumer on `doCheck`, so removing either half of this pairing is refused.
craneLib.buildPackage (
  commonArgs
  // inheritedArtifacts (craneLib.buildDepsOnly (commonArgs // { doCheck = false; }))
  // {
    doCheck = false;
    meta = {
      description = "Copy/paste detector - native Rust engine over 220+ formats (issue #474)";
      homepage = "https://jscpd.dev";
      license = pkgs.lib.licenses.mit;
      mainProgram = "jscpd";
    };
  }
)

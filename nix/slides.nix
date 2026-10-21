# The meetup deck, built to a static site in `result/<deck>/`, which the docs publish at
# `slides/<deck>/`. The npm dependency
# graph is `package-lock.json`, fetched as a fixed-output derivation, so the build itself runs
# offline: `npm ci`, then `slidev build`. Install scripts are off.
#
# LIMIT: `npmDepsHash` pins the lock file's tarballs, not their advisories. `security-audit.yml`
# runs `osv-scanner` over the lock; nothing in the offline nix legs does. Bump the hash with
# `nix build .#slides` after any change to `package-lock.json`, and copy the hash it prints.
{ pkgs }:
let
  deck = "2026-introducing-sutura";
in
pkgs.buildNpmPackage {
  pname = "sutura-slides-${deck}";
  version = "0.0.0";

  src = pkgs.lib.fileset.toSource {
    root = ../slides/${deck};
    fileset = pkgs.lib.fileset.unions [
      ../slides/${deck}/package.json
      ../slides/${deck}/package-lock.json
      ../slides/${deck}/slides.md
      ../slides/${deck}/public/docs-qr.svg
      ../slides/${deck}/theme
    ];
  };

  npmDepsHash = "sha256-ldsofC52tTq4Xe/KeoylBKyLCnYzcR96v6eoEn8SYcE=";
  npmFlags = [ "--ignore-scripts" ];

  # The deck embeds the two diagrams and the mark that the docs site already ships, so there is
  # one copy of each in the tree.
  postPatch = ''
    mkdir -p public/shared
    cp ${../docs/assets/architecture.html} public/shared/architecture.html
    cp ${../docs/assets/deployment.html} public/shared/deployment.html
    cp ${../docs/assets/sutura.svg} public/shared/sutura.svg
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r dist $out/${deck}
    runHook postInstall
  '';
}

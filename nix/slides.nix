# The meetup deck, built to a static site (`result/` is Slidev's `dist/`). The npm dependency
# graph is `package-lock.json`, fetched as a fixed-output derivation, so the build itself runs
# offline: `npm ci`, then `slidev build`. Install scripts are off.
#
# LIMIT: `npmDepsHash` pins the lock file's tarballs, not their advisories. `security-audit.yml`
# runs `osv-scanner` over the lock; nothing in the offline nix legs does. Bump the hash with
# `nix build .#slides` after any change to `package-lock.json`, and copy the hash it prints.
{ pkgs }:
pkgs.buildNpmPackage {
  pname = "sutura-slides-2026-introducing-sutura";
  version = "0.0.0";

  src = pkgs.lib.fileset.toSource {
    root = ../slides/2026-introducing-sutura;
    fileset = pkgs.lib.fileset.unions [
      ../slides/2026-introducing-sutura/package.json
      ../slides/2026-introducing-sutura/package-lock.json
      ../slides/2026-introducing-sutura/slides.md
      ../slides/2026-introducing-sutura/theme
    ];
  };

  npmDepsHash = "sha256-tAG/p0mj38Db/v3005PZGShhM+ZptPBAWIgUoZ8D7eI=";
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
    cp -r dist $out
    runHook postInstall
  '';
}

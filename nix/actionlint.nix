# The pinned actionlint predates GitHub's `concurrency.queue` and self-repository `$/` references.
# Apply upstream parser and validation patches so `just lint-ci` checks the syntax CI uses.
# GitHub's `.patch` output lengthens the abbreviated blob ids on its `index` lines over time, which
# broke a plain `fetchurl` hash. `postFetch` cuts them to seven characters so the hash survives.
# `fetchpatch` is no substitute: it drops the empty `index.js` diff the self-repository test data needs.
# Remove each patch when the locked actionlint release contains its upstream change.
{ pkgs }:

let
  postFetch = ''
    sed -E -i 's/^index ([0-9a-f]{7})[0-9a-f]*\.\.([0-9a-f]{7})[0-9a-f]*/index \1..\2/' "$out"
  '';
in

pkgs.actionlint.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    (pkgs.fetchurl {
      name = "actionlint-concurrency-queue-644076a59742.patch";
      url = "https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562.patch";
      hash = "sha256-5hwylE1swfGkMOLpQhlb4S69fxSt4mJgTOcP8NR6OXk=";
      inherit postFetch;
    })
    (pkgs.fetchurl {
      name = "actionlint-self-repository-bdfa15b644bc.patch";
      url = "https://github.com/rhysd/actionlint/commit/bdfa15b644bc27b4c5944be36003c0edc9ef41d2.patch";
      hash = "sha256-sXRe84ib6LzW5zeQbzDTvO/GMwHz0vRUTIWqjMlNG68=";
      inherit postFetch;
    })
    (pkgs.fetchurl {
      name = "actionlint-self-repository-cache-b02c24b743cc.patch";
      url = "https://github.com/rhysd/actionlint/commit/b02c24b743cc88a26b280339814eb4ece91c32cb.patch";
      hash = "sha256-UvISTp7EdupSp+D7aU/xX6FsuA0+wuIQlZF07qM7UdA=";
      inherit postFetch;
    })
  ];
  preCheck = "go test .";
})

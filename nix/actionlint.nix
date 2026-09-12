# The pinned actionlint predates GitHub's `concurrency.queue`. Apply the upstream parser and
# validation change so `just lint-ci` remains the authority for the workflow syntax that CI uses.
# Remove this patch when the locked nixpkgs actionlint release contains the upstream commit.
{ pkgs }:

pkgs.actionlint.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    (pkgs.fetchurl {
      name = "actionlint-concurrency-queue-644076a59742.patch";
      url = "https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562.patch";
      sha256 = "1f6cb6325337e5d9e54a6b7b0c56112953b30d16332764f0f21591e67527c1a9";
    })
  ];
  preCheck = "go test .";
})

# The pinned actionlint predates GitHub's `concurrency.queue` and self-repository `$/` references.
# Apply upstream parser and validation patches so `just lint-ci` checks the syntax CI uses.
# Remove each patch when the locked actionlint release contains its upstream change.
{ pkgs }:

pkgs.actionlint.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    (pkgs.fetchurl {
      name = "actionlint-concurrency-queue-644076a59742.patch";
      url = "https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562.patch";
      sha256 = "1f6cb6325337e5d9e54a6b7b0c56112953b30d16332764f0f21591e67527c1a9";
    })
    (pkgs.fetchurl {
      name = "actionlint-self-repository-bdfa15b644bc.patch";
      url = "https://github.com/rhysd/actionlint/commit/bdfa15b644bc27b4c5944be36003c0edc9ef41d2.patch";
      sha256 = "3f39b65bb0af8344426aae886a703cd8cf8c29488f7d6b308c096cef7f01f981";
    })
    (pkgs.fetchurl {
      name = "actionlint-self-repository-cache-b02c24b743cc.patch";
      url = "https://github.com/rhysd/actionlint/commit/b02c24b743cc88a26b280339814eb4ece91c32cb.patch";
      sha256 = "27feceb232f5706cceceb9ade2c4db513efcc63fa4a98e3860c2ec64335508fe";
    })
  ];
  preCheck = "go test .";
})

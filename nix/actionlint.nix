# The pinned actionlint predates GitHub's `concurrency.queue` and self-repository `$/` references.
# Apply upstream parser and validation patches so `just lint-ci` checks the syntax CI uses.
# The patches are vendored under `nix/patches/actionlint/` because GitHub's commit `.patch`
# endpoint is not reliable: it answered 503 and failed every merge-group run. They are the
# endpoint's output with the `index` blob ids cut to seven characters, which GitHub lengthens over time.
# Remove each patch when the locked actionlint release contains its upstream change.
{ pkgs }:

pkgs.actionlint.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    # https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562
    ./patches/actionlint/actionlint-concurrency-queue-644076a59742.patch
    # https://github.com/rhysd/actionlint/commit/bdfa15b644bc27b4c5944be36003c0edc9ef41d2
    ./patches/actionlint/actionlint-self-repository-bdfa15b644bc.patch
    # https://github.com/rhysd/actionlint/commit/b02c24b743cc88a26b280339814eb4ece91c32cb
    ./patches/actionlint/actionlint-self-repository-cache-b02c24b743cc.patch
  ];
  preCheck = "go test .";
})

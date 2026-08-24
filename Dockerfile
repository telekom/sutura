# No `# syntax=` directive. That line makes BuildKit fetch its frontend image from
# docker.io BEFORE any build arg is read, so it cannot be pointed at a mirror and it
# ignored BASE_IMAGE entirely - on a network without direct registry egress the build
# died on `auth.docker.io: i/o timeout` before parsing line 2. Nothing here needs a
# frontend newer than the daemon's built-in one.
#
# Build and develop sutura in a container - which is how you get a Nix toolchain on a
# machine where Nix does not run natively, Windows most obviously.
#
# NOT the release artifact. That is built by Nix itself (`nix build .#oci`): one binary,
# no shell, no package manager. This file is the opposite - a shell with the toolchain in
# it.
#
# TARGETS
#   --target dev     interactive development; enters the repo's devenv via direnv (default)
#   --target build   runs the gates and a release build, in that same shell
#
# EVERYTHING THAT TOUCHES THE NETWORK IS CONFIGURABLE.
#
# Defaults are PUBLIC so anyone can build this repo with no local setup. Every one of them
# is an ARG, because a network that cannot reach the public internet directly must be able
# to point each fetch at its own mirror without editing this file:
#
#   BASE_IMAGE          the FHS base to build on. Override to a mirrored one.
#   NIX_IMAGE           where the Nix store is copied from. Override to a mirrored one.
#   HTTP_PROXY_URL      egress proxy, applied to apt, curl, nix and cargo.
#   NO_PROXY_LIST       hosts to exclude from the proxy.
#   NIX_SUBSTITUTER     Nix binary cache. Override to a mirror to avoid direct egress.
#   NIX_TRUSTED_KEY     public key for that cache. Must match the substituter.
#   CARGO_REGISTRY_URL  sparse crates index. Override to a mirror.
#
# Put the values in a gitignored `.env` and let compose pass them (see compose.dev.yaml),
# or supply them directly:
#
#   docker build --target dev -t sutura-dev \
#     --build-arg BASE_IMAGE=<mirror>/debian:13-slim \
#     --build-arg NIX_IMAGE=<mirror>/nixos/nix:2.35.2 \
#     --build-arg HTTP_PROXY_URL=<proxy> \
#     --build-arg NIX_SUBSTITUTER=<mirror-nix-remote> \
#     --build-arg CARGO_REGISTRY_URL=<mirror-crates-index> .

# An FHS base with the Nix store copied in - NOT the nixos/nix image.
#
# This was `FROM nixos/nix` and could not work for half of what the container is for. A
# dynamically-linked Linux binary names its dynamic loader by ABSOLUTE PATH in its ELF
# header, and for x86-64 glibc that path is `/lib64/ld-linux-x86-64.so.2`. NixOS has no such
# file by design - every Nix-built binary is patched to point into the store instead.
# conda-forge binaries are built for ordinary distributions and name the FHS path, so on a
# NixOS-derived image the kernel finds nothing there and exec fails with ENOENT, which bash
# reports as "required file not found" - reading like a missing library rather than a
# missing loader.
#
# The consequence was not subtle: `pixi install` succeeded and NOTHING it installed could
# run. prek, and therefore every git hook, plus mkdocs and mike, were unusable here.
#
# Debian is FHS-native, so both ecosystems work unpatched, and the container now matches CI
# - GitHub runners are glibc/FHS too, so "works in the container" says something about CI.
# Nix arrives by COPYing the store out of the pinned nix image rather than by running an
# installer: no `curl | sh` in the build, nothing unpinned, and no network for that layer.
ARG BASE_IMAGE=debian:13-slim
ARG NIX_IMAGE=nixos/nix:2.35.2

# ---------------------------------------------------------------------- nix ---
# Present only to be copied out of. Nothing is built here.
FROM ${NIX_IMAGE} AS nix

# ------------------------------------------------------------------ toolchain ---
# Shared by both targets, so the Nix/devenv setup exists once rather than twice.
FROM ${BASE_IMAGE} AS toolchain

# The store, plus the environment Nix needs to find itself in it. These are the nix
# image's own values. `/root/.nix-profile/bin` FIRST and not optional: `nix profile add`
# installs there, so without it devenv and direnv install successfully and are then not
# found. The store CA bundle matters too - without
# which every substituter fetch fails certificate verification.
COPY --from=nix /nix /nix
ENV PATH="/root/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/nix/var/nix/profiles/default/sbin:${PATH}" \
    SSL_CERT_FILE=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt \
    GIT_SSL_CAINFO=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt \
    NIX_SSL_CERT_FILE=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt

# What a slim base does not have and Nix needs anyway. `git` because evaluating a flake in a
# git working tree shells out to it; `ca-certificates` for everything that is not Nix (cargo,
# pixi, curl), since the store bundle above only covers Nix's own fetches. Proxy variables are
# already exported above, so apt inherits them.
RUN set -eu;     apt-get update;     apt-get install -y --no-install-recommends       ca-certificates       curl       git       xz-utils;     rm -rf /var/lib/apt/lists/*

ARG HTTP_PROXY_URL=""
ARG NO_PROXY_LIST="localhost,127.0.0.1,::1"
# ONE substituter, deliberately. A second was added here on the theory that nixpkgs `devenv`
# was not in the public cache. It is: `cache.nixos.org` returns 200 for its narinfo and
# `devenv.cachix.org` returns 404 for the same path, so the addition served nothing and
# cost a trusted key. Every substituter is a key that must be trusted - add one only with
# evidence that a path we need comes from it and from nowhere else.
#
# Two failure modes worth recognising before reaching for another one:
#   - A bootstrap chain in the log (stage0-posix, sed, tar) means NOTHING is substituting,
#     not that one package is missing. One uncached package cannot trigger a stage0 build.
#   - Point this at a mirror's PLAIN path, never an `/api/nix/` one. Both answer
#     `nix-cache-info` and `.narinfo` with 200, so `nix build --dry-run` reports "will be
#     fetched" either way - but the `/api/nix/` form emits a `URL:` field carrying a
#     `?narInfoHash=` parameter that then 404s, and Nix responds by compiling from source
#     rather than failing. See .env.example for the two-command check.
ARG NIX_SUBSTITUTER="https://cache.nixos.org"
ARG NIX_TRUSTED_KEY="cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
ARG CARGO_REGISTRY_URL=""

ENV http_proxy=${HTTP_PROXY_URL} \
    https_proxy=${HTTP_PROXY_URL} \
    HTTP_PROXY=${HTTP_PROXY_URL} \
    HTTPS_PROXY=${HTTP_PROXY_URL} \
    no_proxy=${NO_PROXY_LIST} \
    NO_PROXY=${NO_PROXY_LIST}

# Flakes are a prerequisite, not a preference: flake.nix is how the release image and the
# cross-compiled binaries are built. Written idempotently so a base that already provides
# this is left alone.
RUN set -eu; \
    mkdir -p /etc/nix; \
    grep -q 'experimental-features' /etc/nix/nix.conf 2>/dev/null \
      || printf 'experimental-features = nix-command flakes\nmax-jobs = auto\n' >> /etc/nix/nix.conf; \
    printf 'substituters = %s\ntrusted-public-keys = %s\n' "${NIX_SUBSTITUTER}" "${NIX_TRUSTED_KEY}" \
      >> /etc/nix/nix.conf; \
    echo "substituters: ${NIX_SUBSTITUTER}"; \
    nix --version

# devenv and direnv, from the rev in flake.lock rather than from the flake registry.
#
# `nix profile install nixpkgs#devenv` was two bugs in one line. It resolves `nixpkgs` through
# the registry, which fetches channels.nixos.org - so this stage failed outright on a network
# that only permits a mirror, which is the network this file exists to support. And it is
# unpinned, so the image got whatever nixpkgs-unstable was that day: two people building the
# same Dockerfile got different devenv versions, and the layer's cache key said nothing about
# what was in it.
#
# An explicit flake URL consults no registry, and the rev comes from the lock every other build
# path already uses. flake.lock is copied alone so this layer survives edits to devenv.nix,
# which changes far more often.
COPY flake.lock ./
RUN set -eu; \
    rev="$(nix eval --impure --raw --expr \
      '(builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.nixpkgs.locked.rev')"; \
    echo "nixpkgs pinned at ${rev}"; \
    command -v devenv >/dev/null 2>&1 || nix profile add "github:NixOS/nixpkgs/${rev}#devenv"; \
    command -v direnv >/dev/null 2>&1 || nix profile add "github:NixOS/nixpkgs/${rev}#direnv"; \
    devenv version; direnv --version

# Route cargo at a mirrored index when one is configured. Written to /etc so it does not
# collide with the repo's own .cargo/config.toml, which owns linker settings.
RUN set -eu; \
    if [ -n "${CARGO_REGISTRY_URL}" ]; then \
      mkdir -p /usr/local/cargo; \
      printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "sparse+%s"\n' \
        "${CARGO_REGISTRY_URL}" > /usr/local/cargo/config.toml; \
      echo "cargo index -> ${CARGO_REGISTRY_URL}"; \
    else echo "cargo index -> default (crates.io)"; fi
ENV CARGO_HOME=/usr/local/cargo

WORKDIR /work

# ------------------------------------------------------------------------ dev ---
FROM toolchain AS dev

# direnv does nothing without a shell hook. Without it the .envrc silently never loads and
# people fall back to a system toolchain - the exact failure .envrc exists to prevent.
# The hook only. NOT `direnv allow`: that prompt is the trust boundary, and pre-approving it
# means checking out an unreviewed branch and entering the container executes that branch's
# enterShell. The message says what to run.
RUN printf '\neval "$(direnv hook bash)"\necho "run: direnv allow /work"\n' \
      >> /etc/bash.bashrc

# Warm the store from the environment definition alone, so a bind-mounted source tree does
# not trigger a cold build on first use, and so this layer survives code changes.
# flake.lock came in above; re-copying it here would put it back in the same cache key as
# devenv.nix and undo the split.
COPY devenv.nix devenv.yaml* devenv.lock* rust-toolchain.toml flake.nix ./
RUN devenv build 2>/dev/null || echo "devenv warm-up skipped (inputs incomplete at build time)"

# `just setup` on entry, hash-gated. See nix/container-setup.sh for why this is an entrypoint
# rather than a build step: `.git` is dockerignored, so hooks cannot be installed at build
# time, and the caches setup warms live in named volumes that mask the image's own paths.
COPY nix/container-setup.sh /usr/local/lib/sutura-setup.sh
ENTRYPOINT ["bash", "/usr/local/lib/sutura-setup.sh"]
CMD ["/bin/bash", "-l"]

# ---------------------------------------------------------------------- build ---
# A real build in the same shell developers use, so "works on my machine" cannot reappear.
# Runs the gates first: an image that compiles but fails clippy is not a successful build.
FROM toolchain AS build

COPY . .
RUN devenv shell gates
RUN devenv shell -- cargo build --profile release --workspace

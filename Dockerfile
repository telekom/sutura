# syntax=docker/dockerfile:1.7
#
# Build and develop sutura in a container — which is how you get a Nix toolchain on a
# machine where Nix does not run natively, Windows most obviously.
#
# NOT the release artifact. That is built by Nix itself (`nix build .#oci`): one binary,
# no shell, no package manager. This file is the opposite — a shell with the toolchain in
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
#   BASE_IMAGE          the image to build on. Override to a mirrored one.
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
#     --build-arg BASE_IMAGE=<mirror>/nixos/nix:2.35.2 \
#     --build-arg HTTP_PROXY_URL=<proxy> \
#     --build-arg NIX_SUBSTITUTER=<mirror-nix-remote> \
#     --build-arg CARGO_REGISTRY_URL=<mirror-crates-index> .

ARG BASE_IMAGE=nixos/nix:2.35.2

# ------------------------------------------------------------------ toolchain ---
# Shared by both targets, so the Nix/devenv setup exists once rather than twice.
FROM ${BASE_IMAGE} AS toolchain

ARG HTTP_PROXY_URL=""
ARG NO_PROXY_LIST="localhost,127.0.0.1,::1"
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
    nix --version

# devenv and direnv only if the base lacks them — /nix is a large, perfectly cacheable
# layer, so a base that already ships them should not pay for it again here.
RUN set -eu; \
    command -v devenv >/dev/null 2>&1 || nix profile install nixpkgs#devenv; \
    command -v direnv >/dev/null 2>&1 || nix profile install nixpkgs#direnv; \
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
# people fall back to a system toolchain — the exact failure .envrc exists to prevent.
RUN printf '\neval "$(direnv hook bash)"\n[ -f /work/.envrc ] && direnv allow /work >/dev/null 2>&1 || true\n' \
      >> /etc/bash.bashrc

# Warm the store from the environment definition alone, so a bind-mounted source tree does
# not trigger a cold build on first use, and so this layer survives code changes.
COPY devenv.nix devenv.yaml* devenv.lock* rust-toolchain.toml flake.nix flake.lock* ./
RUN devenv build 2>/dev/null || echo "devenv warm-up skipped (inputs incomplete at build time)"

CMD ["/bin/bash", "-l"]

# ---------------------------------------------------------------------- build ---
# A real build in the same shell developers use, so "works on my machine" cannot reappear.
# Runs the gates first: an image that compiles but fails clippy is not a successful build.
FROM toolchain AS build

COPY . .
RUN devenv shell gates
RUN devenv shell -- cargo build --profile release --workspace

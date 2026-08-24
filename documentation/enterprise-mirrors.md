# Building without direct internet egress

Many organisations put an artifact repository (Artifactory, Nexus, or similar) in front of
every public package source, and either block direct egress or force it through an
authenticating proxy. This repo is designed for that case: **nothing that touches the network
is hardcoded.** Every fetch reads its location from the environment, with a public default so
the repo still builds for anyone.

No internal hostname, repository name or credential appears anywhere in this repository. The
tables below give the *shape* of each setting; the values are yours.

## The rule of thumb

**Prefer the mirror over the proxy.** Both can work, but a mirror is a normal HTTPS endpoint
your build reaches directly, while a proxy sits in the path of every connection and is where
TLS interception, `407 Proxy Authentication Required` and half-cached failures come from. If
your organisation offers a mirror for a package type, point at the mirror and leave the proxy
out of that path entirely.

A useful diagnostic: if you see `407`, or TLS errors naming a CA you do not recognise, you are
going through the proxy for something that has a mirror.

## Conventions used below

| Placeholder | Means |
| --- | --- |
| `<repo-host>` | your artifact repository's hostname |
| `<pypi>` | a PyPI remote/virtual repository |
| `<conda>` | a conda remote/virtual repository |
| `<crates>` | a Cargo remote repository |
| `<nix-cache>` | a Nix binary-cache remote repository |
| `<docker>` | a Docker registry mirror, usually reached as a hostname prefix |
| `<generic>` | a "generic"/raw remote, for release tarballs and similar |

---

## 1. Nix - the binary cache

Nix fetches prebuilt store paths from a **substituter**. Point it at your mirror:

```conf
# ~/.config/nix/nix.conf   (or /etc/nix/nix.conf for the daemon)
substituters = https://<repo-host>/<path-to>/<nix-cache>
trusted-public-keys = cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=
```

Two things people get wrong here:

1. **Keep the upstream public key.** A mirror that proxies the public cache serves NARs signed
   by *upstream*, and merely relays them. Substituting your mirror's own key would reject
   every path. Only a cache that re-signs needs its own key.
2. **In multi-user mode the daemon does the fetching.** Editing `~/.config/nix/nix.conf` and
   seeing no change is the usual symptom - the setting belongs in `/etc/nix/nix.conf`, and the
   daemon needs a restart. Alternatively add yourself to `trusted-users` so your per-user
   substituters are honoured.

### If the cache requires authentication

Nix authenticates via netrc, not via a URL with credentials in it:

```conf
# ~/.config/nix/nix.conf
netrc-file = /home/<you>/.config/nix/netrc
```

```
# that netrc file, mode 0600
machine <repo-host>
  login <user>
  password <api-token-or-identity-token>
```

Use a scoped API/identity token, never your account password. A missing or unreadable netrc
shows up as `HTTP error 401` on a `.narinfo` while `nix-cache-info` still succeeds - the
cache-info endpoint is often readable anonymously, which makes the failure look inconsistent
until you know this.

### Flake inputs

Flake inputs are fetched from their source forge, not from the binary cache - but once locked,
their unpacked source *is* a store path, and a substituter that mirrors the public cache
usually serves it. So a locked build often needs no forge access at all. When it does, either
allow the forge specifically, or use a generic remote:

```conf
# Rewrites where a flake input is fetched from, without editing flake.nix.
# Note: nix.conf has no environment-variable expansion; write the literal URL.
access-tokens = <repo-host>=<token>
```

`flake.lock` pins content by hash, so a mirror cannot substitute different content undetected.

---

## 2. Rust - crates and the toolchain

### The crates index and downloads

Cargo reads a sparse index. Configure the replacement globally so it applies to every
checkout, and so this repo's own `.cargo/config.toml` (which owns linker settings) stays
untouched:

```toml
# ~/.cargo/config.toml
[source.crates-io]
replace-with = "mirror"

[source.mirror]
registry = "sparse+https://<repo-host>/<path-to>/<crates>/index/"
```

The trailing slash matters, and `sparse+` is required for an HTTP index - without it cargo
expects a git index and fails with a confusing clone error.

If it needs credentials:

```toml
# ~/.cargo/config.toml
[registries.mirror]
index = "sparse+https://<repo-host>/<path-to>/<crates>/index/"
credential-provider = "cargo:token"
```

```toml
# ~/.cargo/credentials.toml, mode 0600
[registries.mirror]
token = "Bearer <token>"
```

### The toolchain itself

`rustup` fetches from `static.rust-lang.org`. Redirect it with:

```bash
export RUSTUP_DIST_SERVER=https://<repo-host>/<path-to>/<generic>/static.rust-lang.org
export RUSTUP_UPDATE_ROOT=https://<repo-host>/<path-to>/<generic>/static.rust-lang.org/rustup
```

In the Nix path this is moot: `flake.nix` and `devenv.nix` both resolve
`rust-toolchain.toml` through `rust-overlay`, so the compiler arrives as a store path from
the substituter and rustup is not involved.

One caveat worth knowing: an **exact** version pin (`channel = "1.xx.y"`) requires that
specific version to exist on the mirror. A remote that lazily caches on first request will
`404` until something asks for it, which reads like "this version does not exist".

---

## 3. Python - pip, conda, and pixi

This repo uses **pixi** for Python-delivered tooling. Pixi resolves conda packages through
rattler and PyPI packages through uv, so it has its own configuration - and the cleanest place
for it is pixi's *global* config, which keeps mirror URLs out of the repo entirely:

```toml
# ~/.pixi/config.toml   (global; not in the repo)
[pypi-config]
index-url = "https://<repo-host>/<path-to>/<pypi>/simple"

[mirrors]
# Anything resolving against anaconda.org is served from your conda mirror instead.
"https://conda.anaconda.org" = [
    "https://<repo-host>/<path-to>/<conda>"
]
```

This is the recommended arrangement: `pixi.toml` in the repo names only public channels, and
the global config redirects them. Nobody has to edit a committed file, and no internal URL is
ever committed.

Per-invocation alternatives, useful in CI or a container:

```bash
export UV_INDEX_URL=https://<repo-host>/<path-to>/<pypi>/simple    # pypi-dependencies
export RATTLER_MIRROR_...                                          # see pixi's docs; prefer the config file
```

For plain pip and conda outside pixi:

```ini
# ~/.pip/pip.conf  (or %APPDATA%\pip\pip.ini on Windows)
[global]
index-url = https://<repo-host>/<path-to>/<pypi>/simple
```

```yaml
# ~/.condarc
channels:
  - https://<repo-host>/<path-to>/<conda>/conda-forge
default_channels:
  - https://<repo-host>/<path-to>/<conda>/main
channel_alias: https://<repo-host>/<path-to>/<conda>
```

`channel_alias` is the one that catches people: without it, a bare channel name like
`conda-forge` still resolves against anaconda.org.

---

## 4. Docker images

Registry mirrors are usually reached as a **hostname prefix** rather than a config setting:

```
<docker-mirror-host>/library/debian:trixie      instead of   debian:trixie
<docker-mirror-host>/nixos/nix:2.35.2           instead of   nixos/nix:2.35.2
```

On such a network an unprefixed reference does not fall back - it fails. This is why every
image in this repo is a build argument:

```bash
docker build --target dev -t sutura-dev \
  --build-arg BASE_IMAGE=<docker-mirror-host>/nixos/nix:2.35.2 \
  --build-arg NIX_SUBSTITUTER=https://<repo-host>/<path-to>/<nix-cache> \
  --build-arg CARGO_REGISTRY_URL=https://<repo-host>/<path-to>/<crates>/index .
```

Or put the values in a gitignored `.env` and let `compose.dev.yaml` pass them - see
`.env.example` for the full list of names.

---

## 5. TLS interception

If your egress proxy terminates TLS, every tool needs your organisation's CA. Installing it
into the OS trust store is necessary but **not sufficient**, because several toolchains ship
their own bundle:

| Tool | How it finds a CA |
| --- | --- |
| curl, git | `CURL_CA_BUNDLE`, `GIT_SSL_CAINFO`, or the OS store |
| Python / pip | `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `PIP_CERT` |
| Node | `NODE_EXTRA_CA_CERTS` |
| Rust (reqwest, cargo) | the OS store, or `SSL_CERT_FILE` for rustls builds |
| Nix | `NIX_SSL_CERT_FILE`; the **daemon's** environment in multi-user mode |

The container in this repo sets `SSL_CERT_FILE` from `pkgs.cacert`, so add your CA to the
image if it must trust an interception CA.

---

## 6. Proxy variables, if you must

When there is no mirror for something, a proxy is the fallback. Set the lowercase and
uppercase forms - different tools read different ones - and always set `no_proxy`, or internal
hosts get sent to the proxy and fail:

```bash
export http_proxy=http://<proxy-host>:<port>  HTTP_PROXY=$http_proxy
export https_proxy=$http_proxy                HTTPS_PROXY=$http_proxy
export no_proxy="localhost,127.0.0.1,::1,<repo-host>,.<internal-domain>"
export NO_PROXY=$no_proxy
```

Nix in multi-user mode is the exception people lose an afternoon to: the **daemon** performs
the download, so exporting a proxy in your shell changes nothing. It belongs in the daemon's
environment (a systemd drop-in on Linux, a launchd override on macOS).

---

## Checking your setup

Each of these should succeed without touching the public internet:

```bash
# Nix cache reachable and authenticated
curl -fsS https://<repo-host>/<path-to>/<nix-cache>/nix-cache-info

# ... and able to serve an actual path, which is the part that needs credentials
nix build --print-out-paths nixpkgs#hello

# Crates index
curl -fsS https://<repo-host>/<path-to>/<crates>/index/config.json

# Python
pixi run python -c "print('ok')"

# Docker
docker pull <docker-mirror-host>/nixos/nix:2.35.2
```

If `nix-cache-info` returns `200` but a build reports `401`, the cache is readable anonymously
while artifacts are not - go back to the netrc step in §1.

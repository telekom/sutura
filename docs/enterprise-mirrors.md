---
title: Building without direct egress
description: Where each toolchain reads its package source, and the traps behind a mirror or a proxy.
---

# Building without direct egress

Nothing that touches the network is hardcoded here. Every fetch reads its location from the
environment, with a public default. No internal hostname or credential appears in this repository:
the tables give the *shape*, the values are yours.

!!! tip "Prefer a mirror over a proxy"

    A mirror is a normal HTTPS endpoint. A proxy sits in every connection and is where `407` and
    TLS-interception failures come from. A `407` usually means you are proxying something that has
    a mirror.

| Placeholder                                             | Means                                         |
| ------------------------------------------------------- | --------------------------------------------- |
| `<host>`                                                | your artifact repository                      |
| `<pypi>` `<conda>` `<crates>` `<nix-cache>` `<generic>` | the corresponding remote repo                 |
| `<docker-mirror>`                                       | registry mirror, reached as a hostname prefix |

## Nix

```conf
# ~/.config/nix/nix.conf, or /etc/nix/nix.conf for the daemon
substituters = https://<host>/<path>/<nix-cache>
trusted-public-keys = cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=
```

Two traps:

- **Keep upstream's key.** A mirror relays NARs signed by upstream, so its own key would reject
  every path. Only a cache that re-signs needs one.
- **In multi-user mode the daemon fetches**, so editing `~/.config/nix/nix.conf` changes nothing.
  Use `/etc/nix/nix.conf` and restart the daemon, or add yourself to `trusted-users`.

If the cache needs credentials, Nix uses netrc rather than a URL with a password in it:

```conf
netrc-file = /home/<you>/.config/nix/netrc
```

```
machine <host>
  login <user>
  password <api-token>
```

A missing netrc shows up as `HTTP error 401` on a `.narinfo` while `nix-cache-info` succeeds,
because cache-info is often anonymous. Use a scoped token, mode 0600.

Flake inputs come from their forge, but once locked their unpacked source is a store path a mirror
usually serves, so a locked build often needs no forge access. `flake.lock` pins by hash, so a
mirror cannot substitute different content undetected.

## Rust

```toml
# ~/.cargo/config.toml - global, so this repo's .cargo/config.toml keeps owning linker settings
[source.crates-io]
replace-with = "mirror"

[source.mirror]
registry = "sparse+https://<host>/<path>/<crates>/index/"
```

`sparse+` is required and the trailing slash matters. Without them cargo expects a git index and
fails with a confusing clone error.

`rustup` is not involved in the Nix path. `flake.nix` and `devenv.nix` resolve the single pinned
toolchain through rust-overlay from `devco/rust-toolchain-nightly.toml` (the top-level
`rust-toolchain.toml` is the rustup-facing copy of the same pin) - so the compilers come from the
Nix cache and the crates mirror above is all this section needs.

If you do use rustup, set `RUSTUP_DIST_SERVER` and `RUSTUP_UPDATE_ROOT`. An exact version pin needs
that version to exist on the mirror, and a lazily-caching remote `404`s until something asks, which
reads as "no such version".

## Conda, via pixi

pixi is the one resolver for Python and conda here, and `pixi.toml` declares one channel:

```toml
channels = ["conda-forge"]
```

That name is public because this repository is public. The redirect to a mirror goes in pixi's
**global** configuration instead, so the manifest resolves identically for everybody and no internal
URL is ever committed:

```toml
# ~/.pixi/config.toml
[mirrors]
"https://conda.anaconda.org" = ["https://<host>/<path>/<conda>"]
```

The key is the upstream URL, the value is a list, and pixi matches the **longest** key prefix - so
the entry above covers every channel on that host, and a longer key overrides it for one channel.

Two things follow from the value being a list. The original URL is not tried unless you repeat it,
and repodata is fetched from the **first** entry - that file carries the SHA256 of every package,
so it decides what all the later downloads are checked against.

Check what pixi reads rather than what you wrote:

```bash
pixi config list      # the merged values, all layers
pixi info -vvv        # every location searched, in priority order
```

### Where that file lives

Highest priority wins, and a project-local `.pixi/config.toml` is merged on top of all of it -
which is the one file a mirror URL must not go in, because it is inside the repository.

|        | Linux                                                                  | macOS                                            | Windows                                                           |
| ------ | ---------------------------------------------------------------------- | ------------------------------------------------ | ----------------------------------------------------------------- |
| Global | `$PIXI_HOME/config.toml`, else `~/.pixi/config.toml`                   | the same                                         | `%PIXI_HOME%\config.toml`, else `%USERPROFILE%\.pixi\config.toml` |
| User   | `$XDG_CONFIG_HOME/pixi/config.toml`, else `~/.config/pixi/config.toml` | `~/Library/Application Support/pixi/config.toml` | `%APPDATA%\pixi\config.toml`                                      |
| System | `/etc/pixi/config.toml`                                                | `/etc/pixi/config.toml`                          | `C:\ProgramData\pixi\config.toml`                                 |

`--no-config` skips the system and user layers; `--config-file <path>` replaces them with one file.
Both reproduce a resolve without your machine's settings in it.

## pip

**This repository installs nothing from PyPI.** `pixi.toml` has `[dependencies]` only - conda
packages from `conda-forge` - and no `[pypi-dependencies]` table, so no pip, no uv and no PyPI
index takes part in any build, gate or docs render. A `pip.conf` on your machine changes nothing
here.

If you add a `[pypi-dependencies]` entry, pixi resolves it with uv. One thing first:

```toml
# ~/.pixi/config.toml
[pypi-config]
index-url = "https://<host>/<path>/<pypi>/simple"
```

**That does not redirect anything.** Per pixi's documented behaviour, `index-url` and
`extra-index-urls` in the global config are written into a manifest by `pixi init` and are
otherwise not interpreted, because the manifest is meant to be complete on its own. Only
`keyring-provider` and `allow-insecure-host` apply globally.

What does redirect uv is `[mirrors]`, and it needs **two** entries, because the index and the files
are served from different hosts:

```toml
# ~/.pixi/config.toml
[mirrors]
"https://pypi.org/simple" = ["https://<host>/<path>/<pypi>/simple"]
"https://files.pythonhosted.org/packages" = [
  "https://<host>/<path>/<pypi>/packages",
]
```

!!! warning "The failure that looks like a hang"

    With the first entry and not the second, the resolve succeeds against the mirror and every
    download then goes to the public host.

If you run plain `pip` or `conda` on the same machine for other work, they read their own files and
neither is used by this repository:

| Tool         | File                                                                                                    | Key                          |
| ------------ | ------------------------------------------------------------------------------------------------------- | ---------------------------- |
| pip, Linux   | `~/.config/pip/pip.conf` (`$XDG_CONFIG_HOME` honoured)                                                  | `index-url` under `[global]` |
| pip, macOS   | `~/Library/Application Support/pip/pip.conf` where that directory exists, else `~/.config/pip/pip.conf` | the same                     |
| pip, Windows | `%APPDATA%\pip\pip.ini`                                                                                 | the same                     |
| conda        | `~/.condarc`                                                                                            | `channel_alias`              |

`~/.pip/pip.conf` is the legacy path and still works; `pip config debug` prints the exact list. In
`.condarc`, `channel_alias` defaults to anaconda.org, so without it a bare `conda-forge` resolves
there no matter what else you set; `conda config --show-sources` prints what is in effect.

## Docker

Registry mirrors are a hostname prefix, not a setting:

```
<docker-mirror>/nixos/nix:2.35.2      instead of   nixos/nix:2.35.2
```

On such a network an unprefixed reference does not fall back, it fails. So every image is a build
argument:

```bash
docker build --target dev -t sutura-dev \
  --build-arg BASE_IMAGE=<docker-mirror>/nixos/nix:2.35.2 \
  --build-arg NIX_SUBSTITUTER=https://<host>/<path>/<nix-cache> \
  --build-arg CARGO_REGISTRY_URL=https://<host>/<path>/<crates>/index .
```

`.env.example` has the full list; `compose.dev.yaml` reads them from a gitignored `.env`.

## TLS interception

Installing your CA in the OS store is necessary but not sufficient, because several toolchains ship
their own bundle:

| Tool        | Variable                                                                  |
| ----------- | ------------------------------------------------------------------------- |
| curl, git   | `CURL_CA_BUNDLE`, `GIT_SSL_CAINFO`                                        |
| Python, pip | `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`                                     |
| Node        | `NODE_EXTRA_CA_CERTS`                                                     |
| Nix         | `NIX_SSL_CERT_FILE` - in the **daemon's** environment for multi-user mode |

## Proxy, where there is no mirror

Set both cases, and always set `no_proxy`, or internal hosts get sent to the proxy and fail:

```bash
export https_proxy=http://<proxy>:<port>  HTTPS_PROXY=$https_proxy
export no_proxy="localhost,127.0.0.1,::1,<host>"  NO_PROXY=$no_proxy
```

Nix in multi-user mode is the exception: the daemon downloads, so a proxy exported in your shell
changes nothing. It belongs in the daemon's environment.

## Check it

```bash
curl -fsS https://<host>/<path>/<nix-cache>/nix-cache-info
nix build --print-out-paths nixpkgs#hello        # the part that needs credentials
curl -fsS https://<host>/<path>/<crates>/index/config.json
pixi run --frozen python -c "print('ok')"
docker pull <docker-mirror>/nixos/nix:2.35.2
```

`nix-cache-info` returning 200 while a build reports 401 means artifacts need credentials: back to
the netrc step.

## Why none of these values are in the repository

They configure a **network**, not a project. A contributor on a different network needs different
ones, and both would be wrong for the public CI runner, which needs none at all. This repository is
also public, so committing them would publish the shape of an internal estate to everybody who
clones it.

Hence the split: public defaults in the manifests, file locations here, values on your machine.
`.env.example` documents each build argument the container build takes, and the trap that goes with
it.

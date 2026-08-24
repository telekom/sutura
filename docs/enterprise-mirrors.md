# Building without direct internet egress

Nothing that touches the network is hardcoded here. Every fetch reads its location from the
environment, with a public default. No internal hostname or credential appears in this repo;
the tables give the *shape*, the values are yours.

**Prefer a mirror over a proxy.** A mirror is a normal HTTPS endpoint; a proxy sits in every
connection and is where `407` and TLS-interception failures come from. Seeing `407` usually
means you are proxying something that has a mirror.

| Placeholder | Means |
| --- | --- |
| `<host>` | your artifact repository |
| `<pypi>` `<conda>` `<crates>` `<nix-cache>` `<generic>` | the corresponding remote repo |
| `<docker-mirror>` | registry mirror, reached as a hostname prefix |

## Nix

```conf
# ~/.config/nix/nix.conf, or /etc/nix/nix.conf for the daemon
substituters = https://<host>/<path>/<nix-cache>
trusted-public-keys = cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=
```

Two traps:

- **Keep upstream's key.** A mirror relays NARs signed by upstream. Its own key would reject
  every path. Only a cache that re-signs needs one.
- **In multi-user mode the daemon fetches**, so editing `~/.config/nix/nix.conf` changes
  nothing. Use `/etc/nix/nix.conf` and restart the daemon, or add yourself to `trusted-users`.

If the cache needs credentials, Nix uses netrc - not a URL with a password in it:

```conf
netrc-file = /home/<you>/.config/nix/netrc
```

```
machine <host>
  login <user>
  password <api-token>
```

Symptom of a missing netrc: `HTTP error 401` on a `.narinfo` while `nix-cache-info` succeeds,
because cache-info is often anonymous. Use a scoped token, mode 0600.

Flake inputs come from their forge, but once locked their unpacked source is a store path a
mirror usually serves - so a locked build often needs no forge access. `flake.lock` pins by
hash, so a mirror cannot substitute different content undetected.

## Rust

```toml
# ~/.cargo/config.toml - global, so this repo's .cargo/config.toml keeps owning linker settings
[source.crates-io]
replace-with = "mirror"

[source.mirror]
registry = "sparse+https://<host>/<path>/<crates>/index/"
```

`sparse+` is required and the trailing slash matters; without them cargo expects a git index
and fails with a confusing clone error.

`rustup` is not involved in the Nix path. `flake.nix` and `devenv.nix` resolve both pinned
toolchains through rust-overlay - `rust-toolchain.toml`, and `rust-toolchain-nightly.toml`
for the local inner loop - so the compilers come from the Nix cache rather than from a rustup
mirror, and the crates mirror above is all this section needs. If you do use rustup, set
`RUSTUP_DIST_SERVER` and `RUSTUP_UPDATE_ROOT`. Note that an exact version pin needs that
version to exist on the mirror; a lazily-caching remote `404`s until something asks, which
reads as "no such version".

## Python and conda, via pixi

Put it in pixi's **global** config, which keeps mirror URLs out of the repo entirely:

```toml
# ~/.pixi/config.toml
[pypi-config]
index-url = "https://<host>/<path>/<pypi>/simple"

[mirrors]
"https://conda.anaconda.org" = ["https://<host>/<path>/<conda>"]
```

`pixi.toml` then names only public channels and the global config redirects them. `UV_INDEX_URL`
works per-invocation for CI or a container.

For plain pip and conda, `~/.pip/pip.conf` and `~/.condarc`. In `.condarc`, `channel_alias` is
the one people miss: without it a bare `conda-forge` still resolves against anaconda.org.

## Docker

Registry mirrors are a hostname prefix, not a setting:

```
<docker-mirror>/nixos/nix:2.35.2      instead of   nixos/nix:2.35.2
```

On such a network an unprefixed reference does not fall back, it fails. Hence every image is
a build argument:

```bash
docker build --target dev -t sutura-dev \
  --build-arg BASE_IMAGE=<docker-mirror>/nixos/nix:2.35.2 \
  --build-arg NIX_SUBSTITUTER=https://<host>/<path>/<nix-cache> \
  --build-arg CARGO_REGISTRY_URL=https://<host>/<path>/<crates>/index .
```

See `.env.example` for the full list; `compose.dev.yaml` reads them from `.env`.

## TLS interception

Installing your CA in the OS store is necessary but not sufficient - several toolchains ship
their own bundle:

| Tool | Variable |
| --- | --- |
| curl, git | `CURL_CA_BUNDLE`, `GIT_SSL_CAINFO` |
| Python, pip | `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE` |
| Node | `NODE_EXTRA_CA_CERTS` |
| Nix | `NIX_SSL_CERT_FILE` - in the **daemon's** environment for multi-user mode |

## Proxy, where there is no mirror

Set both cases, and always set `no_proxy` or internal hosts get sent to the proxy and fail:

```bash
export https_proxy=http://<proxy>:<port>  HTTPS_PROXY=$https_proxy
export no_proxy="localhost,127.0.0.1,::1,<host>"  NO_PROXY=$no_proxy
```

Nix in multi-user mode is the exception that costs people an afternoon: the daemon downloads,
so a proxy exported in your shell changes nothing. It belongs in the daemon's environment.

## Check it

```bash
curl -fsS https://<host>/<path>/<nix-cache>/nix-cache-info
nix build --print-out-paths nixpkgs#hello        # the part that needs credentials
curl -fsS https://<host>/<path>/<crates>/index/config.json
pixi run python -c "print('ok')"
docker pull <docker-mirror>/nixos/nix:2.35.2
```

`nix-cache-info` returning 200 while a build reports 401 means artifacts need credentials -
back to the netrc step.

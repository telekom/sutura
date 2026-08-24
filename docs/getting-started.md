# Getting started

| Your machine | Route |
| --- | --- |
| Linux, macOS, WSL2 | Nix + devenv + direnv |
| Windows without WSL2 | the dev container |

Both read the same `devenv.nix` and `rust-toolchain.toml`. Behind a proxy or without direct
egress, read [enterprise-mirrors.md](enterprise-mirrors.md) first.

## Nix natively

```bash
curl -L https://nixos.org/nix/install | sh -s -- --daemon
mkdir -p ~/.config/nix
printf 'experimental-features = nix-command flakes\n' >> ~/.config/nix/nix.conf
nix profile install nixpkgs#devenv nixpkgs#direnv
```

Flakes are a prerequisite, not a preference: the build *is* a flake.

Hook direnv into your shell - it does nothing until you do, and this is the step people skip:

```bash
echo 'eval "$(direnv hook bash)"' >> ~/.bashrc   # or zsh / fish equivalent
```

Then once per clone:

```bash
direnv allow    # a security boundary: you have read .envrc and consent to it running
```

Verify:

```bash
cargo --version                 # must match rust-toolchain.toml
gates                           # everything CI runs
```

If `cargo` is missing after the shell loads, the shell evaluated but produced no toolchain -
check that `devenv.nix` resolves `rust-toolchain.toml` through rust-overlay.

## The dev container

```bash
docker build --target dev -t sutura-dev .
docker run -it --rm -v "$PWD:/work" -w /work sutura-dev
```

`--target build` runs the gates and a release build instead, which is the cheapest proof the
container works. Every network-touching argument is a build `ARG`; `compose.dev.yaml` reads
them from a gitignored `.env`.

If you also build natively with `cargo` on Windows, keep the clone somewhere your endpoint
tooling allows build scripts to execute. Cargo runs `build.rs` and proc macros, and a blocked
execution surfaces as a confusing linker error rather than as a policy message.

## What owns what

| Concern | Owner |
| --- | --- |
| Compiler version | `rust-toolchain.toml` - read by rustup and by Nix, so one pin |
| Dev shell, tool versions, task names | `devenv.nix` |
| Release build, cross-compilation, image | `flake.nix` |
| Conda/Python-delivered tooling | `pixi.toml` |
| Hooks | `.pre-commit-config.yaml`, run by `prek` |
| The gates | `xtask/` |

## Commands

```bash
cargo check -p sutura-domain --no-default-features   # fast inner loop
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
fmt                     # rustfmt, and normalise line endings
hygiene                 # the structural gates
gates                   # hygiene + fmt + clippy + tests + deny
ship-check              # the finishing sequence, on the committed branch diff
prek run --all-files    # the hooks
nix build .#oci         # the release image
nix run .#zizmor        # workflow static analysis (nix pins it, not pixi)
```

`--all-features` is not optional: adapters are feature-gated and default-off, so a bare
`cargo clippy --workspace` inspects almost nothing and still reports success.

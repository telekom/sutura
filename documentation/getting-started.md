# Getting started

Two supported ways in. Pick by what your machine can run natively.

| Your machine | Route | Why |
| --- | --- | --- |
| Linux, macOS, WSL2 | **Nix + devenv + direnv** | Nix runs natively; the shell is one `cd` away |
| Windows without WSL2 | **the dev container** | Nix does not run natively on Windows |

Both end up in the same environment, because both read the same `devenv.nix` and the same
`rust-toolchain.toml`. If you are behind a corporate proxy or have no direct internet egress,
read [enterprise-mirrors.md](enterprise-mirrors.md) **first** - every install below fetches
something, and each fetch is redirectable.

---

## Route 1 - Nix natively

### 1. Nix

The upstream installer, which is what the Nix project supports:

```bash
curl -L https://nixos.org/nix/install | sh -s -- --daemon
```

`--daemon` installs multi-user mode, which is what you want on a shared or long-lived
machine. On macOS it is the only supported mode.

Then enable flakes - this repo's build **is** a flake, so this is a prerequisite, not a
preference:

```bash
mkdir -p ~/.config/nix
printf 'experimental-features = nix-command flakes\n' >> ~/.config/nix/nix.conf
```

Verify:

```bash
nix --version          # 2.24 or newer
nix flake --help       # must not say "unknown command"
```

### 2. devenv

devenv provides the developer shell. It is **not** required to build or ship - `nix build`
and plain `cargo build` work without it - but it is how you get the toolchain, the linker,
the gate commands and the hook runner in one place.

```bash
nix profile install nixpkgs#devenv
devenv version
```

### 3. direnv

Without direnv you must remember to type `devenv shell`. With it, the environment loads on
`cd` and unloads when you leave.

```bash
nix profile install nixpkgs#direnv
```

Then hook it into your shell - direnv does nothing until you do, and this is the step people
skip:

```bash
# bash
echo 'eval "$(direnv hook bash)"' >> ~/.bashrc
# zsh
echo 'eval "$(direnv hook zsh)"'  >> ~/.zshrc
# fish
echo 'direnv hook fish | source'  >> ~/.config/fish/config.fish
```

Restart the shell, then once per clone:

```bash
cd <this repo>
direnv allow
```

`direnv allow` is a security boundary, not a formality: it is you saying you have read
`.envrc` and consent to it running. It re-prompts whenever `.envrc` changes.

### 4. Prove it

```bash
cargo --version        # must match rust-toolchain.toml
cargo nextest run --workspace --all-features
gates                  # everything CI runs
```

If `cargo` is missing after the shell loads, the shell evaluated but produced no toolchain.
That is a real failure mode and not a mystery - check that `devenv.nix` resolves
`rust-toolchain.toml` through rust-overlay.

---

## Route 2 - the dev container

For Windows without WSL2, or anywhere you would rather not install Nix on the host.

```bash
docker build --target dev -t sutura-dev .
docker run -it --rm -v "$PWD:/work" -w /work sutura-dev
```

The container has Nix, devenv and direnv, and its shell hook loads the environment on entry.
`--target build` runs the gates and a release build instead of dropping you in a shell, which
is the cheapest way to prove the container is actually usable.

Every network-touching argument is a build `ARG` with a public default - see
[enterprise-mirrors.md](enterprise-mirrors.md) for pointing them at a mirror. `compose.dev.yaml`
reads them from a gitignored `.env`, so you configure once rather than per command.

### A note on Windows paths

If you also build natively with `cargo` on Windows, keep the clone on a path your endpoint
tooling permits to execute build scripts. Cargo compiles and runs `build.rs` and proc macros,
and some managed configurations block execution from temp or profile directories - which
surfaces as a confusing linker or permission error, not as "policy blocked this".

---

## What owns what

Knowing this saves you from bumping a version in the wrong file.

| Concern | Owner |
| --- | --- |
| Rust compiler version | `rust-toolchain.toml` - read by rustup **and** by Nix, so there is one pin |
| Dev shell, tool versions, task names | `devenv.nix` |
| Release build, cross-compilation, the OCI image | `flake.nix` |
| Python-delivered tooling | `pixi.toml` - and nothing else; pixi does not own the Rust toolchain |
| Hooks | `.pre-commit-config.yaml`, run by `prek` |
| The repo gates | `xtask/` |

## Everyday commands

```bash
cargo check -p sutura-domain --no-default-features   # fast inner loop
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
hygiene                                             # the structural gates
gates                                               # hygiene + fmt + clippy + tests + deny
fmt                                                 # rustfmt, and normalise line endings
prek run --all-files                                # the hooks
nix build .#oci                                     # the release image
pixi run zizmor                                     # workflow static analysis
```

`--all-features` matters more here than in most repos: adapters are feature-gated and
default-off, so a bare `cargo clippy --workspace` inspects almost nothing and still reports
success.

<!--
Every link to a file in this repository is written as an absolute github.com URL, and that
is deliberate. This file is rendered twice: by GitHub from the repository root, and by
mkdocs as `docs/contributing.md`, which is a pymdownx.snippets include of this file and
nothing else. A relative link cannot satisfy both - it resolves against the root in one
case and against `docs/` in the other - and the site builds with `--strict`, so a link that
does not resolve fails the build. An absolute URL resolves in both.
-->

# Contributing

The design is settled and the code is a walking skeleton, so most of what exists today is the
environment and the gates that keep the guarantees in `AGENTS.md` from becoming aspirations.
Contributions are welcome. The fastest way to get one merged is to arrive with the gates
already green.

This repository is public and everything in it is world-readable, including comments,
fixtures, branch names and commit messages. Read the first section of
[AGENTS.md](https://github.com/telekom/sutura/blob/main/AGENTS.md) before you write anything down.

## Getting an environment

Three routes. Take the first one your machine allows.

| Route | For | What you get |
| --- | --- | --- |
| Nix + devenv + direnv | Linux, macOS, WSL2 | Every tool a gate uses, at the pinned version |
| The dev container | Windows without WSL2, or a host you would rather not install Nix on | The same shell, in Docker |
| Bare `cargo` | Neither of those | A compiling checkout, and not much else |

The first two read the same `devenv.nix` and the same toolchain files, so they cannot hand
you different compilers. On a network with no direct egress, read
[Building without direct internet egress](https://github.com/telekom/sutura/blob/main/docs/enterprise-mirrors.md) first:
nothing that fetches is hardcoded here, every location is read from the environment, and
the defaults are the public ones.

### Nix, devenv and direnv

The primary path. Nix with flakes enabled, then `devenv` and `direnv`:

```bash
curl -L https://nixos.org/nix/install | sh -s -- --daemon
mkdir -p ~/.config/nix
printf 'experimental-features = nix-command flakes\n' >> ~/.config/nix/nix.conf
nix profile install nixpkgs#devenv nixpkgs#direnv
```

Flakes are a prerequisite rather than a preference: the build *is* a flake.

Then hook `direnv` into your shell. It does nothing until you do, and this is the step
people skip:

```bash
echo 'eval "$(direnv hook bash)"' >> ~/.bashrc   # or the zsh / fish equivalent
```

Then, once per clone:

```bash
direnv allow
```

That is a trust boundary, not a formality: it consents to `.envrc` running, which loads
`devenv.nix`. After it, entering the directory puts the toolchain on `PATH` - both compilers,
`just`, `pixi`, `cargo-nextest`, `cargo-deny`, `betterleaks`, `stax`, `gh`, `rg`, `fd`. Then:

```bash
just setup
just doctor
```

If `cargo` is missing once the shell has loaded, the shell evaluated but produced no
toolchain: check that `devenv.nix` still resolves the two toolchain files through
rust-overlay. If it is there, `cargo --version` reports the nightly pin rather than the
stable one, and the section after next says why.

### The dev container

```bash
docker compose -f compose.dev.yaml run --rm dev
```

`just setup` runs on entry, hash-gated. `nix/container-setup.sh` hashes the files that decide
what setup produces - the four lockfiles, the manifests, both toolchain files, the hook
config - and skips the work when the hash is unchanged. Editing a `.rs` file does not re-run
it; changing `pixi.lock` does. `SUTURA_SKIP_SETUP=1` bypasses it, which is what you want when
running one command in a container whose environment is already current:

```bash
docker compose -f compose.dev.yaml run --rm -e SUTURA_SKIP_SETUP=1 dev bash -lc 'just lint'
```

The named volumes in `compose.dev.yaml` carry the performance rather than tune it.
`target-dir` mounts over `/work/target` inside the bind mount so cargo writes its output to a
native volume instead of through the host bind mount; `/nix`, the cargo registry and the pixi
environment persist so a restart does not re-download. Inside the container, `direnv allow
/work` is still yours to run. The image installs the shell hook and says so on entry, because
pre-approving the `.envrc` would mean checking out an unreviewed branch and having its
`enterShell` execute when you enter.

Without compose, the same image builds and runs directly:

```bash
docker build --target dev -t sutura-dev .
docker run -it --rm -v "$PWD:/work" -w /work sutura-dev
```

`--target build` runs the gates and a release build instead of dropping you in a shell,
which is the cheapest proof that the container itself works. Every network-touching
argument is a build `ARG` rather than a literal, so nothing internal is baked into the
Dockerfile; `.env.example` lists them and `compose.dev.yaml` reads them from a gitignored
`.env`.

### Bare cargo

`rust-toolchain.toml` is the compiler pin and rustup honours it directly, so a clone plus
rustup compiles and tests:

```bash
cargo test --workspace --all-features
```

Be honest with yourself about what is missing. `just`, `prek`, `cargo-nextest`, `cargo-deny`
and `betterleaks` come from Nix and pixi, so on this route the hooks are not installed, the
`just` tasks do not exist, and neither the secret scan nor the supply-chain gate can run
locally. CI runs all of them regardless; you find out on the pull request instead of before it.

On Windows, keep the clone somewhere your endpoint tooling allows build scripts to execute.
Cargo runs `build.rs` and proc macros, and a blocked execution arrives as a confusing linker
error rather than as a policy message.

## just setup

Idempotent, and worth re-running whenever an environment file changes. It:

- installs the git hooks for all three stages (`pre-commit`, `pre-push`, `commit-msg`) through
  `prek`, and unsets a `core.hooksPath` pointing at a directory that no longer exists. This
  repo once spent a whole session believing hooks ran that had never run: an uninstalled hook
  does not complain, it silently never fires;
- installs the pixi environment, which is where the hook runner lives;
- builds `xtask` and `sutura-dev`, so the first hook run is not a cold compile and a broken
  environment fails here rather than inside a git hook;
- ends with `sutura-dev doctor` - what is present, what is missing, what would fail.

`just` with no argument lists every task.

## What owns what

One owner per concern, because two owners for one version is one too many.

| Concern | Owner |
| --- | --- |
| Compiler version, anything shipped | `rust-toolchain.toml`, read by rustup and by Nix |
| Compiler version, the local inner loop | `rust-toolchain-nightly.toml` |
| The dev shell, tool versions, script names | `devenv.nix` |
| The release build, cross-compilation, the image | `flake.nix` |
| Anything delivered as a conda or Python package | `pixi.toml` |
| Which hooks run at which stage | `.pre-commit-config.yaml`, run by `prek` |
| The gates themselves | `xtask/` |
| The name you type for any of it | `justfile` |

## Two toolchains, and it matters which one you get

Read this before you believe a red `cargo clippy`.

The dev shell's bare `cargo` is the pinned **nightly** (`rust-toolchain-nightly.toml`), because
the cranelift codegen backend is nightly-only and it is what makes the inner loop fast. Every
gate instead sources `nix/stable-env.sh`, which puts the pinned **stable**
(`rust-toolchain.toml`) in front and gives it its own target directory.

So a `just` task or a devenv script runs on stable, the same compiler CI uses, while typing
`cargo` yourself runs on nightly. Clippy's lint set differs between channels and this repo
gates on the whole clippy `restriction` category with `-D warnings`, so a bare `cargo clippy`
reports lints stable has never heard of. **Do not conclude your branch is red from that** - run
`just lint`. The separate target directory is not optional either: alternating compilers in one
directory invalidates every artifact in it.

## The gates

| Stage | What runs |
| --- | --- |
| commit | `cargo fmt --check`; clippy over the workspace, `--all-targets --all-features`, `-D warnings`; `hygiene`, the cheap structural gates in one hook; `cargo check` narrowed to the changed packages; a staged secret scan; workflow static analysis when a workflow changed |
| commit-msg | the conventional-commit subject check |
| push | `cargo nextest`, doctests, `cargo-deny`, and a secret scan over the whole tree rather than the staged diff |
| CI | the same gates, plus test causality, the cross-built release binaries and the image |

`hygiene` is one hook because `xtask` owns the list: `check-boundaries`, `max-lines`,
`check-pins`, `unused-deps`, `line-endings`, `text-hygiene`, `check-skills`, `check-guidance`,
`check-workflows`, `check-docs`. `cargo xtask --help` prints them, marked.

Locally, `just gates` is everything CI runs and `just hygiene` is the structural half in
seconds. Scope hook runs while iterating with `just hooks --files <path>`, then sweep with
`just hooks --all-files` before opening a pull request. CI does not enter the dev shell - it
runs flake outputs, so a runner needs `nix` and nothing else. The two cannot drift because they
call the same `xtask` binary and the same cargo subcommands under the same stable pin.

## Commit messages

The `commit-msg` hook runs `cargo xtask commit-msg`, which judges the subject line and nothing
else:

```
<type>[(scope)][!]: <subject>
```

- types: `feat`, `fix`, `refactor`, `chore`, `test`, `docs`, `perf`, `ci`, `build`, `style`,
  `revert`
- at most 72 characters, the width at which `git log --oneline` and GitHub's commit list start
  truncating
- `!` after the type or the scope marks a breaking change
- a space after the colon, and no trailing full stop
- subjects git writes itself (`Merge `, `Revert `, `fixup!`, `squash!`, `amend!`) are exempt

```
feat(semantic): compile a bounded date predicate
fix(catalog-local): reject a dimension absent from the pinned bundle
refactor!: rename the Warehouse port's execute method
```

The body is not checked. Use it for why.

## A test has to be shown to test something

A new or changed test must be red against the base behaviour and green with your change. A test
that passes both ways proves nothing and is worse than no test, because it looks like coverage.

`cargo xtask test-causality --since <base>` checks that mechanically: it re-runs the changed
tests against the base version of the non-test sources, requires at least one to fail with no
unrelated failures, then requires them green on your head. `just causality origin/main` is the
same call. Where the change is not separable that way - impl and test in one file, or a rename
with no behavioural difference - the gate says so and asks for the evidence instead: the command
you ran, the failure you saw before the fix, the pass after. That goes in the pull request. Do
not skip it silently.

`just ship-check` is the finishing sequence. It needs a clean tree and a reachable base ref,
because it judges the committed diff from the merge base - what a reviewer will see - and over
that range it runs the commit-stage hooks, the gates' own unit tests, the causality check and
the pre-push hooks. Run it before saying a change is done.

## Stacked pull requests

A chain of dependent changes ships as one reviewable pull request per link, not as one branch
that grows until nobody can review it. `stax` (`st`) does the rebasing:

```bash
st create feat/thing   # branch off the current one, entering the stack
st modify              # stage and amend into the branch tip
st ss                  # push the stack, opening or updating a PR per branch
st ls                  # what the stack looks like now
st refresh             # sync trunk, restack the stack, submit the updates
```

A branch behind `main` is **rebased**, never merged. A merge commit from `main` into a topic
branch comes back as a rebase request in review, and it also destroys the linear shape `st`
relies on. Run `just ship-check` per branch rather than once for the stack, since each pull
request is reviewed alone and so has to be green alone. Restacking rewrites history by design,
so ask before force-pushing a branch somebody else has.

`stax.toml` at the repo root overlays your global stax config with this repo's forge, so a
machine set up for a different one does not submit stacks there.
`.agents/skills/git-ops/stacked-branches/SKILL.md` has the rest.

## Docs

The site is mkdocs-material, versioned by [mike](https://github.com/jimporter/mike), and every
command goes through pixi's isolated `docs` environment - the docs toolchain is Python and pixi
is the one resolver for Python here.

```bash
just docs          # render the site to site/
just docs-serve    # live reload
just docs-list     # what is published, per mike
just docs-deploy   # one version to gh-pages; `--push` is deliberately absent
```

The API reference under `docs/api/` is generated from rustdoc JSON and committed, so publishing
the site needs no Rust toolchain. Regenerate it when a public type or its documentation changes:

```bash
cargo rustdoc -q -p sutura-domain --all-features -- -Z unstable-options --output-format json
pixi run --frozen python docs/.tools/rustdoc_to_markdown.py target/doc/sutura_domain.json
```

`--output-format json` is an unstable rustdoc option, so run that in the dev shell where the bare
`cargo` is the nightly pin. The generator refuses to run on a `format_version` it was not written
for. Nothing yet fails when the committed pages fall behind the sources, and the API landing page
records that gap rather than leaving it to be discovered.

Those `just` tasks are the local form. CI reaches the same environment through nix -
`nix run .#pixi -- run --frozen -e docs docs` - because a runner has nix on `PATH` and
nothing else, so a bare tool name there exits 127.

Two pages hold no prose of their own. `docs/contributing.md` and `docs/changelog.md` are a
`pymdownx.snippets` include of `CONTRIBUTING.md` and `CHANGELOG.md` at the repository root,
so each of those files has one home and GitHub and the site cannot show different text. The
cost is that a relative link inside an included file resolves against the docs page rather
than against the root, which is why every link to a file in this repository is written as an
absolute github.com URL. `check_paths: true` fails the build when an include path stops
resolving, rather than publishing the literal directive.

Pages live in `docs/` and `nav` in `mkdocs.yml` is explicit rather than derived from filenames.
`cargo xtask check-docs` fails if a page is in no nav entry, if a nav entry names a file that is
not there, or if an asset `mkdocs.yml` names has stopped resolving. The build runs with
`--strict`, so a broken link fails instead of warning.
[Publishing the docs](https://github.com/telekom/sutura/blob/main/docs/publishing.md) covers the versioning and the one
repository setting it needs.

## Opening a pull request

`.github/pull_request_template.md` is the form. It is short because a reviewer's first five
seconds decide how they spend the rest, and it asks for:

- a **review snapshot** row - size, risk, where to look, what evidence exists;
- **what changed**, named as behaviour, a port, an invariant or a boundary, not as a list of
  files;
- **how to review** - the files in reading order, and which diffs to skip as mechanical;
- **validation** - the exact commands and whether they passed, including what you did not run.
  "Should work" is not a result;
- **test causality** - the gate's output, or the evidence above where the gate could not judge
  it;
- **invariants** - which mechanism would have failed if the change were wrong. "Nothing
  mechanical" is an acceptable answer and a useful one: it tells the reviewer the judgement is
  theirs.

One reviewable idea per branch. If describing it needs an "and", split it.

## Conventions that will fail your pull request

- **LF line endings, no trailing whitespace, one final newline.** `.gitattributes` normalises on
  checkin, and `cargo xtask line-endings` plus `cargo xtask text-hygiene` are what fail when
  something gets past it. `just fmt` fixes what is mechanically fixable.
- **No em dashes.** Plain hyphens, in prose and in comments. It is a gate rather than a request
  because the convention was stated from the start, files were reviewed for it by hand, and one
  shipped anyway.
- **No first-party `unsafe`.** `unsafe_code = "forbid"` in the workspace lint table, so a crate
  cannot re-allow it locally and lifting it is a visible diff to that table.
- **`#[expect(.., reason = "..")]`, never `#[allow]`.** `clippy::allow_attributes` makes a bare
  allow a lint error, and an `expect` fails once the warning it suppresses stops firing, so a
  suppression cannot outlive its cause.
- **No file over 1000 lines.** `cargo xtask max-lines`. `.max-lines-ignore` exempts generated
  and vendored output only; a pattern matching anything under `crates/` or `xtask/` fails the
  gate rather than being honoured, so the only way past it is to split the file.
- **No dependency declared and unused.** `cargo xtask unused-deps`. A crate must reference
  every dependency it declares, and every `[workspace.dependencies]` entry must be inherited by
  somebody. Declaring a dependency to satisfy a document is what it exists to stop.
- **`--all-features` on every lint and test entry point.** The adapters are feature-gated and
  default-off, so `cargo clippy --workspace` on its own inspects almost nothing and still
  reports success. A scoped `cargo check -p sutura-domain --no-default-features` is the fast
  inner loop, never the gate.
- **The domain crate acquires no framework dependency.** `cargo xtask check-boundaries` checks
  the whole transitive tree against an allowlist, so a framework reached through an innocuous
  crate fails it too.

`AGENTS.md` holds the full invariants table - every guarantee beside the type, lint, hook or
gate that enforces it, and the rules for changing the query path or the tool surface. Read it
before touching either.

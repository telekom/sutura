# The CRAP gate

CRAP - Change Risk Anti-Patterns - is cyclomatic complexity weighted by the tests that cover
it:

```text
CRAP(f) = CC(f)^2 * (1 - coverage(f))^3 + CC(f)
```

A branchy function with tests scores near its complexity. The same function untested scores an
order of magnitude higher. It is the one number that will not let "complex but fine" and
"untested but trivial" cancel each other out, which is what makes it worth a gate: neither a
complexity limit nor a coverage percentage catches the combination on its own.

Two consequences worth knowing before reading a report. At 100% coverage the quadratic term
collapses and CRAP equals CC, so matching columns mean "fully covered, still complex" - good
news, not a bug. And above CC 30 no amount of coverage keeps a function under the threshold: at
that size the metric stops asking about tests and starts asking for a smaller function.

## What runs, and where

| Command | What it does | Measured cost |
| --- | --- | --- |
| `cargo xtask check-crap` | the policy is a gate, the allowlist is annotated, the scope names real packages | milliseconds, part of `hygiene` |
| `just crap` | the coverage run and the score, exactly as CI runs them | 42 s cold, ~20 s warm |
| `nix build .#checks.x86_64-linux.crap` | what CI runs | 10.2 s of instrumented compile on top of the dependency derivation the clippy and test checks already build |

Measured in the dev container on 16 cores. Of the 20 s warm, about 13 s is the coverage run, 2 s
is the AST analysis, and the rest is building `xtask` itself.

The split is by cost. `check-crap` compiles nothing, so it sits in the `hygiene` sweep and runs
on every commit and inside the Nix sandbox. `crap` compiles the scoped crates with
`-C instrument-coverage`, so it is a task you ask for.

The flake check shares `cargoArtifacts` with the clippy and nextest checks. Not because the
coverage build can reuse them - it cannot, `-C instrument-coverage` changes the rustc invocation
so every dependency is compiled fresh regardless - but because sharing the attribute means no
SECOND dependency derivation is created. That is what keeps the marginal CI cost to the ten
seconds above rather than to another full workspace dependency build, which is what the
`api-docs` check pays for being on a different channel.

## Scope: `sutura-domain`, and why only that

The gate scores `sutura-domain` and nothing else. Two reasons, and the second is the one that
would not change if the machines got faster.

**Cost.** The coverage step, measured in the dev container on 16 cores, each from a cold coverage
profile. This is the part that varies with the scope, which is why it is the part tabulated:

| scope | wall | CPU |
| --- | --- | --- |
| `sutura-domain` | 11.1 s | 50.8 s |
| plus `sutura-catalog-local` and `sutura-semantic` | 3 m 27 s | 15 m 5 s |
| `--workspace` | more than 6 m 18 s | more than 80 m |

The workspace row is a lower bound: that run never reached a test. Coverage instrumentation is a
separate profile, so DataFusion, Arrow and DuckDB are all recompiled and none of the cached
artifacts help. On a four-vCPU runner that is twenty minutes and up, added to a `ci` job whose
timeout was already raised from 60 to 120 minutes because the tests alone were killing it.

**Where the tests live.** Coverage scoped to one package sees only that package's tests. For
`sutura-domain` that is the whole truth - its 120 unit tests are its real test suite and they
are in the crate. For `sutura-semantic` it is not: its real tests are the golden corpus in
`sutura-app`, which pulls both adapters. So a `-p sutura-semantic` coverage run reports `resolve`
at 0% and scores it CRAP 210. Measured, not hypothesised, and seven such functions appear the
moment that crate is added. They are artefacts of where the tests live, not findings, and a gate
that cries wolf seven times gets switched off.

So the rule for widening the scope is not "is it cheap". It is: **does this package's own test
suite exercise its own code?** The list is `SCOPE` in `xtask/src/crap.rs`, one line, with the
measured costs beside it.

**What that misses, stated plainly:** every dialect renderer, the resolver, the planner, the
catalog loader and both adapters. This gate covers the invariant core and nothing else. It is
not a coverage target for the workspace and does not pretend to be one.

## The policy

`.cargo-crap.toml` at the repo root. `cargo crap` reads it directly, so running the tool by hand
applies the same thresholds CI does, with no flag to remember. `cargo xtask check-crap` is what
stops it from rotting:

- `threshold` must be present and positive
- `fail-above = true` must be set - without it the tool prints a table and exits 0, which is a
  report, not a gate
- every `allow` entry must carry a `#` comment saying why, on its line or immediately above it

That last rule is the same one `deny.toml` follows for its licence list, for the same reason: an
allowlist whose entries carry no justification becomes pre-approval for whatever is added next to
it, and nobody can tell later which entries were reasoned about.

**What does not belong in `allow`:** a function that scores badly because it is untested. The fix
for that is a test. An entry is for code whose complexity the metric reads wrongly - a generated
match table, a vendored port - not for code nobody has got round to covering.

## The ratchet is a number, not a baseline

`cargo-crap` can diff against a JSON baseline from a previous run and fail on any regression.
This gate does not, and the reason is structural rather than a preference.

A ratchet needs a baseline from `main`. Fetching one needs the network and `git`, and a Nix build
sandbox has neither - so a gate that downloaded a baseline could not run as a flake check at all.
Committing the baseline instead trades that for a generated file that needs a coverage run to
refresh and a second gate to keep honest, which is one more thing to get wrong.

So the ratchet is `threshold` in `.cargo-crap.toml`: a single reviewed number. Lowering it is a
diff. Raising it is a diff somebody has to defend.

**What that gives up, and it is real:** this gate cannot say "worse than `main`". A function that
goes from CRAP 12 to CRAP 29 passes. It answers "is anything over the line", not "did anything
get worse".

**What it cannot do is pass by accident.** There is no baseline to be absent, and the two ways a
score-based gate normally fails open are both refused by name:

- the coverage run wrote no `SF:` records - refused, because every function would then read as
  uncovered
- the report has no entries, or a scoped package contributed none - refused, because a package
  that produced nothing was missed, not proven clean

Both are failures with a message, never a silent pass. That shape is deliberate: this repo has
already had a gate that "listed files via an absent tool and got an empty list", and reported
success.

## Tool versions, and how they are pinned

`nix/crap.nix` is the one place either tool is resolved, imported by both `flake.nix` and
`devenv.nix` - the same arrangement `nix/duckdb.nix` and `nix/toolchains.nix` use, and for a
stronger reason: those two files have separate nixpkgs pins, and for a tool whose output is a
verdict, two versions mean the dev shell reporting a score CI does not.

| Tool | Version | Route |
| --- | --- | --- |
| `cargo-crap` | 0.4.3 | prebuilt release binary, hash-pinned in `nix/crap.nix` |
| `cargo-llvm-cov` | from the locked nixpkgs | `pkgs.cargo-llvm-cov` |
| `cargo-nextest` | from the locked nixpkgs | `pkgs.cargo-nextest`, already pinned for the tests |

`cargo-llvm-cov` is in nixpkgs, so nothing is hand-rolled for it. `cargo-crap` is not, and it is
fetched as a prebuilt binary rather than built from the crate because from-source means compiling
`clap`, `syn`, `rayon`, `comfy-table` and `indicatif` on the first runner that needs it - minutes
of build for a tool that reads files. A `fetchurl` hash is the same provenance guarantee a
`fetchCrate` hash gives; what changes is that nothing is compiled. The derivation runs
`cargo-crap --version` at build time, so a pin that cannot execute fails where the pin is,
not mid-branch.

The dev shell echoes both versions on entry. `cargo xtask check-crap` fails if the version
`nix/crap.nix` pins is not the version this page states.

## Coverage runs on stable, and the gate makes itself so

The dev shell's bare `cargo` is a nightly with the cranelift codegen backend, because that is
what makes the inner loop fast. Coverage cannot use it: `-C instrument-coverage` is LLVM-specific
and does not exist under cranelift. This is not the channel-consistency argument the lints have -
it is that the instrumentation is absent.

In this repo cranelift arrives as two environment variables,
`CARGO_UNSTABLE_CODEGEN_BACKEND` and `CARGO_PROFILE_DEV_CODEGEN_BACKEND`, which
`nix/stable-env.sh` unsets. There is no `RUSTFLAGS` and no `CARGO_TARGET_*_RUSTFLAGS` anywhere in
the dev shell, and the per-target tables in `.cargo/config.toml` carry linker and target-feature
flags only.

`cargo xtask crap` does not rely on its caller having sourced that file. It unsets the two
variables itself, strips the cranelift flag out of any rustflags variable that carries one while
keeping the linker and library flags beside it, pins `RUSTUP_TOOLCHAIN` to the channel in
`rust-toolchain.toml`, and gives the instrumented profile its own target directory under
`target/crap`. A gate whose failure mode is a silently empty report must not depend on a `source`
line somebody could forget.

## Current state

One function in `sutura-domain` is over the line:

| CRAP | CC | Coverage | Function |
| ---: | ---: | ---: | --- |
| 42.0 | 6 | 0.0% | `Grain::as_str`, `crates/sutura-domain/src/model.rs` |

It is a five-arm `const fn` that no test in `sutura-domain` calls; the dialect renderers in
`sutura-semantic` are what exercise it, and a per-crate coverage run cannot see them. Two more
sit exactly at 30.0 and therefore pass - `RequiredFilter::fmt` and `plan_required_filter`, both
also at 0% from the crate's own tests.

It is **not** allowlisted. The fix is a test.

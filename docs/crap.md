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
| `nix build .#checks.x86_64-linux.crap` | what CI runs, and the only thing that measures | 10.2 s of instrumented compile on top of the dependency derivation the clippy and test checks already build |
| `just crap-delta <base> [head]` | did the CHANGE make anything worse | milliseconds - two JSON files, no compiler and no tool |

Measured in the dev container on 16 cores. Of the 20 s warm, about 13 s is the coverage run, 2 s
is the AST analysis, and the rest is building `xtask` itself.

The split is by cost. `check-crap` compiles nothing, so it sits in the `hygiene` sweep and runs
on every commit and inside the Nix sandbox. `crap` compiles the scoped crates with
`-C instrument-coverage`, so it is a task you ask for. `crap-delta` compiles nothing either, but
it needs a baseline from another commit, which a Nix sandbox cannot fetch - so it is neither a
hygiene gate nor a flake check.

The flake check shares `cargoArtifacts` with the clippy and nextest checks. Not because the
coverage build can reuse them - it cannot, `-C instrument-coverage` changes the rustc invocation
so every dependency is compiled fresh regardless - but because sharing the attribute means no
SECOND dependency derivation is created. That is what keeps the marginal CI cost to the ten
seconds above rather than to another full workspace dependency build, which is what the
`api-docs` check USED to pay for being on a different channel - until it moved onto this same
closure, measured: `nix-store -q --references` on both drvs now names one `sutura-deps`.

**ONE MEASUREMENT, TWO CONSUMERS.** The check writes a path-portable copy of its score report to
`target/crap/baseline.json`, and inside a Nix build it publishes the same file to
`$out/crap-baseline.json` - the only channel out of a build sandbox. The gate does that itself
rather than through a `postInstall` hook in `flake.nix`, for two reasons that `publish_baseline`
in `xtask/src/crap.rs` gives at length: a filename agreed between two files is a filename that
drifts, and `flake.nix` sits at exactly the 1000-line limit `cargo xtask max-lines` enforces. The
absolute threshold and the delta both read that one file, so delta control added no coverage run,
no second dependency closure and no second cache entry - which is the property the whole pipeline
was recently rebuilt around.

Portable means every `file` is repo-relative and the file says so in a `"paths":
"repo-relative"` key that the reader refuses to compare without. That is not tidiness. The
sandbox source root is a per-build directory - one measured run had
`/nix/var/nix/builds/nix-91840-1992088735/lzldwfdkxzbazqvfmmc5fagm3fcxfhkv-source` - so absolute
paths differ between any two builds. Handed a baseline whose root does not match, `cargo crap`
does not fail: it reported 181 unchanged, 4 new and 4 removed for a tree with no changes at all,
losing exactly the functions that share a name inside one file. Four invented functions in a
review comment is worse than a refusal.

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
timeout is 75 minutes, and which a cap of 60 once killed mid-`Tests`.

**Where the tests live.** Coverage scoped to one package sees only that package's tests. For
`sutura-domain` that is the whole truth - its unit tests are its real test suite and they
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

**What delta control changes about that, and what it does not.** The blocker for widening was
never only cost: adding `sutura-semantic` puts seven functions at CRAP 210 into the report on day
one, and the absolute threshold fails all seven immediately. They are artefacts of where the
tests live, not findings, and the only escape the number offers is an allowlist entry - which the
policy refuses for untested code, correctly.

A delta has an answer the number does not: gate that crate on "did YOU make it worse" and leave
its inherited debt alone. That is what rules 2 and 3 below are for, and it is why they are
written even though the number subsumes them today.

**The scope has NOT been widened, and this page is not an argument that it should be.** Two things
still block it, and neither is about the ratchet. The cost is unchanged - 3 m 27 s wall and 15
CPU-minutes for those two extra crates, measured, against a 12-second gate. And a widened scope
needs the absolute threshold relaxed for the inherited set, which means a second policy concept -
"debt admitted on entry" - that nothing here implements. Widening is a separate change with its
own measurements; delta control is what makes it possible rather than what makes it done.

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

## The ratchet is a number AND a delta

Two independent rules, and the order matters: the number is the gate, the delta is a ratchet on
top of it. The number is unconditional and runs on every commit. The delta is not: it needs a
baseline for the exact merge base, and it warns and skips when there is none - see *Where the
baseline comes from* below. So a branch whose merge base has no artifact is judged by the number
alone, which is why the delta is a ratchet and not a guarantee.

### The number

`threshold = 30.0` in `.cargo-crap.toml`, and it has not moved. A function above it fails, on
every commit, with no baseline involved. This is the rule that still holds when there is nothing
to compare against - a first run, a fork, a merge base whose artifact expired - and it is why the
delta could be removed tomorrow without this gate becoming a report.

What it cannot say is "worse than the base". A function that slides from CRAP 12 to CRAP 29
passes, every time, and nothing anywhere records that it moved.

### The delta

`cargo xtask crap-delta` compares this commit's report against the merge base's and applies three
rules. `threshold` is the only number involved - no second knob, because a second number is a
second thing to defend in review.

1. **Crossed the line.** `baseline <= 30 < head`. A function this branch pushed over.
2. **Worse while over the line.** `head > 30` and the score rose.
3. **The budget.** The CRAP points added to PRE-EXISTING functions, summed, may not exceed 30.
   One branch may not add as much rot to code that already existed as a whole new over-the-line
   function is worth.

Rules 1 and 2 are **subsumed today**: nothing may be over 30 at all, so nothing can cross the
line or worsen while over it without the number failing first. They are written anyway, because
they are the two rules that survive the one change that would let this gate cover more than one
crate - see the scope section above. Rule 3 is the one that blocks something the number does not:
slow rot that never crosses 30.

**What deliberately does not fail: a single sub-threshold regression.** Not leniency. At full
coverage CRAP equals CC, so adding one covered branch to a fully-tested function moves it from 5
to 6 permanently and no test can bring it back. A rule that failed that could only be escaped by
an allowlist entry, which the policy forbids for exactly this case - so it would be bypassed or
deleted rather than obeyed. Every regression is **reported**, in the job log and in the pull
request comment, worst first, with its delta. Rule 3 is what stops a hundred of them from adding
up unnoticed.

`epsilon` decides what counts as a change at all, and defaults to 0.01 - deliberately
`cargo-crap`'s own default, so running `cargo crap --baseline` by hand agrees with the gate about
which functions moved. `.cargo-crap.toml` may state one; it does not.

### Where the baseline comes from, and what happens when there is none

A push to `main` uploads that commit's report as the `crap-baseline` artifact, retained 30 days. A
pull request resolves the artifact whose run was on the default branch **and** whose head SHA is
this branch's exact merge base - the value `.github/workflows/ci.yml` already computes with `git
merge-base`, not the base branch tip.

**Exact, and a miss is a warning that skips the comparison rather than a failure.** Both halves of
that are deliberate.

Exact, because an approximate baseline lies in both directions. Against "the latest baseline on
main", work that landed on main after the branch point reads as this branch's regression, and
improvements that landed there read as this branch's improvements. A gate that reports
regressions somebody else caused is a gate that gets switched off.

A warning rather than a failure, because the merge base is not guaranteed to have a baseline and
that has nothing to do with the branch being reviewed. A push to `main` carrying several commits
gets one CI run, so the middle commits never produced an artifact; artifacts expire; a
docs-classified push runs no CRAP step at all. Failing there would fail pull requests for the
shape of somebody else's push, and would train everybody to re-run the job until it went away.
The number above has already run either way, so a skipped delta loses a ratchet, never the gate.

### The pull request comment, and what it costs in permissions

The rendered table goes to the job summary, which needs no token, and is posted as a pull request
comment by the `crap-comment` job. That job holds `pull-requests: write` - the only write scope on
any job a pull request can reach - because creating or editing a pull request comment has no
read-only route. Other workflows do hold write scopes - publishing the site, cutting a release,
the optimised release build, bumping a version and pruning the cache - eight of them across five
files. None is on a job a pull request can start: four of those five do not run on a pull request
at all, and `docs.yml`, which does, gates its publishing job on
`github.event_name != 'pull_request'` and gives the pull request job `contents: read`.

It is a separate job for that reason. It checks nothing out, compiles nothing, runs no code from
the pull request, interpolates no pull request text into a shell body, and skips forks (whose
token is read-only on a `pull_request` event regardless). The workflow-level grant is still
`contents: read`; the `ci` job that builds and tests the branch has `contents: read` plus
`actions: read`, the latter only so it can read the base commit's artifact. The reasoning is
written out beside the grant in `ci.yml`, because a permission without a recorded reason is a
permission the next person cannot audit.

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

## Coverage runs on the nightly toolchain's LLVM backend, and the gate makes itself so

The dev shell's bare `cargo` is the nightly toolchain and DEFAULTS to the LLVM codegen backend
(cranelift is opt-in), which is what CI gates on too. Coverage uses it: `-C instrument-coverage`
is LLVM-specific and would not exist under cranelift, but the default backend supports it - so a
run matches CI without any channel override. It must still not inherit a cranelift cargo.

In this repo cranelift arrives as two environment variables,
`CARGO_UNSTABLE_CODEGEN_BACKEND` and `CARGO_PROFILE_DEV_CODEGEN_BACKEND`, opt-in in the dev
shell. There is no `RUSTFLAGS` and no `CARGO_TARGET_*_RUSTFLAGS` anywhere in the dev shell, and
the per-target tables in `.cargo/config.toml` carry linker and target-feature flags only.

`cargo xtask crap` does not rely on its caller having exported anything. It unsets the two
variables itself, strips the cranelift flag out of any rustflags variable that carries one while
keeping the linker and library flags beside it, pins `RUSTUP_TOOLCHAIN` to the channel in
`devco/rust-toolchain-nightly.toml` for a bare-rustup host, and gives the instrumented profile
its own target directory under `target/crap`. A gate whose failure mode is a silently empty
report must not depend on a `source` line somebody could forget.

## Reading the current state

**There is deliberately no report pasted here.** There used to be one - a function count and the
four highest-scoring functions with their `file:line` - and it rotted the way a pasted report
always does. It was taken when the crate had twelve source files; three of its four line
references had stopped resolving before this paragraph replaced it; and nothing regenerated or
checked it, because
`cargo xtask check-crap` verifies the tool VERSION this page states and not the numbers.

So the page states how to get the current one instead, which is one command:

```bash
just crap
```

It prints every scored function in `SCOPE`, the threshold it is judged against and the verdict, and
leaves `target/crap/baseline.json` behind for `just crap-delta`.
What is worth knowing about the output is the part that does not change with a re-run:

- **`fail-above` is exclusive**, so a function sitting exactly on the threshold passes. Those are
  the ones the delta exists for: the number has no headroom left to give, and the only thing left
  to say about them is whether they move.
- **Nothing over the line is allowlisted.** For a function over it the fix is a test, not an entry
  in a file.
- **A high score is not always a finding.** Coverage scoped to one package sees only that
  package's tests, so a function whose real tests live in another crate scores as uncovered - which
  is the whole argument for `SCOPE` above being the crate whose suite is its own.

`Grain::as_str` is the worked example of a fix, and it is history rather than a current number: it
headed the table at CRAP 42, a five-arm `const fn` no test in the crate called, and is now CC 6 at
100% coverage scoring 6.0. The complexity did not change; the tests did.

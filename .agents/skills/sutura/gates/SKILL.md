---
name: gates
description: What a green run actually covers - what `just validate` can see that nothing else can, the two ways to lose a file from the nix sandbox, which gates fail open versus closed, and the checks that pass while the thing they describe is broken. Open when a gate is unfamiliar or a check passed that should have failed.
---

# What a green run means

`just --list` and `cargo xtask --help` tell you what exists. This file is about what each thing
covers, because the expensive mistake here is believing a green run said more than it did.

## Only `just validate` counts as verified

It runs the nix checks, which build **their own copy of the tree** - the only way to catch a file
the build needs and that copy does not have. Every other command reads the real tree and cannot see
that class of bug.

Two ways to lose a file, both of which have happened:

- **The copy is GIT-DERIVED.** An untracked file is invisible to it, so a new module compiles under
  `cargo` and then does not exist in the sandbox. `git add -N` is enough.
- **`flake.nix`'s source filter drops things.** Its arms match a REPO-RELATIVE path. They used to
  match the absolute one, which made the whole `||` chain short-circuit to true - a nix source root
  IS `/nix/store/<hash>-source`, so the arm written for our own `nix/` directory matched every path
  in the tree and the filter dropped nothing. **So the rule `flake.nix` states is live rather than
  theoretical: any directory a build or a test reads has to be named there.**

**The blast radius is narrower than that sounds, and worth knowing before believing a green run:**
`clippy`, `doctest`, `fmt`, the xtask package and the release builds read the *filtered* copy, while
`nextest`, `hygiene`, `crap` and `api-docs` each set `src = ./.` and read the whole tree - which is
why the gates that inspect repo files are unaffected. It does not reach the dependency closure at
all: crane synthesises that from the manifests, so it does not move whatever the filter does.

**`flake.nix` cannot be fully modularised.** `apps.<name>`, the `packages = ` block and the
`checks = {` block must stay in it, because two xtask gates scan that file for them **textually**
and both fail closed on finding none. A `nix/` module holds what an app or a check *points at*,
never the declaration.

## One owner per concern

Two owners for one version is one too many, and this is the split. It is worth knowing before
adding a pin anywhere.

| Concern | Owner |
| --- | --- |
| Compiler version, anything shipped | `rust-toolchain.toml`, read by rustup **and** by nix |
| Compiler version, the local inner loop | `devco/rust-toolchain-nightly.toml` |
| The dev shell, tool versions, script names | `devenv.nix` |
| The release build, cross-compilation, the image | `flake.nix` |
| Anything delivered as a conda or Python package | `pixi.toml` |
| Which hooks run at which stage | `.pre-commit-config.yaml`, run by `prek` |
| The gates themselves | `xtask/` |
| The name you type for any of it | `justfile` |

`check-pins` fails if a tool appears in both nix and pixi. `nix` is the only pin for a tool whose
version changes what it reports.

## Never hand-write the cargo line

`just lint` and `just test` ARE the gates' invocations. A hand-written line diverges twice, and
fixing only the first still fails:

1. This shell's cargo is **nightly** for the cranelift backend and reports lints stable has not got.
   `nix/stable-env.sh` is the fix.
2. The gate adds **`-D warnings`**, so a bare run turns a `restriction`-category finding into a
   warning that a grep for `^error` does not see. It passes locally and fails the gate.

Both were hit in one session by two different agents, after each read the half of the rule that
named only the first.

The same shell environment **follows cargo into other checkouts and breaks builds there**:
`CARGO_UNSTABLE_CODEGEN_BACKEND`, `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` and two DuckDB path
variables are unscoped. Measured, not theorised: a control build of a C++-linking crate from this
shell aborted with an uncaught foreign exception behind millions of unwind-table errors, because
cranelift's unwind tables cannot carry an exception across the C++/Rust boundary. The same suite is
green with the four unset. **A red run from this shell in another repo is unexplained until you
unset them.** No gate can see a build somewhere else, which is exactly why it is written down.

The `ci` profile inherits `dev`, so anything building `--profile ci` locally - the causality gate
included - hits the same cranelift problem and needs `nix/stable-env.sh` first.

**The local channel split is not uniform, and the non-obvious half is which hooks are which.** On
stable: every `just` task and devenv script, the commit and push clippy hooks, and the coverage hook
- that last one from **absence** rather than preference, because `-C instrument-coverage` does not
exist under cranelift, so `cargo xtask crap` re-establishes stable itself rather than trusting its
caller. On the shell's nightly: the `fmt`, `check-changed` and doctest commit hooks, and anything you
type yourself. Anything that only CODEGENS is faster on cranelift and its verdict does not depend on
the channel, which is why iteration stays there deliberately. Two things where it does depend:
clippy's lint set (above), and rustfmt's *output* - the two channels agree byte-for-byte over this
tree today, so the commit hook is left fast and a push hook builds the exact check CI runs. The
separate target directory is not optional: alternating compilers in one directory invalidates every
artifact in it.

## Direction is per-gate, and each one says so at its own decision

- **Fail open:** `classify` / `check-changed` / `changed-packages`. An unmapped path, a bad base
  ref, an empty diff or a git that will not answer all run *everything* and say why, because the
  expensive failure is a new directory silently skipped, not a wasted CI minute.
- **Fail open, with one veto:** the docker gate (`require_docker` / `absent`). No container runtime
  SKIPS on a developer machine and FAILS in CI, because nix deliberately does not pin docker, so
  "not installed" is a legitimate configuration. The veto is the lesson: a daemon that is
  installed, running and **never answers** is a FAULT on a machine that HAS the tier, not a machine
  without one, so `Missing::WedgedDaemon` refuses to be skipped whatever `SUTURA_DEV_REQUIRE_TIER`
  says. Bounding the probe without this changed an indefinite hang into a green run over an
  unprovisioned tier - **a bounded probe that fails closed is worth nothing if its caller fails
  open**, and the two directions have to be read together.
- **Fail closed:** `clean-branches`. It deletes local branches and the worktrees holding them, so
  anything undetermined KEEPS the branch and the report names the signal that was missing. Dry run
  unless `--delete`, and no flag overrides a refusal. `git branch --merged` is deliberately not its
  mechanism - a squash-merged branch's commits are not ancestors of anything on the default branch,
  so it cannot see the case this repository produces every day.

**Neither direction is a default. What a wrong answer costs decides it, per gate.**

## Hooks, and why clippy runs twice

The push stage repeats the commit stage's compiling hook because **`git rebase` and
`git rebase --continue` run no commit hook at all**, and a conflict resolution used to reach the
remote with nothing having compiled it. `check-hook-tiers` holds two things: the push stage runs a
hook that COMPILES, and that hook's entry is the commit stage's **own** - the second because cargo
keys its fingerprints on the invocation, so a push command differing by one flag rebuilds the
workspace instead of reusing what the commit hook built.

Tiers are bypassable with `--no-verify`, so none of this is an invariant. What the gate holds is
that the tiers documented here are the tiers `.pre-commit-config.yaml` declares. It expresses the
repeated run as a YAML anchor rather than a second copy, because a gate that makes `git push` slow
gets bypassed and then guards nothing: measured, the push clippy is ~1 s after a warm commit hook
and ~76 s into an empty target directory, which the first commit in a fresh clone pays anyway.

**The CRAP gate** scores cyclomatic complexity weighted by the tests covering it - the combination
neither a complexity limit nor a coverage percentage catches alone. Split by cost: `check-crap`
reads the policy and compiles nothing, so it is in the sweep; `just crap` runs the coverage build.
It is scoped to the domain crate because `--workspace` coverage is over six minutes and more than 80
CPU-minutes; `docs/crap.md` carries the measured cost of every wider option and what the scope
therefore does not see.

## Checks that pass while the thing they describe is broken

- **`just validate` does not render the site.** `check-docs` reads the `nav` and the assets; it does
  not build a page, and no nix check does either. So a rustdoc link the api-docs generator copies
  through verbatim can pass `check`, `lint`, `test`, `hygiene`, `api` and every nix check, then fail
  `mkdocs build --strict`. **Measured:** a doc comment linking another crate's path produced
  *"contains an unrecognized relative link"* and aborted the strict build, while `crate::`-prefixed
  links on four other generated pages did not warn. The safe form for a cross-crate reference is
  plain backticks. Which shapes mkdocs accepts **was not established**, which is why this is a rule
  to run `just docs` rather than a gate encoding a boundary nobody measured. It is not in `validate`
  because that recipe would then fail on a clone whose pixi docs environment is not installed, and
  **a gate that fails for an environment reason gets disabled.**
- **Prose in `AGENTS.md` and under `.agents/skills/` is gated, which surprises people editing it.**
  `check-guidance` scans both, and `xtask`'s own unit tests assert literal phrases out of `AGENTS.md`
  as the evidence anchoring a rule - so rewording its opening sentence turns a gate's test red rather
  than failing the gate itself. Run `just hygiene` **and** `cargo nextest run -p xtask` after editing
  either.
- **`check-guidance` reads prose and cannot catch a paraphrase.** It holds forbidden phrases, a
  version pin that must agree wherever it is written, claims known false in each recorded wording,
  and two gated counts. It fails a cited `just` task that does not exist and a cited `cargo` line
  missing `--all-features`. Its own stated blind spot: a comment marker is not stripped, so a claim
  wrapping inside a `#` block is not found.
- **A `just` task cited in a printed Rust string is checked; a raw command line there is not.**
  The scope split is deliberate and was measured before it was taken. What judges a SENTENCE stays
  on prose files, because a rule table written in Rust holds the phrases it forbids and a scan over
  `.rs` would report the gate's own reasoning. What RESOLVES a citation - is this a recipe, is this
  a task - is answered against derived authorities and costs nothing on `.rs`, so `check-guidance`
  reads every published `.rs` file, production regions only, for a backtick span beginning `just `
  or `cargo xtask `. **Measured when it landed: 31 such citations in the workspace, and the check
  found one of them already broken on its first run** - `sutura-dev` printed `just dev-endpoint`
  while the recipe parser kept the `@` off a quiet header in the name, so the only authority for
  "is that a recipe" said no. That is the argument for it. What it does NOT hold: an interpolated
  task NAME, a citation with no backticks, a path, and the half that caught
  `github.com/telekom/sutura#243` in the first place - *an invocation span has to begin `just `*.
  That half stays a unit test in the module whose siblings establish the form, because this tree
  prints `nix run .#…` and shell fragments in backticks legitimately and the general rule would
  fire on correct advice. A citation rule widened past prose without a precision story becomes
  noise, and noise is how a gate gets disabled.
- **A derived number is only a control while a page still states it, and BOTH shapes here got
  that wrong.** A count and a version pin work the same way - derive the value from the tree, then
  compare it against pages under a `mentioned_in` glob - so both hold nothing when no page states
  the value. The count was in that state after the router rewrite carried the invariants table out
  of `AGENTS.md` and its `mentioned_in` list stayed behind; the pin was in it from the start, with
  six pages naming `rust-toolchain.toml` and not one carrying a version, while the success line
  said `1 pin(s)`. **A glob matching nothing was already a failure** for counts; what is new is
  that a value no page states is a failure too, for the count and for the pin, and that the pin is
  now written down where it can be compared. The other silent-green mode is granularity: an entry
  counting FILES over a literal that can repeat inside one is right only by coincidence, which is
  why each entry declares files or occurrences rather than inheriting a default.
- **The gate that fails a false claim carried one, and the reason generalises.** Its scope is prose
  files, so it never read its own source: the *remedy* a claim prints - the sentence handed to a
  reader as the correction - said a transport surface was absent for as long as it took a person to
  notice, invisible to every gate in the repository including itself. Three properties of a remedy
  are mechanical now: it may not repeat a wording any live claim forbids, every repo path it cites
  must resolve, and every task it cites must exist. **The sentence itself still is not** - a remedy
  is prose and nothing derives it, so a wording nobody has registered is held by review. The
  transferable part: when a mechanism's own data is prose, ask what reads THAT, and expect the
  answer to be nothing.
- **A gate that scans a language must lex it, and the failure mode is INVENTING as well as losing.**
  `check-workflows` counted `{` and `}` over `flake.nix`'s raw text to answer "which checks exist".
  Three shapes broke it at once, and two of them were in the tree for months: a brace inside a `#`
  comment (comments were skipped for names and counted for depth, so one sentence quoting the
  block's own header shifted every line below it), a brace inside an indented `''...''` string (a
  tier's body is shell, so `port=` and `realm=` were reported as declared checks), and a `let`
  binding inside a check's value (`let` opens no brace, so seven bindings read as outputs). The
  invented half is the worse one: a caller cannot tell a fabricated name from a real one, and the
  report came out as eighteen workflow references to outputs "that do not exist" rather than as a
  broken parse. **`check-newtype-leaks` had this right first** - it blanks comments and string
  interiors before matching, and says so, because three doc comments in the tree state *there is
  deliberately no `Deref`*. So the pattern to copy is that one. Two rules fell out: blank the
  non-code half before counting anything, and make an unclosed block an ERROR rather than an answer.
- **`nix eval` is the authority for a flake's outputs and cannot be the mechanism here.** Weighed
  and rejected once, so it does not need weighing again: `check-workflows` runs inside
  `checks.hygiene`, a derivation with no nix and no network, and evaluating `checks` needs the
  flake's inputs. The authority is unreachable exactly where the check runs.
- **A count in prose is only as good as the command beside it.** An anchored
  `grep -c '^#\[test\]$'` answers zero for tests in an indented inline `mod tests`. One figure in
  this repo was wrong six times. Write the command and the date, or delete the number.
- **A READOUT under a comment claiming it is an assertion, which is the cheapest version of this
  whole class.** Both `ci.yml` link-check steps ended in `file <path>`, and the older one said so:
  *"Proves the arch, not just the exit code."* It proves neither. Measured: `file` on a path that
  does not exist prints ``cannot open`` and **exits 0**, and nothing compared its answer to the
  triple - so a renamed or missing executable was green, and the feature-probe step reads that name
  out of a manifest. `nix/assert-linked.sh` asserts the two things the sentence claimed and names
  the one it still does not (the libc half, unmeasured, so a musl target linked dynamically
  passes). **The transferable question:** for every command a step runs for its side effect, ask
  what its EXIT CODE is a function of. `file`, `echo`, `grep -c` and any `| head` answer zero on
  inputs a reader would call a failure.
- **A refusal that is weaker than the claim it defends, and the tell is a quantifier.** The same
  step refuses an EMPTY probe manifest, which reads as *a probe cannot silently disappear* and is
  not that claim: with a second binary declaring a probe, deleting the first one's leaves the
  manifest non-empty and the job green. *At least one row* defends nothing about WHICH row. What
  closes it is a second declaration that must agree - here `cargo xtask check-shipped-binaries`
  reconciling `nix/shipped.nix`'s `probeFeatures` against the `cargo build --features` a page
  documents, failing closed when no page documents one at all.
- **A verdict's rules can hold its SPELLING while nothing holds its TRANSITION, and the second is
  usually what the sentence beside it promises.** `check-venues`' `unrun` arrived with three rules -
  the word is in the vocabulary, a `not built` venue may not claim it, the venue's section must use
  it - and all three read the page. Nothing read whether a run had happened, so *the change that
  carries the first green run moves this cell* was, still, a sentence nothing read. The failure is
  silent and permanent: wire the leg into a job, watch it go green on every push, and the page goes
  on telling its next reader that nothing has run it with every gate green. **The question to ask
  of any state token: what reads the thing that makes it STOP being true?** Here the answer was in
  reach - a workflow invoking the venue's `Reached by` task - and the rule that closed it is
  deliberately one-sided, because an invocation is not a green run and the authority for *did this
  pass* is unreachable from the sandbox the gate runs in. A one-sided rule that names its side is
  worth more than a two-sided one nobody can implement.
- **`nix` is the only pin for a tool whose version changes what it reports.** `check-pins` fails if
  a tool appears in both nix and pixi, because two pins are one pin nobody trusts.
- **`check-gate-classification` holds an argument, and stops short of the inputs it argues about.**
  `Kind::Hygiene` carries a `Reads`, so the compiler makes every gate in the sweep say whether a
  `docs/*.md`-only diff can reach what it reads, and the gate fails unless the implementation plan's
  two tables are exactly those two groups - in both directions. What it cannot see is a gate whose
  INPUTS grow into `docs/` while its `Reads` still says `Code`: that is how `check-crap` sat in the
  code group while reading `docs/crap.md` for the `cargo-crap` version. **The reason it gates the
  classification rather than the sweep's size:** a count goes green the moment a new gate is added,
  including one added without being classified, so it holds a number while the sentence the number
  serves rots.

## The causality gate, and how it can lie

`just causality` proves red-before-green by reverting changed files that **added no test** and
keeping every file that **added one**. The trap is orphaning: a test module whose `mod` declaration
lives in a reverted file is never compiled, Rust does not build an unreferenced `.rs`, so the base
tree compiles with none of the new tests and everything passes - verdict *green against base
behaviour*, which is a failure.

**The rule that predicts it:** a file that adds a `#[test]` is never reverted. So *moving tests out
of a file* turns that file from held into revertible and takes the new module's declaration with it.
When a file with tests hits the 1000-line cap, move the **harness** - fakes, fixtures, builders,
anything with no `#[test]` - and keep every assertion where it is. Only a *green* base verdict
fails; "the base tree does not build" and "not separable" both pass, and the second asks for
evidence instead: the command you ran, the failure before, the pass after.

**Expect that harness move to answer INCONCLUSIVE, and know why before reading it as a pass.**
Measured on #119: the new harness file added no `#[test]`, so it is *revertible*, while the test file
declaring `mod <harness>;` added tests and is *held* - so the base tree is a `mod` pointing at a file
that is not there, `E0583`, and the verdict is `INCONCLUSIVE - the base tree does not build`. That is
the gate being honest rather than broken, and it is the **expected** outcome of following the rule
above, not a sign of doing it wrong. What it costs is the proof: causality establishes nothing about
your assertions in that run, so a **mutation** takes its place - break the thing each new test
claims, one at a time, and paste the test that reddens. Scope it to a whole test binary
(`-E 'binary_id(<pkg>::<target>)'`), never a name pattern: a filter that omits the guarding test
reports green and proves nothing, which happened on that same PR before it was caught.

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
  whole class.** Both of the cross link check's steps ended in `file <path>`, and the older one
  said so: *"Proves the arch, not just the exit code."* It proves neither. Measured: `file` on a
  path that does not exist prints ``cannot open`` and **exits 0**, and nothing compared its answer
  to the triple - so a renamed or missing executable was green, and the feature-probe step reads
  that name out of a manifest. `nix/assert-linked.sh` asserts the two things the sentence claimed
  and names the one it still does not (the libc half, unmeasured, so a musl target linked
  dynamically passes). **The transferable question:** for every command a step runs for its side
  effect, ask what its EXIT CODE is a function of. `file`, `echo`, `grep -c` and any `| head` answer zero on
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
anything with no `#[test]` - and keep every assertion where it is. Orphaning now reports itself -
*the tests this diff added did not run on base* - rather than passing as green, because nextest
fails a filter that matches nothing.

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

**Which verdicts pass:** *red on base* and, without proving anything, "the base tree does not
build", "the base run named no failure", "not separable" and "every added test is `#[ignore]`d" -
the last three ask for evidence instead: the command you ran, the failure before, the pass after.
Everything else fails, and the three that fail are the three where the gate has no answer rather
than a bad one: green against base, red outside the diff, and the added tests not running at all.

**Reconstructing that evidence for "not separable": restore the base's OUTPUT, not its code.** The
obvious move - paste the base file's implementation half under the head file's tests - does not
compile whenever the base spells its private items differently, which is usual for a change that
introduced one. Measured on #268: the base module had no `Venue::advice` for the tests to call. What
is behaviourally the base is the STRING each arm produced, so put those back into the head
structure, `git checkout <base> -- <the non-test files the diff also touched>`, and run `just test`:
the failures name themselves and an unrelated red is visible as one. Revert nothing and the run
over-reports green - a justfile recipe the head added is what one of the assertions reads.

**What it could not tell apart until #278.** The base run was the whole suite and any assertion
failure counted, so with fail-fast one unrelated cell was the entire verdict and the tests under
test never ran - `ok - red on base` about something else. Both runs are scoped to the tests the diff
added now, the verdict NAMES what reddened, and a failure outside the diff is its own answer.

**A TEST NAME IS NOT A KEY HERE, and the first fix for #278 was keyed on one.** 23 of this tree's
1782 test-function names are duplicated - `deserialization_goes_through_the_constructor` four times
in `sutura-domain`. Measured on nextest 0.9.143, `test(/(?:^|::)sums(?:::|$)/)` matches six tests in
three packages, one of them a MODULE called `sums`. So a collided failure satisfied both the filter
and the name comparison, and a vacuous added test still got *ok - red on base, green on head*. The
key is now the binary or package, the module path the file contributes, and the name.

**The generalisable half:** the filter and the comparison are two ENFORCERS of one key, not two
independent keys. A second check on the same key catches the enforcer failing and never catches the
key being wrong - so "two mechanisms" is only worth what the key is worth, and the argument to write
down is which of the two it is.

**Two smaller ways the same gate lied, both from believing a word rather than measuring it.**
`ABORT [` is nextest's WINDOWS status; Unix prints `SIG<name> [`, so a base run whose only failure
was an abort parsed to zero failures and printed *the base tree does not build* about a tree that
built fine. And a filterset naming only `#[ignore]`d tests matches nothing, which nextest reports as
`no tests to run` and exit 4 - a false RED, so ignored tests leave the scope rather than being
named in a filter or forced to run. `git grep -c -E '^[[:space:]]*#\[ignore' -- '*.rs'` counts them,
and the same command WITHOUT the anchor answers roughly twice as many across twice as many files -
because most `#[ignore` in this tree is a doc comment ABOUT one. **No figure is written here on
purpose:** the argument holds at any count above zero, so a number would only be a second thing to
keep true - and the review that reported this defect cited the unanchored one.

**Why one commit answered differently in two venues, which is the part nobody could have guessed:
only one venue provisions the tier.** `just causality` sources `nix/with-tier.sh`, which starts
Postgres and exports `SUTURA_DEV_REQUIRE_TIER` into the whole process tree - and the endpoint that
requirement belongs to is published under the ROOT, in a gitignored file the base worktree cannot
have. So the tier-backed cells failed closed there and became the base run's red. `just ship-check`
and CI's `nix run .#causality` provision no tier, so those cells skipped and the verdict was about
the change. **The false green was reachable from the one venue a person runs by hand and cites.**
The base run drops that variable now, in the gate, because only the thing that provisioned a tier
may declare one.

**A `#[cfg(test)]` ITEM IS NOT A TEST, and one attribute was the whole difference between a hard
refusal and a silent pass.** The classifier read a bare added `#[cfg(test)]` as *this file adds a
test* whatever sat beneath it, so a diff adding a test-only HELPER entered the proof and then failed
to name a test that was never there - a refusal no author could act on, since no extractor
improvement reads a name off an item that is not a test. **Measured on #271's head, same tree, same
base:** with the attribute the gate exited 1 (*the added tests could not be NAMED*); with that one
line deleted it exited 0 (*NOT MECHANICALLY SEPARABLE*) over the same ten added tests. So *fix the
failure* was the wrong repair - it would have traded a loud wrong answer for a quiet one.
`causality::attributes` answers four ways now: a named test, a `mod` marker, a `#[cfg(test)]` item
that is NOT a module, or nothing. The first two are still asked to name a test, so the marker
refusal is intact; a non-module item is held at HEAD and asked for nothing, because reverting a
helper the held tests call is `DidNotCompile` and then a pass that proves nothing - over the common
case, since a helper usually exists because a new test needed it.

**A VERDICT IS OVER A SUBSET whenever anything is held back, and it now says which.** A file
carrying an implementation change and a test together is held, and its own tests are deliberately
outside the proof - so `ok - red on base, green on head` was a statement about a subset while
reading as one about the change. A verdict that ran both runs carries `N of M added tests measured`
and NAMES the ones it left out, `NOT MECHANICALLY SEPARABLE` included, where the honest number is
`0 of M`. It STATES rather than FAILS on purpose, and **that decision is measured rather than
argued: replayed over thirteen recent branch diffs, twelve reached a verdict and ALL TWELVE had
`measured < M`** - seven at zero, three partial, two on arms that measure nothing at all. A
fail-on-mismatch rule would have reddened every branch in the sample, and a gate that reddens
correct work gets disabled. **What it therefore is not:** nothing forces the remainder to be
proven. `7 of 8` is an instruction to run a mutation by hand, not a mechanism.

**Four passing arms run NEITHER run, and *every verdict carries the ratio* was false for them** -
which is the same defect class one level up, so it is worth the row. `no changed tests`,
*tests changed but no implementation did*, `EVERY ADDED TEST IS #[ignore]d` and
`NO BASE BEHAVIOUR TO COMPARE AGAINST` all return exit 0 without either run having happened, and two
of the thirteen replayed branches landed on one of them with 3 and 7 added tests. They print a
ZERO-numerator line now (`0 of 3`), because the numerator is what the filterset NAMES and that
equals what was measured only once both runs are done. **So the citable claim is: a branch that ran
the two runs prints the ratio, and a branch that did not says instead that it measured nothing.**

**`N of N` WAS MANUFACTURED OUT OF AN INPUT THE GATE COULD NOT READ, and that is the one wording
that asserts complete coverage.** The denominator comes from a second scan over the whole diff; when
that scan refused, the added set collapsed to empty and the ratio printed `N of N` - or `0 of 0`
beside `NOT MECHANICALLY SEPARABLE`, which reads as *the diff added no tests*. Two live routes,
both measured: the whole-diff scan had **no `is_compiled_rust` filter**, so a changed PROSE page
whose line begins `#[test]` was scanned as a test file (three pages under `.agents/skills/` carry
such a line); and a changed file whose added `#[test]` the extractor could not name did the same
with no prose involved, on a diff of the shape this very branch had. An unestablished denominator
is its own state now and says so. **The transferable question: for every ratio a gate prints, ask
what it prints when the denominator's source answered "I do not know".**

**THE SCAN IS AGGREGATE, and that is how a subset hid.** `Scan::of` answered `Runnable` the
moment ONE provable file named a test, so a second provable file whose added `#[test]` yielded no
name rode along unmeasured with nothing in the output naming it - the common case, not a corner. It
refuses now, ahead of `Runnable`, when a file added an attribute that DECLARES a test and no name
came out, and it counts PER ATTRIBUTE rather than per file: `named > 0` ended the file's inspection,
so a second unnameable attribute BESIDE a nameable one survived the first fix one level down.
**The precision cost of going per-attribute was measured before it was taken** - every `.rs` file
under `crates/`, `xtask/` and `dev/` walked through the extractor, and ZERO test-declaring
attributes fail to name a function - so it refuses nothing this tree writes. **The question to ask
of any aggregate answer: which input did it not need in order to say yes?** And of any per-file
one: what does a SECOND occurrence inside the file do.

**AN EXEMPTION THAT ASSERTS ITS OWN JUSTIFICATION IS NOT AN EXEMPTION.** The other unnameable shape
must not refuse - a `#[cfg(test)] mod tests;` names nothing by design and its module's own file
names the tests - so it was PRINTED as *a test module arrived here; its own file names the tests*.
Nothing read whether that file was in the diff. **A `#[cfg(test)] mod legacy;` added while
`legacy.rs` sits untouched compiles a whole pre-existing module of tests, no added line names any of
them, and the declaring file is held at HEAD so they are in BOTH trees and cannot be red on base
either** - and it passed with `1 of 1`. The refusal for exactly that cause was already in the file
and fired only when no sibling named a test, so one input had two remedies and the one that fired
was the pass. The declaration is RESOLVED now - the module's own file, `#[path]` included - and a
module this diff does not contain is its own refusal with its own remedy (state the evidence; the
extractor is not at fault). **The tell to look for: a printed sentence containing a fact, and no
code that reads it.**

**A GATE THAT SCANS A LANGUAGE MUST LEX IT, and this one read an attribute as ONE line.** A
continuation line starts with neither `#[` nor anything else the search skipped, so the downward
walk from `#[test]` stopped on `clippy::disallowed_methods,` and no name came out. **Measured over
`crates/`, `xtask/` and `dev/` on 2026-09-05: ten sites**, eight a `#[test]` over a wrapped `#[expect(..)]` and
two a wrapped `#[ignore = ".."]` reason string - and the second pair is worse, because
`OnlyIgnored`'s loud PASS was unreachable for them and they entered the filterset as runnable. It
was survivable only while the aggregate scan skipped such a file in silence; the moment `Unreadable`
refused ahead of `Runnable` it became a hard red sending the author of an unrelated change to fix
`xtask`. The bracket balancer is the same lexer that finds where a `{ .. }` item ends, and an
unclosed attribute is an ERROR rather than an answer. **This is the third time in this file that a
scan counted where it should have lexed** - `check-workflows`' braces, `check-docs`' fences, and now
this - so the pattern is the rule, not the instance.

**A GREEN CAUSALITY STEP CAN BE AN INCONCLUSIVE ONE, and a required CI job cannot tell.** Both
`INCONCLUSIVE` arms return `Verdict::Pass`, so `base did not compile` and `the base run named no
failure` are exit 0 - and that is deliberate, because the HARNESS MOVE lands on the first of them
every time and a gate that reddens correct work gets disabled. **Measured, on a finished branch:**
its `ci` causality step was green off `INCONCLUSIVE - the base tree does not build` (a `-D dead-code`
error, because a `#[cfg(test)]` helper was held at HEAD while its only caller was removed at base),
while the same commit refused locally. So the branch had red-before-green evidence in neither venue
and its author had a green check. Both arms now print *this exit PASSES and N of M added tests
measured*; the exit code itself is an open decision. **When citing this gate, cite the VERDICT LINE,
never the step's colour.**

**THE BASE IS THE OTHER WAY IT LIES, and the failing direction is the default one.** `ship-check`
defaults to `origin/main`, so on the second PR of a stack the diff carries the PARENT branch's
implementation: the gate reverts that and finds this branch's tests green, then reports
`FAILED - green against base behaviour` about two halves that do not read each other. The same
commit is green with the stack parent as the base, and `SHIP_CHECK_BASE_REF` is the lever - so a
verdict from this gate is a function of the base ref until you have said which one. Both halves
follow from the same shape: the diff is one revert-set, and nothing asks whether a test's package
could depend on what was reverted. **And the classification is over `.rs` only**, so a test whose
subject is a markdown page has nothing the gate can revert - it reads as *tests changed but no
implementation did*, which passes and proves nothing.

**The hole that remains is the shared target directory.** Both runs use one; cargo treats the two
trees as one unit and decides freshness by mtime, so a build in either overwrites the other's
binaries and the next run silently executes them - reverted source and `env!("CARGO_MANIFEST_DIR")`
included. **So a causality verdict is only about the tree whose binaries are in
`target/causality-target`** - remove that directory before trusting a surprising answer.

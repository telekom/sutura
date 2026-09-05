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

**A check can also fail on an unmodified tree with nothing lost, and it cost a session before it
was named.** The checks decompress one dependency closure for their `target/`, and a build script
may have baked the absolute `$OUT_DIR` it ran in into the code it generated - `utoipa-swagger-ui`
does, into a `rust-embed` `#[folder]`. A linux build directory is `/build` for every derivation so
the literal still resolves; a darwin one is `/nix/var/nix/builds/nix-<pid>-<random>/`, so it
resolves nowhere and a THIRD-PARTY crate fails to compile in a check that changed nothing -
`just validate` red at `checks.nextest` before running a test, on `main`. `flake.nix`'s
`inheritedArtifacts` pairs every `cargoArtifacts` with `nix/purge-baked-out-dirs.sh`, which
regenerates exactly the output naming a directory it no longer sits in: one crate of 138 measured,
so the reuse the checks exist on survives. **What it does not reach:** a generated file naming some
OTHER absolute directory, or a path written into a compiled artifact rather than into the bytes of
the output directory. Both fail the same loud way.

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
  plain backticks. Which shapes mkdocs accepts is established for two cases and no more, which is why
  this is a rule to run `just docs` rather than a gate encoding a boundary nobody measured. It is not
  in `validate` because that recipe would then fail on a clone whose pixi docs environment is not
  installed, and **a gate that fails for an environment reason gets disabled.**
- **`--strict` escalates WARNINGs and not INFO, and that is the boundary to know.** An unrecognized
  relative link is a warning, so `--strict` fails on it. A link into a page `exclude_docs` keeps out
  of the build is **INFO**: measured as *"contains a link to 'implementation-plan.md' which is
  excluded from the built site"*, **exit 0** - a dead link on a published page behind a green docs
  job. `check-docs` holds that one now, with a fail-closed count of the links it read, so the rule
  here is the general one: a new mkdocs behaviour is unproven until you have read the log AND the
  exit status, because half of what it notices it notices at a level `--strict` ignores.
- **The file a gate reads is not always the file that ships, and that defeated the rule the gate
  was added for.** `docs/contributing.md` and `docs/changelog.md` are `pymdownx.snippets` stubs -
  their published body is `CONTRIBUTING.md` / `CHANGELOG.md` at the repo root - so `check-docs`
  read a sixteen-line include directive and counted it as a page. Measured: a link into an
  excluded page written in `CONTRIBUTING.md` left the gate green with the link total unmoved. It
  follows `--8<--` now and resolves a relative target inside an included file against the
  INCLUDING page, the way mkdocs does. **The transferable question: for every file a gate reads,
  ask whether that file is what a reader gets.**
- **Three more ways that one gate read less than its verdict said, found in one review, and each
  is a shape already in this file.** (1) A fence tracker toggling a boolean on any three-backtick
  line inverted on the first NESTED fence and read no link below it - the `check-workflows` rule
  a second time, so `xtask/src/markdown.rs` records the delimiter and its length and makes an
  unclosed block an error rather than an answer. (2) `exclude_docs` was compared as a
  docs-relative path while mkdocs matches it with `pathspec` gitignore semantics, where a pattern
  containing no `/` matches at ANY depth: the pages mkdocs dropped and the pages the gate believed
  were dropped were different sets, and both of that gate's new rules were bypassed on the
  difference. **A gate re-implementing part of a tool has to be measured against the tool, not
  against its documentation.** (3) The only floor was a repo-wide link total, and `.take(1)` on
  the page loop satisfied it with 51 of 52 pages unscanned - *at least one row* again, so the
  floor is per page now. An unreadable page was also dropped in silence eight lines above a
  `FAIL CLOSED` comment, which is the reminder that **a comment is not the direction; the code at
  the decision is.**
- **Prose in `AGENTS.md` and under `.agents/skills/` is gated, which surprises people editing it.**
  `check-guidance` scans both, and `xtask`'s own unit tests assert literal phrases out of `AGENTS.md`
  as the evidence anchoring a rule - so rewording its opening sentence turns a gate's test red rather
  than failing the gate itself. Run `just hygiene` **and** `cargo nextest run -p xtask` after editing
  either.
- **`check-guidance` reads prose and cannot catch a paraphrase.** It holds forbidden phrases, a
  version pin that must agree wherever it is written, claims known false in each recorded wording,
  and the gated counts - `cargo xtask --help` and the tables themselves are the authority for how
  many, and a number here would be a second thing to keep true. It fails a cited `just` task that
  does not exist and a cited `cargo` line missing `--all-features`. Its own stated blind spot: a
  comment marker is not stripped, so a claim wrapping inside a `#` block is not found.
- **A fail-closed arm can cover *cannot say which* and leave *cannot look at all* open, and the
  second is the cheaper defect to ship.** `check-shipped-binaries` gained a refusal over a page
  whose fence never closes, and the read one line above it still said `continue`: measured in
  review, a **non-UTF-8** page under `docs/` documenting a feature `nix/shipped.nix` does not probe
  left the gate at `ok` and **exit 0**, because the other pages kept the reconciled count non-empty.
  Same shape as the unreadable page `check-docs` dropped in silence eight lines above its own
  `FAIL CLOSED` comment. **The question that catches it: for every file a gate is meant to read, is
  the ANSWER over the tree the verdict names?** Both arms are one rule now, and the read guard is
  scoped rather than blanket - a PNG inside a `docs/**` glob is out of SCOPE, not unreadable, and a
  guard that failed on it is one somebody switches off. The test that mirrors the production file
  filter is what keeps those two apart; the first version of it reported `docs/assets/favicon.png`.
- **A lexer's declared divergence has to name BOTH ends of the thing it diverges on.**
  `markdown::opens` declared that an opening fence's indentation is unrestricted, for
  mkdocs-material's admonitions; `closes` diverges identically and said nothing, so a reviewer read
  it as a bug. It is not - a fence opened four columns in is closed four columns in - but the
  omission cost a review round. And the reason was overstated in the other direction: measured
  2026-09-05, `grep -rnE '^\s{4,}```' docs` matches NOTHING, so both divergences are for a shape
  mkdocs renders and this tree does not yet write. **The related loss is real and is not the one the
  limit list named:** a build instruction written as a four-column INDENTED code block is neither
  fenced nor inline, and the fenced reader dropped it silently. Changing the lexer is the wrong fix
  here, because an admonition body IS four columns in and treating it as code would blank every
  admonition `check-docs` reads links out of - so what closes it is a refusal keyed on the full
  instruction shape, read off the PROSE half, where an inline mention is already blanked as a code
  span. It fires on nothing in the tree, which is what a refusal over a shape nobody writes should
  do.
- **A citation that RESOLVES says nothing about the cited file's contents, and that is how four
  comments named `ci.yml` as holding things it never held** - a caller of the image smoke test, the
  command building the `shellcheck` list, the reason a floor of 100 is a floor. None of them was
  falsified by a change; they were false when written. `claims/remedies.rs` deliberately does not
  path-check a bare filename with no slash, because which of eight workflows a sentence meant would
  be a guess - and a path check would have passed anyway, since `ci.yml` exists. So the answer is
  the one #289 took for its printed remedies, applied to prose: **derive the file that holds the
  mechanism and compare it against what the sentence names.** `HOSTED` fails four ways, each
  measured by mutation - the wrong file named, the mechanism moved out of the tree, TWO files
  holding it (two copies to keep in step, and a sentence naming a list orients nobody), and nothing
  attributing it at all, which is the gate-over-silence direction its two siblings each got wrong
  first. **It is a REGISTERED attribution, and that is the limit:** a false pointer nobody has
  registered is invisible, exactly as an unregistered wording is invisible to `CONTRADICTED`. The
  general rule was costed in #289 and refused - a backticked file plus a backticked identifier in
  one sentence caught two of six, and missed the reachable one structurally, because a caller keeps
  the job NAME after the body moves. **The prose discipline that does generalise: name the anchor a
  reader can grep, not the file you believe holds it.**
- **A phrase rule cannot hold a sentence that was TRUE when it was written**, and that is the whole
  argument for the constant check. A published page said an adapter's `IMPERSONATION` was
  `NoPlaceForASubject` while the constant declared `PerSubjectCredential`; the sentence went false
  when a constant three commits away in another file changed, so no recorded wording could have
  forbidden it, and `just api` then regenerated the page from the doc comment faithfully -
  **regeneration is not verification.** What holds it now RESOLVES rather than matches: for a
  sentence naming a constant by intra-doc link and a sibling variant of that constant's enum, the
  declaration is read out of the tree and the sentence has to name the variant it holds. **The
  instance was fixed by deleting the value from the prose, not by correcting it** - where a reader
  can be sent to the declaration, one copy beats two in step - so what keeps the check from passing
  over silence is a FLOOR: at least one doc comment in the tree must be read as stating a value
  correctly, or the verdict is about nothing. Measured when it landed: 6 resolvable pairs in the
  workspace, one of them stating a value, and one contradiction - the reported instance and no
  false positive. **What it does not reach:** a value the compiler resolves and a text scan does not
  (an alias, a `const fn`, a re-export), a description that gets the value wrong without naming a
  sibling variant, and a claim written across two sentences - the window is one sentence, because a
  window of the doc BLOCK reports `sutura-exec-bigquery`'s own module header, which states the value
  correctly and discusses the other variant fourteen lines down.
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
- **A gate's SELF-EXCLUSION fails OPEN, and a predicate one granularity too coarse turns a
  sentence into evidence. Both were in one gate, and both were measured.** `check-examples`
  excluded the single path `xtask/src/examples.rs` from its own scan, so its fixtures could not
  satisfy it - and `mv xtask/src/examples.rs xtask/src/examples/mod.rs` still compiles, silently
  re-admits them, and moves nothing in the verdict but a file count. The 1000-line cap drives that
  move routinely, which makes a path constant there a trap rather than a rule. So the exclusion is
  DERIVED from the crate's own manifest directory, widened to the crate because no gate's test runs
  a deployment example, and it fails closed when it removed nothing: *an exclusion that matches
  nothing is a broken scan*, the `mentioned_in`-glob rule one shape over. The coarse half:
  "is this file test code" was `text.contains("#[test]")`, so the PRODUCTION half of any `src/*.rs`
  carrying a unit-test module was passing evidence, and five files in this tree were test code by
  that substring and by nothing else - the sharpest a module doc reading *"a file that adds no
  `#[test]`"*, classified as test code because the sentence contains the string. The claim is about
  a LINE and `causality::regions::scope` already answers per line. **Ask what granularity a
  predicate answers at, and whether it is the granularity the claim is made at** - and expect the
  verdict to hide the difference, because this one stated the SCAN's size (`one of 223 test
  file(s)`) where the evidence was 16 files for one variant and exactly 1 for the other.
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
- **A SHAPE LIST is the wrong shape of answer, and the floor and the needle are two rules.** The
  rule that a `run:` block looping over the shipped set must refuse an empty one keyed on a line
  beginning `for ` and naming the variable. Three defects, all measured in one review: a guard
  existing only in a COMMENT satisfied it, because the loop search blanked comments and the guard
  search read the raw body - the comment-versus-code split for the third time in one session, and
  the sibling that already did it right was two functions up, every time; `guarded == 0` printed
  *0 step(s) loop over $BINARIES, each refusing an empty one* and passed, the empty-scan defect
  inside the fix for empty-scan defects; and an unguarded `while read -r bin; do ... done <<<
  "$BINARIES"` was INVISIBLE, same variable spelled identically, an empty here-string feeding zero
  lines. The needle is the VARIABLE now - a block that names it, other than in a refusal, reads the
  set - which was measured first: *reads the set* and *loops over the set* select the same eight
  blocks in this tree, so the widening cost nothing. **Neither half substitutes for the other:** the
  floor alone still counted a `while read` step among the eight, and the needle alone still
  permitted a verdict over zero steps. What the widening cannot reach is a read laundered into a
  helper that inherits the variable from the step `env:`, which at least moves the code somewhere
  `just lint-workflows` looks.
- **A single unreadable input dropped in silence, twice more, and the floor did not save it.** Two
  new gates read every workflow with `.ok()` / `else continue` and failed closed only when EVERY
  file was unreadable - so one dropped file was a gating job classified by nothing, or a
  `vars.*` read never compared. Third instance of the shape in this file; the sibling that had it
  right was `hook_coverage`'s own log read. **The question that catches it stays the same:** for
  every file a gate is meant to read, is the ANSWER over the tree the verdict names?
- **A gate that knows two numbers and prints one, again.** `max-lines`' inert-exemption block
  returned before the violations report, so one stale `[warn]` entry left a 1200-line file
  unnamed - the exit code right, the report half of what the gate knew, and a round trip spent.
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
- **`just ship-check` is DIFF-SCOPED, and its green used to say nothing about that.** `prek` filters
  every hook by the changed file set - which is what makes it fast enough to run before a push, and
  is correct behaviour. Measured on a branch whose diff was one workflow file and one README: five
  of ten commit-stage hooks printed `(no files to check)Skipped` - no format check, no clippy, no
  `cargo check`, no shellcheck - and the last line still said `green`. Both numbers were available
  and neither was printed. `cargo xtask hook-coverage` now prints them, and the recipe's own last
  line points at them. **The half nobody would have guessed, measured on prek 0.4.14:** a hook
  silenced with `PREK_SKIP` / `SKIP` prints **no row at all** rather than a skipped one, so a
  verdict computed from the rows would report *8 of 8 ran* over a run with two gates switched off.
  The denominator is derived from `.pre-commit-config.yaml`, and a declared hook with no row is a
  FAILED verdict. **What no filter reaches:** the shell inside `.github/actions/*/action.yml` -
  `zizmor` is pointed at `.github/workflows`, `actionlint` cannot read a composite action at the
  pinned version, and a `run:` block is not a `.sh` file. `just lint-workflows` is the only task
  that reads it, and `ship-check` runs it when the diff touches one rather than reporting the gap.
- **THE SAME GATE THEN REPORTED A FALSE FULL HOUSE THREE WAYS, and the transferable part is that a
  STATUS COLUMN IS NOT AN OBSERVATION.** All three were found in one review of the fix above, and
  each produced output a reader could not tell from a real run. (1) The loop was over the logs it
  was HANDED, so a stage with no `--log` was neither measured nor mentioned: `hook-coverage --since
  HEAD` printed `ok` having read no prek output at all - *cannot say which* was closed and *cannot
  look at all* was open, one level up from where the same shape had just been fixed. The stage
  denominator is derived from the config now, in both directions, so an unclassified stage is a
  refusal too. (2) `Dry Run` mapped to *inspected this diff*: both stage logs captured with
  `prek run --dry-run` printed `pre-commit - 8 of 10 declared hook(s) ran`, `pre-push - 4 of 4`,
  every surface covered and `ok`, exit 0 - **character for character** the lines the real run
  printed, with nothing having executed. And the only end-to-end fixture for the counting rule was
  itself a `--dry-run` capture under a doc comment calling it a real run. (3) **Eight of the fifteen
  declared hooks print `Passed` after deciding not to run** - three of the four on push - because a
  self-skip on a missing tool exits 0. Measured with the `shellcheck` entry verbatim and `nix` off
  `PATH`: notice printed, exit 0. So on any host without nix the verdict was a green run over hooks
  that announced their own abstention.
  **Reading the notice cannot be the mechanism, and that is the measured part:** prek 0.4.14 prints
  a PASSING hook's own output nowhere - verified on a one-hook repository, `Passed` and nothing
  else - and it appears only under `verbose: true`, which would also dump the whole test suite's
  output on every run. So the authority is the shell that DECIDES plus this host's `PATH`, read out
  of the hook's `entry:` and `nix/run-gate.sh`'s `case` arm rather than listed in the gate. **Its
  limit: the `PATH` probed is xtask's own**, which is prek's only because `ship-check` runs both -
  handed a log from another host, it answers about the wrong machine.
- **A green CI leg is not a gate, and only one context here is.** Measured against the API on
  2026-09-05: the `main` branch ruleset requires exactly one context, `ci`, classic branch
  protection is absent (`404 Branch not protected`), and the second repository ruleset is
  `disabled`. So the four `cross / link (<triple>)` legs - and `docs.yml`'s `verify`,
  `security-audit.yml`'s `audit`, `bigquery-acceptance` and `crap-comment` - **are required by
  nothing**, and a red one has never blocked a merge, in the queue or out of it, with no override
  and nobody clicking anything. Everything routed through the `ci` job IS gated, which is how a
  step added there is genuinely gating. `devco/required-contexts` is the record and
  `check-workflows` holds it against the jobs: a required context nothing reports fails the gate,
  because that state is a permanently pending merge rather than an ungated leg. **The
  organisation-level caveat this row used to carry is resolved, and the endpoint is the point:**
  `repos/<owner>/<repo>/rules/branches/<branch>` returns the EFFECTIVE rules - every ruleset that
  applies, at whatever level - and needs only repository read, where `orgs/<org>/rulesets` needs
  `admin:org`. Measured 2026-09-05 on `main`: six rules, every one `ruleset_source_type:
  Repository`, required status checks exactly one context, nothing organisation-sourced. What is
  left unknown is narrower and is not about this branch: an organisation ruleset that does not
  apply here today. The gate still holds only that the record and the tree agree, never that the
  record is true, because no gate can reach the API from `checks.hygiene`.
  **Two ways the record itself over-reached, both prose and both found in review:** *has never
  blocked a merge* is a claim about the PAST that a ruleset's representation does not carry - and
  the legs reported different context strings before they were renamed, so the earlier names are
  exactly what cannot be checked - and *the merge queue takes its signal from that same context* is
  GitHub's documented behaviour rather than a read, since the `merge_queue` rule's own parameters
  are sizes, grouping, merge method and a timeout, with no status-check list. **A dated
  present-tense sentence is the strongest form available here.** And the reader cannot spell the
  contexts the record is ABOUT: a matrix leg reports `<job> (<value>)` and a reusable-workflow call
  `<caller> / <job> (<value>)`, which no `jobs:` key spells - so requiring one printed *no job
  reports it*, which was false about the one remedy that file discusses. Such an entry is refused
  with that sentence now, pointing at the aggregating job whose plain context IS checkable.
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
- **A TEST NAME is the cheapest place for this whole class to hide, and the golden suite carried
  one.** `reformatting_a_document_does_not_move_the_digest` loaded ONE directory twice and compared
  the two digests. Reformatting is a claim about two DIFFERENT inputs, so what it held was repeat-load
  determinism - and it would pass for an adapter that hashed raw source bytes, which is the one shape
  the claim rules out. Measured: with `Description::parse`'s trim removed, and again with the
  frontmatter splitter handing the WHOLE document to the description, that cell stays green while the
  new test comparing two differently-laid-out catalogs through the parser goes red. **The question
  that catches it: does the test have TWO inputs, when the property is about two inputs?** Whatever
  the name says, `assert_eq!(f(x), f(x))` is a determinism check.
- **A COMPARATOR built on a display form erases what it was comparing, and it reached a live venue.**
  Both differential legs compared cells through `Value::render`, so `Null` and the text `"null"`
  compared equal, and so did `Integer(1)` and the text `"1"` - and the BigQuery acceptance leg held a
  COPY of that function, so the rows it compares against a real dataset had the same hole. Two
  transferable parts. **A canonical DISPLAY form is not an equality**: `render` exists so an anchor is
  compared the same way everywhere, and reusing it for *are these two answers the same* throws away
  the variant. And **a comparison policy copied into a second consumer is a control that will diverge
  or be wrong twice**; `sutura_domain::warehouse::agreement` is the one policy now, with its own tests
  and one mutation per property it holds. Its own limit is written at the feature switch: nothing
  asserts a shipped artefact leaves the feature off, because `checks.shipped-features` reads crate
  NAMES and the feature adds none.
- **A gate that COMPILES a test runs nothing, and that hid a whole test CATEGORY.**
  `check-default-features` covers the shipped lane with `cargo check --all-targets` and
  `cargo clippy --all-targets`, both of which stop at metadata, while every venue that RUNS a test -
  `just test`, `just serve-e2e`, `just mcp-e2e`, `just declared-source`, the `nextest` nix check -
  passes `--all-features`. So a `#[cfg(not(feature = ..))]` test was compiled by the first and
  excluded by the second: it read as coverage in a diff and held nothing, and a refusal that stopped
  refusing would have been caught nowhere. **Measured by differencing the two test lists**
  (2026-09-04, `cargo nextest list --workspace` against the same with `--all-features`): 1718 and
  1813 tests, 2 in the first and not the second, 97 the other way round.
  `check-default-feature-tests` runs that lane now,
  in `just gates` and in CI, one invocation per shipped package, and `--no-tests fail` makes an empty
  selection red instead of green. **The transferable half: a lane that only compiles is not a lane
  that covers, and the difference of the two lists is what says whether the hole is a pair or a
  category.**
- **Replacing a `contains` with "a real lexer" means picking the reader by the file's COMMENT
  SYNTAX, and the obvious one is Rust-only.** `serde_parse::scan::code_lines` was named twice as the
  fix for a `contains`-based wiring test - it is a real lexer, of **Rust**: `//`, `/* */`, char
  literals, multi-line string interiors blanked and single-line ones kept. Measured on the two lines
  the test had to reject (2026-09-05): `#  run: nix run .#default-feature-tests` and
  `# cargo run -q -p xtask -- check-default-feature-tests` both survive it **verbatim**, so a fix
  built on it keeps the false green it was chosen to close. For `#`-commented files three readers
  exist already: `workflows::collect` for workflow YAML (`a_comment_is_not_a_reference` holds it
  to skipping a `#` line), `tasks::recipe_body` for a justfile recipe, `warm_start::live_lines`
  for nix.
  **And the limit every one of them shares, `code_lines` included:** they are comment-stripped LINE
  scans, so a needle inside a single-line string on a line that IS live is still a live anchor. What
  they buy is that a commented-OUT line is not one - which is the whole property a wiring assertion
  needs, and nothing more.

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

**AND THE SECOND HALF OF THAT RULE DECIDES WHICH FILE IS MEASURED: a comment-only change holds
nothing back.** `has_non_test_additions` treats a blank line, a comment and an attribute as carrying
no behaviour, so a file whose added test sits beside nothing but a **doc comment** is not
*inseparable* - it stays the diff's PROVABLE test file and its test becomes the whole measurement,
while every file that carries a real implementation change is held back and its tests are not
measured at all. Measured on the branch that wrote this paragraph: a doc correction plus one
characterisation test in `causality/scoped.rs`, three other files carrying the actual change, and the
verdict was `FAILED - green against base behaviour` **about the one test that was never meant to be
causal**. So before adding a test that pins EXISTING behaviour, ask which file it lands in: beside an
implementation change it is held back and merely unmeasured, and beside a doc comment it is the
verdict.

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

**A SECOND CAUSE WEARS THAT SAME VERDICT, and this file recorded only the first one - #332.** *A
commit that changes a public signature a kept-at-HEAD test file calls.* The gate holds test files at
HEAD and reverts implementation files to base, so if the change altered an item's ARITY or TYPE the
held tests cannot compile against the base implementation, the retry puts everything at base, and the
answer is `the base tree does not build` for a reason that has nothing to do with the tests being
non-causal. Hit twice in one day - #331 changed `FederatedPlan::new`'s arity; #286 hit it earlier and
supplied a restatement instead of a mutation, which its review correctly refused. **The verdict is
indistinguishable from the harness move, so an author who reads only the paragraph above does not
recognise their own situation and never reaches for the substitute.** Two substitutes, both used on
#331: **scope the gate per commit** - `just causality <the commit before the signature change>` -
which is the one that yields a real `ok - red on base, green on head`, or **prove by mutation** as
above.

**AND READ THE COUNT WITH THE VERDICT, never on its own.** `N of N added tests measured` beside an
INCONCLUSIVE line means the filterset NAMED N tests, not that any of them ran against base. #307
made those arms print `0 of N`; that fixed the verdict and not the line above it - `prove` printed
the ratio BEFORE the head run, so an inconclusive run carried `measured:  N of N added tests
measured` about twenty lines above its own corrected `0 of N`, one output, one sentence, two
numerators. **What a run prints now, in order:** `filter:` with the filterset, then
`scope:     N of M added tests named`, then the verdict carrying `(X of M added tests measured)`. So
**exactly one line per run carries `added tests measured`, and it is the verdict's** - which is what
makes grepping for that sentence safe. **Held by the compiler, not by the wording:** the measured
sentence takes an `Attributed`, whose `PerTest` carries a `PerTestResults` witness whose field is
private to `causality::base` - so `base::earned` is the only place that can mint one, from two of six
`BaseOutcome` variants that exist only after a base run has been classified. A pre-run caller cannot
spell a non-zero measured numerator without a compile error; that it prints no measured sentence at
all is a unit test. #331's body had to say in prose which numerator was which; there is one to say.

**Which verdicts pass.** One of them proved something: *ok - red on base, green on head*. **Five more
pass having run NEITHER run** - `no changed tests`, *tests changed but no implementation did*,
`EVERY ADDED TEST IS #[ignore]d`, `NO BASE BEHAVIOUR TO COMPARE AGAINST` and
`NOT MECHANICALLY SEPARABLE` - and every one of them asks for evidence instead: the command you ran,
the failure before, the pass after. Four of the five carry `0 of M` beside the prose; the paragraph
on those five further down has the accounting and says which one prints no number. This list named
two of the five, so an author whose verdict was *tests changed but no implementation did* did not
find it and read that everything else fails. **The two INCONCLUSIVE answers are neither pass nor
fail** - "the base tree does not build" and "the base run named no failure" exit **3** since #307,
which is not 0 and not 1: see the paragraph at the end of this section for what each venue does with
it. **Everything else fails**, on two sides. The base run produced an answer that is not evidence
about the change: green against base, red outside the diff, or the added tests not running at all.
Or the scan refused before either run: no added line named a test, a test attribute it could not read
a NAME from, a declaration putting a module of tests this diff does not contain into the build, or a
HEAD run that was not green.

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

**FIVE passing arms run NEITHER run, and *every verdict carries the ratio* was false for them** -
which is the same defect class one level up, so it is worth the row. `no changed tests`,
*tests changed but no implementation did*, `EVERY ADDED TEST IS #[ignore]d`,
`NO BASE BEHAVIOUR TO COMPARE AGAINST` and `NOT MECHANICALLY SEPARABLE` all return exit 0 without
either run having happened, and two of the thirteen replayed branches landed on one of them with 3
and 7 added tests. **Four of the five print a ZERO-numerator line** (`0 of 3`), because the numerator
is what the filterset NAMES and that equals what was measured only once both runs are done. The fifth
prints no number at all: `no changed tests` has a denominator of zero by construction, and `0 of 0`
reads as *the diff added none* rather than as *none was measured*. This paragraph said *four* and
*they all do*; both were corrected by #319, and the count was wrong because
`NOT MECHANICALLY SEPARABLE` was left out of a list the same sentence claimed to include.
**So the citable claim is: a branch that ran the two runs prints the ratio, and a branch that did not
either says it measured nothing or had nothing to count.**

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

**AND ITS INVERSE IS STILL UNREPORTED, which is the shape of the refusal's own limit.** That refusal
reads a `.rs` diff. A pre-existing `#[cfg(feature = "x")] mod tests;` whose `x` a **`Cargo.toml`-only**
diff turns on compiles the same whole module of tests into the build with **zero added `.rs` lines** -
so the plan finds no changed test file, answers `no changed tests - nothing to prove`, and passes.
Reproduced through `plan`. **Strictly asymmetric: a `.rs` diff ADDING that `mod` refuses; a manifest
diff ENABLING it passes.** Nothing in the gate reads a manifest, so this is stated rather than held -
#343 carries the shapes - and the general question it stands for is which *inputs a gate does not read
at all* can change what the tree compiles.

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

**A GREEN CAUSALITY STEP USED TO BE AN INCONCLUSIVE ONE, and no required job could tell.** Both
`INCONCLUSIVE` arms returned `Verdict::Pass`, so `base did not compile` and `the base run named no
failure` were exit 0. **Measured, on a finished branch:** its `ci` causality step was green off
`INCONCLUSIVE - the base tree does not build` (a `-D dead-code` error, because a `#[cfg(test)]`
helper was held at HEAD while its only caller was removed at base), while the same commit refused
locally. So the branch had red-before-green evidence in neither venue and its author had a green
check.

**They exit 3 now (#307), and the shape of the fix is the transferable part.** Failing was rejected on
measured cost - the harness move lands on `DidNotCompile` every time, so does a changed signature on
the retry, and a gate that reddens correct work gets disabled - so the decision moved to the venue
with the **default closed**: 3 is neither 0 nor 1, so a consumer that has not been taught the code
fails on it. Two have been taught it, at one line each, and **both still continue** - `ci.yml` writes
the step summary and emits a workflow annotation, `ship-check` retains the verdict and goes on to the
remaining hooks. **So the job is still green on an inconclusive run.** What changed is that continuing
is a stated decision in one readable place, and `just causality` by hand now exits non-zero. **That
the default stays closed is `check-inconclusive`'s** - the invocation sites are a declared list, and a
venue that suppresses 3 with a `|| true`, or captures it and never compares it against 3, fails the
gate rather than quietly restoring exit-0 semantics.

**WHERE THAT VERDICT IS AND IS NOT VISIBLE, stated because the first version of this paragraph
overstated it and an overstated control is itself the defect.** It said the verdict *reaches the pull
request* / *reaches a reviewer who never opens the log*. It does not. `$GITHUB_STEP_SUMMARY` renders
on the workflow **run summary page** - that is what the variable is for - and a `::warning` with no
`file=`/`line=` is a **path-less annotation**: it lands in the job log, the run's annotations list and
the check run, anchored wherever GitHub chooses rather than where the author chose. Neither surface is
the pull-request conversation. **A reviewer who reads only the conversation sees one green `ci` check
and nothing else** - #307's failure mode with one click removed, not closed. The conversation
mechanism exists in the same file: `crap-comment` posts a marker-keyed sticky comment from a separate
job holding `pull-requests: write`. Routing the inconclusive verdict through that shape is the open
improvement; it was not taken here because the step's exit-3 branch **has executed in no venue** - no
run of this repository has reached it - so a delivery path added on top of it would be assumed rather
than proven. Two things therefore stay unheld: that the author supplied the substitute evidence, and
that anybody saw the verdict. **When citing this gate, cite the VERDICT LINE from the log, never the
step's colour.**

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

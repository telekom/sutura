---
title: What a Scorecard zero says about this repository
description: Why three of the six low rows in the first OpenSSF Scorecard run are not defects, why Code-Review and Branch-Protection are capped by recorded decisions rather than by neglect, what the Signed-Releases row actually measures and why every release artefact was already signed when it reported one, which SAST tool would close a gap this codebase genuinely has and why it cannot run yet, and how much of the OpenSSF Best Practices badge is already satisfiable.
---

# What a Scorecard zero says about this repository

Status: **accepted.** `docs/adr/0024` is the decision to *run* Scorecard and publish its score. This
is the decision about what to do with the score it returned. Nothing here raises the aggregate on
purpose, and one change lowers nothing while making a release page honest.

The first run scored **6.0** - run `34193206416`, commit `4c473cdb`, 2026-09-08. Six checks came
back at zero or below. **Three of them are not findings about this repository at all**, two are the
recorded consequence of decisions already made, and one is a real gap with a blocker in front of it.

| At 6.0 | Now | Check | What it is |
| --- | --- | --- | --- |
| **-1** | **10** | CI-Tests | was a 403 on a private repo; **resolved by the flip, exactly as predicted** |
| 0 | 0 | Maintained | the repository is under 90 days old; not a defect |
| 1 | 1 | Signed-Releases | **every artefact was already signed**; the row counts releases, not artefacts |
| 0 | 0 | Code-Review | structurally unreachable while one identity does the reviewing |
| 3 | 3 | Branch-Protection | capped by the merge-queue arrangement |
| **0** | **2** | CII-Best-Practices | the badge project now exists at `in_progress`; the questionnaire is unfinished |
| 0 | 0 | SAST | **real**; the tool that closes it was blocked and no longer is |
| 0 | 0 | Fuzzing | real, and it is `#146`'s, not this record's |

**Re-measured at `52f7709c` (run `34212763873`), aggregate 6.1.** Two rows moved without anyone
touching a check, and both were predicted here: the visibility flip resolved `CI-Tests` outright,
and registering the badge project moved `CII-Best-Practices` off zero. Nothing else changed, which
is the point - the remaining zeros are the ones this record argues are not defects.

## The limit that qualifies every number below

**The 6.0 run was made while the repository was private.** It is now **public** - `.private` is
`false` and `visibility` is `public`, read 2026-09-08 - so the re-measurement above is the first one
that answers about the project rather than about its visibility. The original run's caveats were:

* **CI-Tests failed outright** because of it, and nothing else. **Confirmed: it is now 10/10**,
  *"30 out of 30 merged PRs checked by a CI test"*. This is the row that proves the caveat was real
  rather than a hedge.
* **CII-Best-Practices, Code-Review and Maintained** are read from APIs that answer for a private
  repository the same way they answer for a neglected one. Re-taken while public, `Code-Review` and
  `Maintained` did **not** move - so their zeros were never about visibility, which is what this
  record claimed and is now measured rather than argued.
* **Signed-Releases and SAST are the two rows that do NOT depend on visibility.** They read the
  release page and the workflow files, and both answer the same way public or private. Every
  conclusion in this record about those two is therefore load-bearing; every conclusion about the
  other four is provisional until the score is re-taken while public.

That distinction is the reason this record is worth having rather than waiting: two rows can be
settled now, and four cannot be settled by anything in this tree.

## Signed-Releases: nothing ships unsigned, and the check does not count artefacts

**The reason text is `1 out of the last 5 releases have a total of 1 signed artifacts`, and it is
crediting one of thirty.** `v0.4.1` carries **73 assets, 30 of them `.sigstore.json` bundles**.
Every binary tarball, every OCI bundle, both SBOM formats, the attribution document and
`image-digests.txt` has one.

Two hypotheses had to be killed before that could be called a measurement artifact, because the
alternative - a signature a third party cannot pair to its artefact - would make every bundle
decorative.

**Hypothesis one: the check does not recognise the `.sigstore.json` convention. Refuted at the
source.** `ossf/scorecard`'s `probes/releasesAreSigned/impl.go`, line 43:

```go
var signatureExtensions = []string{".asc", ".minisig", ".sig", ".sign", ".sigstore", ".sigstore.json"}
```

`.sigstore.json` is on the list. It recognises exactly what this repository writes.

**Hypothesis two: it recognises them and the pairing is wrong. Refuted twice.** First, the probe
never pairs at all - it does not read subject names. Lines 65-95 of that file walk the assets, and
on the first extension match:

```go
signed = true
break
```

**So the per-release count is capped at one by construction.** `releaseLookBack = 5`, and the
evaluation sets `releaseMap[release] = 8` for a signed release and `10` for one with provenance,
then `score = int(math.Floor(float64(score) / float64(totalReleases)))`. One signed release out of
five is `floor(8/5) = 1`, which is the score observed, exactly. **A repository that signed every
artefact of all five releases would report `5 signed artifacts` and score 8.**

Second, and this is the result worth more than anything about the score: **a consumer can pair and
verify them.** Measured on 2026-09-08 against the published `v0.4.1` asset, not asserted:

```console
$ cosign verify-blob --bundle sutura-attribution.md.sigstore.json \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    --certificate-identity https://github.com/telekom/sutura/.github/workflows/release.yml@refs/tags/v0.4.1 \
    sutura-attribution.md
Verified OK
```

Exit 0, from the downloaded bytes and the bundle beside them, with the identity guessed from the
convention rather than looked up. The signatures are usable by someone who has never seen this
repository.

### What the row will do on its own, and the one thing that would move it further

The five-release window is `v0.4.1`, `v0.2.4`, `v0.2.3`, `v0.2.2` and `v0.2.1`, and every one but
the first predates signing. That is history, and it ages out. **Nothing needs to be done for this row to reach 8.**

The last two points need `releasesHaveProvenance`, whose extension list is one entry:
`.intoto.jsonl`. This repository *does* produce SLSA provenance - `actions/attest-build-provenance`
over every subject - but it lands in GitHub's attestation API, not on the release page, so the probe
cannot see it. **Publishing the provenance bundle as a release asset is a real improvement and not a
badge move**, for the same reason `docs/adr/0024` gives for signing blobs at all: `gh attestation
verify` needs the forge and an authenticated client, so provenance today is not verifiable from
mirrored bytes the way the signatures just were. It is **not done here** because it is a change to
the release path that no gate in this tree can exercise, and shipping an unverifiable change to
release signing to gain two points is the trade this record exists to refuse. **`#463`** is where
it is tracked, with the open questions written out.

### Thirteen unsigned checksums: signed, not dropped

Of the 43 non-signature assets on `v0.4.1`, **13 had no bundle, and every one was a `.sha256`
sidecar.** The action excluded them deliberately, and its argument was correct: a bundle over
`foo.tar.gz` already commits to that file's digest, so signing a file whose entire content is that
digest proves nothing new.

**Reversed, on ergonomics rather than cryptography.** The absence of a signature is read by a
person, and on a page holding 30 bundles the 13 files without one are indistinguishable from an
oversight. Worse, it is a trap with a direction: someone who verifies `foo.tar.gz.sha256` and stops
has checked integrity against a file an attacker able to replace the tarball could replace too,
while believing they checked provenance. The signature they needed was one file over.

So `.github/actions/attest-and-sign/action.yml` now signs every file that reaches the release page.
**Signed rather than dropped**, and the case for dropping deserves stating because it is not weak -
removing the sidecars shrinks the release surface and removes the trap outright. It loses to two
things: the sidecars are a published interface a consumer's script may already read, and **every
published byte is signed** is a rule a reader can check, where *every published byte except the ones
we judged uninteresting* is not. That second sentence is the argument the same file already makes
for including sidecars in provenance; the two rules are now consistent. The cost is 13 more
certificates and transparency-log entries per release, against 30 already minted.

## SAST: add CodeQL for Rust, and it cannot run yet

**The zero is honest about a tool and dishonest about a property.** Verbatim: *"SAST tool is not run
on all commits -- score normalized to 0. 0 commits out of 30 are checked with a SAST tool."* What
Scorecard looks for is a fixed list, read from `checks/raw/sast.go` on 2026-09-08: `uses:` steps
matching `^github/codeql-action/analyze$`, `^snyk/actions/.*`, `^facebook/pysa-action$`,
`^JetBrains/qodana-action$`, `^hadolint/hadolint-action$`, or the check runs
`github-advanced-security`, `github-code-scanning`, `lgtm-com`, `sonarcloud`, `sonarqubecloud`.
None is configured here, so the zero is a true statement about that list.

It is not a true statement about static analysis. Inside the one required `ci` context this
repository runs clippy at `--workspace --all-targets --all-features -- -D warnings`, with the whole
`restriction` and `nursery` categories on, `unsafe_code = "forbid"`, and `unwrap_used` / `panic` /
`indexing_slicing` denied; `zizmor` and `actionlint` over every workflow; `shellcheck` over every
script; `cargo-deny` against RustSec; and the 34 hygiene gates `just hygiene` runs, of 56 registered
`cargo xtask` tasks.

**And there is still a gap, which is why the recommendation is to add one rather than to accept the
zero.** Not one of those mechanisms tracks a value from an untrusted source to a sink. That matters
*here* specifically: this codebase compiles SQL from caller input - `-semantic` turns a query into a
plan, `-sql` turns a plan into a statement per dialect - and `SECURITY.md` already names the failure
class as one it treats as a defect: injection where text was interpolated instead of bound, and an
unquoted identifier. **Today that class is held by newtypes and review, not by a tool.**
Interprocedural taint analysis to an injection sink is exactly what CodeQL does and exactly what no
lint does, and CodeQL's Rust support has been generally available since October 2025. That is a
real check that can fail on a real risk in this tree, which is the bar `#459` set.

### Why it is not in this change

**It was blocked when this record was written, and the block has since lifted.** At 6.0 the API
answered `advanced_security.status: "disabled"` with `private: true`, so a SARIF upload needed
GitHub Advanced Security and a CodeQL workflow would have failed on every run - a red job nobody can
fix being worse than a recorded zero. **The repository is now public**, `advanced_security` is no
longer reported at all (the field is absent for public repositories, where code scanning is free),
and the obstacle is gone.

So this is a **deferral whose reason is spent**, not a refusal: **`#464`** is the change, it is now
actionable, and the only reason it is not in this commit is that adding a scanner is its own
reviewable change rather than a rider on a record. The runner-label restriction that breaks
Scorecard's own publication does **not** apply - that is `api.scorecard.dev` verifying the producing
workflow, `#451`/`#456`'s territory, and unrelated to whether CodeQL may run on a self-hosted
runner.

### And when it is added, deliberately not for the maximum score

Scorecard's evaluation weights the check `codeQlWeight = 7` against `sastWeight = 3`, the second
being the per-commit ratio. **So a present, enabled CodeQL workflow carries 7 of the 10 points
whatever the ratio is**, and the remaining 3 are bought by analysing every pull request.

**Take the 7.** Run CodeQL on pushes to `main` and on a weekly cron - not per pull request. A
buildless Rust analysis of this workspace is minutes, not seconds, and the runners are self-hosted,
so the cost is not billed minutes but contention on the same runners `ci` needs; adding it to every
pull request spends that on every push to buy three points. This is the point in the record where
the badge-maximising choice is refused on purpose.

**Two limits, stated because an overstated control is the defect.** It would be a **report, not a
gate** - the only required context is `ci`, and code-scanning alerts block nothing unless somebody
makes them a required check. And CodeQL is a second authority on findings outside the nix pin, the
same cost `docs/adr/0024` accepts for the Scorecard action itself.

### What holds this record

`cargo xtask check-workflows` gained the rule in `xtask/src/workflows/sast.rs`, and its subject is
this record's own claim rather than the score. It refuses three things: `flake.nix` dropping
`-D warnings` from `cargoClippyExtraArgs`; a tree where nothing CI reaches builds
`checks.<system>.clippy`; and **a workflow adding a Scorecard-recognised SAST tool while this file
still says none is configured.** The last direction is deliberate - adding CodeQL is a good change
that makes this section wrong the moment it lands, so the gate refuses the combination and names
the file to edit. Without it, the stand-in argument above was held by recall.

## CII-Best-Practices: one repository setting and about ten sentences

**Not a code problem and not this record's to close.** Registration plus a self-certification
questionnaire is the whole mechanism, exactly the shape the REUSE registration turned out to be in
`docs/adr/0024`'s amendment.

**The project now exists and the row has already moved.** `bestpractices.dev` project **14542**
answers with the correct `repo_url` and one match, at `badge_level: "in_progress"`, and Scorecard
now reports `CII-Best-Practices = 2`, *"badge detected: InProgress"*. That is not a rounding
artefact: `checks/evaluation/cii_best_practices.go` maps the levels to fixed scores -
`inProgressScore = 2`, `passingScore = 5`, `silverScore = 7`, gold `10`. **So finishing the
questionnaire is worth three more points, not ten**, and reaching gold is a different project again.

**And the live form is not the criteria set measured below.** Project 14542 carries the newer **OSPS**
set (AC/BR/DO/GV/LE/QA), not the ~67-item passing list, and **174 of its 196 criteria are unanswered**
on bestpractices.dev today. The per-criterion breakdown is visible only to the registering (owner)
account, so it is not re-verified here. The sweep in the next section was made against that older
list, so treat it
as **an evidence inventory rather than a percentage**: it says which properties this repository can
prove and where the proof lives, and most OSPS items are answerable straight from it. It does not
predict the badge percentage, and the 28% showing today is mostly *unanswered*, not *unmet*.

**Measured against the 67 passing-level criteria on 2026-09-08: 54 MET with citable evidence, 5 met
but undocumented, 4 not met, 4 N/A.** Per category:

| Category | MET | undocumented | NOT MET | N/A |
| --- | --- | --- | --- | --- |
| Basics | 11 | 1 | 1 | 0 |
| Change Control | 7 | 0 | 1 | 1 |
| Reporting | 4 | 3 | 1 | 0 |
| Quality | **13** | 0 | **0** | 0 |
| Security | **15** | 0 | **0** | 1 |
| Analysis | 4 | 1 | 1 | 2 |

The two categories that usually cost a project weeks are the two that are already complete. Quality
is 13/13 on the strength of 2738 `#[test]` plus 122 `#[tokio::test]` across 276 files, a
coverage-derived CRAP gate, and clippy at `restriction` + `nursery` under `-D warnings`. Security is
15/15 and one N/A on signed releases with SLSA provenance and CycloneDX SBOMs, `unsafe_code =
"forbid"`, rustls with no TLS below 1.2, a signing-algorithm parser that refuses symmetric and
`none`, weekly `cargo-deny`, and gitleaks on every commit.

**Three of the four genuinely unmet were one thing, and that thing has happened.** `repo_public`
failed because the repository was private, and `report_archive` and `discussion` failed *because of
that alone* - Issues and Discussions were already enabled and well used, simply not publicly
readable. **The flip converted all three.** Secret scanning is the one part it did not carry:
`secret_scanning` still reads `disabled`, so that remains a setting somebody has to turn on.

The fourth is `dynamic_analysis`, which is **SUGGESTED rather than MUST** - it can be answered
*Unmet, tracked as `#146`* without losing the badge, and `#146` is where fuzzing belongs.

The five undocumented items are prose, and two are the same three lines: `interact` and
`report_process` both want `README.md` to link the issue tracker, `CONTRIBUTING.md` and
`SECURITY.md`. `report_responses` and `enhancement_responses` want one honest sentence that no
external report has arrived yet.

**The verdict the owner asked for: an afternoon of form-filling, and the organisational decision
it was waiting on is already made.** Not ten minutes - 174 of 196 OSPS criteria are unanswered, and
even at a minute each with the evidence already in hand that is a sitting, not a coffee break. But
it is **answering**, not building: the sweep above found 54 of 67 old-list criteria provable from
files that already exist, Quality and Security complete, and the three MUST failures that needed a
disclosure review have been converted by the flip.

**What it is worth, stated so nobody over-invests:** finishing the questionnaire to `passing` moves
Scorecard's row from 2 to 5 - **three points of one check** - and the aggregate by a fraction of
that. Do it because the answers are a useful public inventory of what this repository can prove,
not for the number.

Two things worth fixing whether or not the badge is pursued, found by the same sweep: **`SECURITY.md`
is absent from `mkdocs.yml`'s nav**, so the vulnerability-reporting process is undiscoverable on the
published site, and **`cliff.toml` has no `security` commit group**, so `release_notes_vulns` has no
mechanism the first time it applies.

## Code-Review = 0: unreachable, not neglected

Verbatim: *"Found 0/30 approved changesets -- score normalized to 0."*

**This is arithmetic on a recorded decision, and no configuration change reaches it.** The `main`
ruleset sets `required_approving_review_count: 0` - read from
`gh api repos/telekom/sutura/rules/branches/main` on 2026-09-08, which needs no `admin:org`. It is 0
because review capacity is one person plus agent sessions that all authenticate as **the same
account**, and GitHub will not let an account approve its own pull request. So **no changeset in
this repository can carry an approval**, and Scorecard divides by the changesets it found.

It will read 0 until a second identity exists. That is a decision about who reviews, not a setting,
and it is **deliberately not proposed here**. What is recorded is that the zero is a consequence and
not an omission - because an unexplained zero beside a published score reads as neglect, and this one
is a capacity constraint somebody already reasoned about.

The same ruleset read shows what *is* held: `deletion` and `non_fast_forward` both refused,
`dismiss_stale_reviews_on_push: true`, `required_linear_history`, a squashing merge queue, and
`ci` as the one required status context.

## Branch-Protection = 3: admin enforcement would break the merge queue

The informational lines confirm deletion and force-push are disabled and stale-review dismissal is
on. The warning is *"'branch protection settings apply to administrators' is disabled on branch
'main'"*.

**Enabling it is not free here, and the reason is in the ruleset rather than in an opinion.** The
`main` ruleset carries a `merge_queue` with `min_entries_to_merge: 3` and
`grouping_strategy: ALLGREEN`, and the repository's own operating notes record that a required merge
queue nobody can bypass removes the override path entirely. Bypass is per-ruleset, not per-rule: the
same actor list that lets a maintainer land a stacked pull request out of the queue is what admin
enforcement withdraws. So the trade is **three points of a published score against the ability to
recover the default branch when the queue itself is what is broken** - and with one maintainer, the
queue jamming and the only person who can unjam it being bound by it is not a hypothetical.

**This record does not decide it**, because it is an availability trade the owner owns. It states it
so the gap is a stated one. What removes the trade is the same second identity `Code-Review` needs.

## The two that are not findings

**CI-Tests = -1 is an error, not a low score.** Verbatim: `internal error:
Client.Repositories.ListStatuses: GET .../commits/8c8c19b8.../statuses: 403 Resource not
accessible`. The commit-statuses API refused because the repository was private. **Nothing was
broken, and re-measuring settled it: the row is now 10/10**, *"30 out of 30 merged PRs checked by a
CI test"*. A negative score is Scorecard's own signal for *this check did not run*, and reading it
as a finding is the mistake this row invites.

**Maintained = 0 is a young-project warning.** Verbatim: *"project was created within the last 90
days. Please review its contents carefully."* The repository was created 2026-08-24. It merged 59
pull requests in three days. **It resolves with the calendar and nothing else.**

Neither is actionable, and both are written down here for one reason: a reader of the score who
finds four rows explained and two silent will assume the silent two are the bad news.

## Fuzzing = 0

Real, and `#146` already holds it - *no parser that reads untrusted input is fuzzed, and
`panic = abort` makes a panic process death*. Not duplicated here.

## What this record does not do

It does not raise the aggregate, and it would be a worse record if it did. Three of the six low rows
were a failed measurement, a young repository and a capacity constraint; the fourth is a
questionnaire; and the two with substance were a release page that was already correct and a taint
analysis that was blocked. **The one change that ships signs 13 files that were already covered by a
signature one file over, which buys nothing on the score at all** - the `.sha256` sidecars are
invisible to a probe that breaks on its first match - and closes a trap a person could walk into.

**And the re-measurement is the argument for having written it down rather than acted.** Between
6.0 and 6.1 two rows moved and neither was touched by a change: `CI-Tests` went from `-1` to `10`
because the repository became public, and `CII-Best-Practices` from `0` to `2` because somebody
registered a project. Both were predicted here as *not defects*. Had they been treated as defects,
the work would have been spent on rows that fixed themselves.

## Amendment (2026-09-08, `#464`): what the CodeQL-for-Rust recommendation ran into

The recommendation above says a CodeQL Rust taint check "can fail on a real risk in this tree",
and `#464` is the change, previously blocked on the visibility flip. The repository is public now,
so the SARIF upload can work - and the change ran head-first into a second blocker, this one inside
the tool. It is recorded here because it changes the recommendation's standing: **the specific taint
flow `#464` exists to hold is not reported by the buildless Rust database a CodeQL job here would
build, so a job added today would be green while reporting nothing on exactly the defect it was for.
That is the "runs and analyses nothing" shape this repository refuses, so the workflow is
deliberately not added yet.**

**The measurement.** CodeQL CLI `2.26.4` with `codeql/rust-all` `0.2.20` and `codeql/rust-queries`
`0.1.41` - the bundle `codeql-bundle-v2.26.4`, which is what the action resolves to as of this date
- was run over a **buildless** Rust database (`--build-mode=none`, the mode a job here would use)
built from a probe of the exact shape this tree compiles SQL in: a caller-supplied value is
interpolated into a statement and executed by `rusqlite::Connection::execute` - *interpolated
instead of bound*, the failure class `SECURITY.md` names. The full default Rust query suite found
**no** `rust/sql-injection` finding, and `rust/summary/query-sinks` and `rust/summary/taint-sources`
confirm both endpoints are modelled in that database.

**The cause, isolated by re-measurement rather than assumed.** Six variants, taint traced from a
caller-supplied value to the first argument of `execute`:

| Variant | Reaches the sink |
| --- | --- |
| value passed straight to `execute` | **yes** |
| value bound to a local, then passed | **yes** |
| through `format!` into a local, then passed | no |
| `format!` inline at the call | no |
| through `+` string concat | no |
| through `.to_string()` | no |

`.to_string()` failing rules out anything specific to string formatting: what the four failing
variants share is that the executed value is an owned `String` produced by a **standard-library**
function. The shipped library models exactly that step - `codeql/rust/frameworks/stdlib` carries
`alloc::fmt::format` as `Argument[0]` to `ReturnValue`, `taint`, `manual` - and the row **is
loaded**. It is inert: **the database contains no function whose canonical path is `alloc::fmt::format`,
and none beginning `alloc::` at all**, because a buildless database extracts the crate's own
dependencies but not `alloc`/`std`. The summary has no callable to attach to, so the step silently
does not exist. Every cargo dependency, `rusqlite` included, *is* extracted - which is why the sink
is recognised and the direct variant flows.

**The first reading of this was wrong, and the correction is the part worth keeping.** It was first
recorded here as a missing edge in the query library - no step from a `format_args!` node to the
`String` it produces, in a `cached` predicate no pack could extend. Both halves are false at this
version: the step is modelled, and `summaryModel` is `extensible`, so a pack *can* supply models.
The failure mode is worse than a missing query: **a taint summary that names a callable the database
does not contain produces no finding and no error.** A SAST job can therefore be green because its
models were inert, and nothing in the run says so - the same "looks like coverage" shape this record
refuses, one layer further down.

**What this means for the recommendation.** The recommendation to add CodeQL for Rust stands, and the
next step is now an experiment rather than a wait: **build the database with a real build mode, or
with dependency-and-standard-library extraction, and re-run the probe.** If the flow is then
reported, a shippable job exists today and the only cost is build time in CI; if it still is not,
the gap is in the library after all and the release to watch is the one whose notes mention Rust
taint through owned-`String` construction. Until one of those is measured, adding the job would
trade a recorded zero for a green run that looks like coverage and isn't, which this record already
treats as the worse error.

**What this does not say.** It does not say CodeQL Rust cannot track this class - only that it did
not, in a buildless database at the version named, for the reason isolated above. The variant table
was measured with a caller-supplied parameter as the taint source, which is not itself a modelled
remote source; it isolates propagation, not whether `rust/sql-injection` fires end-to-end. And
nothing here is held by a gate: `xtask`'s SAST rule refuses a Scorecard-recognised scanner while
this record still accepts the zero, so the *absence* of the workflow is enforced - the reason for
the absence is only written down.

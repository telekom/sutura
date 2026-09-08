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

| Score | Check | What it is |
| --- | --- | --- |
| **-1** | CI-Tests | a measurement that failed; not a defect |
| **0** | Maintained | the repository is under 90 days old; not a defect |
| **1** | Signed-Releases | **every artefact was already signed**; the row counts releases, not artefacts |
| **0** | Code-Review | structurally unreachable while one identity does the reviewing |
| **3** | Branch-Protection | capped by the merge-queue arrangement |
| **0** | CII-Best-Practices | a registration nobody has filed; 54 of 67 criteria already met |
| **0** | SAST | **real**, and the tool that would close it cannot run here yet |
| **0** | Fuzzing | real, and it is `#146`'s, not this record's |

## The limit that qualifies every number below

**The run was made while the repository was private, and it is private again as a temporary
unblock** - `gh api repos/telekom/sutura --jq .private` answered `true` on 2026-09-08, after
`docs/adr/0024`'s amendment recorded the flip to public. So:

* **CI-Tests failed outright** because of it, and nothing else.
* **CII-Best-Practices, Code-Review and Maintained** are read from APIs that answer for a private
  repository the same way they answer for a neglected one.
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

**Code scanning cannot accept a result here.** `gh api repos/telekom/sutura` answers
`security_and_analysis.advanced_security.status: "disabled"` and `private: true`, both read on
2026-09-08. Uploading SARIF needs code scanning, which on a private repository needs GitHub Advanced
Security. A CodeQL workflow added today would fail on every run, and a red job nobody can fix is
worse than a recorded zero. **It becomes free the moment the repository is public**, which is
already the intended direction. **`#464`** is the change, blocked on that flip.

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

**Not a code problem and not this record's to close.** The check reads `bestpractices.dev`, which
returns `[]` for this project: nobody has registered it. Registration plus a self-certification
questionnaire is the whole mechanism, exactly the shape the REUSE registration turned out to be in
`docs/adr/0024`'s amendment.

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

**The four genuinely unmet, and three are one thing.** `repo_public` fails because the repository is
private; `report_archive` and `discussion` fail *because of that alone* - Issues and Discussions are
both already enabled and well used, they are simply not publicly readable. Flipping visibility
converts three MUST criteria at once and also switches on free secret scanning, which is currently
`disabled`. The fourth is `dynamic_analysis`, which is **SUGGESTED rather than MUST** - it can be
answered *Unmet, tracked as `#146`* without losing the badge, and `#146` is where fuzzing belongs.

The five undocumented items are prose, and two are the same three lines: `interact` and
`report_process` both want `README.md` to link the issue tracker, `CONTRIBUTING.md` and
`SECURITY.md`. `report_responses` and `enhancement_responses` want one honest sentence that no
external report has arrived yet - true, and unavoidable for a private repository.

**The verdict the owner asked for: an afternoon plus one organisational decision, not a project.**
Not ten minutes, because the visibility flip is a reviewed disclosure event rather than a checkbox -
`AGENTS.md`'s public-repository rules and `docs/getting-started.md`'s own "this note is what to
delete when the repository becomes public" both say so. Budget the disclosure review; the badge form
itself is short.

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
accessible`. The commit-statuses API refused because the repository was private. **Nothing is
broken; re-measure when public.** A negative score is Scorecard's own signal for *this check did not
run*, and reading it as a finding is the mistake this row invites.

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
are a failed measurement, a young repository and a capacity constraint; the fourth is a form nobody
has filed; and the two with substance are a release page that was already correct and a taint
analysis that cannot run until a visibility flip. **The one change that ships signs 13 files that
were already covered by a signature one file over, which buys nothing on the score at all** - the
`.sha256` sidecars are invisible to a probe that breaks on its first match - and closes a trap a
person could walk into.

---
title: Three OSS-readiness scans, and what each discloses
description: Why REUSE compliance is already held by a nix-pinned lint inside the one required CI context and what its catch-all cannot catch, why ORT is declined and which gap that leaves open, why OpenSSF Scorecard runs as the official action even though that costs this repository its nix pin, exactly what publishing a score sends and to whom, and why two README badges are held against the mechanisms they claim.
---

# Three OSS-readiness scans, and what each discloses

Status: **accepted.** REUSE was already built and is unchanged by this record. ORT is **declined**,
with the gap stated. Scorecard is **built** as `.github/workflows/scorecard.yml`, using the
official action and **publishing its results**, with two README badges held against the mechanisms
they claim by `cargo xtask check-workflows`.

Three tools were asked for, either as their official GitHub Action or through a native
integration. The answers differ, and the reason they differ is the same in each case: **a scan is
not free of consequence.** It pins a tool whose version decides what it reports, it may send this
repository's identity somewhere, and a run that nothing requires is a report rather than a control.

## REUSE: already held, and the limit is not where a reader would guess

Not added, because it is here. `REUSE.toml` declares the licence of every file, `LICENSES/`
carries the two texts it references, `nix/reuse.nix` builds the lint from the same nixpkgs pin
`apps.reuse` resolves - one expression, so `just licences` and the check cannot disagree - and
`checks.reuse` runs as the step named *Licensing* inside `ci.yml`'s `ci` job.

**That venue matters and is the strongest thing in this record.** `devco/required-contexts` reads
the `main` ruleset as requiring exactly one status context, `ci` - read on 2026-09-05, and the API
remains the authority. So REUSE is one of the few checks here that genuinely gates a merge, and it
is unconditional within that job: it is not skipped on a docs-only or workflow-only diff, because
its whole job is to judge every file.

**What it catches, and what it does not, measured rather than read off the manual.** Three runs of
`just licences` against this tree on 2026-09-07:

| Mutation | Verdict |
| --- | --- |
| none (baseline) | compliant, `1162 / 1162` files, exit 0 |
| a new `.rs` file with **no** licence header | compliant, `1163 / 1163`, **exit 0** |
| a new `.rs` file declaring `GPL-3.0-only` | **not compliant**, `Fix missing licenses`, exit 1 |

The middle row is the one worth carrying. `REUSE.toml` opens with a `path = "**"` catch-all, so a
file that declares nothing resolves to Apache-2.0 and *"the sutura authors"* - which means **this
check cannot fail on an unattributed file, and a newly vendored third-party file is silently
attributed to us** until somebody appends a block for it. `REUSE.toml` and `nix/reuse.nix` both say
so in their own headers; this record adds the measurement.

So what the lint holds is that the **declaration** is complete and well formed - a licence
referenced with no text, a licence text nothing references, a deprecated identifier, a malformed
expression. It does not police whether the declaration is true. `VENDOR.md` is where a copied tree
gets recorded, and keeping the two in step is a review question. The obvious gate - *every path
`VENDOR.md` names needs a block* - is wrong, because most of what that file lists is recorded as
rewritten rather than copied and is therefore correctly ours.

**`fsfe/reuse-action` was weighed and refused.** It would run the same tool over the same tree and
produce the same verdict, at the cost of a second pin for a tool whose version decides what it
reports - which `cargo xtask check-pins` exists to prevent - and it would run in a venue nothing
requires, alongside a check that already runs in the one venue that does. For the record, the
version considered was `v6.0.0`, commit `676e2d560c9a403aa252096d99fcab3e1132b0f5`; the pinned
nixpkgs supplies `reuse` 6.2.0.

**The badge, and what makes it honest.** `README.md` now carries the REUSE badge from
`api.reuse.software`. It is a true statement of REUSE *compliance* - every file's licence is
declared - and it is **not** a statement that the declarations are correct, which is exactly the
middle row of the table above. Because a badge is a public claim and this repository treats an
overstated control as itself the defect, `cargo xtask check-workflows` holds it against a live
mechanism: the badge may only be claimed while `flake.nix` declares the check **and** some file CI
invokes something from actually builds it. A declaration nothing invokes measures nothing.

Measured on 2026-09-07: `https://api.reuse.software/badge/github.com/telekom/sutura` answers HTTP
200 and renders the word `unregistered`, because api.reuse.software clones the repository to check
it and this one is private. `.../info/github.com/telekom/sutura` answers HTTP 404 for the same
reason. Both become real when the repository is made public; the badge is added now rather than
gated behind that flip.

### No copied tree can arrive attributed to us

**`REUSE.toml` already stated this hazard in prose, named the remedy, and nothing held it.**
Verbatim from its own header: *a newly vendored third-party file is **silently attributed to the
sutura authors*** until the narrowing block is appended, and *adding a vendored tree means appending
a block, never editing the catch-all*. A rule with no mechanism is a wish, so it is a gate now: every
immediate child of `vendor/` must be narrowed by an `[[annotations]]` block **and** recorded in
`VENDOR.md`, and - the other direction, on `check-skills`' precedent - every `vendor/` block must
name a child that exists, so a block cannot outlive its tree while keeping its dated paragraph.

**This is deliberately NOT the mechanism `nix/reuse.nix` weighs and rejects**, and the difference is
what makes it sound. That one was *every path `VENDOR.md` names must have its own block*, which is
wrong because most of what that file lists is recorded *"Rewritten, not copied"* and is therefore
correctly ours - a gate demanding a foreign licence would fail on correct code. This reads a narrower
subject in the other direction: the `vendor/` **directory**, where a true copy lands, so every
subject of the rule is a copy by construction.

**Its limit, stated because `vendor/` is a convention rather than a type:** `.agents/skill-library/`
is also a copy, narrowed by hand and outside this rule's reach, and nothing here would notice a copy
placed somewhere new. Telling a copy from a rewrite in general remains the review question
`nix/reuse.nix` describes. What is closed is the case that recurs.

### So what the badge asserts, exactly

* **It does assert** that every file's licence is *declared* - REUSE compliance, held by
  `checks.reuse` inside the required `ci` job - and, now, that no immediate child of `vendor/` lacks
  its own covering `[[annotations]]` block or is missing from `VENDOR.md`. **Narrowed in review from
  "no tree under `vendor/` is unnarrowed", which read stronger than the rule holds:** a block merely
  *under* a child used to satisfy it, so one file's block marked a whole tree narrowed while its
  siblings resolved to us. The rule now requires the child itself or its whole subtree; it still
  says nothing about a file deeper inside a covered child.
* **It does not assert per-file provenance.** A first-party file with no header still passes, by
  design; the catch-all answers for it. It says nothing about a file *inside* a narrowed tree, and
  nothing about a copy placed outside `vendor/`.

That is the whole claim. Keeping the catch-all was chosen over per-file headers because the
alternative does not close the hole either: `REUSE.toml`'s header records that 420 `.snap` files,
100 of 155 markdown files and the vendored trees cannot carry a header at all, so headers everywhere
would still need three exception groups **plus** a mechanical edit to 485 files.

## ORT: declined, and here is the gap that remains

Declined. Not because licence compliance is uninteresting - it is most of what this repository
already spends effort on - but because the part of ORT that would be **new** here is narrow, and
the part that is broad is already covered by two mechanisms inside the required context.

What is already held, and where:

| Question | Mechanism | Venue |
| --- | --- | --- |
| Is every dependency's licence one we accept? | `deny.toml`'s `[licenses]` - a permissive-only `allow` list with `unused-allowed-license = "deny"`, so an entry nothing uses **fails** rather than pre-approving a future dependency | `nix run .#deny` inside `ci`, **required**; plus weekly in `security-audit.yml` |
| Does any dependency have a known advisory? | the same run's `advisories` check, against the RustSec database | as above |
| What does a distributor have to carry? | `sutura-attribution.md`, a release asset generated by `cargo metadata` and deliberately never committed, held by `cargo xtask check-attribution` (a generation is complete and every licence declared) and `check-attribution-owner` (no committed copy, and the release still generates) | both inside `ci`, **required** |
| Is every file's licence answerable? | `reuse lint` | `checks.reuse` inside `ci`, **required** |

**What ORT would add that none of those does**, stated concretely because "a different report
format over the same facts" would not be worth the cost:

1. **A source scan.** Everything above reads *declared* metadata - a crate's `license` field, a
   `REUSE.toml` annotation. ORT's scanner leg (ScanCode et al.) reads the dependency's **files**
   and finds a licence statement the manifest does not mention: a copyleft snippet vendored into a
   permissively-licensed crate, an unexpected copyright holder, a licence file that disagrees with
   the manifest. `xtask/src/attribution.rs` names this gap in its own header as *"not that the
   licence expression is TRUE of the crate's source"*. It is the one real hole, and ORT is the
   standard way to close it.
2. **Curations.** A reviewed correction to a dependency's licence metadata, kept as data rather
   than as an exception in a config file.
3. **Ecosystems `cargo-deny` cannot see.** `Cargo.lock` is Rust. This repository also resolves
   conda and PyPI packages through `pixi.lock` - the docs toolchain and the Pulumi test
   infrastructure - and neither is covered by any licence mechanism today.

**What it costs, which is why the answer is still no for now.** ORT is a JVM toolchain plus a
curation database; it is **not in nixpkgs** at the revision `flake.lock` pins - measured, not
assumed: `nix eval` on that revision answers *does not provide attribute `ort`* - so it would
arrive either as its GitHub Action (a container this repository cannot reach through
`SUTURA_IMAGE_REGISTRY`) or as a second toolchain outside the nix pin. Either way it becomes a
second authority on a licence verdict, in a venue nothing requires, over a question three required
mechanisms already answer for the ecosystem that ships.

**So the gap is left open and named**, which is the honest position: *no file-level licence scan
of dependency sources, and no licence mechanism at all for `pixi.lock`.* Reconsider when either
becomes the constraint - a copyleft finding in a dependency's source, or a Python package that
ships in something rather than only building the docs.

## Scorecard: the official action, publishing, and what that costs

Built. `.github/workflows/scorecard.yml` runs `ossf/scorecard-action` weekly, on a push to `main`,
on a `branch_protection_rule` change, and on request - with `publish_results: true`.

### The publication is the decision, and it was made deliberately

**`publish_results: true` sends this repository's aggregate score, every check's score and reason
text, the repository name and the scored commit to `https://api.scorecard.dev`** - a public
endpoint, authenticated by a short-lived OIDC token whose subject names this repository and ref.
From there the score reaches the public OpenSSF dataset and the badge.

**It is on because the badge was asked for, and the badge does not render without it.** The
action's own documentation names `publish_results: true` and `id-token: write` as the two
prerequisites. So this is not a default and it is not presented as one: it is a disclosure chosen
in exchange for a public claim somebody wanted. `devco/scorecard-publication` is the dated record,
and `cargo xtask check-workflows` fails if that record is missing or empty while the input is true.

**Reverting is one line.** Set the input `false`; the gate then fails until the badge is removed
from `README.md`, which is the point of holding the two together - a badge served from published
results is a dead image the moment publication stops, and it would go on asserting a score.

**What a run sends even without publishing, because the two are separate and only one is
optional.** The action runs every check it has and takes no check-set argument, so three outbound
queries are part of what running it costs: `Vulnerabilities` to OSV at `osv.dev` (with the
dependency set, not only the name), `CII-Best-Practices` to `bestpractices.dev`, and `Fuzzing` to
the OSS-Fuzz project list. The first duplicates `nix run .#deny` against RustSec, which already
runs inside the required `ci` job.

**The state of the repository when this was decided:** private, and `advanced_security` disabled,
both read from `gh api repos/telekom/sutura` on 2026-09-07. So today the publication has little to
disclose and the badge shows little; both become real when the repository is public.

### The action, and the pin this costs

This repository's rule is that **nix is the only pin for a tool whose version decides what it
reports**, held by `cargo xtask check-pins`. Scorecard reports findings, `pkgs.scorecard` exists in
the pinned nixpkgs at 5.5.0, and the first version of this change used it. **The badge overrides
that**, because the CLI cannot publish - publication is the action's own step - and a second,
nix-pinned local copy of the same tool would be two pins for one verdict, which is the defect the
rule exists to prevent. So the CLI route was dropped rather than kept alongside.

Two costs follow, and neither is hidden:

1. **The SHA does not pin the code.** `ossf/scorecard-action`'s `action.yaml`, read at commit
   `2d1146689b8cda280b9bc96326124645441f03bc` (tag `v2.4.4`) on 2026-09-07, is `using: docker` with
   `image: docker://ghcr.io/ossf/scorecard-action:v2.4.4`. The commit SHA in `uses:` pins that
   manifest; what executes comes from a **mutable registry tag**, and there is no way to write the
   digest from the calling side. Every other third-party reference here is a commit SHA precisely
   so this cannot happen, and this one is the exception.
2. **The image comes from `ghcr.io`.** A network behind a registry mirror has no route to it, and
   an unprefixed image reference does not fall back - it fails. Every image this repository reaches
   itself goes through `SUTURA_IMAGE_REGISTRY`; one named inside a third-party action cannot.

### The permissions, enumerated

`permissions: {}` at the workflow level; the job declares six, of which exactly one is a write:

| Permission | Why |
| --- | --- |
| `id-token: write` | the publication's authentication - a short-lived OIDC token whose subject names this repository and ref, which `api.scorecard.dev` verifies. It grants nothing else and exposes no secret |
| `contents: read` | the checkout, and every file-reading check |
| `actions: read` | workflow runs, for `CI-Tests` and `Dangerous-Workflow` |
| `checks: read` | check runs on a commit, for `CI-Tests` |
| `issues: read` | issue activity, for `Maintained` |
| `pull-requests: read` | review history, for `Code-Review` |

The four reads below `contents` are the ones the action's documentation asks for on a private
repository; without them those checks answer about the token rather than about the repository.

**`security-events: write` is deliberately absent.** It would let the run upload SARIF to code
scanning, and code scanning on a private repository needs GitHub Advanced Security, which is
disabled here as read on 2026-09-07 - so the upload would fail rather than be ignored, and the
permission would be granted for nothing. `results_format` is `json` for the same reason: SARIF
exists to be uploaded, and a format nobody consumes is a step that looks like a control.

### What is not held, and it is most of the value question

* **The workflow is not required.** It reports no status context on a pull request, because it
  deliberately does not run on one - the checks read what is on the default branch, so scoring a
  pull-request head would score a tree that is not the project. The only required context is `ci`.
  **A falling score blocks nothing**, and nothing here holds that anyone looks at it.
* **It has run, and the answer was worse than "it cannot".** The container action executes fine on
  the self-hosted label - and the publication was refused by the API for that same label. See the
  amendment below; this bullet's original prediction ("the first run on `main` is what will say")
  was correct about the venue and wrong about which half would fail.
* **Several checks cannot answer here.** `Branch-Protection` needs an admin token for most of its
  detail and the workflow's token is not one; `Webhooks` needs admin outright. Their scores are a
  statement about the token, not about the repository.
* **A low score is not a finding.** Several signals Scorecard measures - a badge, OSS-Fuzz
  registration, a public security policy - are about being a public project. The repository became
  public on 2026-09-08, so these begin to answer about the project rather than about its visibility.
* **The gate does not check the third party.** `devco/scorecard-publication` is a dated reading of
  the action's documentation. What `api.scorecard.dev` does with what it receives is outside
  anything in this tree.

## Amendment, 2026-09-08: a green publish step that published nothing

**What this record predicted would happen when the repository went public did not.** `scorecard.yml`
ran three times reporting `completed success`, and `api.scorecard.dev` held no record of this project
at all. The three things measured from run `34174327175`'s log settle every question the record left
open, and one of them is not about visibility.

**1. The action does not decline to publish for a private repository - it publishes anyway and the
API refuses.** `options.go`'s `setPublishResults` computes `PublishResults = input && !private`, and
that value is only ever PRINTED: the log shows `Private repository: true` and `Publication enabled:
false` on adjacent lines, and the publish happened regardless, because `main.go` branches on the raw
`INPUT_PUBLISH_RESULTS` environment variable instead. So while this repository was private its score
was signed into the **public sigstore transparency log** (`tlog entry created with index:
2754066664`, three times) and POSTed to `api.scorecard.dev`. That is a disclosure this record did not
anticipate and it happened before the flip, not after. The field carries no `env:` tag at all, so
`Publication enabled:` is a line that always reads `false` - it is not a control and it never was.

**2. The refusal is fail-open inside the action.** `signing.ProcessSignature` retries on a backoff
schedule and then logs `::warning::Unable to POST scorecard results to webapp` and `return nil`, so
`main.go`'s `log.Fatalf` never fires and the step exits 0. Read at the pinned SHA
`2d1146689b8cda280b9bc96326124645441f03bc`. **The publication had no witness, and a green run said
nothing about whether anything landed.**

**3. The refusal was never about being private.** Verbatim:

```text
http response 400 ... {"code":400,"message":"workflow verification failed: workflow verification
failed: scorecard job has invalid runner label: 'rust-mcp', see
https://github.com/ossf/scorecard-action#workflow-restrictions for details."}
```

`api.scorecard.dev` verifies the producing workflow before accepting a score - the code is
`verifyScorecardWorkflow` in `ossf/scorecard-webapp`, read at
`9c2f66d5f6ff56ca4a4ac2fba6ec8dcc5379d31c`, the revision the action's own README cites - and one of
its rules is an allowlist of runner labels: `ubuntu-latest`, `ubuntu-22.04`, `ubuntu-20.04`,
`ubuntu-18.04`, and nothing else. **So the badge could never have resolved, public or private**, and
this record's *"both become real when the repository is made public"* was wrong about the Scorecard
half. Making it public was necessary and not sufficient.

### What changes, and why each is a mechanism rather than a sentence

* **`score` and `published` run on `ubuntu-latest`, and it is THE FIX rather than a preference.**
  `api.scorecard.dev` performs *workflow verification* before accepting a score, and one of its
  rules is an allowlist of runner labels - an anti-tampering measure, documented at
  `ossf/scorecard-action#workflow-restrictions`. **Measured with the repository PUBLIC**, run
  `34198064772`, `completed success`, `Private repository: false`, `publish_results: true`:

  ```text
  error sending scorecard results to webapp: http response 400, status: 400 Bad Request,
  error: {"code":400,"message":"workflow verification failed: workflow verification failed:
  scorecard job has invalid runner label: 'rust-mcp',
  see https://github.com/ossf/scorecard-action#workflow-restrictions for details."}
  ::warning::Unable to POST scorecard results to webapp: http response 400 ...
  ```

  Retried three times, then a warning, and **the step exited 0**. So no amount of visibility or
  permission fixes this: the label is the blocker, and the exception is a requirement of the tool.

  **A deliberate exception, not drift.** Every other `runs-on:` in `.github/` stayed on a larger
  Rust runner - `rust-mcp` when this was written, a pinned size since (`rust-mcp-32core` or
  `rust-mcp-16core`, chosen per job and argued where the job lives), while the jobs that compile
  nothing were moved to `ubuntu-latest` alongside this one: the
  custom runner exists to build the Rust, and these two jobs build nothing - `score` is checkout, a
  third-party container action and upload-artifact; `published` is one `gh api` read and one
  `curl`. **A metadata scanner needs none of the build environment**, which is the whole
  justification.

  **Nothing here asserted the label before this change**, verified rather than assumed: `git grep
  rust-mcp` over `xtask/`, `devco/`, `.agents/`, `nix/` and `justfile` returned nothing outside the
  files this change itself adds. So no rule had to be weakened - and the new rule in
  `publication.rs` now asserts the *opposite* direction, which is the point.

  **The one precondition no file here can read:** whether GitHub-hosted runners are enabled for
  this organisation. If they are not, the scoring job fails to start - loudly, which is the right
  failure mode and better than the silent one it replaces.
* **`cargo xtask check-workflows` now holds `scorecard.yml` against every rule that verification
  applies**, offline, inside the required `ci` job: the runner allowlist, the approved step list,
  every step being a `uses:`, no container or services, no job-level or workflow-level `env:` or
  `defaults:`, no workflow-level write permission, and no other job holding `id-token: write`.
  `xtask/src/workflows/publication.rs` is the reading, with the dated revision beside each
  constant. **This is the rule whose absence let #422 ship a publication that could not land.** Its
  limit: it is a copy of a third party's source, so a label OpenSSF adds later reads as a failure
  here until the constant is updated - the safe direction, and the reason the constant names its
  revision.
* **The step list is the rule that shaped the fix.** Verification refuses the scoring job's results
  if it contains any step that is not a call to one of five approved actions, so the obvious
  witness - a `run:` step after the publish - would have broken the publication it was checking.
  The witness is therefore a separate job, and that job is the gate's own test case.
* **A `published` job reads the scoring step's own log and fails if the publication was refused.**
  The evidence was never missing - the 400 and the `::warning::` were in that step's output every
  time, and nothing read them. So the witness binds to that, not to a poll of the API: the API
  indexes with a lag, so *no record yet* cannot be told from *refused*, and a check on it would
  redden on freshness. The refusal is exact, it is in the same run, and it names its own reason,
  which the job quotes into its error. **What it does not hold:** it trusts the action to log its
  own refusal, so a future image that failed silently would pass. And with no refusal logged on a
  repository that is not public it reports *unverified* rather than success - the publication may
  not have been attempted. It asks the API nothing at all.

### What is still not established, stated rather than assumed

**Whether `api.scorecard.dev` would ALSO refuse a private repository's score is unproven here**, and
the reason is worth keeping: the runner label was refused first, in every run, so nothing has ever
got far enough to find out. That is why the `published` job reports *unverified* rather than
*failed* when no refusal is logged on a repository that is not public.

**The scan itself works and the score is obtainable.** `34198064772` scored **6.1** with the
repository public - so the scan was never the problem, only the publication of its result.
**What the score's low rows mean is `docs/adr/0025`'s decision, not this one's**: that record reads
the first run's zeros and says which are findings, and it cites this file for the decision to run
and publish at all. Nothing here restates it.

### And the REUSE half, which no code could have fixed

The REUSE badge read `unregistered` for the reason this record gives, and the missing step was
manual: registration at `api.reuse.software/register`, a form taking a name, an email and the
project URL, which the service then clones to evaluate. **Done on 2026-09-08** - the badge now reads
`compliant` and `api.reuse.software/info/github.com/telekom/sutura` answers HTTP 200. So that badge
is live and unchanged by this amendment; only the Scorecard half needed a mechanism.

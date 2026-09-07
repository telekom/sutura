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

## ORT: declined, and here is the gap that remains

Declined. Not because licence compliance is uninteresting - it is most of what this repository
already spends effort on - but because the part of ORT that would be **new** here is narrow, and
the part that is broad is already covered by two mechanisms inside the required context.

What is already held, and where:

| Question | Mechanism | Venue |
| --- | --- | --- |
| Is every dependency's licence one we accept? | `deny.toml`'s `[licenses]` - a permissive-only `allow` list with `unused-allowed-license = "deny"`, so an entry nothing uses **fails** rather than pre-approving a future dependency | `nix run .#deny` inside `ci`, **required**; plus weekly in `security-audit.yml` |
| Does any dependency have a known advisory? | the same run's `advisories` check, against the RustSec database | as above |
| What does a distributor have to carry? | `ATTRIBUTION.md`, generated by `just attribution`, held by `cargo xtask check-attribution` offline and byte-compared by `check-attribution-current` | both inside `ci`, **required** |
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
* **It has never run.** The action is a container action and every job in this repository runs on
  the self-hosted `rust-mcp` label; whether that runner can execute a docker container action is
  **unverified** - it cannot be established from a developer machine. The first run on `main` is
  what will say.
* **Several checks cannot answer here.** `Branch-Protection` needs an admin token for most of its
  detail and the workflow's token is not one; `Webhooks` needs admin outright. Their scores are a
  statement about the token, not about the repository.
* **A low score is not a finding.** Several signals Scorecard measures - a badge, OSS-Fuzz
  registration, a public security policy - are about being a public project, and this one is not
  yet.
* **The gate does not check the third party.** `devco/scorecard-publication` is a dated reading of
  the action's documentation. What `api.scorecard.dev` does with what it receives is outside
  anything in this tree.

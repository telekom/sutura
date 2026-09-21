---
name: identity
description: Which half of e2e impersonation exists and exactly where it stops, the mechanisms that are weaker than they read, and what may never be cited as leg 2 delivered. Open before touching tokens, credentials, postures, provenance or an audit record.
---

# Identity

End-to-end impersonation is the point of the product: a query executes as the subject who asked it.
**Be precise about which half exists - the overstatement is the defect here.**

| | State | What that means |
| --- | --- | --- |
| **Leg 1** - knowing who is asking | **Built** | A deployment declaring `security.inbound` verifies a caller's own token from a signature |
| **Leg 2** - a source executing as them | **Built, and unproven** | On BigQuery a source's declared per-source map decides only WHETHER a caller may be served there; the source executes as whatever principal the declared pool resolves that subject to, through workload-identity federation of the subject's own assertion. The port, the broker and the serve composition are built; the hosted venue that would show a pool resolving one is `wired` and **nobody has dispatched it**. The run this row used to cite was of an HTTP exchange the ADBC adoption deleted. Every other source still executes as one identity |

So a deployment can name the subject in every audit record, record which posture each leg ran under,
and **still read every row as one identity.** `docs/adr/0014` and `docs/adr/0010` both warn about
that confusion, which is why the startup log prints the limit beside the mode rather than only the
mode.

**A scope narrows the SURFACE and is not leg 2 arriving early.** Two grants mean a caller can be
allowed to read the catalog and not to ask a question - real least authority over the *operations*.
It buys nothing over the rows: both operations read the same bundle and every question runs with
whatever access the process already had. Describing scope filtering as per-caller **data** access
would be exactly the overstatement to avoid.

`docs/where-identity-is-proven.md` is one row per identity claim and the venue that can honestly
answer it, with what each venue **cannot** answer beside it. Read it before citing a green run as
proof of impersonation.

## Mechanisms that are weaker than they read

- **`verify_anchor` is kept to the boot path by a LINT, not by its type.** The `clippy.toml` ban plus
  one `#[expect]` is what makes a second call site an error under `-D warnings`; an `#[allow]` walks
  past it. Its `AnchorPlan` input is a **self-check on that one caller** - it reads the metric, the
  anchor's range and the coarsest grain off the pinned bundle, so it catches a boot path that
  compiled the wrong question. It is **not a barrier**: every value it reads is publicly
  constructible, and a reviewer disproved the earlier claim by fabricating a tuple that passed four
  guards. A fifth guard is not the answer - Rust has no cross-crate friend visibility, so a
  constructor the application can call is one the workspace can call. **Do not cite the type as a
  control.**
- **The shared-identity witness closes the path from a FILE, not from another crate.** It has no
  `Deserialize`, so a settings file cannot produce it and the posture cannot be reached by leaving a
  key out - but the constructors are `pub`.
- **`agrees_with` compares prose by equality.** A fabricated witness whose text is byte-for-byte
  this source's is indistinguishable from it. Added by a review that found the *shape* being checked
  and the *agreement* not.
- **Two subject shapes, one delivered.** The BigQuery adapter refuses a principal-switch shape by
  name, because GoogleSQL has no proxy-user mechanism and `agrees_with` cannot catch it - both
  shapes are the same POSTURE, so accepting it would submit the job under the transport's own
  credential while provenance reported the answer as impersonated.
- **Recording is not a control**, and the field's own documentation says so: it reaches a caller
  after the rows did. sutura retains nothing, so a record is worth what the deployment's sink is
  worth. **And *disclose instead of refuse* is not the third option it reads as**: `ToolOutcome` has
  two variants, both transports serialize `executed_as` and `rows` in one body per call, and there
  is no streaming and no second message - so the only outcome that reaches a caller without rows is a
  `Refusal`. That is why a federated answer whose legs decide identity differently is refused rather
  than labelled (`ExecutedAs::uniform`, and the `UniformlyExecuted` that
  `PinnedDefinitions::provenance` takes), and why the per-leg record still ships beside it: the
  record documents a disclosure that happened, which is a different job.
- **`SourcePosture` must never reach a `RefusalReason`.** It and `AcknowledgementReason` both derive
  `Serialize`, so a posture value in a refusal publishes the operator's own acknowledgement prose to
  every caller, log and agent context. The refusal carries the LABELS off `SourcePosture::NAMES`.
  Same reason: **compare the posture VARIANT and never the value** - the acknowledgement resolves per
  source, so two ordinary shared legs are two unequal values and one posture, and a `!=` would refuse
  the only federating shape that ships.
- **`docs/adr/0008` part 6's expiry FLOOR is not implemented, and nothing replaces it.** *Is there
  enough life left for what this query may take* lived in `WorkloadIdentityBroker::with_floor`, which
  was deleted with that broker (`docs/adr/0018`, eighth amendment) - and it had **already been
  unwired** since the `wire` removal took the composition that called it, so no served deployment
  ever had a floor. What IS checked is narrower and is the domain's: `Expiry::passed_by` plus
  `Minted::agreeing_with` refuse an **already-dead** credential, and `BoundToTheRequest::still_usable_at`
  is the reader. So a credential with one second left is presented and a question that takes two
  seconds fails at the source. `grep -rn with_floor crates` is empty - do not cite a floor.
  **What the deleted floor is still worth reading for:** it asked `Expiry::passed_by` rather than
  writing its own `unix >= now + floor`, because the two comparisons disagreed at the boundary
  second, and its absence was `Option<NonZeroU64>` rather than a zero sentinel. A second deadline
  comparison beside a documented one is the defect, not the off-by-one.
- **The clock was a port there too, for a measured reason worth keeping if a broker ever needs one
  again.** While it was `SystemTime::now()` inside `mint`, the broker suite minted a fixed 2027
  expiry against the live clock and was therefore *scheduled* to go red in early 2027 - a failure
  nobody would have been looking for. `UnixClock`/`SystemClock` made the instant an input; both are
  deleted. `sutura_runtime::relative_range::WallClock` is the surviving instance of that shape, and
  the reason `CredentialBroker::mint` never took the instant as a parameter is that widening that
  domain port reaches ten implementors across eight crates.
- **The caller's assertion is carried, and whether it is read depends on the broker.** `RequestContext`
  holds it as an `Option<(Secret, Expiry)>` - one field, because material with no lifetime is the
  disagreement that matters; `DeclaredPrincipalBroker` presents it for the driver to federate (a
  subject with none at an impersonating source is refused as `credential_unavailable`), while
  `StaticCredentialBroker` never reads it. `docs/adr/0014` Decision 3 is still open about which document each inbound mode retains to
  fill it. That field is also why `RequestContext` drops `PartialEq`/`Eq`: `==` on credential material
  is a timing oracle.

## Leg 1, and the four things it does not answer

`InboundIdentity` is closed with no default, because defaulting either way is wrong in opposite
directions. The non-obvious parts:

- **Algorithm confusion is unrepresentable twice rather than checked**: no `None` and no `HS*`
  variant, and the key set refuses a symmetric key.
- **The token CLASS is decided separately from the signature**, on the header *after* it, so it is a
  rule about a document the issuer signed rather than about an unauthenticated header. A token with
  no `typ` is refused, so the check is not satisfiable by omission. **This exists because review
  found it missing:** an OIDC ID token has the same issuer and, where the resource identifier is
  also a client id, the same audience - and it verified.
- **Revocation is bounded by AGE, and the caller-driven trigger provably cannot bound it.** A revoked
  key's `kid` is one the cache holds, so nothing fires. That is why both triggers exist, and why the
  age re-read is armed from the lookup path as well as from a timer - forgetting the timer must not
  leave a serving deployment stale.
- **That age bound EXCLUDES a failing refresh, and the exclusion is not small.** A read that fails
  and a document the cache refuses both keep the previous keys verifying, while the reservation
  stamps the *attempt* - so while refresh is unavailable the bound bounds nothing, and the cached
  signing keys are retained for as long as it stays that way. It bypasses neither the signature nor
  the token's own expiry; what it permits is continued trust in keys an earlier refresh established.
  `KeySetCache::stale_for` is the measurement - last **success**, never last attempt - and there is
  no ceiling on it: refusing is an availability-breaking policy, and no record has decided a
  deployment should degrade that way (the issue that was to decide it closed without doing so). The
  bound, stated whole: **a removed key stops verifying within `MAX_KEY_SET_AGE` plus one read while
  refresh works, and after no bounded time while it does not.** **Do not cite the age bound without
  the exclusion.**
- **One write-lock acquisition compares and stamps the window**, so a forged `kid` cannot turn every
  request into an outbound call at any concurrency, and the bound is measured from the last
  *attempt* so a dead source is limited too. **The wording was right and the code was wrong against
  it once:** the check sat outside the lock and review measured three reads where two were required.
  A bound three documents state and concurrency breaks is a defect in the code, not the claim.
- **Nothing binds an assertion to a request and nothing records what has been seen**, so inside the
  lifetime ceiling an intercepted assertion replays. A regression test asserts the replay rather than
  pretending otherwise, and `docs/adr/0014` downgrades its own *"proof that the request transited"*
  wording to a *gateway-issued identity assertion* for that reason.

The other three limits: keys come from a file and there is no JWKS endpoint; of the two metadata
documents `docs/adr/0014` describes, only protected-resource metadata is served, and only in direct
mode; and scopes decide **operations**, not rows.

## Brokers

`StaticCredentialBroker` mints what an operator declared and is **the one every non-`bigquery` build
uses** (and the `sutura` command's). It holds an entry only for a source declared shared, so an
impersonating source gets nothing and the question is refused rather than answered as the process.

`DeclaredPrincipalBroker` (`crates/sutura-exec-bigquery/src/principal.rs`) is what a served
`bigquery` deployment attaches, and it is the reason `RequestContext` carries an assertion at all.
Two maps by source, so one plan reading a shared source and an impersonating one is served by one
broker. **It decides WHETHER, never WHO**: a declared per-source map keyed on the full verified
[`SubjectKey`] says which callers this source may be asked as, and a caller absent from it is refused
rather than widened to the deployment's identity; who they BECOME is the workload-identity pool's,
resolved by Google's token service from the assertion the ADBC transport puts in front of the driver.
It presents the asking assertion's OWN expiry - an earlier round minted `NothingExpires`, which was
honest while it presented a service account's name and became a check that always answers yes the
moment it started presenting credential material.

**The exchanging broker is gone.** `WorkloadIdentityBroker` was the first and only broker that
EXCHANGED (RFC 8693), with an `ImpersonateAsAccount` second hop and a private credential cache
(`docs/adr/0031`, `docs/adr/0032`). Its two HTTP implementors went with the BigQuery `wire`
transport; after that every `StsExchange` in the tree was a test fake and no composition root could
reach the broker, so `docs/adr/0018`'s eighth amendment deleted all 2,571 lines of it. **Nothing in
this tree exchanges a token, and nothing caches a credential.** `security.credential_cache` is still
parsed and still refuses a zero capacity or window - and **is read by nothing**, which is a settings
surface with no mechanism behind it rather than a cache that is merely off.

**What the deleted cache is still worth reading for** (`docs/adr/0031`'s second amendment): the key
must be the whole `PrincipalChain` and never a bare `Subject`. Review found the first version's own
doc claiming an acting agent "is always absent today" while `sutura-http`'s inbound gate already
built one from an `act` claim; a subject-only key would have served a delegated caller the direct
caller's credential.

A broker that could not be **reached** is not a refusal: that is `SurfaceFailure::Broker` and
`503 identity_unavailable`, which shares its status with a dead data system and not its code.

## Wired in serve, and what it is still not

- The BigQuery adapter builds a **workload-identity credential document** per request and hands it
  to the driver: an `external_account` whose `credential_source` is a loopback `url` serving the
  asking subject's OWN verified assertion, nonce-bound, for as long as one request holds it
  (`crates/sutura-exec-bigquery/src/adbc/subject.rs`). No token is exchanged in this tree - Google's
  token service federates the assertion against the pool the source declares. Which of
  [`JobIdentity`]'s arms a leg carries is decided once, above, from what the broker presented, and a
  transport that cannot serve the arm it is handed refuses.
- **`service_account_impersonation_url` is deliberately absent from that document**, asserted null
  by `adbc/subject/tests.rs`. So nothing impersonates a declared account, the federated credential
  IS the pool principal, and the grant that matters is `roles/iam.workloadIdentityUser` on the pool
  - **not** `roles/iam.serviceAccountTokenCreator`, which is what stopped applying. A reader who
  provisions from the older wording grants the wrong thing.
- **`sutura serve` attaches `DeclaredPrincipalBroker`** (`crates/sutura-cli/src/serve/broker.rs`) to
  a served `bigquery` source with a declared `workload_identity`, so an impersonating source is no
  longer refused by name - it is opened. The `sutura` command (`mcp`/`query`) still refuses by name,
  because it attaches only `StaticCredentialBroker`; the two roots are separate binaries and the
  CLI's attaching half is not built. A forgotten attachment cannot silently read every row as the
  deployment: the port refuses a source the broker holds neither half for as
  `credential_unavailable`.
- `CredentialUnavailable` is reachable **through the served binary**: a served impersonating source
  with no inbound gate answers `403 credential_unavailable` (no verified subject to serve), and a
  broker that cannot be reached gets `503` from `SurfaceFailure::Broker`. None of that is an answer
  *under* an asker.
- **No exchange has run against a real Google token service from any code in this tree.** A round of
  this file offered a hosted run on 2026-09-16 as evidence, unqualified. That run was of
  `wire::StsOverHttp`, `wire::IamCredentialsOverHttp` and `WorkloadIdentity::target_for` - all three
  deleted with the HTTP transport (`docs/adr/0018`, fifth and eighth amendments) - so it is a run of
  code this tree does not contain and may not be cited for anything the tree does now.

**What leg 2 rests on, and it is a `wired` venue rather than a run.** The mechanism is built, the
pool is provisioned in `test-infra/pulumi/google`, and the cells are written:
`crates/sutura-exec-bigquery/tests/declared_principal.rs`'s
`each_subject_executes_as_its_own_principal_at_the_declared_pool` with
`the_deployments_own_identity_is_neither_subjects_principal` as its control, each `#[ignore]`d and
failing rather than skipping when the environment is unset, dispatched by
`.github/workflows/bigquery-declared-principal.yml`. **Nobody has dispatched it**, which is why
`docs/where-identity-is-proven.md` records that venue as `wired` and not `yes`. Nothing reachable
from this repository shows Google ACCEPTING an assertion.

**And what ships is not the principal switch either.** A round of this row said the transport sets
`bigquery.impersonate.target_principal` per job with the caller's credential nowhere in the chain,
which was true for two rounds and was rejected; `JobIdentity` has no spelling for it now.

**One consequence for what the declared map buys.** Its KEYS decide which subjects a source may be
asked as; its VALUES - the account addresses - are read by nothing, because the pool resolves a
subject to its own principal and no `service_account_impersonation_url` is sent. Two subjects
therefore need two pool bindings, not two entries in a map.

**The limit on all of the above, stated where the claim is:** `check-guidance`'s leg-2 rule
(`xtask/src/guidance/leg_two.rs`) holds this page to the venue cell, but its scope is
`md`/`nix`/`yml`/`yaml`/`toml`/`sh` - **no gate in this repository can refuse a leg-2 overstatement
in a Rust comment**, and it matches literal wordings, so a paraphrase escapes.

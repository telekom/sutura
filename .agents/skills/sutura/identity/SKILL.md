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
| **Leg 2** - a source executing as them | **Wired in serve, not proven live** | The port, the exchanging broker and the serve composition are built: `sutura-serve`'s `bigquery` build attaches the broker, so an impersonating source is openable as the asker. **No exchanged token has ever run against a real STS and no deployment answers as an asker** - the two-grant acceptance leg cannot run without a project |

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
  worth.
- **`Expiry` used to be read by nothing; the FLOOR now lands it in the broker.** The domain reads no
  clock - `Expiry::passed_by` takes the instant as an argument and `Minted::agreeing_with` makes the
  already-dead check there. The FLOOR (`docs/adr/0008` part 6) - *is there enough life left for what
  this query may take* - lives in the broker adapter, the component that has the configured query
  timeout: `WorkloadIdentityBroker::with_floor` refuses an exchanged credential already inside the
  floor rather than presenting it. The `sutura-serve` composition wires the floor from
  `server.request_timeout_seconds`.
- **The broker's clock is a port, not an ambient read**, and the reason is a measured one: while it
  was `SystemTime::now()` inside `mint`, the broker suite minted a fixed 2027 expiry against the
  live clock and was therefore *scheduled* to go red in early 2027 - a failure nobody would have
  been looking for. `UnixClock` makes the instant an input (`SystemClock` ships, `measured_against`
  is how a test names one), so the floor's decision is asserted at instants decades out. Two of its
  three arms are that a mint *does not ask the time*: a purely shared mint and a broker with no
  floor. Those are held by a clock that always fails, plus a third test firing the same clock
  through a floor that can, so the pair cannot pass vacuously. **The narrower shape this is not:**
  every other time-dependent API here takes the instant as a *parameter*, which is better and is
  unavailable while `CredentialBroker::mint` carries none - widening that domain port reaches ten
  implementors across eight crates.
- **The floor's absence is `None`, never a zero.** `Option<NonZeroU64>`, because the sentinel
  version needed the same "is there a floor" test in `mint` *and* in `clears_floor`, each with a
  paragraph promising the two would not drift. `with_floor` is the one place a zero is read, and the
  served path cannot reach it: `RequestTimeout::parse` already refuses a zero timeout. With no floor
  the adapter refuses nothing - not even an already-past expiry, which is the domain's
  `Expiry::passed_by` at the leg.
- **The floor asks `Expiry::passed_by` rather than comparing, and the boundary is why.** It once
  wrote its own `unix >= now + floor`, a second deadline comparison beside the domain's - which
  counts the boundary second as PASSED on purpose, since `not_after` is whole seconds. The two
  disagreed the wrong way: a credential with *exactly* the floor left was granted here and then
  called expired by the domain at the last instant of the budget it had just cleared. A second
  comparison next to a documented one is the defect, not the off-by-one.
- **The caller's assertion is carried, and whether it is read depends on the broker.** `RequestContext`
  holds it as an `Option<Secret>`; `WorkloadIdentityBroker` exchanges it (a subject with none at an
  impersonating source is refused as `credential_unavailable`), while `StaticCredentialBroker` never
  reads it. `docs/adr/0014` Decision 3 is still open about which document each inbound mode retains to
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
- **One write-lock acquisition compares and stamps the window**, so a forged `kid` cannot turn every
  request into an outbound call at any concurrency, and the bound is measured from the last
  *attempt* so a dead source is limited too. **The wording was right and the code was wrong against
  it once:** the check sat outside the lock and review measured three reads where two were required.
  A bound three documents state and concurrency breaks is a defect in the code, not the claim.
- **Nothing binds an assertion to a request and nothing records what has been seen**, so inside the
  lifetime ceiling an intercepted assertion replays. A regression test asserts the replay rather than
  pretending otherwise, and `docs/adr/0014` downgrades its own *"proof that the request transited"*
  wording to a *gateway-issued identity assertion* for that reason.

The other three limits: keys come from a file and there is no JWKS endpoint; the two metadata
documents `docs/adr/0014` describes are not served; and scopes decide **operations**, not rows.

## Brokers

`StaticCredentialBroker` mints what an operator declared and is **the one every non-`bigquery` build
uses** (and the `sutura` command's). It holds an entry only for a source declared shared, so an
impersonating source gets nothing and the question is refused rather than answered as the process.

`WorkloadIdentityBroker` is the first broker that **exchanges** (RFC 8693) rather than minting from
configuration, and it is the reason `RequestContext` carries an assertion at all. Two maps by
source, so one plan reading a shared source and an impersonating one is served by one broker; one
`Expiry` for the whole answer, the earliest across everything minted, and a floor wired from the
query timeout. `sutura-serve`'s `bigquery` build composes it for a deployment with an impersonating
source, carrying both shapes.

A broker that could not be **reached** is not a refusal: that is `SurfaceFailure::Broker` and
`503 identity_unavailable`, which shares its status with a dead data system and not its code.

## Wired in serve, and the two things it is still not

- The BigQuery adapter declares a per-subject credential and sends the asker's token as its job's
  own bearer, decided **once before anything is built or sent** - the asker's where the leg carried
  one, otherwise the source's own, never both, which is what keeps two concurrent subjects apart at
  that seam. The expiry guard stays on the source's own credential, because a subject's token was
  already checked by the broker that minted it.
- **`sutura-serve` attaches the exchanging broker** to a served `bigquery` source with a declared
  `workload_identity`, so an impersonating source is no longer refused by name - it is opened and
  answers as the asker. The `sutura` command (`mcp`/`query`) still refuses by name, because it
  attaches only `StaticCredentialBroker`; the two roots are separate binaries and the CLI's
  attaching half is not built. A forgotten attachment cannot silently read every row as the
  deployment: the port refuses a source the broker holds neither half for as `credential_unavailable`.
- The exchange has never run against a real STS. No live token has been exchanged, and no answer any
  deployment produced was evaluated under an asker.
- `CredentialUnavailable` is reachable **through the served binary** now: a served impersonating
  source with no inbound gate answers `403 credential_unavailable` (no assertion to exchange), and a
  caller whose exchange the provider refuses gets `503` from `SurfaceFailure::Broker`. None of that
  is an answer *under* an asker.

**What it would take to call leg 2 served, and it is THREE things rather than two.** The list used
to read *a workload-identity pool to exchange against, and a two-grant acceptance leg*. The pool is
provisioned, the leg is written - `sutura-exec-bigquery`'s
`two_subjects_with_different_grants_read_two_different_row_sets` and
`each_principal_is_who_this_source_says_it_is_executing_as`, both `#[ignore]`d and both failing
rather than skipping when their environment is unset - and neither has run. The third was found by
writing the second:

**The shipped exchange cannot become a service account at all.** `wire::StsOverHttp` posts one
RFC 8693 request and returns what comes back, which for a workload-identity pool is a FEDERATED
credential: the provider resolves it to a pool subject, not to an account. Turning that into a
service account is a second call (`iamcredentials`) this adapter does not make, and the test stack
binds no pool principal to either service account. So a real run of the exchange leg reads back a
pool subject and goes red, which is the finding rather than a defect in the leg.

**And one consequence for what a subject token can buy.** A plain exchange yields exactly ONE
identity per subject token - whoever the token's `sub` is - so *two* principals need *two* subject
tokens. One workload identity cannot become two accounts without the hop above, whatever the pool
is configured with.

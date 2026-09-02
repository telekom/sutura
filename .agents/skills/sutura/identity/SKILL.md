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
| **Leg 2** - a source executing as them | **Not served** | The port is built and one adapter can carry a subject credential. No published artifact opens such a source |

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
- **`Expiry` is read by nothing.** The domain has no clock; `docs/adr/0008` part 6 puts the floor in
  the broker adapter.
- **The caller's assertion is carried and unread.** `RequestContext` holds it as an
  `Option<Secret>`, and the shipped broker never reads it. `docs/adr/0014` Decision 3 is still open
  about which document each inbound mode retains to fill it. That field is also why
  `RequestContext` drops `PartialEq`/`Eq`: `==` on credential material is a timing oracle.

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

`StaticCredentialBroker` mints what an operator declared and is **the one every shipped binary
builds**. It holds an entry only for a source declared shared, so an impersonating source gets
nothing and the question is refused rather than answered as the process.

`WorkloadIdentityBroker` is the first broker that **exchanges** (RFC 8693) rather than minting from
configuration, and it is the reason `RequestContext` carries an assertion at all. Two maps by
source, so one plan reading a shared source and an impersonating one is served by one broker; one
`Expiry` for the whole answer, the earliest across everything minted.

A broker that could not be **reached** is not a refusal: that is `SurfaceFailure::Broker` and
`503 identity_unavailable`, which shares its status with a dead data system and not its code.

## Built and not wired - do not cite as leg 2

- The BigQuery adapter declares a per-subject credential and sends the asker's token as its job's
  own bearer, decided **once before anything is built or sent** - the asker's where the leg carried
  one, otherwise the source's own, never both, which is what keeps two concurrent subjects apart at
  that seam. The expiry guard stays on the source's own credential, because a subject's token was
  already checked by the broker that minted it.
- **The composition root refuses that posture BY NAME** while no exchanging broker is attached to a
  served source. That is a COMPOSITION fact and not a capability one, which is why it is an `if` in
  the root rather than a `deliverable_by` arm - the capability check *passes* that adapter.
- The exchange has never run against a real STS. No live token has been exchanged, and no answer any
  deployment produced was evaluated under an asker.
- `CredentialUnavailable` is therefore unreachable end to end on the shipped binary. What provokes it
  is the broker asked directly, `answer` with a refusing broker, and the request path through the
  assembled router - which narrows the limit rather than lifting it: what is shown is that the
  transport reaches the refusal, in a test deployment.

**What it would take:** a workload-identity pool to exchange against, and a two-grant acceptance leg
showing two subjects reading two different row sets.

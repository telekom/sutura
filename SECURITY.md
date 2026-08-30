# Security policy

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository: **Security -> Report a
vulnerability**. That opens a private channel with the maintainers.

Please do not open a public issue for a suspected vulnerability. A public issue is the one
disclosure route we cannot take back.

Include what you have: the affected version or commit, what an attacker can do, and a
reproduction if you have one. A partial report is worth sending.

We aim to acknowledge within five working days and to agree a disclosure timeline with you.

## What is in scope

This project is early, and the scope has changed twice: once when the HTTP surface landed, and again
when the identity path did. **There is a network perimeter and there is now a caller identity, and
both are in scope.** What is built is a governed semantic compiler and executor over local files,
served over HTTP behind a rate limiter and either a shared deployment token, a verified caller token,
or both.

**Read this paragraph before deciding a report is out of scope, because an earlier version of this
file said the opposite and would have talked a valid report out of being sent.** There are now two
credentials and they are not the same thing:

- The **shared bearer token** authenticates the *deployment*. Anyone holding it is the same caller as
  everyone else holding it.
- Where a deployment declares `security.inbound`, sutura verifies the **caller's own** token - a
  signature against a pinned asymmetric algorithm, an issuer, an audience that is this deployment's
  own resource identifier, an expiry, and a required token class. That establishes *who is asking*,
  and it gates *which operations* they may invoke through OAuth scopes.

**What neither of them does is decide which rows an answer contains.** A credential port exists and
every execution of a question goes through it - `Warehouse::execute` has no signature that runs
without a credential a broker minted for that source, and a subject with no credential at a source is
refused rather than answered as the process. But **no adapter in this build can carry a per-subject
credential**: both declare that they have nowhere for one to arrive, and the broker that ships mints
what an operator configured. So a deployment can know exactly who is asking, record it, and still read
every row as one identity.

That makes the scope precise rather than absent:

- A report that a caller can invoke an **operation** they were not granted - past the scope check,
  past the bearer gate, past the token validator - **is a finding, and a high-severity one.**
- A report that one caller can read another caller's **rows** is *not* a finding on the shipped
  build, because no per-subject access exists to breach: every question runs with whatever access the
  process already had, and the startup banner says so beside the mode. **A report that sutura
  *claims* otherwise - in a log line, an audit record, provenance, a doc page, or this file - is a
  finding**, because a deployment that believes it impersonates and does not is the failure this
  distinction exists to prevent.
- A report that the **identity itself** can be forged or confused - a token accepted with the wrong
  class, the wrong audience or an unexpected algorithm; a caller stating its own identity in a header
  or a body; a revoked key that keeps verifying past its bound; an assertion replayed outside the
  window this deployment configured - **is a finding.**

In scope on the perimeter:

- a way past the bearer gate, or a path that reaches data without it
- a way to make the rate limiter ineffective - including bucket keying that a caller can choose,
  and unbounded growth of the limiter's own state
- a way to bypass the request-size or time bounds, or to make the service hold work after the
  caller has been answered
- transport-encryption failures: cleartext where TLS was declared, a certificate reload that opens
  a window, or a configuration that starts permissively where it should refuse
- a startup configuration that is accepted and should not be - the refusals are a control, and one
  that can be talked out of is a bug

And, as before, the things that affect anyone who builds or runs it:

- a way to get unreviewed content into a published artifact or image
- a credential leak, or a path that logs or serialises one
- a build that can be induced to fetch from a source the maintainers did not choose
- a gate that reports success without checking what it claims to check
- a way to get SQL, a table name or a predicate onto the tool surface, or a caller value into a
  statement as text rather than as a bind parameter

And on the identity path, now that one exists:

- a token accepted where it should be refused - the wrong signing algorithm, a symmetric key, an
  audience that is not this deployment's, an absent or wrong token class, a missing or forward-dated
  `iat`, or a lifetime past the ceiling the deployment configured
- a caller-supplied identity that is believed: a header, a request field or a body key that reaches
  the principal chain
- a key removed from the key set that keeps verifying past the bound, or a forged key id that turns
  requests into outbound fetches
- a deployment that declares an inbound identity and serves without one
- an operation invoked without the scope that governs it, on either transport
- an outcome returned without a record having been written first
- a **claim** about identity that the build does not deliver: a record, a provenance value, a log
  line or a document saying a leg ran as the asking subject when no adapter can carry one

Per-**row** access as the calling subject is still the highest-severity class this project will have,
and it does not exist yet. The two credentials above are not steps toward it: what it needs is an
adapter that can carry a per-subject credential, and neither shipped adapter can.

## What we already treat as a defect

Each of these is held by a type, a lint or a gate today, and `AGENTS.md` names the mechanism beside
it. If you can break one, that is a security bug, not a feature request:

- SQL, a table name, or a predicate reaching the tool surface
- a value from a question reaching a generated statement as text rather than as a bind parameter
- an identifier reaching a generated statement unquoted
- a metric's definitional filter absent from, or nameable on, a question about that metric
- a plan silently spanning two data systems
- a bundle serving answers when a declared anchor was not checked, or did not reproduce its number
- a credential appearing in a log, an error, or a serialised value
- a result cache keyed by anything other than the subject first
- a second call site for `Warehouse::verify_anchor`, the one port method that reaches a data system
  with no credential. It belongs to the boot path alone; `clippy.toml` bans it and the boot path holds
  the single `#[expect]`. **Its input type is a self-check and not a barrier** - see *Not yet
  guarantees* below

## Not yet guarantees

These are the design and are **not** enforced, so breaking one is not a vulnerability report - it
is the state of the repository, recorded in the invariant table in `AGENTS.md` and in
[what exists today](https://telekom.github.io/sutura/latest/architecture/#what-exists-today):

- **a query executing as the calling subject.** The port is there and the fallback is not: every
  question goes through a credential a broker minted, and a subject with no credential at a source is
  refused rather than downgraded. What is missing is the other end - no adapter in this build has
  anywhere for a per-subject credential to arrive, so a leg runs under the identity the operator
  configured for that source. A report that this build does not impersonate is the state of the
  repository; a report that it *says* it does is a finding
- **`AnchorPlan` is not a barrier, and citing it as one is the mistake this line exists to stop.**
  It parses a plan as one the pinned bundle itself agrees is a declared anchor's own, so it catches a
  boot path that compiled the wrong question - but every value its constructor reads is publicly
  constructible and Rust has no cross-crate friend visibility, so in-process code that wants to
  construct one can. What keeps the credential-free method to the boot path is the `clippy.toml` ban
  on it, which is a lint rather than a type: it reaches this workspace and an `#[allow]` walks past
  it. A report that the *type* can be constructed outside the boot path is the state of the
  repository, recorded here and on the type
- a **row**-level entitlement between callers. Scopes narrow which operations a caller may invoke and
  decide nothing about which rows an answer contains
- **binding a gateway identity assertion to a request.** The replay window is bounded by this
  deployment's own ceiling and nothing binds an assertion to what it was sent with, so inside that
  window an intercepted assertion replays. A regression test asserts the replay rather than pretending
  otherwise
- a result leaving without provenance in an Arrow schema. There is no Arrow envelope

## Supported versions

Pre-1.0. Only the latest tag is supported, and there are no backports.

# Security policy

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository: **Security -> Report a
vulnerability**. That opens a private channel with the maintainers.

Do not open a public issue for a suspected vulnerability. A public issue is the one disclosure route
we cannot take back.

Include what you have: the affected version or commit, what an attacker can do, and a reproduction
if you have one. A partial report is worth sending.

We aim to acknowledge within five working days and to agree a disclosure timeline with you.

## What counts as a vulnerability

sutura has a network perimeter and a caller identity. Both are in scope. There are two credentials:

- The **shared bearer token** authenticates the deployment. Without `security.inbound`, every holder
  of the token is the same caller.
- With `security.inbound`, sutura verifies the **caller's own** token: the signature (a pinned
  asymmetric algorithm), the issuer, the audience (this deployment's own resource identifier), the
  expiry and the required token class. This shows who is asking. OAuth scopes decide which
  operations the caller can invoke.

Caller authentication does not decide which rows an answer contains. Every question goes through a
credential broker for its source. A BigQuery or ClickHouse source with
`posture: impersonation-at-source` names the account that runs each question: the account that the
source's per-subject map declares for the verified caller. sutura refuses an undeclared subject. It
does not run that subject as the deployment. Other sources run under their declared shared identity,
and row access follows the grants of that identity.

These are findings:

**Identity**

- A caller invokes an operation without the scope that governs it, on either transport. This is a
  high-severity finding.
- The token validator accepts a token that it must refuse: the wrong algorithm, a symmetric key, the
  wrong audience, an absent or wrong token class, a missing or forward-dated `iat`, or a lifetime
  past the ceiling that the deployment configured.
- A caller-supplied identity is believed: a header, a request field or a body key reaches the
  principal chain.
- A removed key keeps verifying past the bound. A forged key id causes an outbound fetch.
- An assertion replays outside the window that the deployment configured.
- A question runs as an account other than the one declared for its verified subject, or one caller
  reads the rows of another caller through an impersonating source.
- A record, provenance value, log line or document says that a leg ran as the asking subject when
  its source used a shared identity.
- An outcome is returned before a record is written.
- A deployment declares an inbound identity and serves without one.

**Perimeter**

- A way past the bearer gate, or a path to data without it.
- A rate limiter that is ineffective: a bucket key that a caller can choose, or unbounded growth of
  the limiter state.
- A way to bypass the request-size or time bounds, or to make the service hold work after the caller
  has its answer.
- A transport-encryption failure: cleartext where TLS was declared, a certificate reload that opens
  a window, or a configuration that starts permissively where it must refuse.
- A startup configuration that is accepted and must be refused. A refusal that a caller can talk out
  of is a bug.
- `/mcp` served with no inbound identity where the startup refusals must stop it: a `multi-user` or
  undeclared mode; an off-host deployment without both the deployment token and the limiter; or a
  source that runs as the asking subject. For `/mcp`, a declared proxy, a TLS terminator or a
  non-loopback host name counts as off-host.

**Build**

- Unreviewed content in a published artifact or image.
- A build that fetches from a source that the maintainers did not choose.
- A gate that reports success without checking what it claims to check.

**Held by a type, a lint or a gate**

`.agents/skills/sutura/invariants` names the mechanism for each of these and the limit it does not
reach. If you can break one, that is a security bug, not a feature request.

- SQL, a table name or a predicate on the tool surface.
- A value from a question in a generated statement as text, not as a bind parameter.
- An identifier in a generated statement without quotes.
- A metric's definitional filter that is absent from, or can be named on, a question about that
  metric.
- A plan that silently spans two data systems.
- A bundle that serves answers when a declared anchor was not checked or did not reproduce its
  number.
- A credential in a log, an error or a serialised value.
- A result cache keyed by anything other than the subject first.
- A second call site of `Warehouse::verify_anchor` or `Warehouse::declared_key`. Both reach a data
  system with no credential. They belong to the boot path, and `clippy.toml` bans both. Each other
  call site needs an `#[expect]` that a reviewer sees.

## Design, not guarantee

A report that breaks one of these is the state of the design, not a vulnerability.
`.agents/skills/sutura/invariants` records each one.

- **`AnchorPlan` is not a barrier.** It parses a plan as one that the pinned bundle agrees is the
  plan of a declared anchor. The values that its constructor reads are public types, and Rust has no
  cross-crate friend visibility, so in-process code can build one. The `clippy.toml` ban keeps the
  credential-free methods to the boot path. That ban is a lint, not a type, and an `#[allow]` passes
  it.
- **A declared cardinality does not hold continuously or on every data system.** sutura counts a
  relationship whose join type says that its target column identifies at most one row against its
  distinct values, once, at startup. A bundle that the data contradicts does not validate. Three
  cases are not covered: a duplicate added after startup (there is no reload); a data system whose
  adapter cannot count; the origin half of a `one_to_one`. A failing probe does refuse the boot.
- **`single-user` is a mode and a reason that the operator writes.** Nothing counts who calls. With
  no inbound identity, `/mcp` answers every caller as the deployment with every capability, as `/v1`
  does.
- **Row-level entitlement between callers.** Scopes narrow which operations a caller can invoke.
  They decide nothing about which rows an answer contains.
- **Binding a gateway identity assertion to a request.** The replay window is the ceiling of this
  deployment. Nothing binds an assertion to what it was sent with, so an intercepted assertion
  replays inside the window. A regression test asserts the replay.
- **The host that receives a caller's token in a delegation exchange.** sutura does not tie the
  `token_endpoint` of a delegation to the inbound issuer. The operator chooses the host, and each
  caller's token goes to it.
- **Provenance in an Arrow schema.** There is no Arrow envelope.
- **Persuasive prompt text.** No mechanism catches text that steers an agent inside what its caller
  may ask.

## Supported versions

Pre-1.0. Only the latest tag is supported, and there are no backports.

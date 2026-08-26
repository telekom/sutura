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

This project is early, and the scope changed when the HTTP surface landed. **There is now a network
perimeter, and it is in scope.** What is built is a governed semantic compiler and executor over
local files, served over HTTP behind a shared bearer token and a rate limiter.

**What that token is, precisely, because it decides what counts as a vulnerability.** It
authenticates the **deployment**, not the caller. There is no request context, no credential broker
and no audit sink, so there is no per-caller identity and no entitlement to get wrong *between*
callers: anyone holding the token can ask anything the catalog certifies, and every question is
answered with whatever access the process already had. A report that one token-holder can read
another's rows is therefore not a finding today - there is no such separation to breach. A report
that the perimeter itself can be crossed is.

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

Once **per-caller** identity lands, the governance boundary becomes the primary surface: anything
that lets a caller read rows they are not entitled to, or that executes a query as an identity other
than the caller's, becomes the highest-severity class of bug this project has. That is a different
control from the deployment token above, and the token is not a step toward it.

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

## Not yet guarantees

These are the design and are **not** enforced, so breaking one is not a vulnerability report - it
is the state of the repository, recorded in the invariant table in `AGENTS.md` and in
[what exists today](https://telekom.github.io/sutura/latest/architecture/#what-exists-today):

- a query executing as anything other than the calling principal, and a downgrade to a service
  identity where a refusal is required. There is no credential broker and no caller identity on the
  query path, so nothing enforces either
- a refusal that is not recorded against the principal chain. There is no audit sink
- a result leaving without provenance in an Arrow schema. There is no Arrow envelope

## Supported versions

Pre-1.0. Only the latest tag is supported, and there are no backports.

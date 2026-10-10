# Security policy

sutura is a server. A release ships binaries, a container image and a Helm chart. The server answers
certified questions for agents over MCP and HTTP. A release build pins every Rust dependency in
`Cargo.lock`, so an advisory against a dependency that a release ships is in scope.

## Scope

In scope: a vulnerability in sutura's own code, or in a dependency that a release ships. For example:

- A caller reads rows or runs a query that the caller may not.
- A question gets past the certified tool surface.
- A value reaches a rendered statement as text.
- A token, a credential or a secret leaks into a log, an error or an answer.
- Untrusted input stops the process. Untrusted input is a token, a catalog document or a data-system
  response.

Generally out of scope:

- Decisions that belong to the operator: which issuer and pool to trust, the grants in each data
  system, which sources a deployment declares, and how the host is hardened.
- Tooling that no release ships: the dev shell, CI-only crates and `xtask`.
- A scanner finding with no path from a shipped binary.

## Supported versions

sutura is pre-1.0. Only the latest release is supported, and there are no backports. A fix ships as
a new release of the binaries, the image and the chart.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository: **Security -> Report a
vulnerability**. That opens a private channel with the maintainers.

Do not open a public issue, pull request or discussion for a suspected vulnerability. A public
report is the one disclosure route we cannot take back.

Include what you have:

- The affected version or commit.
- The impact: what an attacker can do.
- Steps to reproduce.

A partial report is worth sending.

We aim to acknowledge a report within five working days and to agree a disclosure timeline with you.
We fix the problem and credit you, unless you want to stay anonymous. We disclose after a fix is
out, in coordination with you.

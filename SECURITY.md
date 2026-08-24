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

This project is early: the design is settled and the query path is not built. The things worth
reporting today are the ones that affect anyone who builds or runs it:

- a way to get unreviewed content into a published artifact or image
- a credential leak, or a path that logs or serialises one
- a build that can be induced to fetch from a source the maintainers did not choose
- a gate that reports success without checking what it claims to check

Once the query path lands, the governance boundary becomes the primary surface: anything that
lets a caller read rows they are not entitled to, or that executes a query as an identity other
than the caller's, is the highest-severity class of bug this project has.

## What we already treat as a defect

These are stated as guarantees in `AGENTS.md`. If you can break one, that is a security bug,
not a feature request:

- a query executing as anything other than the calling principal
- a downgrade to a service identity where a refusal is required
- SQL, a table name, or a predicate reaching the tool surface
- a credential appearing in a log, an error, or a serialised value
- a result cache keyed by anything other than the subject first

## Supported versions

Pre-1.0. Only the latest tag is supported, and there are no backports.

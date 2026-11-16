---
title: Bound and redact physical text before source operators
description: Column policies transform text before search and reading, with database grants remaining the primary defense.
---

# Bound and redact physical text before source operators

Status: accepted, 2026-11-15.

## Decision

Physical text columns may declare a byte bound and ordered regex redactions. The source SQL wraps
each protected table in a projection that applies redaction, then truncation, before any caller
operator sees the column. Every supported source dialect renders that projection. Failed planning
refuses the statement instead of retrying unmodified SQL. A build without SQL planning refuses
queries over a policy-bearing catalog.

The declaration and pinned schema digests include policies. Conflicting declarations for one
physical column refuse validation. Native dictionary allowlists carry policies so composition
cannot weaken them. Policies require mapped text columns and a catalog without certified metrics;
the certified execution path does not implement these transformations and must not bypass them.

Database permissions remain the first defense. Neither a caller nor an account executing for them
should ever have permission to read secrets they are not authorized to see. Use restricted views
and deny access to underlying values. Redaction is a second layer of defense and never justifies
broader grants. Pattern coverage and engine regex behavior remain operator responsibilities.

## Limits and verification

Replacements are literal, not capture expansions. Native regex syntax can differ between engines.
Bounds conservatively allow four UTF-8 bytes per character plus a visible truncation marker;
Oracle has an additional 4000-byte scalar ceiling. Queries operate on the transformed, potentially
truncated text. No raw LOB reader is widened.

Planner regressions cover secret prefix probes, projections, ordering, minimum, aliases, correlated
uses, nulls and multibyte truncation. SQL snapshots cover every dialect. The existing native source
matrix executes each engine's query path. Cube lacks regex-replacement SQL support, so its scans
run at the source and its policy and query operators run locally. Raw values enter the runtime;
row, memory and deadline limits can refuse these queries sooner. Caller functions are validated
before the policy-only regex function is registered. Served Oracle cells cover CLOB and NCLOB through
both native encryption and verified TLS, with Markdown and native dictionary declarations. These
cells measure their fixtures, not arbitrary regex coverage or deployment grants.

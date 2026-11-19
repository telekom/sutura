---
title: Bound and redact physical text before source operators
description: Column policies transform text before search and reading, with database grants remaining the primary defense.
---

# Bound and redact physical text before source operators

Status: accepted, 2026-11-15.

## Decision

Physical text columns may declare a byte bound and ordered regex redactions. The source SQL wraps
each protected table in a projection that applies redaction before any caller operator sees the
column. Ordinary scalar policies also truncate there. The Oracle LOB amendment below keeps the
redacted LOB whole and bounds only emitted values. Every supported source dialect renders the
projection. Failed planning
refuses the statement instead of retrying unmodified SQL. A build without SQL planning refuses
queries over a policy-bearing catalog.

The declaration and pinned schema digests include policies. Conflicting declarations for one
physical column refuse validation. Native dictionary allowlists carry policies so composition
cannot weaken them. Policies require mapped text columns. Certified metrics may coexist with them
when their declared reads are disjoint: schema pinning checks measure terms, required filters,
time columns, every dimension path's columns and join keys, and cross-fact calendar links. Anchors
use those same metric reads. Refusals name the metric and protected column. The check compares
physical source/table/column identities across model aliases, conservatively treating unqualified
and case-folded spellings as possible aliases. It can therefore refuse spellings an engine would
resolve separately. Authored SQL has no closed column vocabulary, so any policy on its source
refuses validation. Certified execution still applies no text transformation; adding a new read
path requires extending this check.

Database permissions remain the first defense. Neither a caller nor an account executing for them
should ever have permission to read secrets they are not authorized to see. Use restricted views
and deny access to underlying values. Redaction is a second layer of defense and never justifies
broader grants. Pattern coverage and engine regex behavior remain operator responsibilities.

## Limits and verification

Replacements are literal, not capture expansions. Native regex syntax can differ between engines.
Bounds conservatively allow four UTF-8 bytes per character plus a visible truncation marker;
Oracle has an additional scalar ceiling. Outside the Oracle LOB amendment, queries operate on
transformed, potentially truncated text. No raw LOB reader is widened.

Planner regressions cover secret prefix probes, projections, ordering, minimum, aliases, correlated
uses, nulls and multibyte truncation. SQL snapshots cover every dialect. The existing native source
matrix executes each engine's query path. Cube lacks regex-replacement SQL support, so its scans
run at the source and its policy and query operators run locally. Raw values enter the runtime;
row, memory and deadline limits can refuse these queries sooner. Caller functions are validated
before the policy-only regex function is registered. Served Oracle cells cover CLOB and NCLOB through
both native encryption and verified TLS, with Markdown and native dictionary declarations. These
cells measure their fixtures, not arbitrary regex coverage or deployment grants.

## Oracle LOB amendment

Oracle CLOB and NCLOB policies keep the full redacted value inside source SQL. `LIKE`, `instr`
(rendered as Oracle `INSTR`) and `substr` windows therefore search and read beyond the first output
window. Every returned text expression is bounded after source operators, under the smallest bound
of the part's LOB policies. This can also shorten an unrelated text output from that part. Pinned
schema metadata distinguishes CLOB from NCLOB and participates in the schema digest. A LOB-bearing
part of a federated or otherwise local plan refuses before credentials open: a clipped intermediate
value must never decide a later local operator.

The output uses Oracle's existing regex and LOB substring primitives. A candidate window containing
only explicitly enumerated printable ASCII, tab, carriage return and newline uses one UTF-8 byte
per character; other windows retain the conservative four-byte reservation. This is a narrow ASCII
fast path, not byte-exact packing of arbitrary Unicode. CLOB output is capped at 4000 bytes. NCLOB
output, and derived text whose metadata cannot establish CLOB, is capped at 2000 bytes when the part
reads NCLOB, to respect the separate national-character scalar ceiling as well. Both caps include
the visible truncation marker. A configured smaller policy bound still wins. The native cells check
both LOB kinds, a late non-ASCII character, late search terms, masked late secrets and explicit read
windows. Their fixtures do not establish every source charset or regex collation.

Oracle still decides which operators its LOB types support. Full-LOB equality, ordering and minimum
are refused by the native cells; reading uses an explicit `substr` window. An unsupported full-LOB
operation refuses at the source and is never retried on clipped text. Without an explicit per-column acknowledgement, no unredacted pre-filter is introduced.
Redacting a full large table can exceed the request deadline; use a selective predicate on an
unprotected owner, type or key column before a broad text search. No large-table latency claim is
made by the bounded fixture cells.

A query with a value-dependent mapped Oracle NUMBER still uses the caller's original admitted
AST. When that query scans a protected LOB, a CTE column list labels its outputs before the same
final scalar bounds; numeric expressions are never regenerated from the engine's Utf8 plan.

## Acknowledged necessary-condition search amendment

An optional `raw_prefilter` declaration records a nonempty bounded operator reason accepting
secret-dependent search timing. It does not permit a raw result decision. The compiler may add
a raw necessary condition only for a positive conjunctive literal contains search in one table
and scope, beside only conjuncts that cannot raise on any row (a plain column against literals
of its own type, a string only against a text-policy column), with proved comparison semantics and replacement barriers. Unknown shapes preserve
the original redacted-only path. Each nonempty replacement must contain no whole term and block
both boundary overlaps; the proof composes over ordered replacements and the truncation marker
where it is searched. No inference about arbitrary regex languages is required.

DataFusion binary comparison and guarded Oracle CLOB/NCLOB comparison are the initial eligible
venues. Oracle `UPPER` uses its binary character-set mapping; its runtime binary sort check
retains every row on any other or NULL value. No session setting is changed, and unknown
data-bound collations on other source columns remain ineligible. Cube's local policy work is not
an eligible venue and keeps the redacted-only path. The exact transformed predicate still decides
the answer. A hidden raw term may incur extra redaction work and reveal information through
timing; the acknowledgement records acceptance rather than preventing that disclosure.

[The operator guide](../raw-text-search.md) states the configuration and limits. A native timed
comparison over at least one million actual rows, exact answers and hidden-secret probes is
required before claiming measured acceleration. Setup and serving timings are distinct.

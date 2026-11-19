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
window. Protected text outputs and text expressions derived from them are bounded after source
operators, under the smallest bound of the part's LOB policies. Unrelated text and scalar results
pass through. Typed output lineage is preserved before optimization, through projections,
aggregates, windows and union arms. CASE conditions, aggregate filters and ordering, and window
partitioning and ordering do not carry value lineage; other transformations conservatively carry
their inputs. Pinned schema metadata distinguishes CLOB from NCLOB and
participates in the schema digest. A LOB-bearing
part of a federated or otherwise local plan refuses before credentials open: a clipped intermediate
value must never decide a later local operator.

The output uses Oracle's existing regex and LOB substring primitives. A candidate window containing
only explicitly enumerated printable ASCII, tab, carriage return and newline uses one UTF-8 byte
per character; other windows retain the conservative four-byte reservation. This is a narrow ASCII
fast path, not byte-exact packing of arbitrary Unicode. CLOB output is capped at 4000 bytes. NCLOB
output and text derived from NCLOB are capped at 2000 bytes to respect the separate
national-character scalar ceiling as well. An expression or union output with both LOB kinds uses
the NCLOB ceiling; a CLOB-only expression keeps its own ceiling even beside an NCLOB output. Both caps include
the visible truncation marker. A configured smaller policy bound still wins. The native cells check
both LOB kinds, a late non-ASCII character, late search terms, masked late secrets and explicit read
windows. Their fixtures do not establish every source charset or regex collation.

Oracle still decides which operators its LOB types support. Full-LOB equality, ordering and minimum
are refused by the native cells; reading uses an explicit `substr` window. An unsupported full-LOB
operation refuses at the source and is never retried on clipped text. No unredacted pre-filter is introduced.
Redacting a full large table can exceed the request deadline; use a selective predicate on an
unprotected owner, type or key column before a broad text search. No large-table latency claim is
made by the bounded fixture cells.

A query with a value-dependent mapped Oracle NUMBER still uses the caller's original admitted
AST. When that query scans a protected LOB, a CTE column list labels its outputs before the same
final scalar bounds; numeric expressions are never regenerated from the engine's Utf8 plan.

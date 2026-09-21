---
title: Arrow and DataFusion override the hand-written combiner
description: Owner instruction overriding ADR 0007's second amendment and the unmerged ADR 0037. Records the five requirements the federation path is now held to - Arrow native, DataFusion native, DataFusion so federation arrives later rather than being redeveloped, always impersonation-capable, ADBC - and decides the first step of it: datafusion's compression feature is on deliberately, deny.toml carries the one licence it adds, and a compressed CSV or NDJSON source is a stated capability with the extension parsed rather than inferred. States what compressed Parquet does and does not owe to that feature, why the datafusion-federation dependency cannot land ahead of its caller, and the three measurements that bound the remaining steps - the arrow 58/59 split, the ADBC driver manager's own major, and the size of the port change.
---

# Arrow and DataFusion override the hand-written combiner

Status: **accepted by owner instruction**, overriding two records. This one is partly built and says
which parts, per step, beside each measurement.

What it overrides:

- [Federating across different data systems](0007-federating-across-different-data-systems.md)'s
  *second amendment* - *the combiner is NOT DataFusion* - and its *Where the `RowSet`-to-Arrow
  boundary lives* decision, which put the port's currency at `RowSet` for the first federated
  milestone. Both are corrected in place in that record rather than struck.
- ADR 0037, *`DataFusion` is the combiner and the federation boundary is Arrow*, which refused Arrow
  as domain vocabulary on a cost measurement. **That record is not in this branch's tree** - it lives
  unmerged on `feat/datafusion-combiner-0007` together with the `ComputeContext` it justifies - so
  it is overridden here by name and amended where it lands, because amending a file that is not
  present would be fiction.

## The five requirements

The instruction is one direction with five things that must all end up true, and they are written
here because no single step below satisfies more than two of them:

|   | Requirement                                                                             | Where it stands                                                                 |
| - | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| a | **Arrow native**                                                                        | Step 2. The engine already is internally; the port is not.                      |
| b | **DataFusion native**                                                                   | Step 2 makes the combiner reachable; step 3 is the combiner.                    |
| c | **DataFusion so federation arrives later** rather than being redeveloped for multi-node | Groundwork only. `datafusion-federation` is the mechanism and step 4 adopts it. |
| d | **Always impersonation-capable**                                                        | Unchanged by this record and constrained by it: see *What this does not buy*.   |
| e | **ADBC**                                                                                | Built. `sutura-exec-bigquery`'s `adbc` transport is the only BigQuery mode.     |

**No hand row handling is the standing rule this comes from**: `DataFusion`, Arrow, Arrow Flight or
ADBC, and nothing that walks a result one cell at a time. That rule is what makes the blast radius
of step 2 worth paying rather than a refactor for its own sake.

## Step 1, decided and built: compression is a capability, not unification

`datafusion`'s `compression` feature is **on**, and `deny.toml` allows `bzip2-1.0.6`.

**Why this was not already true**, because the reason was a real control rather than an oversight:
the feature pulls `bzip2` -> `libbz2-rs-sys`, whose registry licence field is `bzip2-1.0.6`, and
`deny.toml` sets `unused-allowed-license = "deny"`. So the licence could not be allowed ahead of a
feature that needed it, and the feature could not be turned on while the licence was refused.
`Cargo.toml`'s own comment recorded that pair and ADR 0006 measured it while declining a federation
crate. Breaking the cycle is a decision about what this runtime reads, and the decision is that a
compressed CSV or NDJSON source is a capability worth one permissive licence.

**What the feature buys, precisely, because the two halves are bought by different things:**

- **Compressed Parquet owes it nothing.** Parquet records its codec per column chunk inside the file
  and the `parquet` feature's own codecs read it, so a Snappy, GZIP or ZSTD Parquet file has always
  read. `compression` adds no Parquet capability at all, and the attach path refuses an OUTER codec
  around a Parquet file by name - `orders.parquet.gz` is a wrapper around something already
  compressed, and reading it would mean unwrapping a file nobody meant to write.
- **Compressed CSV and NDJSON is the whole of what it buys.** Those two are plain text, so an outer
  codec is the only compression they have.

**The codec is parsed from the path, not inferred.** `crates/sutura-exec-datafusion/src/attach.rs`'s
`Codec::of_path` answers a suffix this build reads, or refuses a suffix that certainly names a codec
and is not one - `.gzip`, `.lz4`, `.zip`. The failure it replaces is silent rather than loud: handing
a compressed file to the engine as text does not error, it infers a schema from the codec's header
bytes and resolves the table to columns nobody declared. An extension that names no codec at all
(`orders.txt`) stays text, because a CSV may legitimately be called that.

**One list, read two ways.** `sutura_exec_datafusion::candidates` enumerates every file name a
model's table may arrive under - Parquet, then CSV and NDJSON each plain and once per codec - and
`attach_file` dispatches on the same table. Both of `sutura-cli`'s file-source searches read that
function instead of spelling their own preference order, which is what stops a deployment offering a
candidate the engine then refuses, or missing one it reads.

**What is proven and what is not.** `gz` and `bz2` are written by a dev-dependency writer and read
back end to end, `bz2` specifically because it is the licence entry: an allowed licence whose codec
nothing reads is an entry justified by nothing. `xz` and `zst` are **parse-only** - the suffix maps
to the right codec and no cell decodes one - and no gate would notice if this build could not in fact
decode them. Adding a writer for either is the fix if that matters.

## Step 2, decided and not built here: Arrow at the `Warehouse` port

**The decision:** `Warehouse::execute`'s currency becomes Arrow record batches, and the domain names
the Arrow array types. That reverses 0007's *the port's currency stays `RowSet`* and 0037's refusal
of Arrow as domain vocabulary, and it is what lets `sutura-exec-bigquery` stop turning Arrow arrays
into text cells and text cells into domain values on the way out of a driver that already speaks
Arrow.

**Three measurements that bound it, taken on this branch on 2026-09-21.** They are here because two
of them correct how this was scoped, and the third is the one that makes step 2 possible at all:

- **The ADBC driver manager is on the engine's Arrow major.** `adbc_core 0.24.0` declares
  `arrow-array 59.2.0` and `arrow-schema 59.2.0` in `Cargo.lock`, which is the major
  `sutura-exec-datafusion` resolves through `datafusion`. So a batch the BigQuery driver produces can
  reach an Arrow-typed port with no conversion and no C data interface - which `unsafe_code =
  "forbid"` puts out of reach anyway.
- **The 58/59 split is still open, and it bounds which adapter can be Arrow-native rather than
  whether the port can be.** `duckdb 1.10505.0` - the pinned release, checked against
  `index.crates.io` - still declares `arrow ^58`, and `devco/arrow-majors-allow` tolerates the split
  as a DUPLICATE on the stated test *whether any first-party crate names the type*. An Arrow-typed
  port keeps that answer NO for `sutura-exec-duckdb`, which converts through the domain's own row
  vocabulary and names no Arrow type - so the port change does not convert the duplicate into a type
  boundary. What it does forbid until `duckdb-rs` releases its merged arrow-59 bump is a **DuckDB
  adapter that hands its native batches through**, which is the one adapter that must keep building
  rows by hand.
- **The port change is not a signature tweak, which is why it is not in this change.** On this tree:
  52 `fn execute` implementations, 59 `.execute(` call sites, 79 `RowSet::new` constructions and 94
  `.rows()` reads. Most are fakes, and the shape that makes it mechanical rather than a rewrite is
  the domain-side pair above - build batches from rows, read rows from batches - so an adapter or a
  fake that has rows changes one line.
- **`ALLOWED_IN_DOMAIN` is the cost that is not mechanical, and it is paid.** The domain naming
  `arrow-array` and `arrow-schema` took that allowlist's walk from 33 crates to 97. Twenty-two are
  in the FEATURE-RESOLVED tree (`cargo tree -p sutura-domain --all-features`), and two of those
  twenty-two are worth naming rather than counting: a time-zone database (`chrono`,
  `iana-time-zone`) and an entropy source (`getrandom`, through `ahash` through `hashbrown`) are now
  in the hexagon's interior, reachable from no code this crate has. The remaining forty-two are the
  over-broad kind that allowlist already carries - optional and platform-specific edges
  `cargo metadata` resolves for every target, including a `wasm-bindgen` pair and the `windows-*`
  family. **No new lockfile entry**: both crates were already resolved at 59.2.0. Arrow is a data
  format rather than a runtime, a client or an engine, which is the line
  `xtask/src/boundaries/edges.rs` actually draws - and this record is what authorises the entry.

## Step 3, decided and not built here: the combiner is a DataFusion plan

`sutura_domain::plan::FederatedPlan::combine` is replaced by a DataFusion plan over the two legs'
batches, built in an **adapter** crate. `-domain` keeps the plan type and the port and names no
engine: `ALLOWED_IN_DOMAIN`'s stated line is *no runtime, no client, no engine*, and DataFusion is an
engine. Step 2's Arrow port is what makes that seam possible without an adapter calling an adapter -
`sutura-app` calls the combiner, above every adapter, exactly as it calls `combine` today.

**The byte budget must survive, and this is the mechanism.** 0007 decided the working-set bound is
applied *as rows are converted*, because 0009's Decision 3 put the engine's memory pool on what its
own operators reserve and a finished `RowSet` is invisible to it. Under a DataFusion combiner the
join build side, the aggregate state and the sort - which is where the hand-written combine's
`ByteBudget` actually spends - become operator reservations, so the bound is
`sutura-exec-datafusion`'s existing `GreedyMemoryPool` set to the leg budget, never spilling.
**Stated as a limit rather than a claim:** that pool does not count the final materialisation of the
answer, so the budget moves from covering every byte to covering the operators plus whatever the
conversion boundary still counts. A combiner that changed the bound's reach without saying so would
be the defect; if the residual gap is not acceptable the answer is a counted conversion beside the
pool, not a wider claim.

## Step 4, not built and blocked ahead of its caller: `datafusion-federation`

The instruction is to depend on the published crate directly - **no vendor**, superseding the
vendoring approach taken in a concurrent pull request. That is the mechanism requirement (c) is
about: federation pushdown, and later a multi-node move, arrive by adopting it rather than by
redeveloping it.

**It cannot land in this change, and the reason is a gate rather than a preference.**
`cargo xtask unused-deps` is a `Kind::Hygiene` task, so it runs in `just validate`'s hygiene leg, and
it fails a declared dependency no crate references. `datafusion-federation`'s caller is a
`FederationProvider`/`SQLExecutor` implementation with a per-subject `compute_context`, which is step
5's work; declaring the dependency ahead of it is a red leg, not groundwork.

**What is already measured about adopting it, so the next step does not re-derive it.**
`datafusion-federation 0.5.6`'s manifest declares `[dependencies.datafusion] version = "55"` with no
`default-features = false`, and Cargo unifies features additively, so adopting it turns datafusion's
`sql` feature back on no matter which of its APIs is called - a second SQL parser and an unparser in
the closure, beside `polyglot-sql`. `Cargo.toml`'s comment calls `sql` being off the strongest
sentence in that file, and this is what would end it. **So the ecosystem move comes first:** a
one-line upstream change setting `default-features = false` on that dependency, proposed rather than
patched here, which is AGENTS.md's own preference order and what step 3 of
`.agents/skills/sutura/dependencies` asks for before a `[patch]` and long before a vendor.

Its `compression` half is no longer part of that argument: this record turns the feature on
deliberately, so unification can no longer bring it in as a surprise.

## Step 5, not built: a per-subject compute context, wired

`feat/datafusion-combiner-0007` holds a reviewed `ComputeContext` that nothing calls. It stays there
rather than being taken into this change, because the only caller it can have is step 4's provider -
and landing it now would be the orphaned dead code it was held back to avoid.

**The property it holds, recorded here so step 4 does not have to rediscover it.**
`datafusion-federation`'s provider equality is `name() == name() && compute_context() ==
compute_context()`, so two providers for one source on behalf of two different subjects that compare
equal are fused by the optimizer into one federated node executed through one of them - one caller's
scan on the other caller's credential, with no bug on either caller's own path. The context must
therefore carry an **opaque per-subject digest**: unequal per subject so the fusion cannot happen,
and disclosing nothing, because the value is interpolated into plan text and a raw subject there is a
person's identifier in `EXPLAIN` output.

## What this does not buy

**Requirement (d) is unchanged by every step above, and an Arrow port does not advance it.**
`sutura-exec-datafusion`'s `IMPERSONATION` is `NoPlaceForASubject` - one process, one
operating-system identity - so a leg it executes, including a combine, runs as the deployment and not
as the asker. What "always impersonation-capable" constrains is the shape: the combiner may not
become a place where two subjects' rows meet under one credential, which is why step 5's context is a
per-subject digest rather than a convenience. `docs/where-identity-is-proven.md` remains the record of
which venue may be cited for which identity claim, and no step here changes a row in it.

**Compressed sources are not a performance decision.** Nothing here measures read throughput for a
compressed file against a plain one, and the codecs differ by more than a constant. What is decided
is that a deployment may point a source at one.

## Consequences

- `deny.toml` carries one more licence, and `unused-allowed-license = "deny"` keeps it honest: the
  day nothing pulls `libbz2-rs-sys`, that entry fails the gate rather than lingering.
- Two dev-dependency compression writers exist so a test can produce what the engine reads. Neither
  is in a shipped artefact and nothing this workspace ships compresses anything.
- `sutura-cli`'s two file-source searches no longer spell their own format preference, so a format
  added to the engine reaches both without a change there - and a format added to `candidates`
  without an `attach_file` arm is a refusal at attach rather than a mis-read file.
- ADR 0006's federation probe paragraph said the bzip2 licence was one *the allowlist does not
  carry*, with `cargo deny` not run over the probe. That is corrected in place: the licence is
  carried now, deliberately, and `cargo deny check licenses` was run.

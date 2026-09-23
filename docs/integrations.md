---
title: Integrations
---

# Integrations

What this runtime reads definitions from, what it executes against, and how far each one is proven.

Every extent claim below routes to the page or the declaration that holds it. Nothing is restated
here, because a second copy of a capability claim is the copy that rots into an overstatement.

## Metadata sources

A metadata source supplies definitions. `SemanticCatalog::load` takes no request context, so no
catalog can return a different definition per caller - the digest that travels with an answer would
otherwise describe something other than what produced it.

`SemanticCatalog::KIND` is what each adapter is measured against, and it is a per-adapter constant
rather than a description:

| Source                        | Crate                         | `KIND`      | Measured against                               | What it can carry                                                        |
| ----------------------------- | ----------------------------- | ----------- | ---------------------------------------------- | ------------------------------------------------------------------------ |
| Markdown + YAML frontmatter   | `sutura-catalog-local`        | `Golden`    | the hand-written oracle stating the same model | every kind the model defines                                             |
| DataHub                       | `sutura-catalog-datahub`      | `Declaring` | its own `capabilities` declaration             | `docs/adr/0016-what-datahub-can-carry.md`                                |
| OpenMetadata                  | `sutura-catalog-openmetadata` | `Declaring` | its own `capabilities` declaration             | [What an OpenMetadata catalog can carry](what-openmetadata-can-carry.md) |
| OKF Frictionless Table Schema | `sutura-catalog-okf`          | `Declaring` | its own `capabilities` declaration             | [What an OKF-style catalog can carry](what-okf-can-carry.md)             |
| An RDBMS dictionary           | `sutura-catalog-rdbms`        | `Declaring` | its own `capabilities` declaration             | [An RDBMS dictionary as a metadata source](rdbms-catalog-guidance.md)    |

A `Declaring` adapter supplies part of the model and **declares the rest out**. An absence is
declared rather than inferred from silence, which is why a partial source cannot quietly read as a
complete one.

## Data sources

A data source executes a compiled plan. The `Warehouse` port is synchronous and `execute` takes a
`Deadline`.

| Source     | Crate                    | Renders | Executes                                       | Identity it executes under                   |
| ---------- | ------------------------ | ------- | ---------------------------------------------- | -------------------------------------------- |
| BigQuery   | `sutura-exec-bigquery`   | yes     | yes                                            | per subject - `PerSubjectCredential`         |
| Postgres   | `sutura-exec-postgres`   | yes     | yes                                            | one shared credential - `NoPlaceForASubject` |
| DuckDB     | `sutura-exec-duckdb`     | yes     | yes                                            | one process identity - `NoPlaceForASubject`  |
| DataFusion | `sutura-exec-datafusion` | -       | yes, one source's share of a federated answer  | one process identity - `NoPlaceForASubject`  |
| ClickHouse | `sutura-exec-clickhouse` | yes     | yes, behind a default-off `clickhouse` feature | one shared credential - `NoPlaceForASubject` |
| Oracle     | `sutura-exec-oracle`     | yes     | **no**                                         | -                                            |

Oracle renders and cannot be asked to answer: that dialect shipped without an executor, so its
committed goldens pin what this renderer emits and nothing a database agreed to.

**ClickHouse is the newest row and the one whose columns need reading together.** It executes: a
`kind: clickhouse` source is declarable and openable by a build carrying the `clickhouse` feature,
which is default-off and in no published binary. The golden and differential suites run the example
corpus against a real ClickHouse - the server `nix/clickhouse-tier.nix` starts beside the Postgres
tier - and pin its rows, refusals, error and anchor report. Two things the `Executes` column does
NOT say about it: the conformance packs are not bound, because that corpus measured two wrong
answers a fix has to land for first (an `Int64` sum that wraps, a decimal that loses its trailing
zero - `check-conformance-bindings` carries the declaration), and `Warehouse::EXECUTES_LEGS` is
absent on the adapter - so a federated question involving a ClickHouse source is still refused by
the capability gate. Its identity column is the static half and stays there until an adapter change:
`NoPlaceForASubject`, so an `impersonation-at-source` declaration on this kind is refused at the
composition root with the reason that no build delivers it.

## Identity: what "impersonation" does and does not mean here

`Warehouse::IMPERSONATION` says whether there is **a place in an adapter's path** for a subject's own
credential to arrive. It does not say that a subject's identity has been proven to reach that source.
Those are two claims and only the first is a constant.

`IMPERSONATION` is a constant the trait declares **with no default**, so an adapter cannot be
silent about it: a new data source either answers, or does not compile. Saying so explicitly is the
point of the declaration - a file engine is the easiest source in the world to assume nothing about,
and *"nobody declared anything for the engine"* is how a deployment ends up believing its whole
surface impersonates because its network source does.

⚠ The trait forces each adapter to answer; **nothing forces the table above to list every adapter.**
The per-adapter constant in the source is the authority, and this page is a reading of it.

The two legs, stated separately because they are proven to different depths:

- **Leg 1 - knowing who is asking.** Built.
- **Leg 2 - a source executing *as* them.** Built for BigQuery through a declared per-source map,
  and **unproven**: the hosted venue that would show two subjects resolving to two accounts is
  `wired` and nobody has dispatched it. The run this bullet used to cite was of an HTTP exchange the
  ADBC adoption deleted. **No served binary has executed as a caller yet.**

[Where each identity claim is proven](where-identity-is-proven.md) is the venue-by-venue table, and
it is the only place that decides which venue may be cited for which claim.

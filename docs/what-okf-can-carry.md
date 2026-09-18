---
title: What an OKF-style markdown/yaml catalog can carry, and the measure it cannot
description: A research spike in the method of ADR 0016, measured against the SemanticCatalog port - the OKF Frictionless Table Schema (version 1) carries a table's column set and its structural foreign keys and nothing of a metric layer, so an adapter over it is a declaring source that provides the physical model and descriptions and declares the join, the measure, the grain and the anchor out.
---

# What an OKF-style markdown/yaml catalog can carry

Status: **a finding, and the deliverable** of issue #153's first task. The roadmap lists *"OKF style
markdown/yaml"* beside the Wren-style vocabulary this repository already reads through
`sutura-catalog-local`. This record is the ADR-0016-shaped answer to the question that decides the
adapter's whole shape: **can the OKF vocabulary express a complete semantic model as this repository
defines one, or only part of one?** The answer is *only part of one* - it is strictly narrower than
Wren - and the consequence is the same as ADR 0016's: an adapter that **declares** which of the
domain's kinds it provides and which it does not, and is tested against that declaration, rather than
an adapter held to the golden contract.

## What was measured against

Read out of the port rather than out of the prose, exactly as ADR 0016 does. `SemanticCatalog::load`
returns a `PinnedDefinitions`; its halves are a `Definitions` and a `Knowledge`, and
`Definitions::assemble` is the only constructor of the first. So *can an OKF source express our model*
is really *can it fill these fields and survive these checks*:

| What a complete semantic model supplies           | The type in `sutura-domain`                                                                                                                                |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| a physical table and the column set it exposes    | `Model` - `ModelName`, `SourceName`, `TableName`, `BTreeSet<ColumnName>`, `Description`                                                                    |
| a join                                            | `Relationship` - a name, two `(ModelName, ColumnName)` endpoints, and a `JoinType` of `OneToOne`, `ManyToOne` or `OneToMany`                               |
| what a metric measures                            | `Measure` - `Simple(Term)` or `Ratio { numerator, denominator, zero_denominator }`, over a `Term` of `Aggregate(AggregatedColumn)` or `CountIf { column }` |
| a predicate that is part of what the metric means | `Vec<RequiredFilter>` - `Equals`, `NotEquals`, `IsTrue`, `IsNotNull`                                                                                       |
| when, and at what resolution                      | `ColumnName` plus a `BTreeSet<Grain>`                                                                                                                      |
| what it may be broken down by                     | `Dimension` - a column, optionally `via` one relationship, optionally an allowlist of at most `MAX_VALUES_PER_DIMENSION` values                            |
| the number it produced when it was certified      | `Option<Anchor>` - a `TimeRange` and a value as text                                                                                                       |
| what a reader has to know                         | `Knowledge` - phrases, caveats, reviewed absences and worked examples, each with a `Referent`                                                              |

`Definitions::assemble` refuses a dimension reached through a relationship whose declared cardinality
may duplicate rows, as `JoinWouldDuplicateRows`; and every field above is a type with no free-text SQL
anywhere in it - `Measure` has no `expression:` key and no `Option<String>` at any depth.

## The vocabulary this names, and its version

The roadmap's *OKF style* is the **Open Knowledge Foundation's Frictionless Data** vocabulary. The
schema against which an on-disk catalog is written is the **Frictionless Table Schema specification,
version 1**, updated **2021-10-05** (authors Paul Walsh and Rufus Pollock), served at
`specs.frictionlessdata.io/table-schema/`, with its JSON Schema at `specs.frictionlessdata.io/schemas/table-schema.json`.
The descriptor is a JSON object (expressible as the `fields:` block of YAML frontmatter) that **must**
contain a `fields` array and may contain `missingValues`, `primaryKey` and `foreignKeys`. Each field
descriptor must contain `name` and may contain `title`, `description`, `example`, `type`, `format`,
`constraints`, and `rdfType`. This is the exact published vocabulary and version read for this record.

## What the vocabulary actually carries, field by field

| Table Schema property             | Level  | What it is                                                                                                                  | What it also is not                                                                                                                                                             |
| --------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `fields`                          | schema | an ordered array of field descriptors                                                                                       | one per physical column; nothing identifies a *fact* or a *metric*                                                                                                              |
| `name` (required)                 | field  | the column name                                                                                                             | the repository's `ColumnName` is a validated newtype; `name` is a bare string with no parse contract                                                                            |
| `title`, `description`, `example` | field  | human-readable prose and a sample value                                                                                     | free text - no referent-bearing `Knowledge` entity, no `NoteBody` cap, no `Referent`                                                                                            |
| `type`                            | field  | *the logical data type*: `string`, `number`, `integer`, `boolean`, `object`, `array`, `date`, `time`, `datetime`, `year`, … | **a data type, not a measure.** There is no `Aggregate`, no `Ratio`, no `CountIf`, no metric vocabulary anywhere in the spec                                                    |
| `format`                          | field  | a string refining `type` (e.g. `email`, `uri`, `uuid`, a strptime `PATTERN`)                                                | a physical representation hint, not a semantic predicate                                                                                                                        |
| `constraints`                     | field  | value-validation: `required`, `unique`, `minLength`, `maxLength`, `minimum`, `maximum`, `pattern`, `enum`                   | **not** a `RequiredFilter`. These constrain the data file's *values*, evaluated on the logical representation; they are not predicates "that are part of what the metric MEANS" |
| `primaryKey`                      | schema | a field or field list that uniquely identifies each row                                                                     | equivalent to `required: true` on those fields; it is a uniqueness fact, not a join                                                                                             |
| `foreignKeys`                     | schema | `fields` → `reference.resource` / `reference.fields`                                                                        | **declares no cardinality.** There is no OneToMany / ManyToOne / OneToOne marker anywhere in the descriptor                                                                     |

Two absences carry all the weight, and they are the ones ADR 0016 taught this repository to look for
first.

**Measure is absent entirely - there is nothing to refuse, and that is the stricter statement.** The
Table Schema type system is the json-schema type set, which is about the shape of cell values, not
what a metric measures. There is no `expression` key, no dialect-tagged raw string, no independently
authored aggregation function, and no metric or semantic-model entity type at all. ADR 0016's refusal

- *a measure carried as a raw expression string in a foreign dialect is half a definition* - does not
  fire here because there is no slot for a measure to be carried badly in. The correct `capabilities()`
  statement is therefore a **declared absence** of the metric layer, not a refusal of a bad shape. The
  repo's rule that a default is indistinguishable from a decision is not engaged because no default is
  offered.

**A `foreignKey` licenses no dimension, because its cardinality is defaulted, not declared.** The
descriptor names which local fields reference which remote fields and resource, and stops there. It
cannot distinguish a one-to-one from a one-to-many, so a relationship loaded through it would offer no
answer to the `JoinWouldDuplicateRows` check - a relationship whose cardinality may duplicate rows is
exactly the one the assembler refuses to reach a dimension through. ADR 0016 refused DataHub's `N_N`
default for this reason; Table Schema does not even default - it omits the property, which is a
default of "not stated", and therefore licenses nothing.

## The mapping

| The semantic-model field           | What Table Schema offers                                              | Verdict                                                                                              |
| ---------------------------------- | --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `Model` (table + columns)          | one *resource* per table, `fields[].name/type/description` per column | **provides**                                                                                         |
| `Description`                      | `title` / `description` free text                                     | **provides**                                                                                         |
| `Relationship` (join)              | `foreignKeys` - structural endpoints, no cardinality                  | **declares, and does not license** - loadable only where the join is not reached through a dimension |
| `Measure`                          | none                                                                  | **absent, declared**                                                                                 |
| `RequiredFilter`                   | `constraints` - value validation, not meaning                         | **absent, declared**                                                                                 |
| `Grain` + time                     | no time-resolution vocabulary                                         | **absent, declared**                                                                                 |
| `Dimension` with `via` / allowlist | a plain column name; no `via`, no allowlist                           | **partial** - a bare column, no reached-through-relationship dimension                               |
| `Anchor`                           | none                                                                  | **absent, declared**                                                                                 |
| `Knowledge` with `Referent`        | free-text `description` only                                          | **partial** - descriptions travel; referent-bearing phrases/caveats/worked examples do not           |

## Conclusion, and what it costs

**A strictly narrower source than Wren, and therefore the cheapest connector - by declaration.** It can
carry the physical model and the prose, and structural foreign keys that license no dimension; it
cannot carry a metric, a required filter, a grain, an anchor, or a reachable-through-relationship
dimension. The issue anticipated exactly this: *"a vocabulary that turns out to be strictly narrower
than Wren's is a fine outcome - it is one `declaring` registry entry, three universal cells, and its
declaration in front of a reviewer."* The adapter over it is a **declaring** `SemanticCatalog` whose
`capabilities()` names the physical model, the descriptions and (unlicensed) join columns as provided
and everything else as a deliberate absence - the shape `sutura-catalog-datahub` and
`sutura-catalog-local` already establish - and it is tested against that declaration in both
directions (it provides what it declares, and the bundle agrees that what it does not declare does not
arrive).

## Alternatives considered

**Was this really OKF, and not Wren?** The two are different published vocabularies - this repository
already reads the Wren-style element set through `sutura-catalog-local`'s `kind`-bearing documents
with a `definition` digest over a canonical form, and the roadmap names *OKF style* as the second,
separate vocabulary "beside" it. Table Schema is the OKF's own schema language and is unambiguous to
pin. A caution is that the roadmap's word *style* leaves room for a vocabulary that is OKF-inspired
rather than the literal Table Schema; the finding pins the literal Table Schema v1 because it is the
one published, versioned thing a reader can re-check, and a record that names no version is one nobody
can re-check.

**Is an adapter that provides only the physical model worth shipping at all?** Yes, for the reason
the describing half of a `declaring` catalog exists: a source that provides tables and descriptions but
no metric is a legitimate narrow source, and `Definitions::assemble` refuses nothing it does not
provide. Its value is the same as any catalog a deployment already vends - it turns a directory of
OKF-described tables into something `load` can pin and validate - even though the metric layer has to
be authored elsewhere.

## Limits, stated

Written read-only, against the published Table Schema specification (version 1, updated 2021-10-05)
and the `SemanticCatalog`/`Definitions` port in this repository, not against any provisioned instance.
Two claims are open to a live check and should be before an adapter ships: (a) whether any real catalogs
in the wild populate `foreignKeys` in a way this repository would choose to load as a declared, unlicensed
join rather than refuse, and (b) whether the free-text `description` is dense enough to justify loading it
as `Knowledge` descriptions rather than leaving it to the consuming prompt. Neither changes the shape
decision this record makes.

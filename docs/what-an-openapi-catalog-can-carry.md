---
title: What an OpenAPI-described metadata API can carry
description: EXPERIMENTAL. The generic OpenAPI catalog mode reads models, columns, keys, descriptions and many-to-one joins out of an HTTP metadata API through a pinned OpenAPI document and a data-only mapping of JSON Pointers. It carries no metric layer and no knowledge, so the adapter is a declaring source.
---

# What an OpenAPI-described metadata API can carry

Status: **experimental**, issue #1727. `sutura-catalog-openapi` is proven against recorded fixtures
only: a synthetic registry under `crates/sutura-catalog-openapi/src/fixture/example/`, and a vendor
sample whose document is a hand transcription of public reference pages. No live API runs in any
check, so a vendor's real answer shape is not proven by anything here.

## The model

An API is configured, not coded. The operator pins two files:

- the API's **OpenAPI 3.0 or 3.1 document**, as JSON, on disk. It is never fetched.
- a **mapping**: a closed YAML vocabulary that names the `GET` operation that lists the models, the
  optional `GET` operation that lists one model's outgoing links, the fixed path parameters, the
  cursor paging, and an RFC 6901 JSON Pointer for every field read out of an answer.

`Contract::parse` checks the mapping against the document once, before any request. The document is
that check's input and nothing else.

## What it carries

| Sutura field           | Mapping word                              | Notes                                                                              |
| ---------------------- | ----------------------------------------- | ---------------------------------------------------------------------------------- |
| `Model` name and table | `models.name`                             | Both are the listed name. A name outside the identifier grammar is refused by name |
| `Description`          | `models.description` (1 to 4 pointers)    | The first non-empty text. None at all is refused                                   |
| primary key            | `models.primary_key`                      | A string or a list of strings                                                      |
| `Column`               | `models.columns.at` (+ `name` for a list) | An object keyed by column name, or a list whose entries carry the name             |
| column type            | `models.columns.type`                     | The source's own type word, as text                                                |
| column description     | `models.columns.description`              | The first non-empty text                                                           |
| `Relationship`         | `relationships.*`                         | Many-to-one, from `foreign_key` onto the target's single-column primary key        |

`capabilities()` is `Structure` and `Descriptions`, and may provide `ColumnTypes`,
`ColumnDescriptions` and `Relationships`. Knowledge: none.

## What it does not carry

| Not carried                                              | Why                                                                     |
| -------------------------------------------------------- | ----------------------------------------------------------------------- |
| metrics, required filters, grains, allowlists, anchors   | the vocabulary has no word for them                                     |
| a link with no foreign key on this side (a to-many side) | the reverse is the other side's many-to-one; it is skipped, not refused |
| a join onto a composite or missing key                   | refused: one column cannot name the far side of a composite key         |
| knowledge (glossary, notes, aliases)                     | the vocabulary has no word for them                                     |

## Load refusals

`ContractError`, before any request: a document over 16 MiB or a mapping over 64 KiB; a document that
is not OpenAPI 3.0/3.1 (Swagger 2.0 is refused by its own version word); a `$ref` that is not local; an
operation the document does not have, has twice, or has as anything but a `GET`; an operation that
needs a credential other than a bearer token; no `200` JSON answer schema; a path that is not literal
unreserved segments and whole-segment parameters (`OffOriginPath`); an unbound or unused parameter; an
undeclared paging parameter; a pointer the answer schema does not declare; a mapping word outside
the vocabulary.

`OpenApiError`, while reading answers: a pointer that resolves to nothing, a value of the wrong
shape, a name outside the identifier grammar, a missing description, a repeated cursor, an empty page
that names a next one, a cursor over 2048 bytes, a listing over 100,000 entries, and a join onto a
model without a single-column key.

## Security

- **Only the declared endpoint is dialled.** The adapter builds every request target itself: the
  document's path (checked, see above), the mapping's fixed parameters (one unreserved path segment
  each, never `.` or `..`), each model's own parsed name, and a percent-encoded query. The document's
  `servers`, `externalDocs`, `links` and callbacks are never read. The only paging style is a cursor
  sent back as one query value, so no answer can name a URL to follow.
- **Every caller sees what the deployment's token sees.** `SemanticCatalog::load` has no caller, so
  the catalog is read once as the deployment and pinned for everyone. An API that filters metadata
  per user shows every caller the token's view; scope the token to what all callers may see.
- **The document schema is a contract, not a validator.** A pointer is checked to name a place the
  schema declares. An answer that does not match its own schema is refused where a read fails, not
  validated against the schema as a whole.

## Limits

- Fixture-proven only, and no live venue exists.
- OpenAPI 3.0 and 3.1, as JSON. A YAML document must be converted first; Swagger 2.0 is not read.
- A path segment that mixes text and a parameter (`{name}.json`) or holds `:` is refused rather than
  built.
- Auth is the bearer token. Minting a token for each load (OAuth2 client credentials) is not built
  here; a shared token source is the seam it will use.

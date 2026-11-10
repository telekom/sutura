---
title: What a Foundry ontology can carry
description: EXPERIMENTAL. A field-by-field finding for issue 1727 - a Foundry ontology's v2 metadata API supplies the physical model, descriptions and foreign-key joins, and carries no measure, grain, filter, allowlist, anchor or knowledge, so the adapter over it is a narrow declaring source.
---

# What a Foundry ontology can carry

**Experimental.** `sutura-catalog-foundry` is tested against answers written from Foundry's public
API reference, not against a running Foundry. Its shape follows that reference; a tenant may differ.

## What is read

Two GET operations of the ontology API v2, on the declared host only:

| Operation                | Path                                                                       |
| ------------------------ | -------------------------------------------------------------------------- |
| List object types        | `/api/v2/ontologies/{ontology}/objectTypes`                                |
| List outgoing link types | `/api/v2/ontologies/{ontology}/objectTypes/{objectType}/outgoingLinkTypes` |

Both answer a page (`data`, `nextPageToken`). The token goes back as `pageToken` on the same path;
nothing else in an answer is followed. The link read is one list per object type.

## The mapping

| sutura                                                     | Foundry v2                                                              | Verdict                                                                                 |
| ---------------------------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `Model`, its table                                         | `ObjectTypeV2.apiName`                                                  | provided; a name outside the identifier grammar is refused                              |
| `Description`                                              | `description`, else `displayName`                                       | provided; an object type with neither is refused                                        |
| primary key                                                | `primaryKey`                                                            | provided; one property in v2                                                            |
| `Column` and its type                                      | `properties{}` and `dataType.type`                                      | may provide; the type is Foundry's own word (`string`, `long`, `timestamp`)             |
| column description                                         | `PropertyV2.description`, else `displayName`                            | may provide                                                                             |
| `Relationship`                                             | `LinkTypeSideV2` with cardinality `ONE` and `foreignKeyPropertyApiName` | may provide; many-to-one onto the target's primary key, named `<objectType>_<linkType>` |
| the reverse side                                           | the `MANY` side of the same link                                        | not carried: it is the reverse of a carried join                                        |
| many-to-many                                               | two `MANY` sides, no foreign key                                        | not carried: no join table is exposed                                                   |
| metrics, grains, required filters, allowed values, anchors | none in object-type metadata                                            | absent, declared; aggregation is a query-time operation in Foundry                      |
| knowledge                                                  | `aliases` (empty on the list operation), `status`, `visibility`         | not carried                                                                             |

Refused at load, by name: a foreign key on a `MANY` side, a cardinality other than `ONE` or `MANY`,
a foreign-key-backed link to an object type the read did not list, a primary key or foreign key
that is not a property.

## Identity

The reader presents the deployment's own token - a fixed bearer, or one minted by OAuth 2.0 client
credentials at a token endpoint on the same origin, cached until sixty seconds before its declared
lifetime ends (counted from before minting). That margin does not guarantee completion before expiry. Foundry
decides what metadata that token sees, and every caller with the catalog scope sees that view. Use a
service user scoped to what every caller may see. A per-caller catalog is not expressible: the
catalog port loads without a caller.

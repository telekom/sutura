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

| sutura                                                     | Foundry v2                                                              | Verdict                                                                                    |
| ---------------------------------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `Model`, its table                                         | `ObjectTypeV2.apiName`                                                  | provided; a dotted name maps `.` to `_` for the model, and the table keeps the dotted name |
| `Description`                                              | `description`, else `displayName`                                       | provided; an object type with neither is refused                                           |
| primary key                                                | `primaryKey`                                                            | provided; one property in v2                                                               |
| `Column` and its type                                      | `properties{}` and `dataType.type`                                      | may provide; the type is Foundry's own word (`string`, `long`, `timestamp`)                |
| column description                                         | `PropertyV2.description`, else `displayName`                            | may provide                                                                                |
| `Relationship`                                             | `LinkTypeSideV2` with cardinality `ONE` and `foreignKeyPropertyApiName` | may provide; many-to-one onto the target's primary key, named `<objectType>_<linkType>`    |
| the reverse side                                           | the `MANY` side of the same link                                        | not carried: it is the reverse of a carried join                                           |
| many-to-many                                               | two `MANY` sides, no foreign key                                        | not carried: no join table is exposed                                                      |
| metrics, grains, required filters, allowed values, anchors | none in object-type metadata                                            | absent, declared; aggregation is a query-time operation in Foundry                         |
| knowledge                                                  | `aliases` (empty on the list operation), `status`, `visibility`         | not carried                                                                                |

**Dotted API names.** The object-type list serves platform-provided types with dotted API names,
such as `com.palantir.system.Evolution`, beside the ontology's own; the ontology editor does not
show them. Such a type loads like any other. Its model name, and the local name every column is
qualified by, is the API name with each `.` replaced by `_` (`com_palantir_system_Evolution`); its
links are named `<model>_<linkType>`. The table keeps the dotted API name, and a Foundry statement
names it as one backtick-quoted identifier aliased to the local name:
`` FROM `com.palantir.system.Evolution` AS `com_palantir_system_Evolution` ``. Every other SQL
dialect refuses such a table. **Unverified:** Foundry's public Ontology SQL documentation shows an
object type referenced by its RID in backticks (dots inside one identifier) or as
`` `ontologyApiName`.`objectTypeApiName` ``, and says nothing about an API name that is itself
dotted; no tenant has run the quoted whole-name form. The link list for a dotted type is asked at
`.../objectTypes/com.palantir.system.Evolution/outgoingLinkTypes`, also not run against a tenant.

Refused at load, naming the object type and, where there is one, the property or link type: an API
name that cannot be mapped (an empty segment, a segment outside the identifier grammar, or a mapped
name over 63 characters); two API names that map to one model name, naming both; a property, foreign
key or link name outside the grammar; a foreign key on a `MANY` side; a cardinality other than `ONE`
or `MANY`; a link to an object type the read did not list; and a primary key or foreign key that is
not a property. Property and link API names are not mapped: dotted ones are refused by name, because
no tenant has shown one.

## Identity

The reader presents the deployment's own token - a fixed bearer, or one minted by OAuth 2.0 client
credentials at a token endpoint on the same host and minted again before it expires. Foundry
decides what metadata that token sees, and every caller with the catalog scope sees that view. Use a
service user scoped to what every caller may see. A per-caller catalog is not expressible: the
catalog port loads without a caller.

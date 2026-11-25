---
title: Foundry
description: Experimental. Read object types, properties and foreign-key links from a Foundry ontology.
---

# Foundry

!!! warning "Experimental"

    This catalog is tested against answers written from Foundry's public API reference, not yet
    against a running Foundry. Release builds do not include it: build `sutura-cli` with
    `--features foundry`.

The Foundry catalog reads an ontology over Foundry's ontology API v2. The crate is
`sutura-catalog-foundry`, and the catalog kind is `foundry`. It supplies models, column types,
descriptions and joins.

To execute questions on Foundry itself, declare a separate
[`kind: foundry` data source](../data-systems/foundry.md). The catalog's metadata token does not
become a query credential.

## When to use it

- Your organisation keeps its object types and their links in a Foundry ontology.
- An agent needs those names, types and joins while a data system that holds the same data runs
  the queries.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings)
and these keys. It does not read `dir` or `data_dir`.

| Key                                             | Type    | Default                       | Meaning                                                                                                      |
| ----------------------------------------------- | ------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `endpoint`                                      | string  | required                      | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`. |
| `foundry.ontology`                              | string  | required                      | The API name or the resource identifier of the ontology                                                      |
| `token_file`                                    | path    | one of the two                | A file that holds a Foundry bearer token. sutura reads it once at startup.                                   |
| `foundry.client_credentials.client_id`          | string  | one of the two                | The OAuth 2.0 client of this deployment                                                                      |
| `foundry.client_credentials.client_secret_file` | path    | required with `client_id`     | A file that holds the client secret. sutura reads it once at startup.                                        |
| `foundry.client_credentials.scopes`             | list    | none                          | The scopes to ask for, for example `api:ontologies-read`                                                     |
| `foundry.client_credentials.token_path`         | string  | `/multipass/api/oauth2/token` | The path of the token endpoint on `endpoint`. A host is refused.                                             |
| `deadline_seconds`                              | integer | `30`                          | One budget for all requests and token mints; `0` is refused.                                                 |
| `max_response_bytes`                            | integer | `8388608`                     | The size limit for one response. `0` is refused.                                                             |

Write `token_file` or `foundry.client_credentials`, not both. With client credentials, sutura gets
a new token before the old one expires, so a catalog refresh does not fail after one hour.

For TLS, the reader uses `security.outbound`: `transport_anchors` and an optional
`client_certificate` and `client_key`.

## Example

```yaml
catalogs:
  - name: "ontology"
    kind: "foundry"
    version: "v1"
    endpoint: "https://foundry.example.com"
    foundry:
      ontology: "ri.ontology.main.ontology.00000000-0000-0000-0000-000000000000"
      client_credentials:
        client_id: "sutura-catalog-reader"
        client_secret_file: "/run/sutura/foundry-client-secret"
        scopes: ["api:ontologies-read"]
sources:
  ontology:
    kind: "files"
    data_dir: "/srv/sutura/data"
    posture: "shared-service-user"
```

## How the model maps

sutura reads `/api/v2/ontologies/{ontology}/objectTypes` and, for each object type,
`.../objectTypes/{objectType}/outgoingLinkTypes`.

| Foundry                                              | In sutura                                          |
| ---------------------------------------------------- | -------------------------------------------------- |
| An object type                                       | A model. Its description, or else its display name |
| A dotted object type API name                        | A model named with `_` for each `.`                |
| Its properties                                       | The columns, with type and description             |
| Its primary key                                      | The primary key of the model                       |
| A link side with cardinality `ONE` and a foreign key | A many-to-one join, `<objectType>_<linkType>`      |
| A `MANY` side, or a many-to-many link                | Not a join                                         |

sutura binds every model to the source whose alias is the catalog `name`.

Foundry lists platform-provided object types with dotted API names, such as
`com.palantir.system.Evolution`, beside your own; the ontology editor does not show them. They load
too. The model name is the API name with each `.` replaced by `_`, so
`com.palantir.system.Evolution` is the model `com_palantir_system_Evolution`, and a link from it is
`com_palantir_system_Evolution_<linkType>`. A `kind: foundry` data source still queries the
object by its dotted API name, written as one backtick-quoted identifier. That form follows how
Foundry's public Ontology SQL documentation quotes an object type's RID; whether a tenant accepts it
for a dotted API name is not verified.

sutura refuses the whole load, and names the object type, when:

- two API names map to the same model name - both are named, because one would hide the other
- a dotted API name has an empty segment or a segment that is not a name (letters, digits and
  underscore, not starting with a digit), or its model name is longer than 63 characters
- a property, primary key, foreign key or link type name is not a name - the property or link type
  is named too; these are not mapped

## Identity

The reader sends the token of the deployment on every request. It is one token for every caller,
because sutura loads a catalog without a caller's identity. The bundle holds the object types that
this token can see. Use a service user that can see only what every caller may see.

### Narrowing what each caller sees

!!! warning "Built, not observed"

    Held by fakes only: a loopback identity provider and a loopback fake of Foundry's object-type
    list. No Foundry tenant has listed object types under a brokered token for it.

A [`kind: foundry` source](../data-systems/foundry.md#identity) that runs each question with
`foundry_login` can also narrow each caller's VIEW of the catalog. Write
`catalog_visibility: "per-caller"` on that source:

```yaml
sources:
  ontology:
    kind: "foundry"
    endpoint: "https://foundry.example.com"
    ontology: "sales_ontology"
    posture: "impersonation-at-source"
    catalog_visibility: "per-caller"
    foundry_login:
      token_endpoint: "https://idp.example.com/realms/example/broker/foundry/token"
      client_id: "sutura"
      client_secret_file: "/run/sutura/idp-client-secret"
```

For `GET /v1/catalog`, the agent surface's `describe_catalog` tool and its `initialize` prompt,
sutura then lists the source's object types with the caller's own brokered Foundry token - the
same token `foundry_login` gets for a question - and shows only the models on that source whose
object type is in that list. A metric over a hidden model is hidden too, and so is a join to one.
The definitions do not change: the bundle, its digest and its provenance stay the deployment's.

| Value          | Meaning                                                                       |
| -------------- | ----------------------------------------------------------------------------- |
| absent         | The default. Every caller sees every model, and nothing is listed per caller. |
| `"per-caller"` | Narrowed as above. Refused at load on a source without `foundry_login`.       |

One caller's list is reused for 60 seconds, for up to 1024 callers, and never for another caller.
A caller the identity provider answered *not linked* is answered so again for 10 seconds without
asking it. Concurrent first requests for one caller share one listing; the others wait for it inside
their own deadline. Narrowing that has to dial takes an execution slot, the one a question takes,
before it dials anyone, and is shed like a question when every slot is taken; a caller whose list
is held takes no slot. Which case applies is read just before the call, so a list that expires in
between is read without a slot.

What a failure answers - never the unnarrowed catalog:

- A caller with no linked Foundry account: `403 identity_not_linked`.
- Any other identity provider failure: `503 identity_unavailable`.
- Foundry did not list, or refused the caller's token: `503 unavailable`.
- Every execution slot taken, when the list has to be read: the at-capacity answer a question
  gets.

On the agent surface, `initialize` answers a JSON-RPC error instead of a prompt over the whole
catalog: `-32011` (`identity_not_linked`), `-32012` (`identity_unavailable`), `-32013`
(`unavailable`), `-32014` (`at_capacity`), or `-32603` for a wiring defect on this side.

**It narrows what a caller sees. It is not a data control.** Foundry evaluating each question under
the caller's own token is the data control. A caller who names a hidden metric can still ask it;
Foundry then refuses it under their token, and that refusal differs from the one for a metric that
does not exist.

Limits:

- Text in the bundle is not narrowed: a metric's description, a note, or the operator's
  instructions that name a hidden model still reach every caller.
- Models on other sources are not narrowed.
- A metric over catalog-authored SQL is judged by its own model only.
- A permission Foundry removes stays visible for up to 60 seconds.
- Only `sutura serve` narrows: the `sutura` command refuses an impersonating Foundry source.
- Whether a Foundry application's token may list object types, and whether Foundry lists them per
  user as sutura expects, is unverified.

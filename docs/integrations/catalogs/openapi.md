---
title: OpenAPI (experimental)
description: EXPERIMENTAL. Read tables, columns and joins from supported GET/JSON metadata APIs through a pinned OpenAPI document and a closed mapping vocabulary.
---

# OpenAPI (experimental)

**This catalog is experimental.** It is tested only against recorded answers in the repository. No
test runs it against a live API.

The OpenAPI catalog reads the models from an HTTP metadata API. You configure the API, you do not
write code for it. The crate is `sutura-catalog-openapi`, and the catalog kind is `openapi`. It
supplies tables, columns, types, descriptions, primary keys and joins. It supplies no metrics.

## When to use it

- A metadata API lists your tables and their columns, and no other catalog kind reads it.
- You have the API's OpenAPI 3.0 or 3.1 document, as JSON.

## What the catalog directory holds

`dir` holds two files. sutura reads both at startup and checks the one against the other before it
sends a request. A file that does not agree stops the startup.

| File           | Content                                                                    |
| -------------- | -------------------------------------------------------------------------- |
| `openapi.json` | A pinned copy of the API's OpenAPI document. sutura never downloads it.    |
| `mapping.yaml` | Which operations list the models and the links, and where each field sits. |

The mapping names a `GET` operation by its `operationId`, and each field by a JSON Pointer
(RFC 6901) into one answer:

```yaml
base_path: /api # optional: the path of the API on the endpoint
parameters: # fixed path parameters
  workspace: demo
models:
  operation: listDatasets
  items: /items # the list in one answer
  paging: # optional, cursor paging only
    cursor: pageToken # the query parameter that sends the cursor back
    next: /next # where the answer holds the next cursor
    size: { parameter: pageSize, value: 100 }
  name: /name
  description: [/summary, /title] # the first text that is not empty
  primary_key: /key # a string or a list of strings
  columns:
    at: /fields # an object keyed by column name, or a list
    name: /name # only for a list
    type: /type
    description: [/description]
relationships: # optional
  operation: listDatasetLinks
  model_parameter: dataset # the path parameter that takes each model's name
  items: /items
  name: /name
  target: /target
  foreign_key: /column # a link without it is not read
```

[`crates/sutura-catalog-openapi/src/fixture/`](https://github.com/telekom/sutura/tree/main/crates/sutura-catalog-openapi/src/fixture)
has a complete example, and a sample mapping for an ontology API.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings)
and these keys. It reads `dir`, and requires `data_dir` but does not read it. It is built only with
the `openapi` feature of `sutura-cli`, and no published binary has that feature.

| Key                  | Type    | Default   | Meaning                                                                                                      |
| -------------------- | ------- | --------- | ------------------------------------------------------------------------------------------------------------ |
| `endpoint`           | string  | required  | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`. |
| `token_file`         | path    | required  | A file that holds the bearer token. sutura reads it once at startup.                                         |
| `deadline_seconds`   | integer | `30`      | One time limit for one catalog read, all pages and links included. `0` is refused.                           |
| `max_response_bytes` | integer | `8388608` | The size limit for one response. `0` is refused.                                                             |

Every model binds to the source whose alias is the catalog `name`.

## What sutura sends

- sutura sends a `GET` only to the declared `endpoint`. It builds each path from the document's
  path, the fixed parameters and each model's name. It does not use the document's `servers`, and
  it does not follow a redirect or a URL in an answer.
- Each request has the header `Authorization: Bearer <token>`.
- Every caller sees what the token sees: sutura reads the catalog once, as the deployment. Use a
  token that may see only what all callers may see.

## Limits

- Tested only against recorded answers.
- The nominal deadline is checked after each complete response body. Socket I/O gets a five-second
  margin so a late body can be refused; JSON decoding and catalog assembly happen after that check.
- OpenAPI 3.0 and 3.1, as JSON. Swagger 2.0 is refused.
- A path segment that mixes text and a parameter, such as `{name}.json`, is refused.
- The token is read once at startup. A token that expires is not renewed.

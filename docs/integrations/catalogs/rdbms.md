---
title: RDBMS dictionary
description: Read tables, columns and comments from a documentation view or the native PostgreSQL or Oracle dictionary.
---

# RDBMS dictionary

The RDBMS dictionary catalog reads a documentation view by default, or the native dictionary in a
PostgreSQL or Oracle database. The crate is `sutura-catalog-rdbms`, and the catalog kind is
`rdbms`. It supplies the structure, the column types and the descriptions. It supplies no metrics,
so it gives an agent a map of the tables before anyone defines a metric.

## When to use it

- Use it for multi-player governance. Many owners document their tables in one shared database, and
  that database is the source of truth. The [multi player](../../examples/multi-player.md) example shows this model with
  DataHub.
- Your database team documents tables and columns in the database itself.
- Later, you define metrics in a [markdown catalog](markdown.md) over the same tables.

## The documentation view

sutura reads one view named `columns` in the schema `dictionary_schema`. It has one row for each
documented column:

| Column               | Meaning                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `environment`        | The deployment that the row applies to                             |
| `catalog_name`       | The physical catalog above the schema, if statements need one      |
| `schema_name`        | The physical schema of the table                                   |
| `table_name`         | The physical table                                                 |
| `model_name`         | The model name in sutura                                           |
| `table_description`  | The table description, or `NULL`                                   |
| `column_name`        | The physical column                                                |
| `column_ordinal`     | The order of the column in the table                               |
| `column_type`        | The data type, as `information_schema` reports it                  |
| `column_description` | The column description, or `NULL`                                  |
| `is_primary_key`     | `true` if the column is the only column of a primary or unique key |
| `is_deleted`         | `false` for a live row. sutura reads only live rows                |

The view has no foreign-key columns, so this catalog supplies no joins.

## Settings

This catalog reads `name` and `version` from the
[settings that every catalog has](../../integrations.md#catalog-settings). `dir` and `data_dir`
are optional here. The other keys are for this kind only, and sutura refuses them on another kind.

| Key                                                        | Type    | Default             | Meaning                                                                                               |
| ---------------------------------------------------------- | ------- | ------------------- | ----------------------------------------------------------------------------------------------------- |
| `environment`                                              | string  | required            | Selects the rows of the view for this deployment                                                      |
| `source_alias`                                             | string  | required            | The `sources:` alias of the data system that holds the tables                                         |
| `dictionary_schema`                                        | string  | `sutura_dictionary` | The schema of the `columns` view. Letters, digits and `_` only                                        |
| `max_dictionary_rows`                                      | integer | `10000`             | The row limit for one read. `0` is refused                                                            |
| `max_dictionary_bytes`                                     | integer | `8388608`           | The size limit for one read. `0` is refused                                                           |
| `live_row_predicate.column`                                | string  | none                | An extra column that selects live rows                                                                |
| `live_row_predicate.operator`                              | string  | none                | `is_null`, `is_not_null` or `equals`                                                                  |
| `live_row_predicate.value`                                 | string  | none                | The value for `equals`. Refused for the other two operators                                           |
| `connection.dialect`                                       | string  | `postgres`          | `postgres` or `oracle`                                                                                |
| `connection.host`                                          | string  | none                | The database host. Oracle requires it                                                                 |
| `connection.unix_socket`                                   | path    | none                | PostgreSQL only: a socket directory, instead of `host`                                                |
| `connection.port`                                          | integer | required            | The database port                                                                                     |
| `connection.database`                                      | string  | none                | PostgreSQL: the database. Required for PostgreSQL                                                     |
| `connection.service_name`                                  | string  | none                | Oracle: the service name. Required for Oracle                                                         |
| `connection.user`                                          | string  | required            | The login of the catalog                                                                              |
| `connection.password_file`                                 | path    | required            | An absolute path to a file that holds the password                                                    |
| `connection.transport_mode`                                | string  | required            | PostgreSQL: `plaintext`, `verified` or `mutual`. Oracle: `verified` or the weaker `native-encryption` |
| `connection.transport_anchors`                             | path    | none                | The CA file for `verified` and `mutual`                                                               |
| `connection.client_certificate`, `connection.client_key`   | path    | none                | The client identity for `mutual`                                                                      |
| `connection.native_encryption_acknowledged_because`        | string  | none                | Oracle only: required, nonblank reason for `native-encryption`; refused for other modes               |
| `connection.legacy_password_verifier`                      | boolean | `false`             | Oracle only: permits 10G and 11G password logins over `verified` or `native-encryption`               |
| `connection.legacy_password_verifier_acknowledged_because` | string  | none                | Oracle only: required, nonblank reason when the legacy switch is `true`; refused otherwise            |

`sutura serve` reads all catalog entries again at the shortest `refresh_seconds` among them. The default is 900, and `0` is refused. See [Serving](../../serving.md).

`plaintext` needs a loopback IP address as `host`. `verified` and `mutual` need `host`, not
`unix_socket`. A build with the `rdbms` feature reads PostgreSQL dictionaries; the Oracle
dictionary needs a build with the `oracle` feature. Oracle refuses `plaintext` for every host.
For `verified`, write `transport_anchors` naming a PEM file. Those certificates replace the public
authorities, `host` is the name TLS verifies, and sutura presents no client certificate. The Oracle
reader refuses a listener that redirects the connection, before it logs in.

Oracle's `native-encryption` uses TCP with required Oracle Native Network Encryption and Data
Integrity, with only AES-256/192/128 and SHA-512/384/256. **It does not authenticate the listener:**
the unauthenticated Diffie-Hellman exchange does not establish the listener's identity against
an active network attacker. Prefer `verified` when TCPS is available. Native mode requires its own
reason, refuses TLS anchors and client identity fields, and startup logs it at `WARN`. There is no automatic fallback
or algorithm setting.
Native integrity excludes TNS headers and data flags. CBC reuses the derived IV for records within
each protocol phase: equal plaintext block prefixes produce equal ciphertext prefixes. See the
[Oracle source limits](../data-systems/oracle.md#native-encryption-and-legacy-passwords).

The legacy-verifier switch is a separate decision with a separate reason and startup `WARN`.
Without it, an Oracle account with only a 10G or 11G verifier is refused with its type named; an
unknown verifier is always refused. Legacy login exchanges expose material for offline password
guessing when visible, so this switch requires an encrypted transport. NNE's lack of authenticated
peer identity still applies. These settings do not change the catalog identity: every read uses
its declared dictionary login. See the [Oracle source](../data-systems/oracle.md#native-encryption-and-legacy-passwords)
for the same transport guarantees and limits.

## Example

```yaml
catalogs:
  - name: "dictionary"
    kind: "rdbms"
    version: "dictionary-1"
    environment: "prod"
    source_alias: "warehouse"
    connection:
      host: "db.example.com"
      port: 5432
      database: "dictionary"
      user: "sutura_dictionary_reader"
      password_file: "/run/secrets/dictionary-password"
      transport_mode: "verified"
      transport_anchors: "/etc/sutura/db-ca.pem"
```

## Native dictionary

Set `dictionary_source: native_dictionary` to read visible metadata from PostgreSQL's
`information_schema.columns` and catalog comments, or Oracle's `ALL_TAB_COLUMNS`,
`ALL_TAB_COMMENTS` and `ALL_COL_COMMENTS`. The default remains `documentation_schema`.
The native mode needs no documentation view. It refuses `environment`, `dictionary_schema`
and `live_row_predicate`, which select only that view.

```yaml
dictionary_source: native_dictionary
audience: open
include:
  - { schema: INVENTORY, table: "REPO_*" }
exclude_tables: [REPO_SCRIPTS]
exclude_columns: ["SECRET*", "*EMAIL*"]
exclude_types: [CLOB, BLOB, LONG]
```

The optional catalog-level `audience` applies to every inferred model, in either dictionary mode.
Use `open` or `{ restricted: [finance] }`; restricted audiences use `security.audience_mapping`.
Without a declaration, inferred models remain invisible to verified callers. A deployment without
caller verification retains its single-user view. Audience changes enter the definition digest.

Each include is a schema/table pair, and every pair must match a visible table before exclusions.
Patterns are case-sensitive: only `*` (any sequence) and `?` (one character) are special; all other
characters are literal. Match the returned dictionary case: ordinarily uppercase for unquoted
Oracle names, lowercase for unquoted PostgreSQL names, and preserved case for quoted names.
The runtime still refuses identifiers outside its ASCII letter/digit/underscore and 63-byte shape.
There must be 1 through 64 includes, at most 128 exclusions combined, and 1 through 128 bytes per
pattern. SQL schema prefilters bind escaped pattern values; post-read matching is authoritative.

Table exclusions match table names; column exclusions match column names within selected tables.
Type exclusions match the exact native type or its base name before `(`. Each remaining table
becomes one model named after its table, with its physical `schema.table`, exact supported types
and comments. Duplicate model names across schemas, a table with no remaining columns, an empty
selection and an unsupported unexcluded type refuse the entire load. Unbounded Oracle `NUMBER`,
negative-scale or over-38-digit decimals, Oracle `FLOAT` / `BINARY_FLOAT` / `RAW` / `BLOB`,
PostgreSQL `real` / `bytea` / timestamps, and unsupported Oracle timestamp precision are
refused rather than converted to a less precise type. No key, join or cardinality is inferred.

Native dictionaries may compose with Markdown metrics and knowledge over their models. Contributor
ownership and declaration fidelity are checked separately before cross-catalog references link.
For each configured physical source alias, its native table/column allowlist also checks every
Markdown model, including a differently named model of the same physical table. An excluded or
unselected table or column cannot be reintroduced that way; two native authorities for one source
alias refuse. Other mixed catalog kinds remain unsupported.

With planned `run_sql` in a build with `sql`, only visible declared model columns are registered.
Models on one source sharing a physical table contribute the union of their visible columns to one
registration; hidden models add none. This includes projections, filters and joins.
**Raw pass-through SQL does not inherit these exclusions; its access remains the database account's
grants.** These rules change no caller identity or database grants.
`describe_catalog` lists metrics rather than model columns;
absence there alone is not proof that a column cannot be read.

Metadata rows and bytes are bounded before exclusions. PostgreSQL bills each fetched Arrow batch,
and Oracle bills decoded rows; these checks happen after allocation, and are neither a process
memory cap nor a hard elapsed-time deadline. A successful refresh publishes newly allowed columns;
a failed read, selection, composition or warehouse validation retains the whole previous bundle.
The live proof uses disposable PostgreSQL and Oracle fixtures in CI, not a version-universal
claim about database installations.

## Identity

The catalog signs in with its own login, `connection.user`. It never uses the credential of a data
system, because a catalog read has no caller. Each read opens one connection and one read-only
transaction, and rolls it back.

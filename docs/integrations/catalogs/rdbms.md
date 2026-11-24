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
and comments. Duplicate model names across schemas and an empty selection refuse the entire load.
Unbounded Oracle `NUMBER`, negative-scale or over-38-digit decimals, Oracle `FLOAT` /
`BINARY_FLOAT` / `RAW` / `BLOB`, PostgreSQL `real` / `bytea` / timestamps, and unsupported Oracle
timestamp precision have no exact supported mapping; by default, so does a table left with no
remaining column. Both refuse the load in one pass naming every offending column (table, column,
native type) and every emptied table together, not just the first one found - restarting once per
name found no longer applies. The list is bounded in the message (stating the true count even when
truncated), not in what is collected or in what a declared opt-in below still drops.

`on_unsupported_column` (default `refuse`) and `on_empty_table` (default `refuse`) opt a native
catalog out of refusing, independently of each other:

```yaml
catalogs:
  - name: warehouse
    kind: rdbms
    dictionary_source: native_dictionary
    include: [{ schema: public, table: "*" }]
    on_unsupported_column: omit
    on_empty_table: omit
```

`on_unsupported_column: omit` drops a column whose native type has no exact supported mapping
instead of refusing, with a `WARN` log naming the table, column and native type. `on_empty_table:
omit` drops a table left with no included column - after exclusions and, if also set, an omitted
unsupported column - instead of refusing, with a `WARN` log naming the table. The two keys are
independent: omitting a column can still leave its table empty, and that table is refused unless
`on_empty_table` is also `omit`. An omitted column or table is excluded exactly like an explicitly
excluded one: it is not served, not listed by `describe_catalog`, and the native physical
allowlist (below) refuses a composed model that tries to reintroduce it, the same as any other
excluded column. Eligible complete primary keys and foreign keys are read as described below; no
metric is inferred.

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

## Descriptions from another source

`descriptions_from` on an RDBMS catalog layers prose onto the selected dictionary columns.
Entries are ordered: the first nonempty description wins, then the dictionary's own comment.
The join uses the exact physical catalog (when present), schema, table and column. Names are
not case-folded and catalog prefixes are not removed. Unknown and excluded columns are ignored;
the load log records `ignored_columns`. A row's keys and identifiers are always parsed at
construction, so a malformed identifier still refuses regardless of whether the row is ever used.
Its prose, though, is only parsed and validated once the row would actually be used - its key has
matched a selected table or column, and lookup has reached it as the first source with non-empty
prose for that column: an unknown or excluded row's prose, or a row a higher-priority source
already answered for, is never checked, even when it is unusable. Duplicate physical keys within
one source refuse loading. Prose never changes columns, types, exclusions, audience, text policies
or the execution allowlist. The catalog digest includes the resulting prose.

A source can name another declared RDBMS or DataHub catalog:

```yaml
descriptions_from:
  - catalog: documentation
```

An explicitly referenced catalog contributes descriptions only; it does not add its models or
metrics to the serving catalog. Unrelated catalog entries keep their usual role and mixed-kind
restrictions. References must resolve without a cycle (maximum depth 32 and 256 expanded references per consumer), and the binary must
link the referenced reader. Each RDBMS catalog accepts at most 32 description sources.

A plain table needs no special documentation view. Declare its own read-only connection and map
its columns; use `live_row_predicate`, a closed predicate with bound values, rather than SQL:

```yaml
descriptions_from:
  - connection:
      host: 127.0.0.1
      database: documentation
      user: reader
      password_file: /run/secrets/documentation
      transport_mode: plaintext
    table: inventory.column_docs
    key: { schema: object_owner, table: object_name, column: field_name }
    description: description
    live_row_predicate: { column: is_deleted, operator: equals, value: "N" }
    max_dictionary_rows: 10000
    max_dictionary_bytes: 8388608
```

The mapping's connection supports the same PostgreSQL and Oracle transport declarations as the
catalog's connection. Identifiers are quoted exactly as written; an Oracle mapping normally uses
uppercase names for unquoted database columns. `key.catalog` is optional and required when the
served dictionary carries that physical component. A mapping without it matches only addresses
without a catalog component. Table comments can also come from referenced catalogs.

A normalised documentation system often stores a row's schema and table name on a separate parent
table rather than inline on the description row itself. `key.via` resolves the key through that
parent table instead:

```yaml
descriptions_from:
  - connection: { ... }
    table: inventory.column_docs
    key:
      column: field_name
      via:
        table: inventory.object_catalog
        on: { object_id: object_id }
        schema: object_owner
        table_name: object_name
    description: description
    live_row_predicates:
      - { table: inventory.object_catalog, column: is_deleted, operator: equals, value: "N" }
      - { table: inventory.column_docs, column: is_hidden, operator: equals, value: "N" }
```

`key` sets either `schema` and `table` directly, or `via`, never both or neither. `via.on` is a
nonempty mapping of the mapped table's join columns to the parent table's, joined with exact
physical names on the same read-only connection and transaction. `live_row_predicates` is a closed
conjunction of several predicates, each naming the exact physical table it filters - the mapping's
own `table` or, only when `key.via` is set, `via.table` - never a literal `main`/`via` token; it is
mutually exclusive with the single-predicate `live_row_predicate`. A `key.via` join that resolves to
more than one parent row for the same key is refused as a duplicate physical column
(`DescriptionError::Duplicate`), the same way a flat mapping refuses two rows for one key.

`key.match` is `exact` (the default, including when omitted) or `case_insensitive`. It only
changes how a mapped row's own `schema_name`/`table_name`/`column_name` values are compared
against the main dictionary's physical identity after they are read back - never how this
mapping's own SQL quotes or binds identifiers, and never the catalog or `key.via` side of a join,
which stays an exact physical match regardless:

```yaml
descriptions_from:
  - connection: { ... }
    table: inventory.column_docs
    key: { schema: object_owner, table: object_name, column: field_name, match: case_insensitive }
    description: description
```

Use it for a documentation system whose own convention does not preserve the served dictionary's
case, such as one that stores every identifier upper-cased regardless of how the source database
spells it.

The fold is Rust's `str::to_lowercase` - full Unicode case folding, not Oracle's `NLS_UPPER` or
PostgreSQL's locale-aware comparison. An identifier that differs between those and Rust's fold
(Turkish dotless `ı`/`i`, German `ß`) may not match under a mixed-locale deployment; it never
silently mismatches two distinct served identifiers onto one description - a served dictionary
whose own identifiers collide under this fold (for example both `ID` and `id`) refuses the whole
load instead, naming the colliding identifiers. That refusal is not scoped to the colliding rows:
it rejects every `descriptions_from` mapping in the load, including an exact-match source covering
some other, non-colliding table.

Mappings use the existing read-only reader transaction, a returned-row cap plus one, inline byte
accounting and the existing description byte, line and control-character checks. PostgreSQL's
byte refusal runs after libpq has materialized its result; Oracle bills decoded cursor rows.
These limits do not bound database scan work or elapsed query time. Referenced catalogs keep
their own read bounds and their prose projection is additionally bounded by the consuming
RDBMS catalog. Reloads read the description sources again before publishing a new pinned bundle.
`prompt.list_physical_schema: true` exposes column descriptions through `describe_catalog`;
`prompt.catalog_prose: omitted` continues to withhold them.

A matched row whose own prose is unusable (a control character, an invisible character, or over
the byte/line limit) follows `on_invalid_description`, default `refuse` (the behaviour above,
unchanged). The refusal names the key the row resolved to - schema, table and column - never the
offending text, which this layer never logs or carries in an error. `on_invalid_description: omit`
drops just that one row with a `WARN` log naming the same key, falling back to the next ordered
source and, if none is left, the dictionary's own native comment - the same fallback an empty or
absent row already takes.

```yaml
descriptions_from:
  - connection: { ... }
    table: inventory.column_docs
    key: { schema: object_owner, table: object_name, column: field_name }
    description: description
    on_invalid_description: omit
```

Unlike `on_missing_endpoint`, this policy is set per mapping only - there is no catalog-wide
default to inherit, since a `catalog:` reference has no mapping fields of its own to carry it.

## Relationships

Native dictionaries read primary keys and foreign keys in the column read's read-only snapshot.
A foreign key becomes `many_to_one` only when its target is the **whole** primary or unique
constraint. Composite pairs stay ordered and complete. Selecting out a member drops the entire
key or relationship. PostgreSQL 18 metadata must report both constraints enforced, validated and
not deferrable, and the backing unique index valid, immediate, nonpartial and without expressions.
Oracle requires both constraints enabled, validated and not deferrable. Other constraints are
omitted; a unique index without a primary/unique constraint supplies no evidence here.

For undeclared keys and views, add an operator assertion to the RDBMS catalog:

```yaml
relationships:
  - name: events_apps
    from: { table: inventory.events, columns: [tenant, app_id] }
    to: { table: inventory.apps, columns: [tenant, id] }
    cardinality: many_to_one
    check_target_uniqueness: { max_rows: 10000, max_bytes: 1048576 }
```

At most 64 declarations with at most 32 distinct pairs each are accepted. Exact physical names,
visible endpoints and known compatible mapped types are required. Equal mapped types and integer
widths are compatible; unknown or incompatible types always refuse the load, naming the endpoint.
An endpoint absent or excluded from the reading identity's dictionary instead follows
`on_missing_endpoint`, default `refuse` (the behaviour above, unchanged). A declaration with no
`on_missing_endpoint` key of its own follows the catalog's own `on_missing_endpoint` key (also
default `refuse`), so a deployment can opt every declaration into `omit` at once instead of naming
it per relationship. `on_missing_endpoint: omit` (per relationship, or inherited from the catalog)
drops that one declaration with a `WARN` log instead of refusing the whole catalog. An omitted
relationship discloses nothing beyond what the dictionary selection already excluded: it never
joins, and `describe_catalog` never lists it. A misspelled table name in a visible schema
cannot be told apart from a hidden table under `shared-service-user` - either way the selected
dictionary simply has no such table - so under `omit` a typo there is silently dropped with the
same `WARN` log as a real missing grant, never refused. A misspelled *column* on a table the
dictionary did select is still refused under either policy: that table's own columns are visible,
so an absent one is a wrong declaration, not a missing grant.

```yaml
relationships:
  - name: events_apps
    from: { table: inventory.events, columns: [tenant, app_id] }
    to: { table: inventory.apps, columns: [tenant, id] }
    cardinality: many_to_one
    on_missing_endpoint: omit
```

`many_to_one` is the only supported written cardinality. Without the optional check, uniqueness is
an operator assertion. With it, the catalog's own connection checks nonnull full target keys on its
first successful dictionary read. A duplicate or a row/key-byte cap overflow refuses the load: a truncated
prefix never certifies uniqueness. Database window comparison decides duplicates, preserving
native numeric equality. This bounds returned rows and logical key bytes, **not physical scanning,
sorting, query time or concurrent database changes**. After success, refresh does not repeat this
data probe; the assertion is not ongoing enforcement. Concurrent first loads can each run the
probe. An empty target passes.

`describe_catalog` lists visible relationships under each origin model, with cardinalities and
all ordered key pairs when
`prompt.list_physical_schema` is enabled. Both models and every key must be visible. This supplies
join descriptions, never a metric or permission to read additional tables. Metrics still undergo
the existing consistency and fan-out checks; these declarations change none of those rules.

### A documentation relationship view

In `documentation_schema` mode, `relationships_view: relationships` opts into a second view in
`dictionary_schema`. Absence makes no second-view query. Its rows are authored many-to-one
assertions with the same endpoint/type checks as configuration, without a uniqueness probe.
Configuration declarations can also be used with documentation catalogs. A view row has no column
of its own to opt one row out, so it always follows the **catalog's** `on_missing_endpoint` key
(default `refuse`, unchanged): an endpoint absent or excluded from the reading identity's
dictionary either refuses the whole load (`refuse`) or is dropped with the same `WARN` log a
configuration declaration gets (`omit`), naming the relationship and the missing endpoint. Those
names come from the documentation view's own rows - data the service user already reads, not from
operator configuration - and base's `refuse` error already carried the same names; only operators
see the log. A misspelled column on a visible table is still refused either way, same as
configuration.

```yaml
catalogs:
  - name: warehouse
    kind: rdbms
    dictionary_source: documentation_schema
    relationships_view: relationships
    on_missing_endpoint: omit
```

| Column                                           | Contract                                                        |
| ------------------------------------------------ | --------------------------------------------------------------- |
| `environment`                                    | Same exact environment as the column view                       |
| `relationship_name`                              | Required identifier, shared by every pair of one relationship   |
| `origin_catalog`, `target_catalog`               | Nullable physical catalog components                            |
| `origin_schema`, `origin_table`, `origin_column` | Required exact physical origin                                  |
| `target_schema`, `target_table`, `target_column` | Required exact physical target                                  |
| `ordinal`                                        | Integer from 1 through the complete key arity                   |
| `key_arity`                                      | Positive integer, identical on every member                     |
| `cardinality`                                    | Exactly `many_to_one`                                           |
| `is_deleted`                                     | PostgreSQL boolean or Oracle `NUMBER(1)`, as in the column view |

All live members must be present exactly once. Incomplete ordinals, conflicting identities,
unsupported cardinalities and missing/excluded columns refuse the load. Rows of another environment
and deleted rows are excluded. The column view's extra `live_row_predicate` applies only to columns;
this view uses `environment` and `is_deleted`. Metadata rows share the column read's row/byte
budgets and snapshot, including the row past the cap; neither native nor documented relationships
are fetched per request. Oracle folds the configured view/schema selectors as the existing column
reader does; physical endpoint values remain exact.

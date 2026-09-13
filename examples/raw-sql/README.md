# Raw SQL

[ADR 0013](../../docs/adr/0013-a-raw-sql-tool-off-by-default.md)'s off-by-default `run_sql` tool,
turned on over the served Postgres source
[`docs/serving.md`](../../docs/serving.md#a-postgres-source-least-authority-and-its-channel) already
documents - a settings change on top of an ordinary deployment, not a different one.

**This is a settings snippet and one worked question, not a directory of markdown.** `run_sql`
takes no catalog: it is unparsed text handed straight to the data system, so there is nothing here
that resembles `single-player/`'s models and metrics. What changes is `examples/single-player`'s own
served deployment, plus `tools.run_sql.enabled: true` and a role grant. The runnable proof is
`crates/sutura-serve/tests/served.rs`'s `a_postgres_source_answers_a_raw_sql_statement_from_the_served_binary`,
which starts the real `sutura-serve` binary against the provisioned Postgres tier and asks the
question below over `POST /v1/sql/run` - the request and the response shown here are the value that
test asserts the binary returned, not a transcript kept in step by hand. (The body is pretty-printed
for this page; the binary itself answers compact JSON.)

## What this demonstrates, and what it does not

**Demonstrates:** a question with no certified metric, answered from the database's own physical
structure - `dim_customer`'s `segment` column - the moment an operator turns the tool on. That is
[ADR 0013](../../docs/adr/0013-a-raw-sql-tool-off-by-default.md)'s whole argument: a semantic layer
is valuable and it is also organisational work, and a product that refuses every question until one
exists never gets funded to build one. The answer below is exactly the kind of thing a person would
promote to a certified metric next - `examples/single-player/catalog/metrics/` is what that
promotion looks like once it happens - and this directory stops before that step, on purpose: the
promotion is authoring work for a human, not a mechanism this example runs.

**Does not demonstrate:** impersonation. This deployment is `single-user`, the same mode
[ADR 0013](../../docs/adr/0013-a-raw-sql-tool-off-by-default.md) requires unless a source can execute
as the asking subject - and the Postgres adapter cannot, today, so `run_sql` is available here only
because nobody but the one operator is meant to be asking. A `multi-user` deployment declaring the
same source refuses to start with `run_sql` turned on; `crates/sutura-config/src/settings/tests/tools.rs`
holds that refusal, not this directory.

**Also does not demonstrate:** the read-only role itself. The development Postgres tier publishes
exactly one credential, and it is the database's own owner - see *The role, and the limit next to
the claim* below for why, and read that section before concluding the `GRANT`s in it are exercised
by anything here. No test in this repository runs them.

## The settings

Everything `docs/serving.md`'s Postgres section already says about `transport_mode`, the password
file and the role grant applies unchanged. One block is new:

```yaml
security:
  identity: "single-user"
  single_user_because: "one operator, asking one database, no subject to impersonate"

sources:
  local:
    kind: "postgres"
    host: "127.0.0.1"
    port: 5432
    database: "analytics"
    user: "sutura_reader"
    password_file: "/etc/sutura/postgres-password"
    transport_mode: "verified"
    transport_anchors: "/etc/sutura/database-ca.pem"
    posture: "shared-service-user"
    acknowledged_because: "single-user deployment; the raw tool runs under this one role"

tools:
  run_sql:
    enabled: true
```

**`tools.run_sql.enabled` is off unless a deployment writes this line.** There is no environment
default and no feature flag beyond the `postgres` build feature the source itself already needs -
turning the tool on is a diff an operator makes and a reviewer sees.

## The role, and the limit next to the claim

`docs/serving.md`'s guidance for every Postgres source is sharper here, because `run_sql` executes
whatever the caller sent: the connecting role should be able to `SELECT` from the tables this catalog
names and nothing else. `run_sql` wraps every statement in a transaction this adapter opens
`READ ONLY` and always rolls back - a real, server-enforced second control - but that transaction
bounds SQL-visible writes, not what the role itself could otherwise do outside it. The role is the
first and the durable control:

```sql
CREATE ROLE sutura_reader LOGIN PASSWORD '...';
GRANT CONNECT ON DATABASE analytics TO sutura_reader;
GRANT USAGE ON SCHEMA public TO sutura_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO sutura_reader;
```

**Stated as a limit, not closed:** the gate-backed test behind this page runs its raw statement
under the development tier's one fixture role, which owns its database and is not narrowed to
`SELECT` - the tier publishes exactly one credential and that credential has no `CREATEROLE`
attribute, so nothing running as it can provision a second, narrower role to connect as instead.
The SQL above is what an operator runs against a real deployment; it is not exercised by anything in
this repository's own test suite, and a reader should not conclude otherwise from the fact that the
worked question below only reads.

## Running it

```bash
just dev-up                    # or the nix Postgres tier `just test` provisions - either publishes
                                # the same discovery file `sutura_dev::provisioned` reads
just dev-endpoint postgres     # host:port, for the settings file above
```

Load `examples/single-player/data/*.csv` into the database the settings file names - one table per
file, named after the file - point `sutura-serve --features postgres` at the settings above, and ask:

```bash
curl -s -X POST http://127.0.0.1:<port>/v1/sql/run \
  -H "Authorization: Bearer <access_token>" \
  -H "Content-Type: application/json" \
  -d '{"statement":"select segment, count(*) as customers from dim_customer group by segment order by segment"}'
```

```json
{
  "outcome": "raw_rows",
  "columns": ["segment", "customers"],
  "rows": [["business", "12"], ["consumer", "27"], ["wholesale", "1"]]
}
```

Read what is missing from that body as carefully as what is in it: no `provenance`, no
`definition_version`, no `definition_digest`, no `executed_as` - there is nowhere on this outcome's
wire shape to put any of them. `crates/sutura-serve/tests/served.rs`'s
`a_postgres_source_answers_a_certified_question_from_the_served_binary` asks this same Postgres
deployment for `recurring_revenue` and gets back `outcome: "answer"` with a `provenance` object
beside the rows: same transport, same bearer gate, and a reader can tell which kind of answer they
are holding from the shape of the reply alone, without trusting a label anybody could have gotten
wrong.

`just serve-e2e` runs the cell that drives this end to end against the real tier and the real
binary; `just test` is where that tier is provisioned. No fixed port is involved either way - the
kernel chooses one, the same way every other served example in this repository does.

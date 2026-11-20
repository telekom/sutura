# Opting a column into a raw search prefilter

Redacted text still decides the answer. An operator can allow a cheaper raw necessary condition
to discard rows that cannot match the redacted predicate:

```yaml
columns:
  - name: SCRIPT
    type: CLOB
    max_text_bytes: 8192
    redact:
      - pattern: "(?i)secret[^[:space:]]*"
        replace: "[masked]"
    raw_prefilter:
      acknowledged_because: this source accepts secret-dependent search timing
```

The same `raw_prefilter` object is supported inside a native dictionary's `column_rules` entry.
The reason is required, trimmed, at most 400 characters, and may contain no controls or invisible
formatting characters. At least one `redact` rule is required, because the prefilter only narrows a
redacted search; the declaration is refused otherwise. Omitting the object disables the prefilter. The complete declaration is
persisted and participates in catalog and schema identity; aliases of a physical column must agree.

The acknowledgement accepts a timing disclosure. A secret containing the requested term can make
an extra row pass the raw test and incur redaction work, even though it contributes no answer.
Repeated timing observations can therefore reveal information about raw values. Database grants
and restricted source views remain the primary defense; this option does not authorize broader
access. The reason records the operator's decision, never proof that the disclosure is harmless.

Eligibility is deliberately narrow: one physical table, one query scope, a positive conjunctive
`column LIKE '%TERM%'`, and a nonempty ASCII alphanumeric term of at most 128 bytes. Every other
conjunct beside it must be unable to raise on any row: a plain column compared (`=`, `<>`, `<`,
`<=`, `>`, `>=`, `IS [NOT] NULL`, `IN`, `BETWEEN`) with literals of that column's own type, with no
function, cast, arithmetic, subquery or second column. A string literal qualifies only against a
column that carries a text policy: a type the dialect does not list, and an unbounded Oracle
`NUMBER`, are pinned as text too, and the source could convert their stored values and raise on
one row. A negative literal never qualifies against an unsigned column. Narrowing by a string on
an ordinary text column therefore keeps redacted-only execution. A source may test the raw text before a
sibling, so a sibling that can raise on one row would make "error" versus "empty answer" a result
signal about that row's unredacted text. Any other sibling, including a second `LIKE`, keeps
redacted-only execution. Joins, nested
queries, CTEs, disjunctions, negations, collation overrides, escapes, inner wildcards and uncertain
alias scopes retain redacted-only execution. Each ordered replacement must be nonempty ASCII,
contain no full term, and start and end with characters absent from that term. These barriers
prevent a replacement from creating a match or bridging deleted text. Where clipping happens
before search, the truncation marker must pass the same proof. Unknown shapes fall back.

The current eligible comparison arms are DataFusion's binary `LIKE` and Oracle native CLOB/NCLOB
`LIKE`, with an optional `UPPER` around that column on Oracle. Cube's local policy execution is not
an eligible arm and keeps redacted-only execution. Oracle
uses its binary character-set uppercase mapping, not `NLS_UPPER`. At execution time only a binary
`NLS_SORT` permits raw row rejection; every other value, including NULL, retains all rows. No
session setting is changed. Non-LOB Oracle columns and other source dialects retain redacted-only
execution because their pinned schemas do not establish binary comparison semantics. This is a
generic policy with conservative engine eligibility, not universal acceleration.

Pinned text columns carry a collation fact, `sutura:collation` in the schema's field metadata and
under the schema digest, so a change re-pins. `binary` means byte-for-byte comparison; a named
collation is never binary; absent means unknown, and unknown is never binary. The fact is pinned
only on a column whose declared type the dialect reads as text, never on one that fell back to
text. DataFusion and ClickHouse have no per-column collation and pin `binary`, over any collation a
catalog authors for them. A native Postgres dictionary records the column's collation for the
base types `text`, `varchar`, `bpchar` and `name` only: the database default is `binary` for
`LIKE`, because it is always deterministic, and so are the built-in `pg_catalog.C` and
`pg_catalog.POSIX`. Any other collation is recorded by its schema-qualified name and is never
binary, including a user schema's own `C`. A `citext` column or a domain over text is unknown. A
Markdown column may declare `collation: binary` or a name. That declaration is an authored claim,
trusted as far as its author is. Two models over one physical column that pin different known
collations are refused at pinning, naming the column; an unknown one contradicts nothing. Oracle
records nothing here, because its check is the run-time `NLS_SORT` above. No other dialect reads
the fact yet; recording it enables no engine.

What `binary` does not cover. It holds for `LIKE` and equality, not for ordering under a locale, and
a type can still change equality: Postgres `bpchar` ignores trailing spaces in `=`. A
`postgres_fdw` foreign table's `default` is the remote server's default. ClickHouse tables on the
MySQL engine compare `=`, `<` and `>` on the MySQL server, while `LIKE` stays in ClickHouse.
DuckDB's global `default_collation` setting can contradict an authored `binary`. Adding the fact
moves the schema digest of every schema that now carries one, once, at upgrade: any with a
DataFusion or ClickHouse text column, and any native Postgres read. A schema with none hashes as
before.

The exact redacted predicate remains above the transformed table view. The raw predicate only
adds a necessary condition inside that view. A database may reorder work, so eligibility alone
does not establish a speedup. Native measurements must report actual row count, setup time,
baseline query time and opted-in query time separately, along with exact result equality and
hidden-secret probes. They describe their engine version and fixture, not a production latency
guarantee.

## A statement over a policy column that runs out of time

A text policy masks every row of the column before the statement's own predicate sees it, so a
search over a policy column with no other filter reads the whole column. Where such a statement
runs past the request's deadline at a source that applies the policy itself, the refusal is
`policy_search_needs_narrowing` (`422`), not a generic `deadline_exceeded`. Add a filter on a column
without a policy so that fewer rows reach the masking.

It is named only when a part of the statement reads a column under a text policy and the source
applies the policy. The reason is given where the source applies the policy; at Cube it is given once
filters reach Cube, which a separate change adds.

What this does not do:

- It is a backstop, not a fix. The statement still does not finish, and the policy still masks
  every row it reads. The refusal only names the cause and the remedy.
- It is not given at Cube. A Cube source holding a protected table is sent only bare scans, and the
  policy and every predicate run in this deployment over the rows Cube answers. A filter on a column
  without a policy is not sent to Cube, so the remedy cannot help there. A timeout stays
  `deadline_exceeded`, and a protected Cube table with more than 10,000 rows cannot be searched: that
  size refusal stays `result_too_large` or `too_many_rows`.
- A timeout in the local part of the plan stays `deadline_exceeded`, whether or not a policy column
  was read. So does a timeout of a statement that reads no policy column, a budget already spent
  before a part runs, and a timeout while planning.
- The engine decides "reads a column" from the columns the planned scans project. A statement over
  a policy column that is not a search, such as one that only projects it, is named the same way
  when it times out.
- No separate time budget is added. The refusal rides the request's existing deadline, so it
  arrives when that deadline does, not earlier.

The reason shows that a column the caller can already read carries a text policy. It names no
column, pattern or replacement, and nothing about the policy beyond its existence.

## Narrowing a Cube search

Cube cannot apply a text policy, so the engine applies it over the raw rows Cube answers, and a
Cube answer holds at most 10,000 rows. A search over a protected Cube table that reads more than
that could not answer even with a narrowing filter. Cube is therefore sent the part of the `WHERE`
that reads no policy column. This needs no declaration, and it adds no raw condition on a policy
column.

A conjunct is sent only when it is one of these, over a column that carries no text policy:

- a text column equal to a text literal, or in a list of text literals;
- `IS NULL` or `IS NOT NULL` of any such column;
- an `AND` or `OR` of those, because the engine spells a short `IN` list as an `OR`.

Every other conjunct stays local, over the masked text: any conjunct that reads a policy column, a
text order or inequality, `LIKE`, a number, a time or a boolean, and anything with a function, cast,
arithmetic, `NOT` or subquery, and an `OR` with one such side. The limit is deliberate. Cube
compares text by its warehouse's collation and holds numbers at their own precision, so only the listed forms cannot make Cube
answer fewer rows than the engine would keep. A conjunct that raises at Cube on a row the local plan
would not reach changes which statements fail, so none that can raise is sent.

Each sent conjunct also stays in the local filter, above the policy, so a Cube that answers more
rows than the conjunct selects is corrected. A Cube that answers fewer is not, and nothing here
proves that none does: the listed forms are chosen so that equal text is equal under any collation,
and the proof of that is by reading, not by a Cube run. The 10,000-row cap, the memory budget and
the deadline still apply to what comes back, so a filter that leaves more than 10,000 rows, or a
search whose only condition reads the policy column, still cannot answer.

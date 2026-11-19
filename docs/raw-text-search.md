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

The current eligible comparison arms are DataFusion's binary `LIKE`, ClickHouse's byte-wise `LIKE`
and Oracle native CLOB/NCLOB `LIKE`, with an optional `UPPER` around that column on Oracle. Cube's local policy execution is not
an eligible arm and keeps redacted-only execution. Oracle
uses its binary character-set uppercase mapping, not `NLS_UPPER`. At execution time only a binary
`NLS_SORT` permits raw row rejection; every other value, including NULL, retains all rows. No
session setting is changed. Non-LOB Oracle columns and other source dialects retain redacted-only
execution because their pinned schemas do not establish binary comparison semantics. This is a
generic policy with conservative engine eligibility, not universal acceleration.

Pinned text columns now carry a collation fact, `sutura:collation` in the schema's field metadata and
under the schema digest, so a change re-pins. `binary` means byte-for-byte comparison; a named
collation is never binary; absent means unknown, and unknown is never binary. DataFusion and
ClickHouse have no per-column collation and pin `binary`. A native Postgres dictionary records the
column's collation: the database default is `binary` for `LIKE`, because it is always deterministic,
and so are the built-in `pg_catalog.C` and `pg_catalog.POSIX`. Any other collation is recorded by
its schema-qualified name and is never binary, including a user schema's own `C`. A Markdown column may declare
`collation: binary` or a name. That declaration is an authored claim and is trusted as far as its
author is. Oracle records nothing here, because its check is the run-time `NLS_SORT` above. Only
ClickHouse reads the fact so far.

ClickHouse has no column collation and its `LIKE` is case-sensitive and byte-wise, so a column of its
pinned schema carries the collation fact `binary` and a column pinned anything else (unknown or a
named collation, which only a hand-built schema can carry) keeps redacted-only execution. The raw
test reads the stored column qualified by its table, in the form `` `table`.`column` ``: ClickHouse
resolves a bare name to the projection alias of the same name unless `prefer_column_name_to_alias`
is set, and a raw test that read the alias would read the redacted value, discard nothing and
still return the same rows. The statement text is what the golden cell asserts; no answer can tell
the two reads apart, so only the recorded timings show the effect. The redaction patterns of
`replaceRegexpAll` are RE2 expressions, and a replacement is a literal (`$` and `\` are refused), so
no backreference can rebuild a term from a matched value. ClickHouse has no `UPPER` arm. The
ClickHouse tier measures equality and timings; the RE2 and alias statements above are documented
behaviour, not measured here.

The exact redacted predicate remains above the transformed table view. The raw predicate only
adds a necessary condition inside that view. A database may reorder work, so eligibility alone
does not establish a speedup. Native measurements must report actual row count, setup time,
baseline query time and opted-in query time separately, along with exact result equality and
hidden-secret probes. They describe their engine version and fixture, not a production latency
guarantee.

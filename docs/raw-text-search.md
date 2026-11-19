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

The current eligible comparison arms are DataFusion's binary `LIKE`, Oracle native CLOB/NCLOB
`LIKE` with an optional `UPPER` around that column, and PostgreSQL `LIKE` on a column whose
collation the dictionary recorded as binary. Cube's local policy execution is not an eligible arm
and keeps redacted-only execution. Oracle uses its binary character-set uppercase mapping, not
`NLS_UPPER`. At execution time only a binary `NLS_SORT` permits raw row rejection; every other
value, including NULL, retains all rows. No session setting is changed. Non-LOB Oracle columns and
other source dialects retain redacted-only execution because their pinned schemas do not establish
binary comparison semantics. This is a generic policy with conservative engine eligibility, not
universal acceleration.

PostgreSQL binds a collation to each column, so the pinned column decides, and an unknown or named
collation falls back. Binary means the database default (always deterministic), `pg_catalog."C"` or
`pg_catalog."POSIX"`. A deterministic collation makes `LIKE` compare character for character, so
the raw test and the redacted test agree. From PostgreSQL 18 `LIKE` also accepts a nondeterministic
(case- or accent-insensitive) collation, which matches text that differs byte for byte and is looser
than the replacement proof; earlier versions refuse it. `regexp_replace`, which the policy view
uses, refuses a nondeterministic collation outright, and a raw test that emptied the scan first
would turn that refusal into an empty answer about unredacted text. Any other named collation, even
a deterministic one, keeps redacted-only execution, and PostgreSQL has no `UPPER` arm. A `char(n)`
column is eligible: the view reads it as `text` without the padding, and the padding is never part
of an ASCII alphanumeric term. The record is the dictionary's as of its read, so a later
`ALTER ... COLLATE` takes effect at the next re-pin.

The exact redacted predicate remains above the transformed table view. The raw predicate only
adds a necessary condition inside that view. A database may reorder work, so eligibility alone
does not establish a speedup. Native measurements must report actual row count, setup time,
baseline query time and opted-in query time separately, along with exact result equality and
hidden-secret probes. They describe their engine version and fixture, not a production latency
guarantee.

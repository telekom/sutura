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

The exact redacted predicate remains above the transformed table view. The raw predicate only
adds a necessary condition inside that view. A database may reorder work, so eligibility alone
does not establish a speedup. Native measurements must report actual row count, setup time,
baseline query time and opted-in query time separately, along with exact result equality and
hidden-secret probes. They describe their engine version and fixture, not a production latency
guarantee.

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

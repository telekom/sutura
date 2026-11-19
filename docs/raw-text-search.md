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

## A search that runs out of room

A text policy masks every row of the column before the statement's own predicate sees it, so a
search over a policy column with no other filter reads the whole column. Where such a statement
cannot finish, the refusal is `policy_search_needs_narrowing` (`422`), not a generic timeout or size
refusal. Add a filter on a column without a policy so that fewer rows reach the masking.

It is named in three cases, and only when the statement reads a column under a text policy:

- A part of the statement runs past the request's deadline at its source, on any engine.
- The local part of the plan runs past the request's deadline.
- A Cube part answers more rows than the per-leg row cap. Cube cannot apply a text policy, so this
  deployment applies it over the rows Cube answers, and the cap counts those rows before any filter
  over the masked text. At every other source the policy runs first, so the same cap is an ordinary
  `too_many_rows` or `result_too_large`.

What this does not do:

- It is a backstop, not a fix. The statement still does not finish, and the policy still masks
  every row it reads. The refusal only names the cause and the remedy.
- A timeout of a statement that reads no policy column keeps `deadline_exceeded`. A budget already
  spent before a part runs, and a timeout while planning, also keep it: neither reads a column.
- The engine decides "reads a column" from the columns the planned scans project. A statement over
  a policy column that is not a search, such as one that only projects it, is named the same way
  when it runs out of room.
- No separate time budget is added. The refusal rides the request's existing deadline, so it
  arrives when that deadline does, not earlier.

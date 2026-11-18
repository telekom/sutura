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
formatting characters. Omitting the object disables the prefilter. The complete declaration is
persisted and participates in catalog and schema identity; aliases of a physical column must agree.

The acknowledgement accepts a timing disclosure. A secret containing the requested term can make
an extra row pass the raw test and incur redaction work, even though it contributes no answer.
Repeated timing observations can therefore reveal information about raw values. Database grants
and restricted source views remain the primary defense; this option does not authorize broader
access. The reason records the operator's decision, never proof that the disclosure is harmless.

Eligibility is deliberately narrow: one physical table, one query scope, a positive conjunctive
`column LIKE '%TERM%'`, and a nonempty ASCII alphanumeric term of at most 128 bytes. Joins, nested
queries, CTEs, disjunctions, negations, collation overrides, escapes, inner wildcards and uncertain
alias scopes retain redacted-only execution. Each ordered replacement must be nonempty ASCII,
contain no full term, and start and end with characters absent from that term. These barriers
prevent a replacement from creating a match or bridging deleted text. Where clipping happens
before search, the truncation marker must pass the same proof. Unknown shapes fall back.

The current eligible comparison arms are DataFusion's binary `LIKE`, including Cube's local policy
execution, and Oracle native CLOB/NCLOB `LIKE`, with an optional `UPPER` around that column. Oracle
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

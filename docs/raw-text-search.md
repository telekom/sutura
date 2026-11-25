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

Every column that declares `raw_prefilter` logs one `INFO` line at load only (`sutura-app`'s
`verify_and_validate`; a later refresh re-pins the schema through `pin_schemas` directly and does not
repeat the line) naming the model, the column and the distinct characters its replacements contain -
the bytes an admitted term must avoid at a position that could overlap one, per the barrier proof
below. The replacement text itself is config, not a secret, and participates in schema identity
already; logging the character set it contains discloses nothing a definitions review would not.

The acknowledgement accepts a timing disclosure. A secret containing the requested term can make
an extra row pass the raw test and incur redaction work, even though it contributes no answer.
Repeated timing observations can therefore reveal information about raw values. Database grants
and restricted source views remain the primary defense; this option does not authorize broader
access. The reason records the operator's decision, never proof that the disclosure is harmless.

Eligibility is deliberately narrow: one physical table, one query scope, a positive conjunctive
`column LIKE '%TERM%'`, and a nonempty term of at most 128 bytes, each byte ASCII alphanumeric or
`_` (LIKE's own single-character wildcard). Every other
conjunct beside it must be unable to raise on any row: a plain column compared (`=`, `<>`, `<`,
`<=`, `>`, `>=`, `IS [NOT] NULL`, `IN`, `BETWEEN`) with literals of that column's own type, with no
function, cast, arithmetic, subquery or second column. A string literal qualifies against declared
scalar text, including an ordinary column without a text policy. Pinning records that provenance
as `sutura:scalar_text=true`, under the schema digest. A bare Arrow text type or a collation fact
does not prove it: an unlisted type and an unbounded Oracle `NUMBER` are pinned as text too, and
the source could convert their stored values and raise on one row. Oracle `CLOB` and `NCLOB` are
not scalar text for these sibling comparisons. A negative literal never qualifies against an unsigned column.
A source may test the raw text before a
sibling, so a sibling that can raise on one row would make "error" versus "empty answer" a result
signal about that row's unredacted text. Any other sibling, including a second `LIKE`, keeps
redacted-only execution. Joins, nested
queries, CTEs, disjunctions, negations, collation overrides, escapes, an inner `%` (a term that is
not exactly `LIKE '%TERM%'` once its one leading and one trailing `%` are stripped - an inner `_` is
the admitted wildcard above, not this) and uncertain alias scopes retain redacted-only execution.

Each replacement must be nonempty ASCII. Soundness needs every alignment of the term (read as a
`_`-wildcard pattern) against the replacement, at every shift with a nonempty overlap, to carry at
least one *literal* (non-`_`) term byte that disagrees with the replacement's byte there - a `_`
never forces a mismatch, because it is LIKE's own wildcard and agrees with anything, including a
replacement's own byte.

**This is wider than the pre-`_` rule, including for a term with no `_` in it.** The old rule refused
a term that contained the replacement's first or last byte at all; this rule only needs each overlap
to carry a disagreement somewhere in it. Term `ac` against replacement `ab`, or `MXY` against
`MASKED`, or `PRODUCTFEEAMT` against `PASSWORD=***`, are refused by the old rule (each shares a
boundary byte with its replacement) but admitted here, because every alignment that overlaps still
carries a byte that disagrees. A term that contains `_` is admitted by the identical rule, with a `_`
position never counted as a disagreement - so a replacement that was eligible for a term is not
guaranteed to stay eligible once a byte in that term is changed to `_`, and a term that starts or ends
with `_` is refused against every replacement (the overlap that is that lone `_` alone can never
carry a disagreement). These barriers prevent a replacement from creating a match or bridging deleted
text. Where clipping happens before search, the truncation marker must pass the same proof. Unknown
shapes fall back. No dialect's `ESCAPE` clause is read on this path (`escape_char` must be absent), so
an escaped literal `_` is not distinguished from the wildcard and does not change eligibility.

An alignment whose entire overlap is a lone `_` is refused even though the `_` itself cannot force a
mismatch: admitting it would also need every redaction rule to replace exactly one raw character
with exactly one character, because a rule that matches an empty span or a span of any other length
shifts what the needle lines up against on either side. Concretely, pattern `$` (end of input, a
zero-width match) with replacement `[m]` against raw text `ab`: the redacted text is `ab[m]`, which
matches `%ab_%` (`_` standing for `[`), while the raw text `ab` does not. Or pattern `b+` (a
multi-character match) with replacement `x` against raw text `abba`: the redacted text is `axa`,
which matches `%a_a%` (`_` standing for `x`), while the raw text `abba` does not. Refusing every
lone-`_` overlap outright is cheaper than proving each rule's pattern always matches exactly one
character, and is not a case where the refusal is known to be unnecessary.

A term containing `_` also lets a caller skip positions it does not know, trading away part of the
timing protection the acknowledgement already accepts: a term like `KEY________` (one literal prefix,
the rest wildcards) probes only the stored length and prefix at that position, at the single-character
granularity LIKE's `_` gives it, without needing the right byte at any of the wildcarded positions.

**Recommendation:** choose a replacement that starts and ends with a non-alphanumeric, non-`_`
character (for example `[masked]` rather than `MASKED`). Since an admitted term is ASCII alphanumeric
or `_` only, such a replacement can never be fully matched by an overlap confined to its boundary
bytes, which keeps the barrier argument easy to re-check by eye. This only helps an *inner* `_` (not
at either end of the term) against a replacement of two or more characters. It changes neither of
the two cases the lone-`_`-overlap rule decides outright: a term with `_` at either end is refused
against every replacement regardless (that lone `_` is the whole overlap at the boundary alignment
and can never carry a disagreement), and a one-character replacement refuses a term with `_`
anywhere in it (every alignment's overlap with a single-byte replacement is exactly that one byte).

The eligible comparison arms are DataFusion's binary `LIKE`, ClickHouse's `LIKE` for the
admitted ASCII-alphanumeric-or-`_` needle, Oracle native CLOB/NCLOB `LIKE` with an optional `UPPER`, and
PostgreSQL, DuckDB and BigQuery text columns pinned `binary`. Cube's local policy execution adds
no raw condition on a policy column. Oracle uses its binary character-set uppercase mapping, not
`NLS_UPPER`; only a binary `NLS_SORT` permits raw row rejection, and any other or NULL value retains
all rows. No session setting is changed. Non-LOB Oracle columns retain redacted-only execution.
Eligibility does not establish a speedup.

DuckDB's `LIKE` consults a column's `NOCASE` or `NOACCENT` collation, and `REGEXP_REPLACE` keeps it
(measured on 2.0.0; an ICU locale, `NFC` and `default_collation` did not change `LIKE`). Its catalog
views report a collated column as plain `VARCHAR`, so only an authored `collation: binary` admits
it. A named, unreadable or missing fact falls back. Nothing checks that authored pin at run time.
For DuckDB alone the replacement barrier folds ASCII case: a case-insensitive comparison cannot
make an admitted replacement create a match. The native cell covers an incorrect binary pin over
a `NOCASE` column; other collations and versions remain outside that measurement.

PostgreSQL binds a collation to each column, so the pinned column decides, and an unknown or named
collation falls back. Binary means the database default (always deterministic), `pg_catalog."C"` or
`pg_catalog."POSIX"`, or a `binary` an author wrote in a local catalog document. A wrong authored
`binary` is the operator's error: where the real collation is looser than byte order (case- or
accent-insensitive), a raw test can drop a row the redacted test would match, so the answer is
incomplete. Before PostgreSQL 18 the raw `LIKE` itself raises on such a column, and the error then
stands for an unredacted value. The extra timing the acknowledgement accepts still applies. A deterministic
collation makes `LIKE` compare character for character, so the raw test and the redacted test
agree. From PostgreSQL 18 `LIKE` also accepts a nondeterministic collation, which matches text that
differs byte for byte and is looser than the replacement proof; earlier versions refuse it.
`regexp_replace`, which the policy view uses, refuses a nondeterministic collation outright, and a
raw test that emptied the scan first would turn that refusal into an empty answer about unredacted
text. For the same reason a conjunct beside the search on another policy column counts as unable to
raise only where that column is pinned binary as well. Any other named collation, even a
deterministic one, keeps redacted-only execution, and PostgreSQL has no `UPPER` arm.

Limits. The record is the column's collation as of the read, so a later `ALTER ... COLLATE` takes
effect at the next re-pin. The tier's database default is `C`; a non-`C` deterministic default is
documented, not exercised. For a `postgres_fdw` foreign table the collation read is the local
declaration, while the remote server does the comparing, so the record is the operator's to keep
true. A `char(n)` column is eligible: the view reads it as `text` without the padding, so the raw
condition (`'%needle%'` over an ASCII-alphanumeric-or-`_` needle, literal bytes and `_` alike) runs
against the same unpadded text the redacted comparison does - there is no padding space either side
could match, create or hide.

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
records nothing here, because its check is the run-time `NLS_SORT` above. PostgreSQL, DuckDB,
BigQuery and ClickHouse use the fact to decide admission.

What `binary` does not cover. It holds for `LIKE` and equality, not for ordering under a locale, and
a type can still change equality: Postgres `bpchar` ignores trailing spaces in `=`. A
`postgres_fdw` foreign table's `default` is the remote server's default. ClickHouse tables on the
MySQL engine compare `=`, `<` and `>` on the MySQL server, and the raw `LIKE` is forwarded there
too: ClickHouse's external-database query transform allows `like` and `notLike`, and the MySQL
storage engine passes no blocklist, so on a MySQL or PostgreSQL engine table the raw test runs
under the remote's collation. That is safe for a literal needle byte: a case- or accent-insensitive
remote returns more rows, never fewer, and the exact redacted predicate still decides. A needle's
`_` adds a second, unmeasured cross-engine question - whether the remote MySQL or PostgreSQL server's
own `LIKE` gives `_` the same one-character wildcard meaning ClickHouse's `LIKE` does, which this
forwarding path never checks and the native cells do not exercise (they measure ClickHouse's own
`LIKE`, not a forwarded one) - stated here as a limit, not relied on. Before PostgreSQL 18 a
nondeterministic collation makes the remote refuse `LIKE`, which costs availability, not secrecy.
This is read from ClickHouse's behaviour, not measured here.
DuckDB's global `default_collation` changes `=` and `ILIKE`, but not `LIKE` (measured on 2.0.0);
a raw statement cannot set it. Adding the fact moves the schema digest of every schema that now carries one, once, at upgrade: any with a
DataFusion or ClickHouse text column, and any native Postgres read. Declared scalar-text provenance
also moves a schema's digest once at upgrade, even without a collation fact. Both records come
from the catalog's declared type; nothing here verifies an authored type against the stored one.

BigQuery admits a `STRING` column only where its Markdown catalog declares `collation: binary`.
No BigQuery dictionary reader supplies this fact; a missing or named collation falls back, and
nothing checks the author's declaration at run time. Dataset or table defaults can therefore
contradict the pin. The native million-row venue observed `LIKE` matching seven binary rows and
eight rows under stored `und:ci`. Redaction-only and redaction-plus-clipping searches over that
collated column both ended in a collation refusal, with the opt-in off and on. An unrecorded binary
column returned identical rows without an added raw condition. A deliberately mislabelled binary
pin added the raw condition and still received a collation refusal in this fixture; that observation
is not a guarantee for another expression or collation. The raw condition uses a backtick-quoted,
qualified column: a double-quoted token is a string literal in BigQuery. Timings are wall-clock
measurements, not slot statistics or a claim of lower billed bytes.

DataFusion and ClickHouse have no per-column collation. Production pinning sets `binary` over any
authored collation, so neither engine can supply a native collated-column fallback case. ClickHouse
still refuses named or unknown facts in a hand-built statement schema; the capability cell holds
that guard. DataFusion needs no column fact because its comparison is fixed by the engine.

ClickHouse has no column collation and its `LIKE` is case-sensitive. ClickHouse documents `LIKE`
matching as UTF-8, undefined on invalid UTF-8, so the byte-wise property holds for the admitted
needle shape only: ASCII alphanumeric or `_`, no escape. A needle with no `_` makes the pattern a
plain substring search; a needle that uses `_` relies on ClickHouse's `LIKE` giving `_` the
one-character (one UTF-8 code point, not one byte) wildcard meaning the barrier proof assumes, which
it does, because the raw test and the redacted test both run as ClickHouse's own `LIKE`: one engine
evaluates both sides, so they necessarily agree on what `_` means. That stops holding for a table on
the MySQL or PostgreSQL engine above, where the raw test can be forwarded to a different engine's own
`LIKE` while the redacted test still runs locally - exactly the cross-engine agreement left unmeasured
there. A column of its pinned schema carries the collation fact `binary` and a column pinned
anything else (unknown or a
named collation, which only a hand-built schema can carry) keeps redacted-only execution. The raw
test reads the stored column qualified by its table, in the form `` `table`.`column` ``: ClickHouse
resolves a bare name to the projection alias of the same name unless `prefer_column_name_to_alias`
is set, and a raw test that read the alias would read the redacted value, discard nothing and
still return the same rows. The statement text is what the golden cell asserts; no answer can tell
the two reads apart, so only the recorded timings show the effect. The redaction patterns of
`replaceRegexpAll` are RE2 expressions, and a replacement is a literal (`$` and `\` are refused), so
no backreference can rebuild a term from a matched value. ClickHouse has no `UPPER` arm. The
ClickHouse tier measures equality and timings; the RE2 and alias statements above are documented
behaviour, not measured here. ClickHouse column types are authored in the catalog: nothing reads
`system.columns`. The adapter pins neither the analyzer setting nor `prefer_column_name_to_alias`;
a raw test that read the alias would be safe, since it only repeats the redacted predicate, and
would lose the speedup.

The exact redacted predicate remains above the transformed table view. The raw predicate only
adds a necessary condition inside that view. A database may reorder work, so eligibility alone
does not establish a speedup. Native measurements must report actual row count, setup time,
baseline query time and opted-in query time separately, along with exact result equality and
hidden-secret probes. They describe their engine version and fixture, not a production latency
guarantee.

## A statement over a policy column that reaches a source bound

A text policy masks every row of the column before the statement's own predicate sees it, so a
search over a policy column with no other filter reads the whole column. Where such a statement
runs past the request's deadline at its source, or a Cube answer exceeds a size or row bound before
local masking and filtering, the refusal is `policy_search_needs_narrowing` (`422`). Add a
supported filter on a column without a policy so that fewer rows reach the masking.

It is named only when the affected source part reads a column under a text policy. At Cube the
policy runs locally, and supported filters on columns without a policy narrow the source answer
before local masking. The same source bounds and deadline still apply.

What this does not do:

- It is a backstop, not a fix. The statement still does not finish, and the policy still masks
  every row it reads. The refusal only names the cause and the remedy.
- At Cube, only the supported policy-free comparisons below narrow its answer. A filter that keeps
  too many rows, or only searches the protected text, still reaches the same bound. Other sources'
  size refusals keep `result_too_large` or `too_many_rows`: their policy already runs at the source.
- A timeout in the local part of the plan stays `deadline_exceeded`, whether or not a policy column
  was read. So does a timeout of a statement that reads no policy column, a budget already spent
  before a part runs, and a timeout while planning.
- The engine decides "reads a column" from the columns the planned scans project. A statement over
  a policy column that is not a search, such as one that only projects it, is named the same way
  when it times out.
- No separate time budget is added. The refusal rides the request's existing deadline - narrowed
  earlier for a policy-column search leg specifically, never widened. `POLICY_LEG_TEARDOWN_MARGIN`
  (2s, `crates/sutura-app/src/raw.rs`) is carved out of such a leg's own deadline on top of the
  one-second margin already reserved for transport reply overhead, so the adapter's own teardown and this leg's local
  masking have room to finish and carry the named refusal back before the transport's generic `408`
  backstop fires. Where narrowing would leave too little budget to run at all (a low
  `request_timeout_seconds` deployment), the leg keeps the unnarrowed remaining budget instead and
  the race is open again. **Limit:** there is no tokio-level abort of the adapter's own call - an
  adapter that ignores its handed deadline can still lose this race and surface as the transport's
  `408` rather than this named refusal, same as before the margin existed.

The reason shows whether a column the caller can already read declares a `raw_prefilter` at all
(`NotDeclared`, for no `raw_prefilter` or an empty `redact` list), and carries an `ineligible`
field naming why the raw prefilter itself could not push the search down - `catalog::eligible`'s
own answer over the needle and the column's policy, computed once before the leg runs. Where that
reason is the needle agreeing with a configured replacement, it also carries the 0-based index of
the first redaction rule it agreed with, which tells the caller the column's `redact` list holds
at least that many entries (index N implies N + 1) and nothing about what any of them matches or
replaces.

`ineligible` is absent (`None`) wherever that check was never reached at all - not an exhaustively
enumerable list, but including: no dialect capability admits the column, an unsupported collation,
a declared but non-LOB Oracle column, a column with no text policy at all, or Cube running its
text policy locally - plus every statement shape the search-recognition pass does not parse as one
isolated, row-preserving `LIKE '%needle%'` comparison over the column at all: a projection-only
read with no comparison to recognize, a sibling conjunct that could raise on an unredacted row, a
join, subquery or CTE, a renaming or shadowing alias, and any other shape that pass's own
structural checks decline. No variant, and no absence of one, names a column, pattern or
replacement, and nothing about the policy leaks beyond what is stated here.

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
rows than the conjunct selects is corrected. A Cube that answers fewer is not: the listed forms
are chosen so that equal text is equal under any collation. The native Cube cell checks equality
over a million-row ClickHouse-backed fixture, including duplicate text, NULL and hidden terms;
it does not establish comparison behaviour for every warehouse Cube can serve. This venue exercises
the transport and planner. Named-refusal classification is covered separately by the application
cell with the real planner and Cube adapter over a scripted transport; it is not a native served
caller proof. The planner keeps policy-column predicates local, including when
their pins name collations. The 10,000-row cap, the memory budget and
the deadline still apply to what comes back, so a filter that leaves more than 10,000 rows, or a
search whose only condition reads the policy column, still cannot answer.

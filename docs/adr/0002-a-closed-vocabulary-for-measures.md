---
title: A closed vocabulary for measures
description: Why a measure gained shapes and terms and a metric gained definitional filters, why the extensible axis is the term rather than the shape, and why a closed vocabulary is not the escape hatch the first-party models decision refused.
---

# A closed vocabulary for measures

Status: accepted. It widens the measure vocabulary of
[the first-party models decision](0001-first-party-semantic-models.md) and supersedes none of its
argument. That record is still right that no free-text SQL may reach a statement. This one is a
correction about which sentence in it was load-bearing.

Amended once, and the amendment is recorded rather than rewritten away: the first version of this
decision made a conditional count a *sibling* of a ratio, which left the seventh of the seven
metrics below unsayable. [Two levels, not three siblings](#two-levels-not-three-siblings) is the
correction and the rest of the record stands.

## Context

[The first-party models decision](0001-first-party-semantic-models.md) allowed a measure to be
exactly one aggregate over one declared column. That is safe, and it cannot express the metrics
people actually certify.

Measured rather than argued. Against the seven metrics of a real semantic layer, that vocabulary
could express **two**. These are the five it could not:

| Metric | What it means | What was missing |
| --- | --- | --- |
| `mrr` | `SUM(mrr_eur)`, over rows where `status = 'active'` | The aggregate fitted. The filter had no field at all |
| `active_subscribers` | `COUNT(DISTINCT subscription_key)`, over the same rows | The same absent field |
| `arpu` | `SUM(mrr_eur) / COUNT(DISTINCT customer_key)` | A ratio of two aggregates over two different columns |
| `churn_rate` | `COUNTIF(churned_in_month) / COUNT(DISTINCT subscription_key)` | A ratio, and a conditional count inside it |
| `avg_data_usage_gb` | `SUM(data_gb) / COUNT(DISTINCT subscription_key)` | A ratio again |

Two gaps, then, and each of them appears more than once: a predicate that is part of what the metric
means, with nowhere to write it, and an aggregate divided by another aggregate.

Neither gap is exotic. A revenue figure that means "active subscriptions only", and an average that
is one total over a count, are close to the first two things anybody asks a semantic layer for. So a
vocabulary that covers two definitions out of seven is not a conservative start. It routes the other
five to the path where something upstream must already have rendered dialect-correct SQL, which is
the precondition the first-party path exists because it is so often absent. Five sevenths of a real
catalog is not a corner either.

## Decision

**A closed vocabulary of measure terms and shapes, plus filters that belong to a metric's
definition.**

Two levels. A **term** is what one number is computed from; a **shape** says how terms combine. Each
is a variant the generator has an arm for, and each is named by a word the author writes rather than
inferred from which fields happen to be present:

| Term | What it says | Why it is not a special case of the other |
| --- | --- | --- |
| `aggregate` | One aggregate from the closed set, over one declared column: `SUM(amount_cents)` | It is the original vocabulary, unchanged |
| `count_if` | How many rows have this boolean column true | `COUNT(col)` counts non-null rows, so it counts the `false` ones too. Saying "how many are true" as a count of a boolean column is a wrong number that raises no error, and rendering it correctly differs per dialect |

| Shape | What it says | Why it is not a special case of the other |
| --- | --- | --- |
| `simple` | One term | It is the original vocabulary, unchanged |
| `ratio` | One term divided by another, over possibly different columns | `SUM(revenue) / COUNT(DISTINCT customer)` is not the mean of a column, and computing it as `avg(revenue)` is a different and wrong number |

Beside them, `required_filters`: predicates over the metric's own columns, with four operators -
`equals`, `not_equals`, `is_true`, `is_not_null`.

All seven metrics fit. Nothing in either list is a string that becomes SQL.

### Two levels, not three siblings

This is the amendment, and it is worth stating as a correction rather than as the design having
always been this: the first version of this decision listed `simple`, `count_if` and `ratio` as three
sibling **shapes**. Six of the seven metrics fitted. `churn_rate` did not, and the reason was
structural rather than incidental - a conditional count was a whole measure, so it could not be a
*half* of a ratio, and `COUNTIF(churned) / COUNT(DISTINCT subscription)` had every ingredient present
and nowhere to write it.

The error was factoring the vocabulary one level too high. What needs to be extensible is the leaf,
not the arithmetic around it: `count_if` is a kind of *number*, and `ratio` is a way of *combining*
numbers, and putting them side by side asserted that the two were the same kind of thing. The
alternative on that path is a shape per combination - `count_if`, then `count_if_over_count`, then
whatever the next numerator is - which is the same mistake repeated once per metric.

So a term is the axis that widens. A term added is one domain variant, one plan variant, one arm per
generator, and every shape gets it in both positions for free. The cost of the correction was
negative: three shape arms in each of the two generators collapsed into one two-arm term helper, and
the seventh metric became sayable.

**What it is deliberately NOT is a conditional variant of the aggregate set.** `Aggregate` is a pure
function-name set - `as_str` returns the word an author writes, and each generator has exactly one
`match` over it that maps a name to a call. Putting `count_if` inside it would make
`{ aggregate: count_if, column: x }` and the term's own spelling two catalog spellings of one
measure, compiling to two plan shapes and two digests for a definition that is identical, which is
the "two paths for one value" this record rejects everywhere else. Avoiding the two spellings by
giving the file format its own copy of the aggregate set with one extra word buys the other half of
the problem: a set that has to be kept in step with the domain's, whose failure mode is an aggregate
no document can express and nothing anywhere failing to say so.

### The property being defended was never "one aggregate"

It was **no free-text SQL**, and the distance between those two is the whole of this decision.

Every leaf in a measure is a column the model declares. Every operation is a variant with a
generator arm behind it. No string reaches a statement unexamined, and there is still no field an
author could write `sum(price * quantity)` into. That property is what makes a catalog reviewable,
and it holds identically over one shape, over two shapes and two terms, and over whatever the next
correction adds.

"One aggregate over one column" was a *means* to it, and a narrow one. Mistaking the means for the
end is the error this record corrects, and the cost of the error was not theoretical: the constraint
read as a security boundary, so widening it read as a concession, and the effect was to send five of
seven real definitions to a path that needed a renderer nobody had.

### A required filter is definitional, not requested

This is the half that reads like a convenience feature and is not one.

`mrr` *means* recurring revenue from active subscriptions. A statement that omits that predicate
returns a different number under the same certified name. That is the failure this repository exists
to prevent, arrived at by omission rather than by tampering - the more likely of the two, and much
the harder to notice, because nothing errors and the number looks plausible.

So a required filter is applied to every question about the metric, and **a caller cannot see it,
choose it or turn it off.** It is also not a dimension. A dimension is something a caller may group
by or filter on, and either of those is a way to ask a web-only metric for the store figure, so a
metric carrying a required filter over a column does not declare that column as a dimension. The
metric that answers "how does this split by channel" is the one that declares the dimension and
carries no required filter.

Two mechanisms keep that checkable rather than intended. The plan records where each predicate came
from, `Definition` or `Requested`, so a golden asserts the definitional ones are present in every
plan compiled for the metric. And a required filter's value is **bound as a parameter** rather
than written into the statement, even though it comes from the catalog rather than from a caller. Not
because the catalog is untrusted the way a caller is: because a value that is sometimes inlined and
sometimes bound is a generator with two paths, and the inlining path is the one that will eventually
be handed caller text.

### What is still unrepresentable, deliberately

The cost [the first-party models decision](0001-first-party-semantic-models.md) stated is unchanged,
and worth restating rather than assuming. An expression over two columns (`sum(price * quantity)`),
a window function and a three-table join still cannot be said. Each needs an expression language,
and an expression language here is exactly the escape hatch that record argues against: it turns
"the catalog describes the data" into "the catalog can say anything", and every check downstream then
has to reason about text somebody wrote. Those definitions belong on the other path, authored where
the expressiveness lives and taken as given.

Widening the vocabulary again is the same kind of change this one is. A third term or a third shape is
an entry
here. A field that holds an expression is a different entry, and the question it has to answer is
still not whether the expression is parsed, but what happens the first time one arrives that we
cannot parse.

## What does not change

Every guarantee the earlier record listed still has the same mechanism behind it. These are the ones
this decision could plausibly have broken:

| Guarantee | Still held by |
| --- | --- |
| A catalog holds no free-text SQL | Every shape, every term and every operator is an enum variant, with `deny_unknown_fields` at every depth. An unrecognised shape or term is an error naming what it found, and a misspelled key is a load failure rather than a field silently dropped. A term is read through a `try_from` struct rather than by an external tag, because the tag word and the field word would be the same word; `#[serde(untagged)]` is refused for the reason it always was - it reports "data did not match any variant", which names nothing |
| No value from a question reaches the statement as text | Unchanged, and now wider than the question: a definitional filter's value is bound too, so the generator has one path for a value rather than two |
| Every column a measure reads is a column its model declares | `Measure::columns()` reports all of them in one place and the consistency check walks it - now as `Term::column` mapped over `Measure::terms`, so a term cannot be reported by one shape and forgotten by another. A shape whose second column went unreported would let a metric name a column its model does not have, which is why the ratio shape reporting both is a test rather than a convention |
| A join cannot silently change a measure | A relationship declares its cardinality, and a dimension reached through one that may duplicate rows is refused. The ratio shape raises the stakes without changing the mechanism: fan-out corrupts a denominator as readily as a sum |
| Two result columns cannot share a label | Unchanged. A measure is projected under the metric's own name, and a dimension may not take that label or the time bucket's |
| A definition cannot change meaning between two invocations | The digest is over the canonical form of the parsed definitions, so a new shape or an added required filter moves it, and it travels with the answer |
| No SQL, table, predicate or row id on the tool surface | `Query` still has no field for one. This decision widens what a *catalog* may say and nothing about what a *caller* may say - and a required filter is the sharpest case of that, being a predicate the caller can neither express, see, nor remove |

One new check arrives with the decision, and it is mechanical rather than argued: **a definitional
predicate is in the plan for every question about its metric, and its value is in the parameter list
rather than in the statement.** Two goldens over the question corpus, both asserted against the plan
and the parameter list rather than against a string search of the SQL, and neither of them something
a caller can influence in either direction - which is the reason they have to be asserted here.

## Consequences

- A new term or a new shape is a code change: a domain variant, a plan variant, a generator arm, and
  a golden. It is not a YAML edit, and that is the intended cost. Either one decides what a certified
  number means, so it should get the review a code change gets rather than the review a configuration
  edit gets. A term is the cheaper of the two by construction, which is the point of the two levels:
  it is one arm rather than one arm per shape.
- `zero_denominator` on a ratio is required rather than defaulted, and it is a **word** rather than a
  boolean. "A rate over an empty period is null" and "a rate over an empty period is an error" are
  both defensible, the difference shows up only in the periods where it matters, and a definition
  should say which it means. A default would be one of us deciding it once, for every metric in every
  catalog - and `zero_safe: true` was barely better, because it recorded that somebody had thought
  about it without recording what they concluded. The two words are `yields_null` and `fails`, named
  after what a zero denominator does; the obvious spelling of the first is unavailable because in
  YAML a bare `null` is the null literal, and a document writing it would fail with a type error
  about a line that looks right.
- The generator acquired a per-dialect concern it did not have. A conditional count is `COUNTIF` in
  one dialect and `SUM(CASE WHEN .. THEN 1 ELSE 0 END)` in another; a null-yielding division is a
  `NULLIF` in one and a named safe-divide in another. That is exactly the kind of thing that must not
  appear in a catalog document, and it is one more reason the goldens are per dialect and read as a
  diff rather than typed.
- An anchor is compared as rendered text at the metric's coarsest declared grain, which means a
  float-valued metric is anchorable only when its value is one rounding of exactly-represented
  operands. `examples/single-player`'s `churn_rate` is anchored for that reason - both halves are
  counts - and `data_per_subscription`, a sum of decimals over a count, is not. Worth stating here
  because the ratio shape is what introduced float-valued metrics to this vocabulary.
- A metric's name now carries a claim that has to be checked. `web_revenue` reads as "revenue, and
  web" until you notice that a caller cannot ask it for the store figure. The origin recorded on a
  plan predicate is what turns that from a reading of the name into an assertion.

## Alternatives considered

**A free-text SQL expression, parsed at load, as the reference modelling languages do.** The most
expressive option, and drop-in compatible with the documents people already have, which is a real
argument and the reason it keeps coming back. Rejected for the reason the earlier record gives: it
puts a parser on the definition path, where a parse failure becomes a runtime refusal instead of a
review comment. Parsing at load rather than per request is better, and it is not enough - a catalog
that loads on one version and fails to parse on the next fails readiness for every metric in it, and
the expression is still text somebody wrote that every check downstream has to reason about.

**Keep one aggregate, and push everything else to the pinned-statement path.** No new vocabulary and
no new generator arms. Rejected because the five metrics above would then have to arrive as SQL
rendered upstream, and that requires an upstream renderer: the precondition the first-party path
exists because it is absent. So this alternative answers "we cannot express your metric" with
"install the thing you did not have". It also puts the split between the two authoring modes on how
complicated a metric is, rather than on where it was certified, which is the distinction that
actually means something.

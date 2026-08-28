---
kind: metric
name: revenue_per_churned_subscription
model: subscriptions
measure:
  ratio:
    numerator: { aggregate: sum, column: mrr_cents }
    denominator: { count_if: churned_in_month }
    zero_denominator: fails
time_column: month
grains: [month]
---
How much recurring revenue the month carried for each subscription it lost, in minor units.

The mirror of `churn_rate`, and that is the whole reason it is here. There a conditional count
is the NUMERATOR of a ratio; here it is the DENOMINATOR. Nothing about the vocabulary makes the
two positions different - a term is usable in either half of either shape - but a catalog that
only ever wrote one of them would demonstrate half of that claim and read as though it had
demonstrated all of it.

No status filter, and this is the argument `churn_rate` makes about its denominator turned
around. The rows the denominator counts are the terminated ones, so narrowing the metric to the
surviving state would take them out of the numerator while leaving them in the denominator: the
month's revenue would shrink and the count of what it lost would not. The numerator is therefore
the whole base, which is also what the name means - revenue the month carried, not revenue it
kept.

## `fails`, and why it is the only one here that says so

`zero_denominator: fails` says an empty denominator is a fault and not a figure, and it is true of
this definition: a month with no terminations does not have a larger revenue-per-termination, it
has none, and answering a number for it would answer a different question under this name.
`churn_rate` and the other two ratios all choose `yields_null`, which is right for a share - a
month with no subscriptions has no churn share - and wrong for this one.

**This is the only document in this repository that chooses the word, and the only one that puts a
conditional count in a denominator.** The other was the e-commerce catalog the golden suite used to
carry, which did both and which this directory replaced, so writing it down here is what stopped the
coverage leaving with it. A variant nothing executes looks covered because the enum has a test for
its spelling, which is exactly how `fails` came to answer the string `inf` under a certified metric
name; the correction was `Value::Real` refusing a non-finite cell, and what keeps that correction
honest is a question that reaches it. It arrived here before that catalog was retired rather than
being noticed missing afterwards.

The two questions beside it are that pair. June 2026 has three terminations, so it answers a real
figure. January 2026 has seventy rows and none of them terminated, which is the shape that reaches
the word: the period is not empty, there is a group to answer for, and the denominator is
nevertheless zero. That question FAILS rather than being refused, and the difference is the
governance one - a refusal is something a caller asked for and may not have, and this caller asked
something the definition permits.

## Why no anchor

Not the float argument the other ratios give, because it would not apply: both halves are exact
integers in a double, so this is one division of two exactly-represented values, and `churn_rate`
carries an anchor on precisely that reasoning.

The reason is `fails`. An anchor is one range an author wrote down, re-executed before the bundle
may answer anything, so anchoring this metric would mean picking a range where the denominator
happens not to be zero and making the readiness of the whole catalog depend on that continuing to
be true. The period this metric exists to demonstrate is the one where it has no figure, and an
anchor cannot be that period. What certifies it instead is the pair of questions above, whose rows
and whose failure are both pinned.

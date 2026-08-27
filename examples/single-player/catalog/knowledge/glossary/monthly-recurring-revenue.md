---
kind: glossary
term: monthly recurring revenue
synonyms:
  - MRR
  - recurring revenue
  - monatlicher Umsatz
  - wiederkehrender Umsatz
means: { metric: recurring_revenue }
---
The recurring part of what subscriptions bill in a month, and in this catalog it is always the
ACTIVE figure: `recurring_revenue` carries a definitional filter on the subscription's status, so
there is no way to ask it for a total that includes the subscriptions the month lost.

No metric here REPORTS that wider figure. The nearest thing is the numerator of
`revenue_per_churned_subscription`, which is the whole base and is never reported on its own - so a
question about revenue including the terminated subscriptions is one to decline rather than to
assemble out of parts.

Reported in minor units. A June figure of 202121 is 2021.21 in currency, and the caveat on the
metric says so where whoever is about to quote it will see it.

"MRR" is the abbreviation people say out loud. The two German phrases are what the same question
looks like when it arrives in German, which here it does about half the time.

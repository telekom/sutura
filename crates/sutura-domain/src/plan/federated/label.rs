//! The label namespace a question cannot reach, and the one function that assigns it.

use crate::federation::Federation;

/// The one definition of what a carried leaf is projected under.
///
/// The splitter and the combiner both call this, so the column the combiner reads a leaf from and
/// the label the splitter projected it under cannot disagree - there is no second copy of the rule.
///
/// **Position, and nothing else.** A ratio of two sums - `sum(a) / sum(b)` - is one aggregating
/// function twice, so naming by aggregate would give both leaves one label and a combine that
/// divides a column by itself. Position cannot collide, and it is all a leg needs: a leg carries one
/// metric, so the metric's name distinguishes nothing inside it. The answer's measure comes back
/// under the metric's own certified name, which `FederatedPlan`'s `measure_label` holds.
pub fn labels(federation: &Federation) -> Vec<InternalLabel> {
    (0..federation.carried().len()).map(InternalLabel::Leaf).collect()
}

/// A column label the splitter and the combiner agree on, in a namespace no question can name.
///
/// **Why a type rather than a convention.** A federated fact leg's result carries public dimension
/// labels beside internal ones - the column the legs are joined on, and one per carried leaf of the
/// measure. The splitter spelled the first with the physical remote join COLUMN's text and the
/// second as `metric__{n}`, and both of those are legal identifiers: a metric with a legal dimension
/// named `customer_key`, backed by a different column, projected two fact columns under one label
/// and the combiner refused the answer it could not disambiguate. A dimension is free to be named
/// anything [`DimensionName`](crate::model::DimensionName) accepts, so the internal labels are what
/// has to move - keeping a legal question legal is the constraint, not a naming rule for authors.
///
/// **What makes the two namespaces disjoint, and it is one character.** Every rendering here starts
/// with a digit, which `crate::model`'s identifier parser refuses as a FIRST character - a leading
/// digit is legal in some dialects and not others, so it was already refused for portability. No
/// `DimensionName`, `MetricName`, `ColumnName` or `TableName` can therefore spell one of these, for
/// any spelling and any length. `federated/tests.rs` asserts that by parsing every constructible
/// label as each of those names and requiring the refusal, rather than leaving it to this paragraph.
///
/// **The disjointness has a second half, and it is the one a parse check cannot answer: every target
/// has to ACCEPT the label as a quoted alias.** The character this scheme is built on is the one
/// [`BadFirstCharacter`](crate::model::InvalidIdentifier::BadFirstCharacter) refuses because it is
/// *"legal in some dialects and not others, so accepting it would make a model portable by luck"* -
/// so the same sentence that justifies the namespace is the reason to doubt it. Two different
/// questions live under it: whether a target accepts a digit-leading string as an **identifier**,
/// which is what that refusal is about and where the targets do differ, and whether it accepts one
/// as a **quoted select alias**, which is the only position this scheme puts it in.
/// `sutura_sql::generate`'s `aliased` quotes every alias or refuses to render, and `GROUP BY` and
/// `ORDER BY` carry the EXPRESSION rather than the alias, so a leg's statement spells an internal
/// label in exactly one place.
///
/// What is established, and by what:
///
/// | Claim | Venue | Mechanism |
/// | --- | --- | --- |
/// | the four dialects' **parsers** accept the alias quoted | `polyglot_sql`, in-process | `every_leg_statement_parses_here` in `crates/sutura-app/tests/golden/legs.rs`, whose own doc states the limit: it parses and stops, and a failure at the service *"is otherwise only discoverable by running it"* |
/// | `BigQuery` **executes** it and answers under that field name | the real service | measured by hand 2026-09-05, and held from now on by `an_internal_label_survives_as_an_alias_at_the_service` in `crates/sutura-exec-bigquery/tests/acceptance.rs`, which the `bigquery-acceptance` job runs |
/// | `DuckDB` 1.5.5 executes it | a live engine | measured by hand in review, 2026-09-05: `SELECT 1 AS "0_link", 2 AS "0_leaf_0"` answers both columns under those names. Not held by a test - the vehicle is dev-only and no cell asks this |
///
/// `BigQuery` is the target that had to be asked rather than reasoned about, because it is the one
/// whose documentation restricts a **column name** to a letter or an underscore first. Asked twice on
/// 2026-09-05, as a dry run and as a real job each time: bare aliases
/// (``SELECT 1 AS `0_link`, 2 AS `0_leaf_0`, 3 AS `0_leaf_26` ``), and then a statement in the shape
/// the generator actually emits - a `CAST(DATE_TRUNC(..) AS DATE)` bucket and a `sum(..)` measure
/// aliased into this namespace, with `GROUP BY` and `ORDER BY .. NULLS LAST` over the expressions.
/// Both are accepted and both come back with the field names the statement asked for. So the
/// documented restriction is on a **declared column** and not on a quoted alias.
///
/// **The limit, next to the claim:** `Postgres` and `ClickHouse` are asserted at the parser only.
/// Neither has an execution venue for a LEG - each leaves `EXECUTES_LEGS` at its default `false` -
/// so what stands for them is a quoted-identifier argument rather than a run. Read the row above
/// for what each one is worth.
///
/// **The reason that sentence changed rather than the claim:** it used to say *the whole federated
/// path is gated by a defaulted-`false` `EXECUTES_LEGS` that only the dev-only `DuckDB` vehicle
/// sets*, which stopped being true when the engine declared the constant and a published build
/// began answering two sources. The limit for these two dialects is unaffected - it never rested on
/// the path being gated, only on neither having a venue.
///
/// **Every value is valid, so there is nothing to check.** A `usize` position out of a plan's leaf
/// range is a wiring defect the combiner reports as a missing column, not a label this type could
/// have refused - which is why the variants carry their data in the open and no constructor is
/// fallible. What the type buys is that the TEXT can only come from here.
///
/// **The other half of the namespace is held elsewhere, and this is the whole of it in one place.**
/// The labels a result carries besides these are public: the answer's dimension labels, the time
/// bucket's [`TIME_BUCKET_LABEL`](crate::catalog::TIME_BUCKET_LABEL) and the measure's metric name.
/// Those are held against each other at load by
/// [`Definitions::assemble`](crate::catalog::Definitions::assemble) -
/// `DimensionShadowsTimeBucket`, `DimensionShadowsMeasure`, `TwoDimensionsOneLabel` and
/// `LabelShadowsTable`, each case-folded to the coarsest dialect rule. So a public label collides
/// with another public label at load, and cannot collide with an internal one at all. **The limit:**
/// nothing compares the two halves, because a leading digit makes the comparison unnecessary - which
/// is the property the test asserts, and the thing to re-establish if this spelling ever changes.
///
/// **Both halves are now held by a type, and that is `telekom/sutura#337`.** This half is
/// [`InternalLabel`]; the public half is [`ResultLabel`](crate::plan::ResultLabel), whose only
/// constructors take a `DimensionName`, a `MetricName`, [`InternalLabel`] or nothing at all - so a
/// computed string is not a label a plan can carry, and the derivation
/// `sutura_semantic::plan::federated_plan` used to be trusted to keep is the constructor's shape
/// instead. What that changes about the paragraph above: the two namespaces are still disjoint
/// *because* of the leading digit, and what the types add is that no producer can put a value in
/// both. **The limit, next to the claim:** what a `ResultLabel` records is that the text came from
/// something already parsed, never WHICH of the four constructors produced it - so a value built by
/// [`ResultLabel::internal`](crate::plan::ResultLabel::internal) is accepted anywhere a label is
/// taken, the bucket's position included. Nothing here reads the provenance back, because nothing
/// needs to: the disjointness is the leading digit.
///
/// **Length is bounded by construction, which the scheme it replaces was not.** The identifier limit
/// is 63 characters because that is the tightest among the data systems targeted, and it is a
/// *silent* limit there: a longer alias is truncated rather than rejected, so two distinct leaf
/// columns become one. `metric__{n}` over a 63-character metric name is 66 characters, so the old
/// scheme could produce exactly that. Nothing here reads a metric's name, and the widest label a
/// `usize` can index is 27 characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InternalLabel {
    /// The column the two legs are joined on, in either leg's result.
    Link,
    /// One carried leaf of the measure, by its position in carried order.
    Leaf(usize),
}

/// The label the link column carries in both legs' results.
const LINK: &str = "0_link";

/// What one carried leaf's label carries before its position.
const LEAF: &str = "0_leaf_";

impl InternalLabel {
    /// The text this label carries in a leg's result.
    ///
    /// The one place the reserved namespace is spelled. A caller that needs it as a column name
    /// takes it from here, so no call site holds a second copy of the spelling.
    #[inline]
    pub fn label(self) -> String {
        match self {
            Self::Link => String::from(LINK),
            Self::Leaf(position) => format!("{LEAF}{position}"),
        }
    }
}

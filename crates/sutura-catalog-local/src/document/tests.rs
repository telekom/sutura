//! Tests for the frontmatter documents, moved out of `document.rs` rather than deleted: the
//! thousand-line cap under `crates/` is held by a gate, so the block became a file.

use std::collections::BTreeSet;

use super::{Description, DocumentKind, InvalidMetricDocument, InvalidModelDocument, KindProbe, MetricDoc, ModelDoc};
use sutura_domain::catalog::{
    Audience, AudienceGrant, DimensionValue, InconsistentDefinitions, InvalidDimensionValue, InvalidViaChain,
};
use sutura_domain::expression::InvalidComputation;
use sutura_domain::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
use sutura_domain::model::AudienceId;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, RelationshipName};

fn metric_doc(yaml: &str) -> Result<MetricDoc, serde_norway::Error> {
    serde_norway::from_str(yaml)
}

fn description(raw: &str) -> Description {
    Description::parse(raw).expect("a test description is a description")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn value(raw: &str) -> DimensionValue {
    DimensionValue::parse(raw).expect("a test value is a value")
}

fn aggregated(aggregate: Aggregate, raw: &str) -> Term {
    Term::Aggregate(AggregatedColumn::new(aggregate, column(raw)))
}

const MINIMAL_METRIC: &str = "
kind: metric
name: revenue
model: orders
measure:
  simple: { aggregate: sum, column: amount_cents }
time_column: order_date
grains: [month]
audience: open
";

/// The minimal document with a different measure block substituted in.
///
/// `measure` is written the way it appears in a file, already indented under `measure:` and
/// ending in a newline, so each measure test below reads as one statement about one shape
/// instead of a second copy of every other field.
fn metric_measuring(measure: &str) -> String {
    format!(
        "kind: metric\nname: revenue\nmodel: orders\nmeasure:\n{measure}time_column: order_date\ngrains: [month]\naudience: open\n"
    )
}

#[test]
fn a_minimal_metric_document_parses() {
    let doc = metric_doc(MINIMAL_METRIC).expect("a minimal metric is a metric");
    let metric = doc
        .into_domain(description("Net revenue."))
        .expect("no dimensions cannot be duplicated");
    assert_eq!(metric.name(), &MetricName::parse("revenue").expect("a name"));
    assert_eq!(
        metric.measure(),
        Some(&Measure::Simple(aggregated(Aggregate::Sum, "amount_cents")))
    );
    assert!(metric.supports_grain(Grain::Month));
    assert!(!metric.supports_grain(Grain::Day));
    assert_eq!(metric.description(), "Net revenue.");
    assert!(metric.anchor().is_none());
}

/// `docs/adr/0028` step 2: `audience:` was an unknown field before this. Red on a tree with no
/// `audience` field on `MetricDoc`, green once it exists and parses.
#[test]
fn a_metric_document_declaring_its_audience_loads_and_carries_it() {
    let doc = metric_doc(MINIMAL_METRIC).expect("a document declaring `audience: open` loads");
    let metric = doc
        .into_domain(description("Net revenue."))
        .expect("no dimensions to duplicate");
    assert_eq!(metric.audience(), &Audience::Open);
}

/// The `restricted:` spelling needs `singleton_map` on the field, unlike the bare-scalar
/// `open` - this is the one test that asks the REAL `MetricDoc`, not a proxy wrapper.
#[test]
fn a_metric_document_may_restrict_its_audience() {
    let yaml = format!("{MINIMAL_METRIC}audience:\n  restricted: [finance]\n").replace("audience: open\n", "");
    let metric = metric_doc(&yaml)
        .expect("a restricted audience is a metric field")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    let grant = AudienceGrant::parse(BTreeSet::from([AudienceId::parse("finance").expect("a test id")])).expect("grants");
    assert_eq!(metric.audience(), &Audience::Restricted(grant));
}

/// The refusal half: no default means absence is a parse error, never a silent `open`.
#[test]
fn a_metric_document_with_no_audience_key_is_refused_by_name() {
    let yaml = "kind: metric\nname: revenue\nmodel: orders\nmeasure:\n  \
                simple: { aggregate: sum, column: amount_cents }\ntime_column: order_date\ngrains: [month]\n";
    let err = metric_doc(yaml).expect_err("a metric with no audience declaration is not a metric document");
    assert!(err.to_string().contains("audience"), "{err}");
}

#[test]
fn authored_sql_is_the_other_computation_a_metric_document_can_choose() {
    let yaml = "
kind: metric
name: spread
model: orders
authored_sql:
  portable: MAX(amount_cents) - MIN(amount_cents)
time_column: order_date
grains: [month]
audience: open
";
    let metric = metric_doc(yaml)
        .expect("authored_sql is a metric field")
        .into_domain(Description::default())
        .expect("exactly one computation is present");
    assert_eq!(metric.computation().kind(), "authored_sql");
    assert!(metric.measure().is_none());
}

#[test]
fn a_metric_document_must_choose_exactly_one_computation() {
    let without = "kind: metric\nname: revenue\nmodel: orders\ntime_column: order_date\ngrains: [month]\naudience: open\n";
    assert_eq!(
        metric_doc(without)
            .expect("the document shape is readable")
            .into_domain(Description::default()),
        Err(InvalidMetricDocument::Computation(InvalidComputation::Nothing))
    );

    let both = format!("{MINIMAL_METRIC}authored_sql:\n  portable: SUM(amount_cents)\n");
    assert_eq!(
        metric_doc(&both)
            .expect("both keys are individually readable")
            .into_domain(Description::default()),
        Err(InvalidMetricDocument::Computation(InvalidComputation::Both))
    );
}

#[test]
fn a_model_document_may_name_a_table_in_another_dataset_or_project() {
    // The authoring surface for a multi-project estate, and there is no second key: the same
    // `table:` value carries one part, two or three. That is what keeps every document already on
    // disk unchanged - the bare form is asserted here beside the qualified ones so the
    // compatibility claim is in the same test as the feature.
    let doc = |table: &str| {
        serde_norway::from_str::<ModelDoc>(&format!(
            "
kind: model
name: orders
source: local
table: {table}
columns: [amount_cents]
"
        ))
    };
    for path in ["orders", "sales.orders", "analytics-prod.sales.orders"] {
        let model = doc(path)
            .unwrap_or_else(|e| panic!("{path} is a table a document may name: {e}"))
            .into_domain(description("Orders."))
            .expect("no primary key here to be inconsistent");
        assert_eq!(model.table().to_string(), path);
        assert_eq!(
            model.table_name().as_str(),
            "orders",
            "a column is qualified by the last part"
        );
    }

    // And what a document may NOT write. A quote inside a part would end the quoting the
    // generator relies on; a fourth part names nothing.
    drop(doc("a.b.c.d").expect_err("nothing names a table four deep"));
    drop(doc("'sales'.orders").expect_err("a quote is not part of a name"));
}

#[test]
fn a_column_s_long_form_carries_a_type_a_description_and_nullability_and_may_mix_with_the_short_form() {
    let yaml = "
kind: model
name: orders
source: local
table: orders
columns:
  - name: amount_cents
    type: NUMERIC
    description: The order total, in minor units.
    nullable: false
  - order_date
primary_key: [order_date]
";
    let model = serde_norway::from_str::<ModelDoc>(yaml)
        .expect("a mixed short/long column list parses")
        .into_domain(description("Orders."))
        .expect("order_date is one of the declared columns");
    let amount = model.column(&column("amount_cents")).expect("amount_cents is declared");
    assert_eq!(
        amount.data_type().map(sutura_domain::catalog::ColumnType::as_str),
        Some("NUMERIC")
    );
    assert_eq!(amount.description(), "The order total, in minor units.");
    assert_eq!(amount.nullable(), Some(false));
    // The short form still writes a bare column, with none of the three.
    let date = model.column(&column("order_date")).expect("order_date is declared");
    assert_eq!(date.data_type(), None);
    assert_eq!(date.description(), "");
    assert_eq!(date.nullable(), None);
    assert_eq!(model.primary_key(), &BTreeSet::from([column("order_date")]));
}

/// A `type:` this crate cannot represent - here, over `MAX_COLUMN_TYPE_CHARS` - is dropped, not
/// refused: the document still loads and the column carries no type.
#[test]
fn a_column_type_too_long_to_represent_is_dropped_rather_than_refusing_the_load() {
    let long_type = format!("STRUCT<{}z STRING>", "a STRING, ".repeat(80));
    assert!(long_type.len() > sutura_domain::catalog::MAX_COLUMN_TYPE_CHARS);
    let yaml = format!(
        "
kind: model
name: orders
source: local
table: orders
columns:
  - name: amount_cents
    type: \"{long_type}\"
"
    );
    let model = serde_norway::from_str::<ModelDoc>(&yaml)
        .expect("a long type still parses as text")
        .into_domain(description("Orders."))
        .expect("dropping an unrepresentable type is not a refusal");
    assert_eq!(
        model
            .column(&column("amount_cents"))
            .expect("amount_cents is declared")
            .data_type(),
        None
    );
}

#[test]
fn a_column_s_long_form_still_refuses_an_unknown_key() {
    // `untagged` cannot name WHICH key was wrong - `ColumnEntryDoc`'s own doc names this the same
    // limit `AnchorLiteral` already states: every variant failed, and the message says only that a
    // mapping matched neither the bare-name form nor the long form. What matters here is that it
    // refuses at all rather than silently dropping `typo`.
    let yaml = "
kind: model
name: orders
source: local
table: orders
columns:
  - name: amount_cents
    typo: NUMERIC
";
    drop(serde_norway::from_str::<ModelDoc>(yaml).expect_err("a misspelled long-form key matches no column shape"));
}

#[test]
fn a_primary_key_naming_a_column_the_model_does_not_declare_is_refused() {
    // Checked at construction (`Model::with_primary_key`), against this model's own columns only -
    // there is no map of models here for a key to be checked against the wrong one of.
    let yaml = "
kind: model
name: orders
source: local
table: orders
columns: [amount_cents]
primary_key: [order_id]
";
    let doc = serde_norway::from_str::<ModelDoc>(yaml).expect("the document itself parses");
    let err = doc
        .into_domain(description("Orders."))
        .expect_err("order_id is not one of the declared columns");
    assert_eq!(
        err,
        InvalidModelDocument::PrimaryKey(Box::new(
            sutura_domain::catalog::InconsistentDefinitions::UnknownPrimaryKeyColumn {
                model: sutura_domain::model::ModelName::parse("orders").expect("a test model is a model"),
                column: column("order_id"),
            }
        ))
    );
}

#[test]
fn a_misspelled_key_is_an_error_and_not_a_dropped_field() {
    // The bug this prevents, and the reason `deny_unknown_fields` is on every shape here:
    // `colums:` loads a model with no columns at all. It then passes every consistency check
    // that only looks at what is declared, and refuses every question for a reason that says
    // nothing about a typo three directories away.
    let yaml = "
kind: model
name: orders
source: local
table: orders
colums: [amount_cents]
";
    let err = serde_norway::from_str::<ModelDoc>(yaml).expect_err("a misspelled key is not a field");
    assert!(err.to_string().contains("colums"), "{err}");
}

#[test]
fn a_document_carrying_sql_is_refused_by_name() {
    // The load-bearing half of the first-party-models decision: there is no field a statement
    // fits in, so an attempt to add one to a catalog document fails naming the field rather
    // than being quietly ignored.
    let yaml = format!("{MINIMAL_METRIC}sql: \"SELECT 1\"\n");
    let err = metric_doc(&yaml).expect_err("sql is not a metric field");
    assert!(err.to_string().contains("sql"), "{err}");
}

#[test]
fn a_free_text_expression_is_refused_by_name() {
    // The same check from the other direction: the reference modelling languages spell a
    // measure `expression: sum(amount)`, and somebody will try it here.
    let yaml = format!("{MINIMAL_METRIC}expression: \"sum(amount_cents)\"\n");
    let err = metric_doc(&yaml).expect_err("expression is not a metric field");
    assert!(err.to_string().contains("expression"), "{err}");
}

#[test]
fn an_unknown_aggregate_names_the_closed_set() {
    // A measure is an aggregate from a closed set. `median` is a reasonable thing to want and
    // this is what says so, rather than generating SQL for a function nobody checked.
    let yaml = MINIMAL_METRIC.replace("aggregate: sum", "aggregate: median");
    let err = metric_doc(&yaml).expect_err("median is not one of the aggregates");
    assert!(err.to_string().contains("median"), "{err}");
}

#[test]
fn a_count_if_term_has_a_word_of_its_own_in_the_format() {
    // The number this prevents: "how many rows are true" written as a count of a boolean column.
    // `COUNT(col)` counts non-null rows, so it counts the `false` ones too, and the wrong answer
    // arrives with no error anywhere on the way. A format with no word for the thing that was
    // meant is a format that pushes the author towards the spelling that silently lies.
    let yaml = metric_measuring("  simple: { count_if: churned_in_month }\n");
    let metric = metric_doc(&yaml)
        .expect("count_if is a term")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    assert_eq!(
        metric.measure(),
        Some(&Measure::Simple(Term::CountIf {
            column: column("churned_in_month"),
            model: None,
        }))
    );
}

#[test]
fn a_ratio_measure_keeps_its_two_terms_apart() {
    // The number this prevents: a ratio collapsed into `avg`. `sum(mrr) / count(distinct
    // customer)` is not the mean of a column, and a document that could only say `avg` would
    // send whoever wanted the real figure to a hand-written statement outside this catalog.
    let yaml = metric_measuring(concat!(
        "  ratio:\n",
        "    numerator: { aggregate: sum, column: mrr_eur }\n",
        "    denominator: { aggregate: count_distinct, column: customer_key }\n",
        "    zero_denominator: yields_null\n",
    ));
    let metric = metric_doc(&yaml)
        .expect("a ratio is a measure shape")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    assert_eq!(
        metric.measure(),
        Some(&Measure::Ratio {
            numerator: aggregated(Aggregate::Sum, "mrr_eur"),
            denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
            zero_denominator: ZeroDenominator::Null,
        })
    );
}

#[test]
fn a_conditional_count_can_be_written_as_a_ratio_numerator() {
    // The document that could not be written before, and the reason the vocabulary was
    // refactored: a churn rate is a conditional count over a distinct count. While `count_if`
    // was a sibling of `ratio` rather than a term inside one, this file had to be split into two
    // certified metrics and a division somebody did by hand.
    let yaml = metric_measuring(concat!(
        "  ratio:\n",
        "    numerator: { count_if: churned_in_month }\n",
        "    denominator: { aggregate: count_distinct, column: subscription_key }\n",
        "    zero_denominator: yields_null\n",
    ));
    let metric = metric_doc(&yaml)
        .expect("a conditional count is a term like any other")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    assert_eq!(
        metric.measure(),
        Some(&Measure::Ratio {
            numerator: Term::CountIf {
                column: column("churned_in_month"),
                model: None,
            },
            denominator: aggregated(Aggregate::CountDistinct, "subscription_key"),
            zero_denominator: ZeroDenominator::Null,
        })
    );
}

#[test]
fn an_unknown_measure_shape_is_refused_by_name() {
    // What external tagging buys, and the reason the measure is not inferred from which fields
    // are present: an unrecognised shape fails naming the word that was written. Inferred from
    // fields, the same document would fail with "data did not match any variant", which names
    // nothing and sends the reader to guess which of two shapes was nearly right.
    let yaml = metric_measuring("  median: { column: amount_cents }\n");
    let err = metric_doc(&yaml).expect_err("median is not a measure shape");
    assert!(err.to_string().contains("median"), "{err}");
}

#[test]
fn an_unknown_term_is_refused_by_name_too() {
    // A term is flat rather than externally tagged, so `deny_unknown_fields` on the struct
    // behind it is what keeps the same property one level down: an unrecognised term names the
    // word that was written and lists the words that exist, instead of "data did not match any
    // variant" - which is exactly what `#[serde(untagged)]` on the term would have produced.
    let yaml = metric_measuring("  simple: { median: amount_cents }\n");
    let err = metric_doc(&yaml).expect_err("median is not a term");
    let message = err.to_string();
    assert!(message.contains("median"), "{message}");
    assert!(message.contains("count_if"), "{message}");
}

#[test]
fn half_a_term_is_refused_by_what_is_missing() {
    // The one cost of the flat form, and it is paid in a sentence rather than in a guess. An
    // aggregate with no column and a column with no aggregate are both documents somebody meant
    // something by, so each is named for what it is short of.
    let no_column = metric_measuring("  simple: { aggregate: sum }\n");
    let err = metric_doc(&no_column).expect_err("an aggregate with no column is not a term");
    assert!(err.to_string().contains("no `column`"), "{err}");

    let no_aggregate = metric_measuring("  simple: { column: amount_cents }\n");
    let err = metric_doc(&no_aggregate).expect_err("a column with no aggregate is not a term");
    assert!(err.to_string().contains("no `aggregate`"), "{err}");

    // Both at once is refused rather than resolved by precedence: the document means one of
    // them, and picking one would certify a number nobody asked for.
    let both = metric_measuring("  simple: { aggregate: sum, column: amount_cents, count_if: paid }\n");
    let err = metric_doc(&both).expect_err("two terms in one mapping is not a term");
    assert!(err.to_string().contains("a term is one thing"), "{err}");
}

#[test]
fn a_ratio_without_zero_denominator_is_an_error_and_not_a_default() {
    // Both behaviours are defensible: a rate over a period with no rows is arguably null and
    // arguably a failure. So a default here would pick one on the author's behalf and the
    // document would not record which. The failure that hides behind it is a metric that reads
    // as null in one deployment and refuses in the next, with identical definitions on disk.
    let yaml = metric_measuring(concat!(
        "  ratio:\n",
        "    numerator: { aggregate: sum, column: mrr_eur }\n",
        "    denominator: { aggregate: count_distinct, column: customer_key }\n",
    ));
    let err = metric_doc(&yaml).expect_err("zero_denominator has no default to fall back on");
    assert!(err.to_string().contains("zero_denominator"), "{err}");
}

#[test]
fn the_null_case_is_spelled_yields_null_because_yaml_owns_the_word_null() {
    // Why the two words are not `null` and `fail`. In YAML a bare `null` is the null literal, so
    // the most natural spelling in the whole vocabulary would hand the deserializer a unit value
    // and fail with a type error about a line that looks right. Named after what a zero
    // denominator does instead, so the field and its value read as one sentence and neither of
    // them can collide with a scalar YAML resolves itself.
    let ratio = |zero_denominator: &str| {
        metric_measuring(&format!(
            concat!(
                "  ratio:\n",
                "    numerator: {{ aggregate: sum, column: mrr_eur }}\n",
                "    denominator: {{ aggregate: count_distinct, column: customer_key }}\n",
                "    zero_denominator: {}\n",
            ),
            zero_denominator
        ))
    };
    for (word, expected) in [("yields_null", ZeroDenominator::Null), ("fails", ZeroDenominator::Fail)] {
        let metric = metric_doc(&ratio(word))
            .expect("both words are words")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        assert_eq!(
            metric.measure(),
            Some(&Measure::Ratio {
                numerator: aggregated(Aggregate::Sum, "mrr_eur"),
                denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
                zero_denominator: expected,
            })
        );
    }
    // And the spelling that would have been the trap: refused, rather than read as the variant
    // whose Rust name it happens to match.
    let err = metric_doc(&ratio("null")).expect_err("a YAML null is not a zero-denominator behaviour");
    assert!(err.to_string().contains("zero_denominator"), "{err}");
}

#[test]
fn a_misspelled_key_inside_a_term_is_refused_too() {
    // `deny_unknown_fields` has to hold at every depth, not only on the outer document. Without
    // it on the term, `agregate:` is dropped and the error becomes one about a term that says
    // nothing, printed next to a line that plainly names an aggregate - which sends the reader
    // looking for a field they can see rather than at the typo in it.
    let yaml = metric_measuring("  simple: { agregate: sum, column: amount_cents }\n");
    let err = metric_doc(&yaml).expect_err("a misspelled key nested in a term is not a field");
    assert!(err.to_string().contains("agregate"), "{err}");
}

#[test]
fn every_required_filter_operator_is_written_as_a_named_operator() {
    // The four predicates a definition may carry, pinned as a set. A filter whose operator did
    // not parse would drop out of the metric, and a metric that quietly lost its predicate
    // answers every question with a larger number under the same certified name.
    let filters = concat!(
        "required_filters:\n",
        "  - equals: { column: channel, value: web }\n",
        "  - not_equals: { column: channel, value: store }\n",
        "  - is_true: { column: is_paid }\n",
        "  - is_not_null: { column: shipped_at }\n",
    );
    let metric = metric_doc(&format!("{MINIMAL_METRIC}{filters}"))
        .expect("all four operators are operators")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    let expected = vec![
        RequiredFilter::Equals {
            column: column("channel"),
            value: value("web"),
        },
        RequiredFilter::NotEquals {
            column: column("channel"),
            value: value("store"),
        },
        RequiredFilter::IsTrue {
            column: column("is_paid"),
        },
        RequiredFilter::IsNotNull {
            column: column("shipped_at"),
        },
    ];
    assert_eq!(metric.required_filters(), expected.as_slice());
}

#[test]
fn an_unknown_required_filter_operator_is_refused_by_name() {
    // The same closed-vocabulary check as for measure shapes. `greater_than` is a reasonable
    // thing to want, and this is what says so, rather than a predicate nobody generated an arm
    // for being dropped from a definition that claims to carry it.
    let yaml = format!("{MINIMAL_METRIC}required_filters:\n  - greater_than: {{ column: amount_cents, value: 0 }}\n");
    let err = metric_doc(&yaml).expect_err("greater_than is not one of the four operators");
    assert!(err.to_string().contains("greater_than"), "{err}");
}

#[test]
fn a_required_filter_value_is_parsed_by_the_document_it_arrives_in() {
    // The wired path for the third authored string. A definitional filter's value used to be a
    // `String` with `deny_unknown_fields` around it and no character rule inside it, so a
    // right-to-left override in this line rendered as `status = "active"` in `sutura
    // definitions` and bound something else - the finding already closed for an authored SQL
    // fragment and a glossary phrase, at a channel that still had it. `DimensionValue`'s
    // `try_from` is what makes the frontmatter reader the enforcement point.
    for (value, needle) in [
        ("act\u{202E}ive", "invisible"),
        ("\"  active\"", "spacing"),
        ("\"\"", "empty"),
    ] {
        let yaml = format!("{MINIMAL_METRIC}required_filters:\n  - equals: {{ column: status, value: {value} }}\n");
        let err = metric_doc(&yaml).expect_err("a value that is not a dimension value is not one");
        assert!(err.to_string().contains(needle), "{value}: {err}");
    }
}

#[test]
fn required_filters_default_to_empty_when_the_key_is_absent() {
    // Most metrics carry no definitional predicate, so the key is optional. The direction that
    // must not be confused is the other one: absent has to mean "no predicate", never "not
    // decided yet", because a filter list that could be unset is a filter list something
    // downstream would eventually treat as a hint.
    let metric = metric_doc(MINIMAL_METRIC)
        .expect("a minimal metric is a metric")
        .into_domain(Description::default())
        .expect("no dimensions to duplicate");
    assert!(metric.required_filters().is_empty(), "a minimal metric demands no filter");
}

/// A dimension declared twice is refused, **by the domain and not by this adapter**.
///
/// Why dimensions are a list and not a map: a YAML mapping with a repeated key keeps the last
/// value silently, so the metric would load with the second definition and the author would have
/// no way to tell which one is live.
///
/// The refusal used to be this crate's own `InvalidMetricDocument::DuplicateDimension`, and that
/// is #266's D4: the `DataHub` adapter read a sequence too and collected it into a map, so the same
/// content became two different `Definitions` depending on which adapter loaded it. What this
/// asserts now is the domain's variant coming back through the transparent wrap, which is the
/// same value the `DataHub` adapter's sibling test asserts.
#[test]
fn a_dimension_declared_twice_is_refused_rather_than_deduplicated() {
    let yaml = format!(
        "{MINIMAL_METRIC}dimensions:\n  - name: region\n    column: region_code\n  - name: region\n    column: other_code\n"
    );
    let doc = metric_doc(&yaml).expect("two list entries are valid YAML");
    assert_eq!(
        doc.into_domain(Description::default()).unwrap_err(),
        InvalidMetricDocument::Inconsistent(Box::new(InconsistentDefinitions::DuplicateDimension {
            metric: MetricName::parse("revenue").expect("a name"),
            dimension: DimensionName::parse("region").expect("a name"),
        }))
    );
}

/// A `via:` chain arrives as the declared order, and a single name stays a one-hop chain.
///
/// The scalar-or-seq shape is the format's only addition: an author who chains two relationships
/// writes a sequence, and an author who writes one name gets exactly the document every existing
/// catalog file already has - byte-identical, so no catalog rotates for this feature.
#[test]
fn a_via_chain_arrives_in_declared_order_and_a_single_name_is_one_hop() {
    let yaml = format!(
        "{MINIMAL_METRIC}dimensions:\n  - name: region\n    column: code\n    via: [orders_customer, customers_regions]\n"
    );
    let metric = metric_doc(&yaml)
        .expect("a sequence of names is valid YAML")
        .into_domain(Description::default())
        .expect("the chain is not checked here");
    let dimension = metric
        .dimension(&DimensionName::parse("region").expect("a name"))
        .expect("declared");
    assert_eq!(
        dimension.via(),
        Some(
            &[
                RelationshipName::parse("orders_customer").expect("a name"),
                RelationshipName::parse("customers_regions").expect("a name")
            ][..]
        )
    );

    let yaml = format!("{MINIMAL_METRIC}dimensions:\n  - name: region\n    column: id\n    via: orders_customer\n");
    let metric = metric_doc(&yaml)
        .expect("a single name is valid YAML")
        .into_domain(Description::default())
        .expect("the chain is not checked here");
    let dimension = metric
        .dimension(&DimensionName::parse("region").expect("a name"))
        .expect("declared");
    assert_eq!(
        dimension.via(),
        Some(&[RelationshipName::parse("orders_customer").expect("a name")][..])
    );
}

/// The empty chain is refused naming the metric and the dimension.
#[test]
fn a_via_chain_with_no_relationship_in_it_is_refused() {
    let yaml = format!("{MINIMAL_METRIC}dimensions:\n  - name: region\n    column: code\n    via: []\n");
    let doc = metric_doc(&yaml).expect("an empty sequence is valid YAML");
    assert_eq!(
        doc.into_domain(Description::default()).unwrap_err(),
        InvalidMetricDocument::EmptyChain {
            metric: MetricName::parse("revenue").expect("a name"),
            dimension: DimensionName::parse("region").expect("a name"),
            cause: InvalidViaChain::Empty,
        }
    );
}

#[test]
fn an_anchor_value_may_be_written_with_or_without_quotes() {
    // Both forms occur in a file a person edits, and the unquoted one failing with "invalid
    // type: integer" is a true message about a file that looks right.
    for literal in ["197122", "\"197122\""] {
        let yaml = format!("{MINIMAL_METRIC}anchor:\n  range: {{ start: 2026-06-01, end: 2026-07-01 }}\n  value: {literal}\n");
        let metric = metric_doc(&yaml)
            .expect("both spellings parse")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        let anchor = metric.anchor().expect("the document declared one");
        assert_eq!(anchor.value(), "197122");
    }
}

/// An anchor value a reader could not read is refused, and the refusal says which metric.
///
/// The value `1971<U+200F>22` renders as an ordinary number in every terminal and every diff,
/// and reaches the operator who decides whether a metric still means what it claimed. It used to
/// load: `Anchor::new` took a `String` and nothing on the path looked at it.
///
/// **The typed cause is what this asserts, and it is why the parse is not in the serde path.**
/// `AnchorLiteral` is `untagged`, so a `try_from` on the field would collapse this to *data did
/// not match any variant*. Here the author gets the metric, the rule and the code point.
#[test]
fn an_anchor_value_a_reader_could_not_read_is_refused() {
    let yaml =
        format!("{MINIMAL_METRIC}anchor:\n  range: {{ start: 2026-06-01, end: 2026-07-01 }}\n  value: \"1971\u{200F}22\"\n");
    let refusal = metric_doc(&yaml)
        .expect("the document is well-formed YAML")
        .into_domain(Description::default())
        .expect_err("a direction-changing character is not an anchor value");
    assert_eq!(
        refusal,
        InvalidMetricDocument::AnchorValue {
            metric: MetricName::parse("revenue").expect("a name"),
            cause: InvalidDimensionValue::InvisibleCharacter {
                value: String::from("1971\u{200F}22"),
                code: 0x200F,
            },
        }
    );
}

#[test]
fn an_anchor_range_must_still_be_a_range() {
    // The domain's parsing applies through the document: an inverted anchor range is refused
    // here rather than becoming an anchor that can never match.
    let yaml = format!("{MINIMAL_METRIC}anchor:\n  range: {{ start: 2026-07-01, end: 2026-06-01 }}\n  value: 1\n");
    drop(metric_doc(&yaml).expect_err("an inverted range is not a range"));
}

#[test]
fn a_kind_probe_reads_the_tag_without_the_rest() {
    // The dispatch step. It has to tolerate every other field, which is exactly why it cannot
    // be the shape that denies unknown ones.
    let probe: KindProbe = serde_norway::from_str(MINIMAL_METRIC).expect("the tag is readable on its own");
    assert_eq!(probe.kind(), DocumentKind::Metric);
    assert_eq!(probe.kind().as_str(), "metric");
}

#[test]
fn an_unknown_kind_is_refused() {
    let err = serde_norway::from_str::<KindProbe>("kind: dashboard\n").expect_err("a dashboard is not a catalog document");
    assert!(err.to_string().contains("dashboard"), "{err}");
}

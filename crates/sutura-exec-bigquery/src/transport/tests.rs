//! What this module's own newtypes refuse, split out of `transport.rs` when it took the file past
//! the 1000-line ceiling `cargo xtask max-lines` enforces. `JobDeadline`/`JobRequest` have no tests
//! of their own here - `crate::tests` (the fake-transport suite) and `crate::tests` (the canned
//! wire's own suite) are where a request actually carrying the right clock is proven; this file is
//! the parse-and-refuse half of the resource-name and field-type newtypes.

use super::{DatasetId, NamedResource, ProjectId, UnusableResourceName};

#[test]
fn a_project_id_that_could_leave_a_url_path_segment_is_refused() {
    // The reason this type re-checks a value the settings tree already refused: THIS crate is the
    // one whose transport writes it into a request path.
    for hostile in [
        "acme/../other",
        "acme?alt=json",
        "acme#f",
        "acme%2f",
        "a b",
        "ACME",
        "acm\u{00e9}",
    ] {
        assert!(ProjectId::parse(hostile).is_err(), "{hostile:?} was accepted as a project id");
    }
    assert_eq!(
        ProjectId::parse("acme-analytics").expect("a plain id parses").as_str(),
        "acme-analytics"
    );
}

#[test]
fn a_refusal_names_a_position_and_not_the_value() {
    let err = ProjectId::parse("acme/one").expect_err("a slash is refused");
    assert!(!err.to_string().contains("acme"), "{err}");
    assert_eq!(
        err,
        UnusableResourceName::Character {
            what: NamedResource::Project,
            at: 4
        }
    );
    // The wording an operator reads is the Display impl's, in one place, rather than a literal
    // repeated at each construction site - so this is what a rename would have to move.
    assert_eq!(err.to_string(), "the character at position 4 is not allowed in a project id");
    assert_eq!(
        DatasetId::parse("  ").expect_err("whitespace is empty").to_string(),
        "a dataset id cannot be empty"
    );
}

#[test]
fn a_dataset_id_keeps_its_case_and_refuses_a_hyphen() {
    // Case-sensitive, so folding would name a dataset that does not exist. A hyphen is legal in a
    // project id and not in a dataset id, which is why the two are not one type.
    assert_eq!(
        DatasetId::parse("Analytics_Prod").expect("mixed case parses").as_str(),
        "Analytics_Prod"
    );
    assert_eq!(
        DatasetId::parse("analytics-prod"),
        Err(UnusableResourceName::Character {
            what: NamedResource::Dataset,
            at: 9
        })
    );
    assert_eq!(
        ProjectId::parse("analytics-prod")
            .expect("a hyphen IS in a project id")
            .as_str(),
        "analytics-prod"
    );
}

// `a_type_name_the_endpoint_sends_decodes_to_the_vocabulary_this_adapter_maps` WENT WITH
// `FieldType`. It asserted that a query response's legacy type spellings - `INTEGER`, `FLOAT`,
// `BOOLEAN` - decoded into this crate's own closed vocabulary. Those spellings are JSON schema
// fields of the deleted HTTP wire transport; the ADBC driver announces an Arrow schema, so there is
// no type NAME to decode and `sutura_domain::warehouse::arrow`'s own mapping is what decides which
// Arrow types this workspace reads. `docs/adr/0039` records the move, and
// `every_mapped_type_passes_the_schema_pass_and_float32_does_not` in
// `crates/sutura-domain/src/warehouse/arrow/tests.rs` is what holds the closed set now.

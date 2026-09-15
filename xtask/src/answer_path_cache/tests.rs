use super::{ANSWER_PATH_TYPES, cached_answer_path_type, contains_word, struct_field_lines, undeclared};
use crate::serde_parse::scan::code_lines;

#[test]
fn a_hashmap_holding_an_answer_path_type_is_found() {
    assert_eq!(
        cached_answer_path_type("std::collections::HashMap<Subject, RowSet>"),
        Some(("HashMap", "RowSet"))
    );
}

#[test]
fn a_btreemap_holding_a_compiled_plan_is_found() {
    assert_eq!(
        cached_answer_path_type("BTreeMap<QuestionDigest, QueryPlan>"),
        Some(("BTreeMap", "QueryPlan"))
    );
}

#[test]
fn wrapped_in_a_mutex_is_still_found() {
    // The shape a rate-limit window ALREADY takes in this workspace (`sutura-app/src/spend.rs`) -
    // a map behind a lock is still a map, and the wrapper must not hide the value type from this
    // gate.
    assert_eq!(
        cached_answer_path_type("std::sync::Mutex<HashMap<Subject, AnsweredRaw>>"),
        Some(("HashMap", "AnsweredRaw"))
    );
}

#[test]
fn an_unrelated_value_type_is_not_flagged() {
    // The real field in `sutura-app/src/spend.rs`: a rate-limit window, not an answer.
    assert_eq!(cached_answer_path_type("std::sync::Mutex<HashMap<Subject, Window>>"), None);
}

#[test]
fn a_name_that_merely_contains_an_answer_path_type_is_not_flagged() {
    // `MalformedRowSet` is a real, different type in this workspace
    // (`sutura_domain::warehouse::MalformedRowSet`) - a substring match would misread it as
    // holding `RowSet`.
    assert_eq!(cached_answer_path_type("HashMap<QuestionDigest, MalformedRowSet>"), None);
    assert!(!contains_word("MalformedRowSet", "RowSet"));
}

#[test]
fn a_map_with_no_top_level_comma_is_not_a_value_match() {
    // Not a shape a real map's generics take, and `last_top_level` refuses to guess which of one
    // argument is the value.
    assert_eq!(cached_answer_path_type("HashMap<RowSet>"), None);
}

#[test]
fn a_field_on_a_source_that_never_mentions_a_map_is_not_flagged() {
    assert_eq!(cached_answer_path_type("BTreeSet<RowSet>"), None);
}

/// [`struct_field_lines`] over a struct whose generic parameter list pushes the `{` to a later
/// line, and whose one field wraps a map behind a lock - the two shapes real fields in this
/// workspace already take.
#[test]
fn field_lines_are_read_past_a_where_clause() {
    let source = "pub struct Warehouses<W>\nwhere\n    W: Clone,\n{\n    by_source: std::sync::Mutex<HashMap<Subject, W>>,\n}\n";
    let code = code_lines(source);
    let fields = struct_field_lines(&code);
    assert_eq!(fields.len(), 1, "{fields:?}");
    assert_eq!(fields[0].0, 5);
    assert!(fields[0].1.contains("by_source"), "{}", fields[0].1);
}

#[test]
fn a_tuple_struct_is_out_of_scope() {
    let source = "struct Digest(String);\n";
    assert_eq!(struct_field_lines(&code_lines(source)), Vec::new());
}

#[test]
fn a_struct_declared_and_closed_on_one_line_has_no_fields_to_read() {
    let source = "struct Empty {}\n";
    assert_eq!(struct_field_lines(&code_lines(source)), Vec::new());
}

#[test]
fn a_second_structs_fields_do_not_bleed_into_the_first() {
    let source = "struct A {\n    x: u8,\n}\nstruct B {\n    y: HashMap<Subject, RowSet>,\n}\n";
    let fields = struct_field_lines(&code_lines(source));
    assert_eq!(fields.len(), 2, "{fields:?}");
    assert_eq!(fields[1].0, 5);
}

/// The reverse direction: [`undeclared`] on an empty `declared` set reports every entry, and on
/// the full set reports none - mirroring `newtype_leaks::undeclared`'s own test over its SEALED
/// table.
#[test]
fn undeclared_reports_exactly_what_is_missing() {
    assert_eq!(
        undeclared(&[]).len(),
        ANSWER_PATH_TYPES.len(),
        "an empty scan reported nothing"
    );
    let all: Vec<&'static str> = ANSWER_PATH_TYPES.iter().map(|entry| entry.name).collect();
    assert!(undeclared(&all).is_empty(), "a complete scan still reported something");
    let missing: Vec<&'static str> = all.iter().skip(1).copied().collect();
    let reported = undeclared(&missing);
    assert_eq!(reported.len(), 1, "{:?}", reported.iter().map(|e| e.name).collect::<Vec<_>>());
    assert_eq!(reported.first().map(|entry| entry.name), all.first().copied());
}

/// Every declared type is a real name in this workspace, checked against the file the table says
/// declares it - so an entry pointed at a moved or renamed type is a rule guarding nothing rather
/// than a silent pass.
#[test]
fn every_answer_path_type_is_declared_where_this_list_says_it_is() {
    let Some(root) = crate::repo::root() else {
        panic!("the repo root is what this gate depends on");
    };
    for entry in ANSWER_PATH_TYPES {
        let text = std::fs::read_to_string(root.join(entry.declared_in))
            .unwrap_or_else(|why| panic!("{} declares {}: {why}", entry.declared_in, entry.name));
        assert!(
            super::declares(&code_lines(&text), entry.name),
            "{} does not declare `{}` - the rule is guarding a name",
            entry.declared_in,
            entry.name
        );
    }
}

/// The gate's own scan, over a scratch tree rather than over the repo - so the refusal below is
/// exercised without asserting on this crate's own source text.
fn scan_over(tree: &crate::scratch_tree::Tree, anchors: &[&str]) -> Result<super::Scanned, crate::repo::Refusal> {
    super::scan(crate::repo::collect_files(tree.root(), tree.root(), &["rs"]), anchors)
}

/// **The refusal itself, not only the predicate it is built from.** A `HashMap` field holding a
/// `RowSet` is refused at [`super::decide`]'s own level - the shape the falsifier in
/// `task_table::architecture` seeds into the shared sweep, held here against a scratch tree of
/// its own.
#[test]
fn a_hashmap_field_holding_a_rowset_is_refused() {
    let tree = crate::scratch_tree::Tree::of(
        "answer-path-cache-violation",
        &[(
            "crates/sutura-app/src/leaky.rs",
            b"struct SubjectCache {\n    seen: std::collections::HashMap<u8, RowSet>,\n}\n",
        )],
    );
    let found = scan_over(&tree, &[]).expect("a readable tree scans");
    assert_eq!(
        found.violations.len(),
        1,
        "{:?}",
        found.violations.iter().map(|v| &v.field).collect::<Vec<_>>()
    );
    assert_eq!(found.violations[0].holds, "RowSet");
    assert_eq!(super::decide(&found), crate::Verdict::Fail);
}

/// A field whose value type is unrelated to every declared answer-path type passes - the same
/// scan the violation test above runs, over a field that IS read (`struct_field_lines` does not
/// skip it, proven by `field_lines_are_read_past_a_where_clause` and its neighbours) and simply
/// does not match. `decide` is not asserted here: over a scratch tree it also carries the
/// reverse-direction check against files this small tree does not have, which
/// `the_real_tree_holds_no_answer_path_cache` and
/// `every_answer_path_type_is_declared_where_this_list_says_it_is` already hold against the real
/// tree.
#[test]
fn a_field_holding_an_unrelated_type_passes() {
    let tree = crate::scratch_tree::Tree::of(
        "answer-path-cache-clean",
        &[(
            "crates/sutura-app/src/clean.rs",
            b"struct Windows {\n    seen: std::collections::HashMap<u8, Window>,\n}\n",
        )],
    );
    let found = scan_over(&tree, &[]).expect("a readable tree scans");
    assert!(
        found.violations.is_empty(),
        "{:?}",
        found.violations.iter().map(|v| &v.field).collect::<Vec<_>>()
    );
}

/// A file outside [`super::SCOPE`] holding the exact same leak is invisible to this gate by
/// construction - the deliberate limit this module's header states: an adapter's own cache is out
/// of scope, never excluded case by case.
#[test]
fn a_map_outside_the_scoped_crates_is_not_scanned() {
    let tree = crate::scratch_tree::Tree::of(
        "answer-path-cache-out-of-scope",
        &[(
            "crates/sutura-exec-bigquery/src/leaky.rs",
            b"struct SubjectCache {\n    seen: std::collections::HashMap<u8, RowSet>,\n}\n",
        )],
    );
    let Err(why) = scan_over(&tree, &[]) else {
        panic!("a tree with nothing in scope produced a verdict");
    };
    assert!(matches!(
        why,
        crate::repo::Refusal::Empty | crate::repo::Refusal::NothingJudged { .. }
    ));
}

/// **Measured before it was written, and re-measured here rather than only asserted:** today's
/// real tree carries no map holding an answer-path type. This is the "starts green" half of the
/// gate's own claim, run as a test so a regression here fails the suite and not only a manual read.
#[test]
fn the_real_tree_holds_no_answer_path_cache() {
    let Ok(census) = crate::repo::all_files() else {
        panic!("the repo root is what this gate depends on");
    };
    let found = super::scan(census, &super::anchors()).expect("the real tree scans");
    assert_eq!(
        super::decide(&found),
        crate::Verdict::Pass,
        "{:?}",
        found.violations.iter().map(|v| &v.field).collect::<Vec<_>>()
    );
}

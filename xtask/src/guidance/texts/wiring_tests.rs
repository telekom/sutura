//! The wiring between the census and the checks, in a file of its own so causality can measure it:
//! an inline test in an implementation file is never measured. The tests drive
//! [`crate::guidance::texts::tree_problems_from_texts`] from a fixture tree, two of them through
//! [`crate::guidance::inspect_listing`] - the seam that holds the settled-text and fail-closed
//! behaviour: what the checks read is the census's `Texts`, not the disk twice.

#[test]
fn an_invalid_utf8_file_in_scope_still_fails_closed() {
    let tree = crate::scratch_tree::Tree::of("rv1218-utf8-rs", &[("crates/x/src/lib.rs", &[0x2f_u8, 0x2f, 0xff])]);
    let census = crate::repo::collect_files(tree.root(), tree.root(), &["rs"]);
    let (files, texts, _witness) = super::super::inspect_listing(Ok(census)).expect("the file census");
    let (problems, ..) = super::super::tree_problems_from_texts(tree.root(), &texts, &files, &super::super::in_scope(&files));
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/x/src/lib.rs: in the") && p.contains("could not be read")),
        "RV1218-C {problems:#?}"
    );
}

#[test]
fn a_cfg_test_inside_a_block_comment_does_not_open_a_region() {
    let tree = crate::scratch_tree::Tree::of(
        "rv1218-cfg-comment",
        &[(
            "crates/x/src/lib.rs",
            b"/*\n#[cfg(test)]\n*/\nfn f(p: &P) -> u8 {\n    p.memory_pool()\n}\n",
        )],
    );
    let census = crate::repo::collect_files(tree.root(), tree.root(), &["rs"]);
    let (files, texts, _witness) = super::super::inspect_listing(Ok(census)).expect("census");
    let (problems, ..) = super::tree_problems_from_texts(tree.root(), &texts, &files, &[]);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("crates/x/src/lib.rs:5:") && p.contains("`.memory_pool()` in production code refutes")),
        "RV1218-A {problems:#?}"
    );
}

#[test]
fn the_census_wiring_uses_the_settled_text_not_the_disk() {
    let tree = crate::scratch_tree::Tree::of(
        "rv1218-sealed-wire",
        &[("crates/x/src/lib.rs", b"pub fn f(p: &P) -> u8 {\n    p.memory_pool()\n}\n")],
    );
    let files = vec![String::from("crates/x/src/lib.rs")];
    let mut texts = super::super::texts::Texts::default();
    texts.insert("crates/x/src/lib.rs", b"pub fn f() -> u8 {\n    1\n}\n");
    let (problems, ..) = super::super::tree_problems_from_texts(tree.root(), &texts, &files, &[]);
    assert!(
        !problems
            .iter()
            .any(|p| p.contains("`.memory_pool()` in production code refutes")),
        "RV1218-D {problems:#?}"
    );
}

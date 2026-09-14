use super::{LEAKY, SEALED, in_scope, last_segment, leaked_by, trait_impls, trait_path};
use crate::serde_parse::scan::code_lines;

/// The sealed types a source's TRAIT impls hand out, as the gate reads them.
fn handed_out(source: &str) -> Vec<(&'static str, String)> {
    let code = code_lines(source);
    trait_impls(&code)
        .into_iter()
        .filter_map(|(_, header)| super::hands_out_contents(&header, &[]))
        .collect()
}

/// The sealed types a source's INHERENT methods hand out, as `(sealed, how)`.
fn exposed(source: &str) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    super::exposing_methods(&code_lines(source), "probe.rs", &[], &mut out);
    out.into_iter().map(|open| (open.sealed, open.how)).collect()
}

/// The leaky traits a header names, as the gate reads them.
fn leaks(source: &str) -> Vec<(&'static str, String)> {
    let code = code_lines(source);
    trait_impls(&code)
        .into_iter()
        .filter_map(|(_, header)| leaked_by(&header))
        .collect()
}

#[test]
fn a_deref_on_a_newtype_is_found() {
    let source = "impl core::ops::Deref for Digest {\n    type Target = str;\n}\n";
    assert_eq!(leaks(source), vec![("Deref", String::from("Digest"))]);
}

#[test]
fn an_unqualified_deref_is_found_too() {
    assert_eq!(leaks("impl Deref for Digest {\n}\n"), vec![("Deref", String::from("Digest"))]);
}

#[test]
fn a_borrow_with_a_type_argument_is_found() {
    // The shape that makes `Borrow` worth gating: it promises equal hashing for `str`, and a
    // `parse` that folds case makes that promise false.
    let source = "impl std::borrow::Borrow<str> for Phrase {\n}\n";
    assert_eq!(leaks(source), vec![("Borrow", String::from("Phrase"))]);
}

#[test]
fn the_mutable_pair_is_found() {
    let source = "impl DerefMut for Digest {\n}\nimpl BorrowMut<str> for Phrase {\n}\n";
    let found = leaks(source);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|(name, _)| *name == "DerefMut"), "{found:?}");
    assert!(found.iter().any(|(name, _)| *name == "BorrowMut"), "{found:?}");
}

#[test]
fn a_generic_impl_is_read_past_its_own_parameters() {
    let source = "impl<'a, T: Clone> Deref for Holder<'a, T> {\n}\n";
    assert_eq!(leaks(source), vec![("Deref", String::from("Holder<'a, T>"))]);
}

#[test]
fn a_where_clause_on_a_later_line_does_not_hide_the_trait() {
    let source = "impl<T> Deref for Holder<T>\nwhere\n    T: Clone,\n{\n}\n";
    assert_eq!(leaks(source).len(), 1);
}

#[test]
fn as_ref_is_the_alternative_and_is_not_a_leak() {
    // Deliberately absent from `LEAKY`: it promises nothing about hashing or ordering, and the
    // guide recommends it wherever a borrow is genuinely wanted.
    assert!(
        leaks("impl AsRef<str> for Digest {\n}\n").is_empty(),
        "AsRef is the non-leaky alternative"
    );
}

#[test]
fn an_ordinary_trait_impl_is_not_a_leak() {
    let source = "impl core::fmt::Display for Digest {\n}\nimpl TryFrom<String> for Digest {\n}\n";
    assert!(leaks(source).is_empty(), "Display and TryFrom are not leaks");
}

#[test]
fn an_inherent_impl_is_not_a_trait_impl() {
    assert!(
        leaks("impl Digest {\n    pub fn as_str(&self) -> &str {\n        &self.0\n    }\n}\n").is_empty(),
        "an inherent impl is not a trait-impl leak"
    );
}

#[test]
fn a_higher_ranked_bound_is_not_read_as_the_split() {
    // `for<'a>` is not the ` for ` that separates the trait from the type, and reading it as
    // one would make every such impl unclassifiable.
    let source = "impl<F: for<'a> Fn(&'a str) -> u8> Deref for Wrapper<F> {\n}\n";
    assert_eq!(leaks(source), vec![("Deref", String::from("Wrapper<F>"))]);
}

#[test]
fn prose_saying_there_is_deliberately_no_deref_is_not_an_impl() {
    // Load-bearing rather than tidy: this repo says exactly that in three doc comments, so a
    // scan over raw text would fail on the sentences that explain the rule.
    let source = "/// There is deliberately no `Deref` here.\n// impl Deref for Digest {}\npub struct Digest(String);\n";
    assert!(leaks(source).is_empty(), "comment prose about Deref is not an impl");
}

#[test]
fn an_impl_inside_a_multi_line_string_is_not_an_impl() {
    // Which is what keeps this module's own fixtures invisible to the gate that reads them.
    let source = "fn fixture() -> &'static str {\n    r#\"\nimpl Deref for Digest {\n}\n\"#\n}\n";
    assert!(leaks(source).is_empty(), "an impl inside a string literal is not a real impl");
}

#[test]
fn a_trait_path_drops_its_arguments() {
    assert_eq!(trait_path("impl std::borrow::Borrow<str>"), Some("std::borrow::Borrow"));
    assert_eq!(trait_path("impl<T> Borrow<T>"), Some("Borrow"));
    assert_eq!(last_segment("std::borrow::Borrow"), "Borrow");
    assert_eq!(last_segment("Borrow"), "Borrow");
}

#[test]
fn vendored_code_is_out_of_scope_and_rust_files_are_in_it() {
    // Upstream's shape is not ours to lint; `VENDOR.md` is where a local change is argued.
    assert!(in_scope("crates/sutura-domain/src/lib.rs"));
    assert!(!in_scope("vendor/mimalloc_rust/src/lib.rs"));
    assert!(!in_scope("AGENTS.md"));
}

#[test]
fn the_four_line_defeat_of_the_whole_mechanism_is_refused() {
    // Measured on `565ebaae`, when this gate did not have this rule: these four lines compiled,
    // `.take(3)` at the real call site then produced a verdict over 3 of 1177 subjects at exit
    // 0, and the gate counted the impl - `244` to `245` - without refusing. So the headline
    // property of #419 was held by nobody adding four lines.
    let source = "impl IntoIterator for Census {\n    type Item = String;\n}\n";
    assert_eq!(handed_out(source).len(), 1, "{:?}", handed_out(source));
}

#[test]
fn a_reference_a_lifetime_and_a_generic_argument_do_not_hide_the_sealed_type() {
    // `&Census` is the shape that would otherwise be the same four lines with one character
    // added, and `IntoIterator for &T` is the idiomatic spelling of exactly this leak.
    for target in ["&Census", "&'a Census", "&mut Census", "Census<'a>"] {
        let source = format!("impl<'a> IntoIterator for {target} {{\n}}\n");
        assert_eq!(handed_out(&source).len(), 1, "{target} hid the sealed type");
    }
}

#[test]
fn as_ref_is_the_alternative_everywhere_except_on_a_sealed_type() {
    // The one place the guide's recommended alternative is still a leak: a witness type has
    // nothing it may lend. `sutura-domain`'s own `AsRef` impls are untouched by this rule,
    // which is why it is scoped to `SEALED` rather than added to `LEAKY`.
    assert_eq!(handed_out("impl AsRef<[String]> for Census {\n}\n").len(), 1);
    assert!(
        handed_out("impl AsRef<str> for Digest {\n}\n").is_empty(),
        "AsRef hands out nothing to leak"
    );
    assert!(
        leaks("impl AsRef<[String]> for Census {\n}\n").is_empty(),
        "not a global rule"
    );
}

#[test]
fn an_ordinary_trait_impl_on_a_sealed_type_is_not_a_leak() {
    // The rule is about handing out the contents, not about sealing the type off entirely.
    assert!(
        handed_out("impl core::fmt::Debug for Census {\n}\n").is_empty(),
        "Debug promises nothing to hand out"
    );
    assert!(handed_out("impl Drop for Swept {\n}\n").is_empty(), "Drop hands out nothing");
}

#[test]
fn an_accessor_is_refused_by_its_return_shape_and_not_only_by_its_name() {
    // The name list alone would be defeated by a rename, which is a one-word diff. Measured
    // with the method kept ALIVE so `dead_code` could not take the credit:
    // `xtask/src/repo/census.rs:227: fn subjects returning &[String] on Census`, exit 1.
    let named = "impl Census {\n    fn iter(&self) -> Something {\n        todo!()\n    }\n}\n";
    assert_eq!(exposed(named).len(), 1, "{:?}", exposed(named));

    let unnamed = "impl Census {\n    pub(crate) fn subjects(&self) -> &[String] {\n        &self.of\n    }\n}\n";
    assert_eq!(
        exposed(unnamed).len(),
        1,
        "a name no list would guess: {:?}",
        exposed(unnamed)
    );

    let vector = "impl Swept {\n    fn taken(&self) -> Vec<String> {\n        Vec::new()\n    }\n}\n";
    assert_eq!(exposed(vector).len(), 1, "{:?}", exposed(vector));

    let iterator = "impl Census {\n    fn walk(&self) -> impl Iterator<Item = &str> {\n        None.into_iter()\n    }\n}\n";
    assert_eq!(exposed(iterator).len(), 1, "{:?}", exposed(iterator));
}

#[test]
fn a_parameter_carrying_a_slice_is_not_a_return_value() {
    // `Census::inspect` takes `&[&str]` and a `fn(&str) -> bool`, so reading anything but the
    // return type would refuse the production signature this rule exists to protect.
    let real = "impl Census {\n    pub(crate) fn inspect(self, must_judge: &[&str], scope: Scope, judge: impl FnMut(&str, &[u8])) -> Result<Inspected, Refusal> {\n        todo!()\n    }\n}\n";
    assert!(exposed(real).is_empty(), "{:?}", exposed(real));
}

#[test]
fn the_declared_transitional_door_is_the_one_exception_and_it_is_scoped_three_ways() {
    // `into_listing` hands out a plain `Vec` on purpose and its own bound is the call-site
    // count in `xtask/src/repo/census.rs`. It is exempt on THREE axes - type, name and return
    // shape - because each was measured escaping on its own. Built from parts so this source
    // does not itself read as a call site to that count: the first run of the count test
    // reported 45 against 44 real ones, and the extra was this fixture.
    let (sealed, name, returns) = super::DECLARED_DOOR;
    let door = format!(
        "impl {sealed} {{\n    pub(crate) fn {name}(self, _caller: Unmigrated) -> {returns} {{\n        todo!()\n    }}\n}}\n"
    );
    assert!(exposed(&door).is_empty(), "{:?}", exposed(&door));

    // The measured hole: the same NAME on another sealed type, handing out all five verdict
    // numbers. Exempt before this was scoped; refused now.
    let elsewhere = format!(
        "impl Inspected {{\n    pub(crate) fn {name}(self) -> (usize, usize, usize, usize, usize) {{\n        todo!()\n    }}\n}}\n"
    );
    assert_eq!(exposed(&elsewhere).len(), 1, "{:?}", exposed(&elsewhere));

    // And the same type and name handing back something else.
    let reshaped = format!("impl {sealed} {{\n    pub(crate) fn {name}(self) -> Vec<String> {{\n        todo!()\n    }}\n}}\n");
    assert_eq!(exposed(&reshaped).len(), 1, "{:?}", exposed(&reshaped));
}

#[test]
fn a_trait_nobody_has_heard_of_cannot_hand_out_a_sealed_type_either() {
    // **The fourth shape.** `SEQUENCE_TRAITS` refuses the traits it knows and the method scan
    // used to walk inherent impls only, so five lines of a custom trait restored `.take(3)` at
    // the production call site: `246 trait impl(s) … 4 sealed witness type(s) still hold their
    // contents`, exit 0, suite green, clippy clean.
    let custom = "impl Subjects for Census {\n    fn subjects(self) -> Vec<String> {\n        self.of\n    }\n}\n";
    assert_eq!(exposed(custom).len(), 1, "{:?}", exposed(custom));

    // A reference receiver and an unknown trait, which is the same escape one character wider.
    let borrowed = "impl<'a> Lend for &'a Census {\n    fn all(&self) -> &[String] {\n        &self.of\n    }\n}\n";
    assert_eq!(exposed(borrowed).len(), 1, "{:?}", exposed(borrowed));

    // An ordinary trait impl on a sealed type is still nobody's business: the rule is about the
    // METHOD's signature, not about sealing the type off entirely.
    let debug = "impl core::fmt::Debug for Census {\n    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {\n        todo!()\n    }\n}\n";
    assert!(exposed(debug).is_empty(), "{:?}", exposed(debug));
    let dropped = "impl Drop for Swept {\n    fn drop(&mut self) {\n        todo!()\n    }\n}\n";
    assert!(exposed(dropped).is_empty(), "{:?}", exposed(dropped));
}

#[test]
fn a_file_declaring_its_own_type_of_a_sealed_name_means_its_own() {
    // Matching is by LAST SEGMENT, so it is blind to modules - and the trait half of this rule
    // immediately reported `xtask/src/worktree_state.rs:205: fn missed returning
    // &[&'static str] on Inspected`, which is #417's own witness type of the same name. A file
    // that declares its own `Inspected` means that one.
    let local = "pub(crate) struct Inspected {\n    missed: Vec<String>,\n}\n\nimpl Inspected {\n    fn missed(&self) -> &[String] {\n        &self.missed\n    }\n}\n";
    let code = code_lines(local);
    let shadowed = super::shadowing(&code, "probe.rs");
    assert_eq!(shadowed, vec!["Inspected"], "the file's own declaration was not seen");
    let mut out = Vec::new();
    super::exposing_methods(&code, "probe.rs", &shadowed, &mut out);
    assert!(out.is_empty(), "{:?}", out.iter().map(|o| o.sealed).collect::<Vec<_>>());

    // And the same source WITHOUT the declaration is the real sealed type again, so the
    // exclusion is the declaration rather than the file name.
    assert_eq!(
        exposed("impl Inspected {\n    fn missed(&self) -> &[String] {\n        todo!()\n    }\n}\n").len(),
        1
    );

    // The file the list NAMES is never shadowed by its own declaration.
    assert!(
        super::shadowing(&code_lines("pub(crate) struct Census {\n}\n"), "xtask/src/repo/census.rs").is_empty(),
        "the declaring file cannot shadow its own entry"
    );
}

#[test]
fn a_method_of_an_unsealed_type_is_nobodys_business() {
    let other = "impl Digest {\n    fn iter(&self) -> Vec<String> {\n        Vec::new()\n    }\n}\n";
    assert!(exposed(other).is_empty(), "{:?}", exposed(other));
}

#[test]
fn a_fn_nested_inside_a_method_is_not_a_method_of_the_type() {
    // Depth-tracked, because a helper inside a body is not reachable through the type.
    let nested = "impl Census {\n    fn verdict(&self) -> String {\n        fn helper() -> Vec<String> {\n            Vec::new()\n        }\n        String::new()\n    }\n}\n";
    assert!(exposed(nested).is_empty(), "{:?}", exposed(nested));
}

#[test]
fn a_sealed_type_the_scan_never_found_is_a_failure_rather_than_a_quiet_pass() {
    // The refusal, not just its predicate. Nothing was found declared, so every entry is
    // reported; find them all and none is.
    assert_eq!(super::undeclared(&[]).len(), SEALED.len(), "an empty scan reported nothing");
    let all: Vec<&'static str> = SEALED.iter().map(|entry| entry.name).collect();
    assert!(super::undeclared(&all).is_empty(), "a complete scan still reported something");
    let missing: Vec<&'static str> = all.iter().skip(1).copied().collect();
    let reported = super::undeclared(&missing);
    assert_eq!(reported.len(), 1, "{:?}", reported.iter().map(|e| e.name).collect::<Vec<_>>());
    assert_eq!(reported.first().map(|entry| entry.name), all.first().copied());
}

#[test]
fn every_sealed_type_is_declared_where_this_list_says_it_is() {
    // The reverse direction, so the list cannot rot into guarding a name. `run` reports this as
    // a FAILURE rather than as an absence of findings; measured by pointing one entry at a file
    // that exists and does not declare it: `FAILED - `Discovered` is declared sealed but
    // `xtask/src/repo/census.rs` does not declare it`, exit 1.
    let Some(root) = crate::repo::root() else {
        panic!("the repo root is what this gate depends on");
    };
    for entry in SEALED {
        let text = std::fs::read_to_string(root.join(entry.declared_in))
            .unwrap_or_else(|why| panic!("{} declares {}: {why}", entry.declared_in, entry.name));
        assert!(
            super::declares(&code_lines(&text), entry.name),
            "{} does not declare `{}` - the rule is guarding a name",
            entry.declared_in,
            entry.name
        );
        assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
    }
}

#[test]
fn every_leaky_trait_says_what_to_do_instead() {
    for entry in LEAKY {
        assert!(!entry.name.is_empty(), "an entry names a trait");
        assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
        assert!(!entry.instead.is_empty(), "{} has no fix", entry.name);
    }
}

/// The gate's own scan, over a scratch tree rather than over the repo.
///
/// `repo::collect_files` is an existing census door and it takes a ROOT, which is what makes the
/// migrated scan testable without a checkout: the extension arm does not open a file, so a sealed
/// fixture reaches [`super::scan`]'s read and fails there - which is the path under test.
fn scan_over(
    tree: &crate::scratch_tree::Tree,
    extensions: &[&str],
    anchors: &[&str],
) -> Result<super::Scanned, crate::repo::Refusal> {
    super::scan(crate::repo::collect_files(tree.root(), tree.root(), extensions), anchors)
}

/// `xtask/src/newtype_leaks.rs:274`, re-measured on `bf59f9dc`: `let Ok(text) = read_to_string(..)
/// else { continue }` printed `465 file(s)` readable and `464 file(s)` with one in-scope file at
/// mode `000`, both at exit 0. The census owns the read now, so the same input refuses and NAMES
/// the file.
#[cfg(unix)]
#[test]
fn an_unreadable_in_scope_file_refuses_and_names_it() {
    let anchor = "xtask/src/anchor.rs";
    let mut tree = crate::scratch_tree::Tree::of(
        "newtype-leaks-sealed",
        &[(anchor, b"// the anchor\n"), ("crates/thing/src/lib.rs", b"// in scope\n")],
    );
    let control = scan_over(&tree, &["rs"], &[anchor]).expect("a readable tree scans");
    assert_eq!(control.read, 2, "{}", control.witness);

    if !tree.seal("crates/thing/src/lib.rs") {
        // Mode bits ignored for this uid; asserting a refusal here would assert nothing.
        return;
    }
    let Err(why) = scan_over(&tree, &["rs"], &[anchor]) else {
        panic!("an unreadable in-scope file produced a verdict over the rest of the tree");
    };
    assert!(
        why.describe().contains("crates/thing/src/lib.rs"),
        "the refusal has to name the file it could not read: {}",
        why.describe()
    );
}

/// #412's trap, held for this gate: a PNG is OUT OF SCOPE rather than unreadable, and a remedy
/// that refuses every file it did not decode reddens a correct tree. `check-shipped-binaries` was
/// the gate that did it.
#[test]
fn a_binary_file_out_of_scope_is_not_a_refusal() {
    let anchor = "xtask/src/anchor.rs";
    let tree = crate::scratch_tree::Tree::of(
        "newtype-leaks-binary",
        &[
            (anchor, b"// the anchor\n"),
            ("docs/diagram.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00"),
        ],
    );
    let found = scan_over(&tree, &["rs", "png"], &[anchor]).expect("a PNG is out of scope, not unreadable");
    assert_eq!(found.read, 1, "only the Rust file is in scope: {}", found.witness);
}

/// In scope, readable, and not valid UTF-8. The bytes are decoded lossily rather than handed back
/// to a `read_to_string` that would turn a decode failure into an unread file - which is the drop
/// this migration removed, respelt.
#[test]
fn an_in_scope_file_that_is_not_utf8_is_still_judged() {
    let tree = crate::scratch_tree::Tree::of(
        "newtype-leaks-lossy",
        &[("xtask/src/anchor.rs", b"impl Deref for Digest {\n}\n\xff\xfe// \xff\n")],
    );
    let found = scan_over(&tree, &["rs"], &["xtask/src/anchor.rs"]).expect("invalid UTF-8 is not unreadable");
    assert_eq!(found.read, 1, "{}", found.witness);
    assert_eq!(
        found.leaks.len(),
        1,
        "the rule still ran over the decodable part: {}",
        found.witness
    );
}

/// A scope that matched nothing refuses, so this gate has no `scanned == 0` floor of its own to
/// satisfy by reading almost nothing.
#[test]
fn an_empty_scope_refuses_rather_than_reporting_zero() {
    let tree = crate::scratch_tree::Tree::of("newtype-leaks-empty", &[("docs/page.md", b"no source here\n")]);
    let Err(why) = scan_over(&tree, &["md"], &[]) else {
        panic!("a tree with no Rust source produced a verdict");
    };
    assert!(why.describe().contains("NONE of them"), "{}", why.describe());
}

/// The anchors are derived from [`SEALED`] rather than declared again, so an entry whose file moved
/// refuses by path instead of the scan silently reading nothing there.
#[test]
fn a_sealed_entrys_file_that_is_not_in_the_tree_refuses() {
    let tree = crate::scratch_tree::Tree::of("newtype-leaks-anchor", &[("xtask/src/present.rs", b"// present\n")]);
    let Err(why) = scan_over(&tree, &["rs"], &["xtask/src/moved-away.rs"]) else {
        panic!("an anchor absent from the tree produced a verdict");
    };
    assert!(why.describe().contains("moved-away.rs"), "{}", why.describe());
}

/// Every [`SEALED`] file is an anchor, deduplicated - and each is in this gate's own scope, since
/// an anchor a scope excludes can never be discharged.
#[test]
fn the_anchor_set_is_the_sealed_tables_distinct_files_and_all_of_them_are_in_scope() {
    let anchors = super::anchors();
    assert!(!anchors.is_empty(), "an empty anchor set declares nothing");
    for entry in SEALED {
        assert!(anchors.contains(&entry.declared_in), "{} is not an anchor", entry.name);
    }
    for path in &anchors {
        assert!(in_scope(path), "{path} is an anchor this gate's scope excludes");
    }
    let mut sorted = anchors.clone();
    sorted.dedup();
    assert_eq!(sorted.len(), anchors.len(), "the anchor set repeats a path: {anchors:?}");
}

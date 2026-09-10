use super::{LEAKY, SEALED, in_scope, last_segment, leaked_by, run, trait_impls, trait_path};
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

#[test]
fn run_refuses_a_seeded_tree_that_carries_a_real_first_party_deref() {
    // THE #371 PROOF, at the run()-level item 5 says was never there: the shared falsifier tree
    // has no `.rs` file, so this gate used to refuse on its `scanned == 0` floor and a real
    // Finding `Fail -> Pass` flip stayed green. Seeding the gate's OWN violation makes the
    // refusal come from `LEAKY`'s Deref rule instead - the arm the falsifier sweep now asserts.
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this test moves the process's current directory, so it needs nextest's one-process-per-test"
    );
    let tree = std::env::temp_dir().join(format!("sutura-newtype-seed-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&tree));
    std::fs::create_dir_all(tree.join("src")).expect("a seed directory");
    std::fs::write(tree.join("flake.nix"), "{ }\n").expect("the first root marker");
    std::fs::write(tree.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("the second root marker");
    std::fs::write(
        tree.join("src").join("leaky.rs"),
        "struct Digest(String);\nimpl core::ops::Deref for Digest {\n    type Target = str;\n}\n",
    )
    .expect("the real first-party Deref");

    let original = std::env::current_dir().expect("a current directory");
    std::env::set_current_dir(&tree).expect("point the process at the seeded tree");
    let verdict = run(&[]);
    std::env::set_current_dir(&original).expect("restore the current directory");
    drop(std::fs::remove_dir_all(&tree));

    assert_eq!(
        verdict,
        crate::Verdict::Fail,
        "a real first-party `impl Deref`, in scope with a real violation, must make the gate \
         refuse - `github.com/telekom/sutura#371`"
    );
}

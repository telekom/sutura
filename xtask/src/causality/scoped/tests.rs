use super::{AddedTest, Scan, Silent};
use crate::causality::diff::ChangedFile;
use crate::causality::fixtures::{changed, manifest, tree};
use crate::causality::names::Ident;
use crate::causality::regions::PostImage;

/// The names `Scan::of` found runnable, as plain strings.
fn runnable(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> Option<Vec<String>> {
    let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
    match Scan::of(files, &owned, read) {
        Scan::Runnable(scoped) => Some(scoped.tests().iter().map(|one| String::from(one.name())).collect()),
        _ => None,
    }
}

/// The filterset for the tests `provable` added.
fn filterset(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> String {
    let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
    match Scan::of(files, &owned, read) {
        Scan::Runnable(scoped) => scoped.filterset(),
        other => panic!("expected runnable tests, got {other:?}"),
    }
}

#[test]
fn the_base_run_is_scoped_to_the_tests_the_diff_added() {
    // The property the whole module exists for: the run names the ADDED test and nothing
    // else. `existing` is unchanged context in the same module and must not be scoped in,
    // because a verdict about it is a verdict about the suite rather than the change.
    let file = concat!(
        "pub fn open() -> u8 {\n",       // 1
        "    1\n",                       // 2
        "}\n",                           // 3
        "#[cfg(test)]\n",                // 4
        "mod tests {\n",                 // 5
        "    #[test]\n",                 // 6
        "    fn existing() {}\n",        // 7
        "    #[test]\n",                 // 8
        "    fn added_one() {}\n",       // 9
        "    #[tokio::test]\n",          // 10
        "    async fn added_two() {}\n", // 11
        "}\n",                           // 12
    );
    let files = vec![changed(
        "crates/x/src/a.rs",
        8,
        &[
            "    #[test]",
            "    fn added_one() {}",
            "    #[tokio::test]",
            "    async fn added_two() {}",
        ],
    )];
    let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(
        runnable(&files, &["crates/x/src/a.rs"], &read),
        Some(vec![String::from("added_one"), String::from("added_two")])
    );
}

#[test]
fn a_marker_whose_module_file_is_in_the_diff_names_nothing_and_refuses() {
    // The fail-closed shape. `#[cfg(test)]` makes the file a test file and names no
    // function, so the scan comes back empty - and empty has to be unrepresentable rather
    // than "run everything", which is the unfiltered run this module replaced. `tests.rs` is
    // in the diff, so the declaration IS accounted for; nothing named a test all the same.
    let files = vec![
        changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]"]),
        changed("crates/x/src/tests.rs", 1, &["use super::f;"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
        ("crates/x/src/tests.rs", "use super::f;\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    assert!(matches!(
        Scan::of(&files, &[String::from("crates/x/src/lib.rs")], &read),
        Scan::Unnamed
    ));
}

#[test]
fn a_declaration_whose_module_file_is_absent_from_the_diff_refuses() {
    // THE FINDING. `lib.rs` gains `#[cfg(test)] mod legacy;` while `legacy.rs` already exists
    // and is untouched, so a whole module of pre-existing tests becomes compiled: no added
    // line names any of them, and `lib.rs` is held at HEAD, so they are in BOTH trees and
    // cannot be red on base either. One nameable test elsewhere used to be enough to land
    // this on the passing `silent` arm, whose printed sentence claimed *its own file names
    // the tests* - a fact nothing checked.
    let files = vec![
        changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod legacy;"]),
        changed("crates/x/src/other.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod legacy;\n"),
        (
            "crates/x/src/legacy.rs",
            "#[test]\nfn old_one() {}\n#[test]\nfn old_two() {}\n",
        ),
        ("crates/x/src/other.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/other.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Enabled(ref refused) => {
            assert_eq!(refused.len(), 1, "one declaration this diff cannot account for");
            assert_eq!(refused.first().map(|one| one.path.as_str()), Some("crates/x/src/lib.rs"));
            assert_eq!(refused.first().map(|one| one.module.as_str()), Some("crates/x/src/legacy.rs"));
        }
        other => panic!("expected Enabled ahead of Runnable, got {other:?}"),
    }
}

#[test]
fn an_attribute_the_diff_cut_below_its_twin_gates_nothing_new() {
    // THE MISREAD. `mod fresh;` goes in above `#[cfg(test)] mod legacy;`. The post-image is the
    // same under either cut of the diff, and the cut `git diff` chose adds the SECOND attribute:
    // it sits over `mod legacy;`, which base already gated, so `legacy.rs` is not this diff's
    // business. Read as a new gate, it refused the change for a module nothing enabled - and the
    // declaration that WAS added, `fresh`, went unanswered.
    let lib = "fn f() {}\n#[cfg(test)]\nmod fresh;\n#[cfg(test)]\nmod legacy;\n";
    let files = vec![
        changed("crates/x/src/lib.rs", 3, &["mod fresh;", "#[cfg(test)]"]),
        changed("crates/x/src/fresh.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", lib),
        ("crates/x/src/fresh.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/src/legacy.rs", "#[test]\nfn old() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/fresh.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Runnable(ref scoped) => assert_eq!(
            scoped.silent(),
            [Silent {
                path: String::from("crates/x/src/lib.rs"),
                module: Some(String::from("crates/x/src/fresh.rs")),
            }]
        ),
        other => panic!("expected `fresh` to account for the declaration, got {other:?}"),
    }
    // THE LIMIT OF IT. The same added block with `fn f() {}` above it has no twin: the trailing
    // attribute is new, and `legacy` was not gated before, so this still refuses.
    let lib = "fn f() {}\nmod fresh;\n#[cfg(test)]\nmod legacy;\n";
    let files = vec![
        changed("crates/x/src/lib.rs", 2, &["mod fresh;", "#[cfg(test)]"]),
        changed("crates/x/src/fresh.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", lib),
        ("crates/x/src/fresh.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/src/legacy.rs", "#[test]\nfn old() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    match Scan::of(&files, &provable, &read) {
        Scan::Enabled(ref refused) => {
            assert_eq!(refused.len(), 1, "one declaration this diff cannot account for");
            assert_eq!(refused.first().map(|one| one.module.as_str()), Some("crates/x/src/legacy.rs"));
        }
        other => panic!("expected the new gate over `legacy` to refuse, got {other:?}"),
    }
}

#[test]
fn every_out_of_line_declaration_in_a_file_is_accounted_for_not_only_the_last() {
    // `legacy` is declared first and `fresh` last, and only `fresh.rs` is in the diff. Answering
    // for the last declaration alone said the file was accounted for, and the module that enabled
    // a whole file of pre-existing tests went by unremarked.
    let lib = "fn f() {}\n#[cfg(test)]\nmod legacy;\n#[cfg(test)]\nmod fresh;\n";
    let files = vec![
        changed(
            "crates/x/src/lib.rs",
            2,
            &["#[cfg(test)]", "mod legacy;", "#[cfg(test)]", "mod fresh;"],
        ),
        changed("crates/x/src/fresh.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", lib),
        ("crates/x/src/fresh.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/src/legacy.rs", "#[test]\nfn old() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/fresh.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Enabled(ref refused) => {
            assert_eq!(refused.len(), 1, "`legacy` is the one declaration nothing accounts for");
            assert_eq!(refused.first().map(|one| one.module.as_str()), Some("crates/x/src/legacy.rs"));
        }
        other => panic!("expected `legacy` to refuse, got {other:?}"),
    }
}

#[test]
fn an_unreadable_post_image_is_refused_rather_than_called_a_test_module() {
    // The other half of the same finding. `attributes::adds` answers `TestModule` for a file
    // it cannot read, which is the fail-closed direction - but this scan could not read it
    // either, so it landed on `silent` and printed *a test module arrived here*. Nothing knew
    // that. Delivering the fail-closed direction is what this arm is.
    let files = vec![
        changed("crates/x/src/gone.rs", 1, &["#[cfg(test)]"]),
        changed("crates/x/src/other.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/other.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/gone.rs"), String::from("crates/x/src/other.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/gone.rs")]),
        other => panic!("expected Unreadable, got {other:?}"),
    }
}

#[test]
fn a_file_not_in_the_proof_is_not_scanned() {
    // A file carrying both an implementation change and its tests is excluded from the proof
    // by the plan. Its tests must not reach the run either: they cannot be red on base,
    // because their own implementation is never reverted.
    let held = concat!(
        "fn fixed() -> u8 { 2 }\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    #[test]\n",
        "    fn held() {}\n",
        "}\n"
    );
    let files = vec![changed("crates/x/src/held.rs", 4, &["    #[test]", "    fn held() {}"])];
    let read = tree(&[("crates/x/src/held.rs", held), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(runnable(&files, &[], &read), None);
}

#[test]
fn a_relocated_declaration_is_accounted_for_by_the_file_it_actually_names() {
    // The `#[path]` half of the resolution, at the level that refuses, and the fixture has to
    // DIVERGE from the layout or it proves nothing: `#[path = "shared/cells.rs"] mod support;`
    // in `tests/golden.rs` names `tests/shared/cells.rs`, while the layout would look for
    // `tests/golden/support.rs`. Not in the diff is a REFUSAL now, so a resolver that read the
    // layout instead of the attribute would redden a correct change - and the first version of
    // this test used `#[path = "golden/catalogs.rs"]`, where the two agree by coincidence and
    // ignoring the attribute reddened nothing.
    let target = concat!(
        "#[cfg(test)]\n",                  // 1
        "#[path = \"shared/cells.rs\"]\n", // 2
        "mod support;\n",                  // 3
    );
    let files = vec![
        changed(
            "crates/x/tests/golden.rs",
            1,
            &["#[cfg(test)]", "#[path = \"shared/cells.rs\"]", "mod support;"],
        ),
        changed("crates/x/tests/shared/cells.rs", 1, &["#[test]", "fn sums() {}"]),
    ];
    let read = tree(&[
        ("crates/x/tests/golden.rs", target),
        ("crates/x/tests/shared/cells.rs", "#[test]\nfn sums() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![
        String::from("crates/x/tests/golden.rs"),
        String::from("crates/x/tests/shared/cells.rs"),
    ];
    match Scan::of(&files, &provable, &read) {
        Scan::Runnable(ref scoped) => assert_eq!(
            scoped.silent(),
            [Silent {
                path: String::from("crates/x/tests/golden.rs"),
                module: Some(String::from("crates/x/tests/shared/cells.rs")),
            }]
        ),
        other => panic!("expected the relocated declaration to be stated, got {other:?}"),
    }
}

#[test]
fn a_path_no_package_owns_is_refused_rather_than_skipped() {
    // Nothing compiles it, so it has no test to run - and inventing a package name for it
    // would put a name nextest does not know into the filter. A REFUSAL rather than a skip,
    // which is the change: the file is part of the proof, and skipping it measures a subset.
    let files = vec![changed("stray/a.rs", 1, &["#[test]", "fn sums() {}"])];
    let read = tree(&[("stray/a.rs", "#[test]\nfn sums() {}\n")]);
    match Scan::of(&files, &[String::from("stray/a.rs")], &read) {
        Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("stray/a.rs")]),
        other => panic!("expected Unreadable, got {other:?}"),
    }
    // And whatever it added, not only a `#[test]`: a declaration in a file cargo compiles
    // nothing from cannot enable tests, so *its own file names them* is not sayable either.
    let declaring = vec![changed("stray/lib.rs", 2, &["#[cfg(test)]", "mod tests;"])];
    let read = tree(&[
        ("stray/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
        ("stray/tests.rs", "#[test]\nfn sums() {}\n"),
    ]);
    match Scan::of(&declaring, &[String::from("stray/lib.rs")], &read) {
        Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("stray/lib.rs")]),
        other => panic!("expected Unreadable for a path no package owns, got {other:?}"),
    }
}

#[test]
fn one_nameable_test_does_not_mask_a_file_this_could_not_name() {
    // THE MASKING. This scan is aggregate - one nameable test anywhere made the whole answer
    // `Runnable` - so a provable file whose added `#[test]` yielded no name rode along
    // unmeasured, unmentioned. `b.rs` adds the attribute over a line no function name comes
    // out of, and `a.rs` naming its test fine is what used to hide it.
    let files = vec![
        changed("crates/x/src/a.rs", 1, &["#[test]", "fn reads_fine() {}"]),
        changed("crates/x/src/b.rs", 1, &["#[test]", "let _ = 1;"]),
    ];
    let read = tree(&[
        ("crates/x/src/a.rs", "#[test]\nfn reads_fine() {}\n"),
        ("crates/x/src/b.rs", "#[test]\nlet _ = 1;\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/a.rs"), String::from("crates/x/src/b.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/b.rs")]),
        other => panic!("expected Unreadable ahead of Runnable, got {other:?}"),
    }
}

#[test]
fn one_nameable_attribute_does_not_mask_another_in_the_same_file() {
    // The masking one level down, which `named > 0` left open: the file's inspection ended on
    // the first name, so the second attribute was dropped from the filter with nothing naming
    // it - and `super::coverage` could not surface it either, because both of its numbers come
    // from this extractor. Counting per ATTRIBUTE closes it, and the precision cost was
    // measured at zero over this tree before it was taken (this module's header).
    let file = concat!(
        "#[test]\n",       // 1
        "fn plain() {}\n", // 2
        "#[test]\n",       // 3
        "let _ = 1;\n",    // 4
    );
    let files = vec![changed(
        "crates/x/src/a.rs",
        1,
        &["#[test]", "fn plain() {}", "#[test]", "let _ = 1;"],
    )];
    let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    match Scan::of(&files, &[String::from("crates/x/src/a.rs")], &read) {
        Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/a.rs")]),
        other => panic!("expected Unreadable for the unnameable second attribute, got {other:?}"),
    }
}

#[test]
fn a_test_module_that_names_nothing_is_stated_rather_than_refused() {
    // The other unnameable shape, and it must NOT refuse: `lib.rs` gains
    // `#[cfg(test)] mod tests;` and the module's own file arrives in the same diff naming the
    // tests. That is the ordinary way a test module is added, so refusing per file would
    // redden a correct change - the declaration is carried as `silent`, RESOLVED to the file
    // that names them, and printed instead.
    let files = vec![
        changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod tests;"]),
        changed("crates/x/src/tests.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
        ("crates/x/src/tests.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/tests.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Runnable(ref scoped) => {
            assert_eq!(
                scoped.tests().iter().map(AddedTest::name).collect::<Vec<&str>>(),
                vec!["added"]
            );
            assert_eq!(
                scoped.silent(),
                [Silent {
                    path: String::from("crates/x/src/lib.rs"),
                    module: Some(String::from("crates/x/src/tests.rs")),
                }]
            );
        }
        other => panic!("expected Runnable with the declaration stated, got {other:?}"),
    }
}

#[test]
fn an_inline_module_added_around_existing_tests_is_stated_with_no_second_file() {
    // The third silent shape, and it must not refuse either: an added `mod tests {` whose
    // body is unchanged context enables nothing - those lines were already compiled. There is
    // no second file to look for, so the sentence beside it may not claim one.
    let file = concat!(
        "fn f() {}\n",            // 1
        "#[cfg(test)]\n",         // 2
        "mod tests {\n",          // 3
        "    #[test]\n",          // 4
        "    fn existing() {}\n", // 5
        "}\n",                    // 6
    );
    let files = vec![
        changed("crates/x/src/a.rs", 2, &["#[cfg(test)]", "mod tests {"]),
        changed("crates/x/src/b.rs", 1, &["#[test]", "fn added() {}"]),
    ];
    let read = tree(&[
        ("crates/x/src/a.rs", file),
        ("crates/x/src/b.rs", "#[test]\nfn added() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    let provable = vec![String::from("crates/x/src/a.rs"), String::from("crates/x/src/b.rs")];
    match Scan::of(&files, &provable, &read) {
        Scan::Runnable(ref scoped) => assert_eq!(
            scoped.silent(),
            [Silent {
                path: String::from("crates/x/src/a.rs"),
                module: None,
            }]
        ),
        other => panic!("expected the inline module to be stated, got {other:?}"),
    }
}

#[test]
fn two_tests_are_one_expression() {
    let files = vec![changed(
        "crates/x/tests/t.rs",
        1,
        &["#[test]", "fn one() {}", "#[test]", "fn two() {}"],
    )];
    let read = tree(&[
        ("crates/x/tests/t.rs", "#[test]\nfn one() {}\n#[test]\nfn two() {}\n"),
        ("crates/x/Cargo.toml", &manifest("x")),
    ]);
    assert_eq!(
        filterset(&files, &["crates/x/tests/t.rs"], &read),
        concat!(
            "(binary_id(=x::t) & test(/^(?:.*::)?one(?:::|$)/))",
            " + (binary_id(=x::t) & test(/^(?:.*::)?two(?:::|$)/))"
        )
    );
}

#[test]
fn an_ignored_test_leaves_the_scope_rather_than_emptying_the_run() {
    // Measured on the pinned nextest: a filterset naming only `#[ignore]`d tests matches
    // nothing and exits 4 with `error: no tests to run`, which the gate read as a failure.
    // `#[ignore]` is legal on either side of `#[test]`, so both orders are dropped, and the
    // runnable neighbour is still proven.
    let file = concat!(
        "#[test]\n",                      // 1
        "#[ignore = \"needs a tier\"]\n", // 2
        "fn below() {}\n",                // 3
        "#[ignore]\n",                    // 4
        "#[test]\n",                      // 5
        "fn above() {}\n",                // 6
        "#[test]\n",                      // 7
        "fn runs() {}\n",                 // 8
    );
    let files = vec![changed(
        "crates/x/tests/t.rs",
        1,
        &[
            "#[test]",
            "#[ignore = \"needs a tier\"]",
            "fn below() {}",
            "#[ignore]",
            "#[test]",
            "fn above() {}",
            "#[test]",
            "fn runs() {}",
        ],
    )];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(
        runnable(&files, &["crates/x/tests/t.rs"], &read),
        Some(vec![String::from("runs")])
    );
}

#[test]
fn a_test_under_an_attribute_the_formatter_wrapped_is_still_named() {
    // THE DEFECT. `#[test]` over a wrapped `#[expect(..)]` is how eight tests in this tree are
    // written, and the downward search stopped on `clippy::disallowed_methods,` - so
    // `function_name` got that instead of a signature and no name came out. Survivable while
    // the scan was aggregate and silently skipped the file; after `Scan::Unreadable` refuses
    // ahead of `Runnable` it is a hard red on a correct change, which is the failure mode
    // `report_unreadable`'s own doc argues against.
    let file = concat!(
        "#[test]\n",                                  // 1
        "#[expect(\n",                                // 2
        "    clippy::disallowed_methods,\n",          // 3
        "    reason = \"exposing it IS the test\"\n", // 4
        ")]\n",                                       // 5
        "fn expose_secret_returns_the_value() {}\n",  // 6
        "#[test]\n",                                  // 7
        "fn reads_fine() {}\n",                       // 8
    );
    let files = vec![changed(
        "crates/x/src/a.rs",
        1,
        &[
            "#[test]",
            "#[expect(",
            "    clippy::disallowed_methods,",
            "    reason = \"exposing it IS the test\"",
            ")]",
            "fn expose_secret_returns_the_value() {}",
            "#[test]",
            "fn reads_fine() {}",
        ],
    )];
    let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(
        runnable(&files, &["crates/x/src/a.rs"], &read),
        Some(vec![
            String::from("expose_secret_returns_the_value"),
            String::from("reads_fine")
        ])
    );
}

#[test]
fn an_ignore_the_formatter_wrapped_still_leaves_the_scope() {
    // The worse half of the same defect, because it costs a PASS rather than a name.
    // `#[ignore = ".."]` continued with a trailing `\` is one attribute over two lines, and
    // reading the second as an item detached the `#[ignore]` from the test - so the only
    // added test entered the filterset, nextest matched nothing, and `Scan::OnlyIgnored`'s
    // loud pass was unreachable. Two tests in `crates/sutura-catalog-datahub/tests` are
    // written exactly this way.
    let file = concat!(
        "#[test]\n",                                                    // 1
        "#[ignore = \"needs `just dev-up-datahub`; run that task \\\n", // 2
        "            instead\"]\n",                                     // 3
        "fn the_provisioned_surface_answers() {}\n",                    // 4
    );
    let files = vec![changed(
        "crates/x/tests/t.rs",
        1,
        &[
            "#[test]",
            "#[ignore = \"needs `just dev-up-datahub`; run that task \\",
            "            instead\"]",
            "fn the_provisioned_surface_answers() {}",
        ],
    )];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
        Scan::OnlyIgnored(ref names) => {
            assert_eq!(
                names.iter().map(Ident::as_str).collect::<Vec<&str>>(),
                vec!["the_provisioned_surface_answers"]
            );
        }
        other => panic!("a wrapped `#[ignore]` still leaves the scope, got {other:?}"),
    }
}

#[test]
fn a_diff_whose_every_added_test_is_ignored_is_named_not_refused() {
    // The other half of the same measurement, and the reason it is a third answer rather
    // than the empty scan: an all-`#[ignore]`d diff is not an extractor bug, so it must not
    // print one. The names come back so the report can say what it could not measure.
    let file = "#[test]\n#[ignore]\nfn acceptance() {}\n";
    let files = vec![changed(
        "crates/x/tests/t.rs",
        1,
        &["#[test]", "#[ignore]", "fn acceptance() {}"],
    )];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
        Scan::OnlyIgnored(ref names) => {
            assert_eq!(names.iter().map(Ident::as_str).collect::<Vec<&str>>(), vec!["acceptance"]);
        }
        other => panic!("an ignored test is named, got {other:?}"),
    }
}

#[test]
fn a_signature_the_formatter_wrapped_after_the_paren_is_still_named() {
    // WHAT `function_name` ACTUALLY REACHES, which its doc used to record as a measurement and
    // nothing executable held. rustfmt breaks a signature too long for one line AFTER the `(`,
    // never before the name, so the first line of a wrapped `async fn` still carries
    // `fn <name>(` - and a rename of the extractor or a narrowing of its split characters
    // would have taken that claim with it in silence.
    let file = concat!(
        "#[tokio::test]\n",                                                    // 1
        "async fn the_exchange_chain_joined_through_the_transport_answers(\n", // 2
        "    provisioned: &Provisioned,\n",                                    // 3
        ") -> Result<(), Box<dyn std::error::Error>> {\n",                     // 4
        "    Ok(())\n",                                                        // 5
        "}\n",                                                                 // 6
    );
    let files = vec![changed(
        "crates/x/tests/t.rs",
        1,
        &[
            "#[tokio::test]",
            "async fn the_exchange_chain_joined_through_the_transport_answers(",
            "    provisioned: &Provisioned,",
            ") -> Result<(), Box<dyn std::error::Error>> {",
            "    Ok(())",
            "}",
        ],
    )];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(
        runnable(&files, &["crates/x/tests/t.rs"], &read),
        Some(vec![String::from("the_exchange_chain_joined_through_the_transport_answers")])
    );
}

#[test]
fn an_added_line_inside_a_signature_this_diff_did_not_open_is_named_by_the_enclosing_item() {
    // `github.com/telekom/sutura#1025`. UNTIL NOW, THE STATED LIMIT: the same wrapped
    // signature, with only its PARAMETER line added - a def-interior edit - named nothing,
    // because `function_name` never walks UP from an interior line and `declared_under` only
    // ever asks about an ADDED attribute. `super::edited` asks the opposite direction, from the
    // PRE-existing `#[tokio::test]` down to this same function, so the enclosing item names it.
    let file = concat!(
        "#[tokio::test]\n",                                                    // 1
        "async fn the_exchange_chain_joined_through_the_transport_answers(\n", // 2
        "    provisioned: &Provisioned,\n",                                    // 3
        ") -> Result<(), Box<dyn std::error::Error>> {\n",                     // 4
        "    Ok(())\n",                                                        // 5
        "}\n",                                                                 // 6
    );
    let files = vec![changed("crates/x/tests/t.rs", 3, &["    provisioned: &Provisioned,"])];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(
        runnable(&files, &["crates/x/tests/t.rs"], &read),
        Some(vec![String::from("the_exchange_chain_joined_through_the_transport_answers")])
    );
}

#[test]
fn a_stray_attribute_does_not_reach_down_the_file() {
    // The attribute is the last line of its module, so there is no function under it. Naming
    // the next test in the file would scope in something the diff did not add - so it is
    // refused instead, which is what a test-declaring attribute yielding no name now means.
    let file = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    #[test]\n",
        "}\n",
        "#[test]\n",
        "fn elsewhere() {}\n"
    );
    let files = vec![changed("crates/x/src/a.rs", 3, &["    #[test]"])];
    let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert_eq!(runnable(&files, &["crates/x/src/a.rs"], &read), None);
}

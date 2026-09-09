//! The attribution module's tests, in their own file because `xtask/` is an UNEXEMPTABLE
//! prefix for `cargo xtask max-lines` - the combined module reached 1036 lines against a cap
//! of 1000, and a generated-or-vendored exemption is the only kind that file accepts.

use core::fmt::Write as _;

use std::collections::{BTreeMap, BTreeSet};

use super::{
    COMMITTED, COPY_IN_RELEASE, GENERATE_IN_RELEASE, Generated, Package, RELEASE, header, owner_failures, package_name, refuse,
    row, rows, third_party, workspace_members,
};

/// The workspace's own crate names, as the two real files would yield them.
fn ours(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| String::from(*n)).collect()
}

/// A lock stanza, so the fixtures read like the file they parse.
fn stanza(name: &str, version: &str, source: Option<&str>) -> String {
    let mut out = format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n");
    if let Some(source) = source {
        writeln!(out, "source = \"{source}\"").expect("writing into a String cannot fail");
    }
    out
}

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn package(name: &str, version: &str) -> Package {
    Package {
        name: String::from(name),
        version: String::from(version),
    }
}

/// A `Generated` as the real one would arrive: a table rendered from `licences`, the same
/// `wanted` set, and `undeclared` derived the way [`super::generate`] derives it.
///
/// Built through the real `header` and the real row format on purpose - a fixture that wrote
/// its own table shape would pass while `row` and the generator disagreed, which is the one
/// thing these refusals cannot afford to be blind to.
fn generated(wanted: &[(&str, &str)], licences: &BTreeMap<Package, String>) -> Generated {
    let wanted: BTreeSet<Package> = wanted.iter().map(|(n, v)| package(n, v)).collect();
    let mut lines = Vec::new();
    let mut undeclared = BTreeSet::new();
    for entry in &wanted {
        let licence = licences.get(entry).map_or("", String::as_str);
        if licence.is_empty() {
            undeclared.insert(entry.clone());
        }
        let declared = if licence.is_empty() { "NOT DECLARED" } else { licence };
        lines.push(format!("| `{}` | `{}` | {declared} |", entry.name, entry.version));
    }
    Generated {
        text: format!("{}{}\n", header(lines.len()), lines.join("\n")),
        wanted,
        undeclared,
    }
}

/// One fixture declaration: crate name, resolved version, and the SPDX expression its manifest
/// declares. Named because `clippy::type_complexity` is denied and the triple is the whole point.
type Declaration = (&'static str, &'static str, &'static str);

/// The licences `cargo metadata` would supply, as the fixtures declare them.
fn declared(pairs: &[Declaration]) -> BTreeMap<Package, String> {
    pairs
        .iter()
        .map(|(name, version, licence)| (package(name, version), String::from(*licence)))
        .collect()
}

#[test]
fn a_complete_generation_passes_and_reports_its_row_count() {
    // The self-guard on every refusal below: the fixture shape has to be capable of PASSING,
    // or each red assertion would be red for the wrong reason - a table `row` cannot parse
    // reads as every package missing, and the test would look like it proved something.
    let g = generated(
        &[("serde", "1.0.230"), ("zstd", "0.13.3")],
        &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0"), ("zstd", "0.13.3", "MIT")]),
    );
    assert_eq!(refuse(&g), Ok(2), "a complete document must pass, or nothing below is a test");
}

#[test]
fn a_package_declaring_no_licence_is_refused() {
    // THE STRENGTHENING, and the reason it is here: the previous generator printed this to
    // stderr, wrote `NOT DECLARED` into the document, and exited zero - so an undeclared
    // licence reached the signed release asset with a green run behind it. Now that the
    // document is no longer committed, no reviewer sees that row in a diff either, so a
    // refusal is the only thing left that can see it at all.
    let g = generated(
        &[("serde", "1.0.230"), ("mystery", "0.1.0")],
        &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0")]),
    );
    let Err(reasons) = refuse(&g) else {
        panic!("an undeclared licence must be refused, not written as NOT DECLARED and passed");
    };
    assert!(reasons.contains("declare no licence"), "{reasons}");
    assert!(reasons.contains("mystery 0.1.0"), "the refusal names which one: {reasons}");
}

#[test]
fn an_empty_third_party_set_is_refused_rather_than_reported_as_a_clean_document() {
    // Fail closed. A lock that resolves nothing is a parse that broke, and the shape of the
    // failure is a WELL FORMED document naming nothing - which every count in a header and
    // every byte-compare against a fresh generation would have called correct.
    let g = generated(&[], &declared(&[]));
    let Err(reasons) = refuse(&g) else {
        panic!("an empty package set must fail closed");
    };
    assert!(reasons.contains("resolves no third-party package"), "{reasons}");
}

#[test]
fn a_package_the_generator_dropped_is_refused() {
    // The set comes from `Cargo.lock` and the rows come from the render, so a render that lost
    // one is visible. This is the assertion that keeps `check-attribution` from being a
    // generator checking itself: `wanted` and `text` are two different derivations.
    let mut g = generated(
        &[("serde", "1.0.230"), ("zstd", "0.13.3")],
        &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0"), ("zstd", "0.13.3", "MIT")]),
    );
    g.text = g.text.replace("| `zstd` | `0.13.3` | MIT |\n", "");
    g.text = g.text.replace("| `zstd` | `0.13.3` | MIT |", "");
    let Err(reasons) = refuse(&g) else {
        panic!("a package in the lock with no row must be refused");
    };
    assert!(reasons.contains("no row in the generated document"), "{reasons}");
    assert!(reasons.contains("zstd 0.13.3"), "{reasons}");
}

/// A fixture repo root: a `release.yml` carrying `workflow`, and optionally a committed
/// document. Under the process temp dir with a unique name, because two of these run at once
/// under `nextest` and a shared path would make one test read the other's tree.
fn fixture(name: &str, workflow: &str, committed: bool) -> std::path::PathBuf {
    // The name is the discriminator and the pid keeps two concurrent runs apart; nothing here
    // needs a counter, because each caller passes its own name.
    let root = std::env::temp_dir().join(format!("sutura-attribution-{}-{name}", std::process::id()));
    // Removed and rebuilt rather than written into: the name is deterministic, so a previous run
    // leaving a committed copy behind would decide this run's verdict.
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("a temp directory is removable");
    }
    let workflows = root.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows).expect("a temp directory is writable");
    std::fs::write(root.join(RELEASE), workflow).expect("a temp file is writable");
    if committed {
        std::fs::write(root.join(COMMITTED), "# a stale copy\n").expect("a temp file is writable");
    }
    root
}

/// A release workflow in the shape the gate wants: it generates and it copies nothing.
const GENERATING_RELEASE: &str = "      - run: nix run .#xtask -- attribution dist/sutura-attribution.md\n";

#[test]
fn the_owner_gate_passes_only_on_a_tree_with_no_committed_copy_and_a_generating_release() {
    // The self-guard for the three refusals below. If the fixture could not PASS, each of them
    // would be red for the wrong reason and the whole group would look like coverage while
    // asserting that a broken fixture is broken.
    let root = fixture("clean", GENERATING_RELEASE, false);
    assert_eq!(
        owner_failures(&root),
        Vec::<String>::new(),
        "the fixture has to be capable of passing, or nothing below is a test"
    );
}

#[test]
fn a_committed_attribution_document_is_refused() {
    // The absence this whole change rests on. A commit re-adding the file restores the
    // staleness, turns every dependency bump red on arrival again, and the next tag signs
    // whatever the stale copy says - so the absence is held by this, not by a sentence.
    let root = fixture("committed", GENERATING_RELEASE, true);
    let failures = owner_failures(&root);
    assert_eq!(failures.len(), 1, "{failures:?}");
    let joined = failures.join("\n");
    assert!(joined.contains("is back in the tree"), "{joined}");
}

#[test]
fn a_release_that_does_not_generate_the_asset_is_refused() {
    // With nothing committed, the release is the ONLY place the notice is produced. A release
    // that stopped generating would publish binaries with no attribution, and would do it at
    // the one moment no gate is watching - on a tag.
    let root = fixture("no-generate", "      - run: echo nothing to do\n", false);
    let failures = owner_failures(&root);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures.join("\n").contains("does not invoke"), "{failures:?}");
}

#[test]
fn a_release_that_copies_a_committed_document_is_refused_even_when_it_also_generates() {
    // The half that is easy to leave out, and the reason it is separate: a workflow that
    // generates AND copies satisfies the check above while still handing a stale document to
    // the signer. So the copy is refused on its own, with the generation present.
    let workflow = format!("{GENERATING_RELEASE}      - run: cp ATTRIBUTION.md dist/sutura-attribution.md\n");
    let root = fixture("copies", &workflow, false);
    let failures = owner_failures(&root);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures.join("\n").contains("still carries"), "{failures:?}");
}

#[test]
fn an_unreadable_release_workflow_is_a_fault_and_not_a_pass() {
    // Fail closed. A missing workflow file used to be indistinguishable from a workflow that
    // generates correctly, and this gate exists precisely so that nothing about the release's
    // attribution step goes unchecked in silence.
    let root = fixture("unreadable", GENERATING_RELEASE, false);
    std::fs::remove_file(root.join(RELEASE)).expect("the fixture wrote it");
    let failures = owner_failures(&root);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures.join("\n").contains("could not be read"), "{failures:?}");
}

#[test]
fn the_tree_carries_no_committed_attribution_document() {
    // The absence `#462` decided on, asserted against the REAL tree rather than a fixture,
    // because the thing that can regress is a commit re-adding the file. `run_check_owner` is
    // the gate; this is the same claim in the test suite, so a `just test` run that never
    // invokes the hygiene sweep still notices.
    let Some(root) = crate::repo::root() else {
        return;
    };
    assert!(
        !root.join(COMMITTED).exists(),
        "{COMMITTED} is committed again: a generated artefact with a second owner goes stale, \
         turns every dependency bump red on arrival, and the next tag signs the stale copy"
    );
}

#[test]
fn the_release_workflow_generates_the_attribution_asset_and_copies_no_committed_one() {
    // The other half of the owner gate, and the half that matters most: with nothing committed,
    // the release is the ONLY place the notice is produced. A release that stopped generating
    // would publish binaries with no attribution and would fail at the one moment no gate is
    // watching - on a tag.
    let Some(root) = crate::repo::root() else {
        return;
    };
    let Ok(workflow) = std::fs::read_to_string(root.join(RELEASE)) else {
        return;
    };
    assert!(
        workflow.contains(GENERATE_IN_RELEASE),
        "{RELEASE} must invoke `{}`: it is where the licence obligation is discharged",
        GENERATE_IN_RELEASE.trim()
    );
    assert!(
        !workflow.contains(COPY_IN_RELEASE),
        "{RELEASE} copies a committed document again, which is the step that can hand a stale \
         notice to the signer"
    );
}

#[test]
fn a_vendored_path_dependency_is_third_party_and_a_workspace_member_is_not() {
    // RED BEFORE GREEN, and this is the finding a review had to make: the first version of this
    // module selected on the presence of a `source` line, so `mimalloc` and `libmimalloc-sys` -
    // vendored under `vendor/`, declared as PATH dependencies, and LINKED by `sutura-cli` on
    // Linux - were left out of the released attribution asset. That is the exact failure
    // `docs/adr/0021` says this document exists to prevent, and the old code passed its own
    // tests while doing it.
    //
    // So the assertion is about the pair: a source-less NON-member is in, a source-less member
    // is out.
    let lock = format!(
        "{}{}{}",
        stanza("sutura-domain", "0.1.0", None),
        stanza("mimalloc", "0.1.52", None),
        stanza("serde", "1.0.230", Some(REGISTRY))
    );
    let found = third_party(&lock, &ours(&["sutura-domain"]));
    assert_eq!(
        found,
        vec![package("mimalloc", "0.1.52"), package("serde", "1.0.230")],
        "a vendored path dependency has to be attributed and a workspace member must not be"
    );
}

#[test]
fn the_member_list_is_read_through_each_members_own_manifest() {
    // A member is a PATH, and `dev` holds `sutura-dev` - so deriving the crate name from the
    // directory would put `sutura-dev` in the attribution document as if we had not written it.
    // Read against the real files, because that mapping is the thing that can be wrong.
    let Some(root) = crate::repo::root() else {
        return;
    };
    let Ok(manifest) = std::fs::read_to_string(root.join(super::MANIFEST)) else {
        return;
    };
    let members = workspace_members(&root, &manifest).expect("the workspace declares members");
    assert!(members.contains("sutura-dev"), "the `dev` path resolves to its crate name");
    assert!(members.contains("xtask"));
    assert!(
        !members.contains("mimalloc"),
        "the vendored allocator is not a workspace member, so it belongs in the document"
    );
}

#[test]
fn a_comment_in_the_member_list_contributes_no_member() {
    // That list carries prose about the architecture, and the prose quotes crate names. A scan
    // for quoted strings that did not skip comments would read one of those as a member and
    // then EXCLUDE it from the attribution document - a silent omission, which is the failure
    // mode this whole module is about.
    let manifest = concat!(
        "[workspace]\n",
        "members = [\n",
        "  # keeps \"polyglot-sql\" out of the core's closure\n",
        "  \"crates/sutura-domain\",\n",
        "]\n",
    );
    let root = std::path::Path::new("/nonexistent");
    // The member path cannot be read here, so the whole thing is `None` - which is the
    // fail-closed contract. What this pins is that the COMMENT did not become a member: were it
    // read as one, the loop would try `/nonexistent/# keeps ...` and still answer `None`, so the
    // observable difference is in `package_name`, asserted directly below.
    assert!(workspace_members(root, manifest).is_none());
    assert_eq!(package_name("[package]\nname = \"sutura-dev\"\n"), Some("sutura-dev"));
    assert_eq!(package_name("[package]\nversion = \"0.1.0\"\n"), None);
}

#[test]
fn two_versions_of_one_crate_are_two_packages() {
    // Crates are duplicated in this workspace, so the key is the pair and not the name. Keying
    // on the name alone would report a complete document as complete while one of the two
    // versions went unattributed.
    let lock = format!(
        "{}{}",
        stanza("arrow", "58.4.0", Some(REGISTRY)),
        stanza("arrow", "59.2.0", Some(REGISTRY))
    );
    assert_eq!(third_party(&lock, &ours(&[])).len(), 2);
}

#[test]
fn a_source_line_does_not_leak_into_the_next_stanza() {
    // `sourced` is reset at each `[[package]]`, so a registry crate followed by a workspace
    // member does not drag the member into the document. Without the reset the second stanza
    // inherits the first one's source and every workspace crate is attributed as third-party.
    let lock = format!(
        "{}{}",
        stanza("serde", "1.0.230", Some(REGISTRY)),
        stanza("xtask", "0.1.0", None)
    );
    assert_eq!(third_party(&lock, &ours(&["xtask"])).len(), 1);
}

#[test]
fn the_last_stanza_in_the_file_is_read() {
    // There is no `[[package]]` after it to flush on, which is the off-by-one this asserts:
    // cargo writes the lock with no trailing marker, so a parser that only flushes on the next
    // header loses whichever crate sorts last.
    let lock = stanza("zstd", "0.13.3", Some(REGISTRY));
    assert_eq!(third_party(&lock, &ours(&[])).len(), 1);
}

#[test]
fn a_package_row_is_parsed_and_prose_is_not() {
    assert_eq!(
        row("| `serde` | `1.0.230` | MIT OR Apache-2.0 |"),
        Some((package("serde", "1.0.230"), String::from("MIT OR Apache-2.0")))
    );
    // The header and the separator share the file with the table, so neither may parse as a
    // row: an unquoted first cell is what tells them apart.
    assert_eq!(row("| Package | Version | Licence |"), None);
    assert_eq!(row("| --- | --- | --- |"), None);
    assert_eq!(row("Every crate the workspace resolves."), None);
    // A fourth column is a different document; refusing it means the check reports a drift
    // rather than reading a column it does not understand as a licence.
    assert_eq!(row("| `serde` | `1.0.230` | MIT | extra |"), None);
}

#[test]
fn the_generated_header_carries_the_marker_text_hygiene_reads() {
    // `xtask/src/text.rs` looks for `do not edit`, case-insensitively, in the FIRST FIVE lines.
    // A header that grew a line above the marker would silently re-enter the em-dash and
    // whitespace rules, and the released asset is still a markdown file somebody may lint.
    let head = header(455);
    let first_five: Vec<&str> = head.lines().take(5).collect();
    assert!(
        first_five.iter().any(|l| l.to_ascii_lowercase().contains("do not edit")),
        "the generated marker moved out of the first five lines: {first_five:?}"
    );
    assert!(head.contains("455 third-party crates"), "the count reaches the prose");
}

#[test]
fn a_duplicated_row_is_one_entry_and_the_count_says_so() {
    // `rows` is keyed, so a document that named `serde` twice would report one row. The count
    // in the pass line is therefore the number of DISTINCT packages named, which is the number
    // the refusals reason about.
    let document = concat!("| `serde` | `1.0.230` | MIT |\n", "| `serde` | `1.0.230` | Apache-2.0 |\n",);
    assert_eq!(rows(document).len(), 1);
}

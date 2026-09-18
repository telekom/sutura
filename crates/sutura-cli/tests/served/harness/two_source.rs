//! The two-`files`-sources fixture: one catalog, one line of derivation, two `sources:` entries.
//!
//! Split out of `harness.rs` by the same `max-lines` reason as every sibling module there - see
//! its own header for why the split moves the HARNESS and not a test.

use std::path::{Path, PathBuf};

use super::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, config_path, derived_beside, example_root, files_source, settings_over};

/// The SECOND data system, which only the two-source deployment declares.
pub(crate) const LOOKUP_SOURCE: &str = "geo";

/// **The two-source deployment: the example's own catalog with ONE line rewritten, and two `files`
/// entries over the one data directory.**
///
/// The rewrite puts the `customers` model on [`LOOKUP_SOURCE`], which is the whole difference - it
/// is the same one-line derivation `crates/sutura-app/tests/differential/federated.rs` makes, and
/// it is derived rather than committed for that file's reason: a second-source topology is one
/// deployment's, not something a single-source quickstart can state.
///
/// **The two sources share a data directory, and the isolation is real anyway.** `open_files` builds
/// one adapter per declared `files` entry and attaches only that entry's models' tables, so neither
/// engine has the other's table registered and a join across them has to happen above the port or
/// not at all. What is under test is that behaviour of the composition root, so pointing both at
/// one directory removes a variable rather than adding one - two directories would differ in what
/// was copied as well as in what was attached.
///
/// A `Rewrite` that found nothing PANICS: a derivation that silently stopped applying would leave
/// this deployment single-source and the test below green over a whole-plan answer.
pub(crate) fn settings_spanning_two_sources(case: &str) -> String {
    let example = example_root();
    let data = example.join("data");
    let catalog = derived_catalog(case, &example.join("catalog"), "customers.md", LOOKUP_SOURCE);
    let sources = format!("{}{}", files_source(LOCAL_SOURCE, &data), files_source(LOOKUP_SOURCE, &data));
    settings_over(
        &catalog,
        &data,
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    )
}

/// The example catalog, copied, with one model moved off [`LOCAL_SOURCE`] onto `moved_to`.
///
/// Copied rather than edited in place for the obvious reason and one less obvious: this suite runs
/// beside every other gate in one checkout, so a test that rewrote a committed document would
/// change what a concurrent run reads. Shared by [`settings_spanning_two_sources`] and `two_kind`'s
/// own settings builder, one model per caller.
pub(crate) fn derived_catalog(case: &str, from: &Path, model_file: &str, moved_to: &str) -> PathBuf {
    let root = derived_beside(&config_path(case));
    drop(std::fs::remove_dir_all(&root));
    copied(from, &root);
    let model = root.join("models").join(model_file);
    let text = std::fs::read_to_string(&model).expect("the derived catalog carries the model this case moves");
    let moved = text.replace(&format!("source: {LOCAL_SOURCE}"), &format!("source: {moved_to}"));
    assert_ne!(
        moved,
        text,
        "{} no longer declares `source: {LOCAL_SOURCE}`, so this deployment is not two-source and \
         the question below would be answered whole",
        model.display()
    );
    std::fs::write(&model, moved).expect("the derived model document is writable");
    root
}

/// One directory tree, copied.
///
/// `std::fs` has no recursive copy and this suite has no dev-dependency that does; the catalog is
/// two levels of markdown, so a six-line walk is cheaper than a crate.
fn copied(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the derived catalog directory is creatable");
    let entries = std::fs::read_dir(from).unwrap_or_else(|cause| panic!("{} is not readable: {cause}", from.display()));
    for entry in entries {
        let entry = entry.expect("a directory entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copied(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a catalog document is copyable");
        }
    }
}

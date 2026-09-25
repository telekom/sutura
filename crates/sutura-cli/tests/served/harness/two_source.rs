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
    without_the_chained_dimension(&catalog);
    let sources = format!("{}{}", files_source(LOCAL_SOURCE, &data), files_source(LOOKUP_SOURCE, &data));
    settings_over(
        &catalog,
        &data,
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    )
}

/// Drops the one dimension the example reaches through a CHAIN, which this topology cannot carry.
///
/// **Not a convenience: a federated plan carries one link and one lookup TABLE.** `sales_area` is
/// reached `subscription_customer` then `customer_region`; hop 1 is the link into the second data
/// system and there is nowhere for hop 2 to be planned. With `customers` moved onto
/// [`LOOKUP_SOURCE`] the chain would cross at hop 1 and come back at hop 2 -
/// `sutura_domain::catalog` refuses that bundle by name, so `sutura serve` would stop at boot
/// rather than answer, which is exactly what it did before this call existed.
///
/// Applied only here, and not inside [`derived_catalog`]: `two_kind`'s and `bigquery.rs`'s
/// deployments move `daily_usage` instead, and [`without_the_product_family_dimension`] is their
/// own strip, for the chain `daily_usage` is now the root of.
///
/// A block that no longer matches PANICS, for [`derived_catalog`]'s reason: a strip that silently
/// stopped applying would leave this deployment refusing to start and the diagnosis somewhere else.
fn without_the_chained_dimension(root: &Path) {
    let metric = root.join("metrics").join("recurring_revenue.md");
    let text = std::fs::read_to_string(&metric).expect("the derived catalog carries the metric this case edits");
    let stripped = text.replace(CHAINED_DIMENSION, "");
    assert_ne!(
        stripped,
        text,
        "{} no longer declares the chained dimension this fixture strips, so a corpus edit has          left this deployment describing something else",
        metric.display()
    );
    std::fs::write(&metric, stripped).expect("the derived metric document is writable");
}

/// The block [`without_the_chained_dimension`] removes, as `examples/single-player` writes it.
const CHAINED_DIMENSION: &str = "  - name: sales_area\n    column: sales_area\n    via: [subscription_customer, customer_region]\n    values: [central, north_east, south_west]\n    description: >\n      Which sales area the customer's region rolls up into. The one dimension here\n      reached through a chain of two relationships rather than one.\n";

/// Drops `voice_minutes`'s one chained dimension, for [`without_the_chained_dimension`]'s reason
/// applied to `daily_usage` instead of `customers`.
///
/// **`daily_usage` is no longer the model with no `via` relationship at all.** Since
/// `usage_subscription` landed, `product_family` reaches through it and then
/// `subscription_product` - hop 1 crosses into whichever source `daily_usage` moves to, and hop 2
/// stays on `subscriptions`'s own source. Moving `daily_usage` off [`LOCAL_SOURCE`] (`bigquery.rs`'s
/// and `two_kind`'s own deployments both do) makes hop 2 cross AGAIN, back onto the model's own
/// source - `sutura_domain::catalog::Definitions::assemble` refuses that bundle by name, the same
/// shape [`without_the_chained_dimension`] exists for. Called wherever `daily_usage.md` moves.
#[cfg(feature = "postgres")]
pub(crate) fn without_the_product_family_dimension(root: &Path) {
    let metric = root.join("metrics").join("voice_minutes.md");
    let text = std::fs::read_to_string(&metric).expect("the derived catalog carries the metric this case edits");
    let stripped = text.replace(PRODUCT_FAMILY_VIA_USAGE, "");
    assert_ne!(
        stripped,
        text,
        "{} no longer declares the chained dimension this fixture strips, so a corpus edit has \
         left this deployment describing something else",
        metric.display()
    );
    std::fs::write(&metric, stripped).expect("the derived metric document is writable");
}

/// The block [`without_the_product_family_dimension`] removes, as `examples/single-player` writes it.
#[cfg(feature = "postgres")]
const PRODUCT_FAMILY_VIA_USAGE: &str = "dimensions:\n  - name: product_family\n    column: product_family\n    via: [usage_subscription, subscription_product]\n    values: [convergent, fixed_internet, mobile, tv]\n    description: >\n      The kind of product the subscription that used the minutes belongs to. Reached through\n      `usage_subscription` - the compound join from a usage day to the monthly snapshot -\n      and then on to the product. Grouping by it does not multiply the minutes, because the\n      compound key stops every usage day from joining every month that subscription existed.\n";

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

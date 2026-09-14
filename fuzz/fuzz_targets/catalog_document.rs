//! A catalog document: arbitrary bytes through the real loading port.
//!
//! **The boundary.** Catalog content is untrusted by this repository's threat model - whoever
//! authors a definition is not the operator. The bytes go through `LocalCatalog::load`, which is
//! the `SemanticCatalog` port method the service calls, so the frontmatter split, `serde_norway`,
//! the document shapes and the domain newtypes behind them (`Phrase`, `NoteBody`, `Description`,
//! `QualifiedTable`, `Date`) are all reached the way a deployment reaches them.
//!
//! **The real port and not a re-implementation of it, deliberately.** Driving the public pieces
//! in sequence would have been faster per iteration and would have fuzzed a copy of the read path
//! that can drift from the read path - which is the failure mode where a harness runs, finds
//! nothing, and looks like coverage. The cost is one file write per iteration.
//!
//! **What is asserted beyond "did not abort".** That the load is deterministic: the same bytes
//! yield the same verdict and, when they load, the same definition digest. The digest is taken over
//! the serialized definitions and is what a pinned answer cites, so a load that depended on
//! anything but its input would move a digest without a definition changing.
//!
//! **One fuzzed document beside fixed scaffolding, and that choice is what makes the target reach
//! anything.** A metric names a model and a glossary/caveat/example names a metric, so a directory
//! holding only a generated document assembles almost never - the load fails on the missing
//! reference before `Definitions::assemble` or `Knowledge::assemble`, the digest and the knowledge
//! half are ever reached. The scaffold therefore carries one metric on top of the one model, plus a
//! second model, so a generated document of ANY `DocumentKind` - not only `metric`/`model` - can
//! resolve: a knowledge document points `about`/`means`/`question` at the scaffold metric
//! (`subscription_count`), and a relationship document joins the two scaffold models.
//!
//! **The limit.** One generated document per iteration, so nothing here reaches a *pair* of
//! generated documents disagreeing with each other, or the directory walk's symlink handling.
//! Non-UTF-8 bytes are converted lossily before the write (see the note inline below); the harness
//! therefore does not cover `std::fs::read_to_string`'s own non-UTF-8 refusal in the read path.
//!
//! **Provenance.** Ported from `origin/test/fuzz-the-untrusted-parsers:fuzz/fuzz_targets/catalog_document.rs`
//! as a content port rather than a rebase - its own scaffolding predates the merged fuzz-crate
//! structure this file now lives in. The `LocalCatalog::new`/`.load()`/`PinnedDefinitions::digest()`
//! signatures used below are unchanged from that branch; the scaffold itself (a scaffold metric, a
//! second model) is new here, to close the gap a design review found: with one bare model and no
//! metric, a glossary, caveat, example or relationship document could never load `Ok`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog};

/// The fixed model a generated metric, relationship or knowledge document may refer to.
const SCAFFOLD_MODEL: &str = "---
kind: model
name: subscriptions
source: local
table: fact_subscription
columns: [subscription_key, month, status, segment, product_key]
---
One row per subscription per month.
";

/// A second fixed model, so a generated `kind: relationship` document has two ends to join -
/// distinct from `subscriptions` and from every model name a committed seed itself declares
/// (`customers` in `minimal-model`), so neither collides with the other.
const SCAFFOLD_SECOND_MODEL: &str = "---
kind: model
name: products
source: local
table: dim_product
columns: [product_key, name]
---
One row per product.
";

/// A fixed metric on `subscriptions`, named apart from every metric a committed seed itself
/// declares (`active_subscriptions` in `minimal-metric`, `mit_umlaut` in `multibyte-frontmatter`),
/// so a generated glossary, caveat or example document has something real to point `means:`,
/// `about:` or `question:` at without colliding with what the seed under test declares itself.
const SCAFFOLD_METRIC: &str = "---
kind: metric
name: subscription_count
model: subscriptions
measure:
  simple: { aggregate: count_distinct, column: subscription_key }
time_column: month
grains: [month]
---
How many subscriptions existed, the scaffold's own metric.
";

/// One directory for the whole run, reused per iteration.
///
/// Per-process rather than per-iteration: creating and removing a directory 100k times measures the
/// filesystem, not the parser. The generated document is truncated and rewritten instead.
fn catalog() -> &'static LocalCatalog {
    static ROOT: std::sync::OnceLock<LocalCatalog> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("sutura-fuzz-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the fuzz harness can create its own scratch directory");
        std::fs::write(root.join("model.md"), SCAFFOLD_MODEL).expect("the fuzz harness can write its own scaffolding");
        std::fs::write(root.join("product.md"), SCAFFOLD_SECOND_MODEL)
            .expect("the fuzz harness can write its own scaffolding");
        std::fs::write(root.join("metric.md"), SCAFFOLD_METRIC).expect("the fuzz harness can write its own scaffolding");
        LocalCatalog::new(
            SourceName::parse("fuzz").expect("a fixture source name is one"),
            root,
            DefinitionVersion::parse("0.0.0-fuzz").expect("a fixture version is one"),
        )
    })
}

fuzz_target!(|data: &[u8]| {
    let catalog = catalog();
    let document = catalog.root().join("generated.md");
    // Lossy, and this line is the difference between a harness that explores the parser and one
    // that explores `str::from_utf8`. The branch this target is ported from measured it directly:
    // writing the raw bytes, four minutes and 27305 executions did not reach a planted
    // char-count-as-byte-offset defect in the frontmatter splitter, because the reader takes the
    // document through `read_to_string` - so every mutation that introduced a multibyte character
    // also had to leave the whole rest of the file valid UTF-8, and one that did not was rejected
    // before any parser ran and returned no coverage to steer by. With this conversion the same
    // defect fell out at execution 717. That defect was fixed within the same iteration on that
    // branch and is not a shipped regression; the `multibyte-frontmatter` seed records the finding
    // going forward rather than reproducing a live bug.
    let text = String::from_utf8_lossy(data);
    if std::fs::write(&document, text.as_bytes()).is_err() {
        return;
    }

    let first = catalog.load();
    let second = catalog.load();
    match (first, second) {
        (Ok(one), Ok(two)) => assert_eq!(
            one.digest(),
            two.digest(),
            "two loads of one document disagreed about the definition digest"
        ),
        (Err(_), Err(_)) => (),
        _ => panic!("one document loaded on one attempt and refused on the other"),
    }
});

//! A DERIVED FILE ANSWERS FOR THE CRATE IT DERIVES FROM: `docs/api/<crate>.md` and a
//! golden snapshot select the category of the crate or adapter they derive from, via the
//! same `category_from_crate` mapping a crate path would use. Its own file for the
//! unexemptable 1000-line cap, and because the seam is the question: every other rule in
//! `select` decides from the path itself, these two from the crate the file derives from.

use super::{CATALOG_PREFIX, DATA_SOURCE_PREFIX, category_from_crate};

/// The generated API pages, one per crate: `docs/api/<crate>.md`.
const API_DOC_PATH: &str = "docs/api/";
/// The per-dialect golden snapshots, one file per adapter: `crates/sutura-app/tests/snapshots/`.
const SNAPSHOT_PATH: &str = "crates/sutura-app/tests/snapshots/";

/// The category a generated API page selects: the category the crate the page derives from
/// names, reusing the same mapping a crate path would use, so `docs/api/<crate>.md` selects
/// exactly what `crates/<crate>/src/lib.rs` selects - and nothing for a crate on neither axis.
pub(super) fn api_doc_category(path: &str) -> Option<String> {
    api_doc_crate(path).and_then(category_from_crate)
}

/// The crate a generated API page derives from, if `path` is one: `docs/api/<crate>.md`.
fn api_doc_crate(path: &str) -> Option<&str> {
    let crate_name = path.strip_prefix(API_DOC_PATH)?.strip_suffix(".md")?;
    (!crate_name.is_empty() && !crate_name.contains('/')).then_some(crate_name)
}

/// The golden snapshot suffixes, one per adapter: a data system's dialect or a catalog's snapshot
/// key. `markdown` is a snapshot key, not a crate name - the reference catalog is
/// `sutura-catalog-local`, so it selects the `local` catalog.
const SNAPSHOT_DATA_SYSTEMS: &[&str] = &["postgres", "clickhouse", "duckdb", "bigquery", "oracle", "datafusion"];
const SNAPSHOT_CATALOG_SYSTEMS: &[&str] = &["markdown", "okf", "datahub", "openmetadata", "rdbms"];

/// The category a per-dialect golden snapshot selects: the category the adapter crate the snapshot
/// names, reusing the same mapping a crate path would use, so `<anything>@<system>.snap` selects
/// exactly what `crates/sutura-exec-<system>/src/lib.rs` (or the catalog's) selects - and nothing
/// for a suffix no adapter names, which keeps falling open to core.
pub(super) fn snapshot_category(path: &str) -> Option<String> {
    let system = snapshot_system(path)?;
    let crate_name = if SNAPSHOT_DATA_SYSTEMS.contains(&system) {
        format!("{DATA_SOURCE_PREFIX}{system}")
    } else if SNAPSHOT_CATALOG_SYSTEMS.contains(&system) {
        // `markdown` is the one snapshot key whose adapter is named differently: the reference
        // catalog is `sutura-catalog-local`, so its snapshots select the `local` catalog.
        format!("{CATALOG_PREFIX}{}", if system == "markdown" { "local" } else { system })
    } else {
        return None;
    };
    category_from_crate(&crate_name)
}

/// The adapter a golden snapshot names, if `path` is one: `crates/sutura-app/tests/snapshots/<anything>@<system>.snap`.
fn snapshot_system(path: &str) -> Option<&str> {
    let file = path.strip_prefix(SNAPSHOT_PATH)?.strip_suffix(".snap")?;
    let (_, system) = file.rsplit_once('@')?;
    (!system.is_empty() && !system.contains('/')).then_some(system)
}

#[cfg(test)]
mod tests {
    use super::super::tests::selected;

    #[test]
    fn an_adapter_s_own_api_page_selects_its_category_not_core() {
        let cats = selected(&[
            "crates/sutura-catalog-datahub/src/lib.rs",
            "docs/api/sutura-catalog-datahub.md",
        ]);
        assert!(!cats.core);
        assert!(cats.needs("catalog_datahub"));
        assert!(
            !cats.needs("data_source_bigquery"),
            "regenerating a catalog's own page must not run another adapter's legs"
        );
    }

    #[test]
    fn a_shared_crate_s_api_page_still_selects_core() {
        let cats = selected(&["docs/api/sutura-domain.md"]);
        assert!(
            cats.core,
            "a shared crate's page names no category, so it must run everything"
        );
        for cat in [
            "data_source_datafusion",
            "data_source_duckdb",
            "data_source_postgres",
            "data_source_bigquery",
            "catalog_local",
            "catalog_datahub",
            "identity",
        ] {
            assert!(cats.needs(cat), "{cat} must run when core runs");
        }
    }

    #[test]
    fn an_adapter_s_own_snapshot_selects_its_category_not_core() {
        let cats = selected(&[
            "crates/sutura-exec-clickhouse/src/lib.rs",
            "crates/sutura-app/tests/snapshots/x__sql@clickhouse.snap",
        ]);
        assert!(!cats.core);
        assert!(cats.needs("data_source_clickhouse"));
        assert!(
            !cats.needs("data_source_bigquery"),
            "regenerating a data system's own snapshots must not run another adapter's legs"
        );
    }

    #[test]
    fn a_markdown_snapshot_selects_the_local_catalog_and_an_unnamed_one_selects_core() {
        let cats = selected(&["crates/sutura-app/tests/snapshots/x__plan@markdown.snap"]);
        assert!(!cats.core);
        assert!(cats.needs("catalog_local"));
        assert!(
            !cats.needs("catalog_datahub"),
            "a markdown snapshot is the reference catalog's, not another catalog's"
        );
        let core = selected(&["crates/sutura-app/tests/snapshots/qualified_plan_qualified-project.snap"]);
        assert!(
            core.core,
            "a snapshot no adapter names must run every category, not be skipped"
        );
        for cat in [
            "data_source_datafusion",
            "data_source_duckdb",
            "data_source_postgres",
            "data_source_bigquery",
            "catalog_local",
            "catalog_datahub",
            "identity",
        ] {
            assert!(core.needs(cat), "{cat} must run when core runs");
        }
    }
}

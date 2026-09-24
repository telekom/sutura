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

/// The category a generated API page selects: the data-source or catalog category the crate the
/// page derives from names, reusing the same mapping a crate path would use. A page for a crate
/// on neither axis - or for one whose crate path would select another category, like the
/// identity transport `sutura-http` - falls open to core. The page errs toward running more:
/// the page never selects `identity`, but the crate path does.
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
    use std::collections::BTreeSet;

    use super::super::tests::selected;
    use super::{SNAPSHOT_CATALOG_SYSTEMS, SNAPSHOT_DATA_SYSTEMS, SNAPSHOT_PATH, snapshot_category};
    #[test]
    fn an_unlisted_snapshot_suffix_selects_core() {
        // The no-suffix half of the markdown cell never reaches the unlisted-suffix arm:
        // `snapshot_system` refuses it first. A suffix that is spelled but that no list holds
        // is the arm's own case, and it must fall open to core.
        let cats = selected(&["crates/sutura-app/tests/snapshots/x__sql@snowflake.snap"]);
        assert!(cats.core, "a suffix no list names must run every category, not a made-up one");
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
    fn a_nested_path_under_the_api_docs_is_not_a_page() {
        // `docs/api/<crate>.md` is one level deep; a path that is deeper names no crate.
        let cats = selected(&["docs/api/sutura-exec-bigquery/x.md"]);
        assert!(cats.core, "a nested docs/api path must run every category, not a made-up one");
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
    fn the_snapshot_suffix_lists_name_exactly_what_the_registry_names() {
        fn names(dense: &[String], arm: &str) -> BTreeSet<String> {
            super::super::arm_raw(dense, arm)
                .expect("the registry holds the arm")
                .iter()
                .map(|args| args.first().expect("a cell has a name").clone())
                .collect()
        }
        // The two lists are the only names the snapshot arm reads. They must be the registry's
        // own names, split by axis, and every suffix on disk must resolve to a category.
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(super::super::REGISTRY)).expect("the registry is in the repo");
        let (_, dense) = crate::conformance::scan::lexed(&text);

        let data = names(&dense, crate::conformance::scan::REGISTRY_ARM);
        let catalogs = names(&dense, crate::conformance::scan::CATALOGS_ARM);
        assert!(
            !data.is_empty(),
            "no data-system arm read - the oracle would pass over any list"
        );
        assert!(
            !catalogs.is_empty(),
            "no catalogs arm read - the oracle would pass over any list"
        );
        let dir = root.join("crates/sutura-app/tests/snapshots");

        assert_eq!(
            SNAPSHOT_DATA_SYSTEMS
                .iter()
                .copied()
                .map(String::from)
                .collect::<BTreeSet<_>>(),
            data,
            "the data-system list and the registry's data-systems arm must name the same systems"
        );
        assert_eq!(
            SNAPSHOT_CATALOG_SYSTEMS
                .iter()
                .copied()
                .map(String::from)
                .collect::<BTreeSet<_>>(),
            catalogs,
            "the catalog list and the registry's catalogs arm must name the same systems"
        );

        let entries = std::fs::read_dir(&dir).expect("the snapshot directory is in the repo");
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(system) = name
                .strip_suffix(".snap")
                .and_then(|stem| stem.rsplit_once('@').map(|(_, s)| s))
            else {
                continue;
            };
            let path = format!("{SNAPSHOT_PATH}{name}");
            assert!(
                snapshot_category(&path).is_some(),
                "{path}: its suffix {system} names no category, so it would fall open to core"
            );
        }
    }

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

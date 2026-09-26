//! `catalog.kind: datacontract`, served for real: a composed deployment boots over a directory of
//! ODCS v3 contract documents and lists what it measures - the same "a served-binary test per kind
//! that boots and lists" acceptance `served/okf.rs`'s own header states for issue #970, carried here
//! for `#973`.
//!
//! # Why this asks nothing, and lists rather than answers
//!
//! `sutura-catalog-datacontract` provides `Structure` and may-provide `Descriptions`, `ColumnTypes`,
//! `ColumnDescriptions` and `Relationships`, and declares no measure, no grain and no anchor
//! (`docs/what-a-data-contract-can-carry.md`). A bundle it produces therefore has zero certified
//! metrics, so there is nothing here to ask - the honest coverage is that the process boots (which
//! needs the physical table's own file attached, exactly as `markdown`/`okf` deployments do) and the
//! `/catalog` route describes the bundle it was handed: zero metrics, the right version, and a
//! digest.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch is the same shape `served/okf.rs` names: point the
//! served binary's `open_one_data_contract_catalog` at a root the settings did not declare, and the
//! deployment never boots - RED. GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, start_configured, v1};

    /// The catalog's declared name, reused as the `sources:` entry's own name: `DataContractCatalog`
    /// stamps every model's `SourceName` with the catalog's own declared name, so the engine has to
    /// be declared under the identical alias for the physical table to attach.
    const CATALOG: &str = "physical";

    /// One ODCS v3.1.0 contract: an `orders` table, two columns, no relationships - a faithful,
    /// sparse contract rather than a partial one.
    const CONTRACT: &str = "version: 1.0.0\nkind: DataContract\nid: served-boot-1\nstatus: active\napiVersion: v3.1.0\nschema:\n  - name: orders\n    description: Orders, one row per placed order.\n    properties:\n      - name: order_id\n        primaryKey: true\n      - name: total\n";

    /// A directory this test owns: the data-contract catalog root (one contract) and, in the same
    /// directory, the CSV the `files` engine attaches for the table it names - a sibling of the
    /// settings directory the way `served/okf.rs`'s own `CatalogDir` is, so `written()` clearing
    /// the settings directory cannot remove it.
    struct CatalogDir(PathBuf);

    impl CatalogDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the catalog directory is creatable");
            std::fs::write(path.join("orders.yaml"), CONTRACT).expect("the contract is writable");
            std::fs::write(path.join("orders.csv"), "order_id,total\nO1,19.99\n").expect("the CSV is writable");
            Self(path)
        }
    }

    impl Drop for CatalogDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings a `datacontract`-kind deployment needs: `catalogs[].dir` is the contract
    /// directory, `catalogs[].data_dir` unread by this kind (the same stated limit `served/okf.rs`
    /// notes for `okf`) and `sources.<name>.data_dir` is where the CSV actually is - the same
    /// directory, since this test keeps both together.
    fn settings(catalog: &CatalogDir) -> String {
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"datacontract\"\n    \
                 dir: \"{dir}\"\n    \
                 data_dir: \"{dir}\"\n    \
                 version: \"{VERSION}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{dir}\"\n    \
                 posture: \"shared-service-user\"\n",
            dir = catalog.0.display(),
        )
    }

    #[test]
    fn a_data_contract_catalog_boots_and_lists_zero_metrics_from_the_served_binary() {
        let case = "datacontract-boot-and-list";
        let catalog = CatalogDir::prepared(case);
        let deployment = start_configured(case, &settings(&catalog));

        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["metrics"].as_array().map(Vec::len),
            Some(0),
            "a data-contract catalog declares no measure, so a served listing must carry none: {}",
            reply.body
        );
        assert_eq!(
            body["provenance"]["definition_version"], VERSION,
            "the served bundle must be stamped with the declared version: {}",
            reply.body
        );
        assert!(
            body["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "the served listing carries no definition digest: {}",
            reply.body
        );
    }
}

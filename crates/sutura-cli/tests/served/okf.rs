//! `catalog.kind: okf`, served for real: a composed deployment boots over a directory of OKF
//! Frictionless Table Schema descriptors and lists what it measures - issue #970's own acceptance,
//! "a served-binary test per kind that boots and lists".
//!
//! # Why this asks nothing, and lists rather than answers
//!
//! `sutura-catalog-okf` provides `Structure` and `Descriptions` and declares no measure, no grain
//! and no anchor (its own module header). A bundle it produces therefore has zero certified
//! metrics, so there is nothing here to ask - the honest coverage is that the process boots (which
//! needs the physical table's own file attached, exactly as a `markdown` deployment's does) and the
//! `/catalog` route describes the bundle it was handed: zero metrics, the right version, and a
//! digest. The `definition_digest` assertion below is what pins the "and digest" half of that.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch is the root-path one the lane's L1 ran: point the
//! served binary's `open_one_okf_catalog` at `/nowhere/okf-mutation` instead of the declared
//! `catalogs[].dir`, and the deployment never boots - RED. GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, start_configured, v1};

    /// The catalog's declared name, reused as the `sources:` entry's own name: `OkfCatalog` stamps
    /// every model's `SourceName` with the catalog's own declared name, so the engine has to be
    /// declared under the identical alias for the physical table to attach.
    const CATALOG: &str = "physical";

    /// One OKF Table Schema descriptor: a `subscriptions` table, two columns, a description (OKF
    /// requires one - `OkfCatalogError::MissingDescription` otherwise) and no `primaryKey`/
    /// `foreignKeys` at all, which is a faithful, sparse descriptor rather than a partial one.
    const DESCRIPTOR: &str = "description: Subscriptions, one row per active plan.\n\
         fields:\n  \
           - name: subscription_id\n  \
           - name: amount_cents\n";

    /// A directory this test owns: the OKF catalog root (one descriptor) and, in the same
    /// directory, the CSV the `files` engine attaches for the table it names - a sibling of the
    /// settings directory the way `served/datahub.rs`'s own `DataDir` is, so `written()` clearing
    /// the settings directory cannot remove it.
    struct CatalogDir(PathBuf);

    impl CatalogDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the catalog directory is creatable");
            std::fs::write(path.join("subscriptions.yaml"), DESCRIPTOR).expect("the descriptor is writable");
            std::fs::write(path.join("subscriptions.csv"), "subscription_id,amount_cents\nS1,1999\n")
                .expect("the CSV is writable");
            Self(path)
        }
    }

    impl Drop for CatalogDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings an `okf`-kind deployment needs: `catalogs[].dir` is the descriptor directory,
    /// `catalogs[].data_dir` unread by this kind (the same stated limit `served/datahub.rs` notes
    /// for `datahub`, carried here for `okf` too) and `sources.<name>.data_dir` is where the CSV
    /// actually is - the same directory, since this test keeps both together.
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
                 kind: \"okf\"\n    \
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
    fn an_okf_catalog_boots_and_lists_zero_metrics_from_the_served_binary() {
        let case = "okf-boot-and-list";
        let catalog = CatalogDir::prepared(case);
        let deployment = start_configured(case, &settings(&catalog));

        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["metrics"].as_array().map(Vec::len),
            Some(0),
            "an OKF catalog declares no measure, so a served listing must carry none: {}",
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

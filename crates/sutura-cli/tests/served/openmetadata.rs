//! `catalog.kind: openmetadata`, served for real: a loopback `OpenMetadata` fake (issue #970's own,
//! reused from `sutura_catalog_openmetadata::test_support` rather than rebuilt here) backs a
//! composed deployment that boots and lists the bundle it measures over HTTP - issue #970's wave-one
//! E2E plan's lane 2 cell, "a served-binary test per kind that boots and lists", for the
//! `OpenMetadata` adapter.
//!
//! # Why this asks nothing, and lists rather than answers a certified metric
//!
//! `sutura-catalog-openmetadata` provides `Structure`, `Descriptions` and `Relationships` and
//! declares the metric/grain/cardinality as declared-and-empty may-provide kinds - a metric whose
//! measure is an expression string is reported-not-defined and never minted into a certified
//! `Metric` (`docs/what-openmetadata-can-carry.md`). The fake's own `metrics_page` carries such a
//! metric, so the served bundle has a reported-not-defined metric and nothing a question can name.
//! The honest coverage, exactly like `served/okf.rs`, is that the process boots (which needs the
//! physical table's file attached to the declared source) and the `/catalog` route lists the bundle:
//! the models it supplies, the declared version, and a digest.
//!
//! # The source mapping this test exercises, and why the data is `files`-backed
//!
//! `open_one_openmetadata_catalog` (`crates/sutura-cli/src/catalog.rs`) fixes the service -> source
//! alias mapping to the single literal `"warehouse"`, answered by the CATALOG's OWN declared name -
//! the same convention `sutura_catalog_openmetadata::fixture::over_fixture_source` uses. So this
//! deployment's `sources:` entry is named IDENTICALLY to its `catalogs:` entry (`"metrics"`, below),
//! and its `kind` is `files`: nothing here needs a real data system, because the mapping only cares
//! that the NAME matches.
//! `crates/sutura-exec-datafusion` reads one CSV per table, named after the model's `table`, which
//! is why the two files below are `orders.csv`/`customers.csv` - the tables the test-suite's
//! `tables_page` declares carry those model names.
//!
//! # The fixture reused
//!
//! `test_support::happy_path_answers()` serves the SAME pages
//! `crates/sutura-catalog-openmetadata/tests/http_reader.rs` certifies against - unmodified, so this
//! test cannot drift from that crate's own proof of the wire shape.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: revert `crates/sutura-cli/src/catalog.rs`'s
//! `Openmetadata` arm to the unconditional refusal it replaced - RED, the deployment never boots.
//! GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_openmetadata::test_support::FakeServer;

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, start_configured, v1};

    /// The catalog's declared name, reused as the `sources:` entry's own name: the fixed
    /// `"warehouse"` alias maps the service the fixture serves to this name, and the engine has to
    /// be declared under it for the physical tables to attach.
    const CATALOG: &str = "metrics";

    /// `orders`'s rows - a placeholder file so the model's table attaches to a real source; the
    /// `OpenMetadata` corpus supplies descriptions, not row data, and this test lists rather than asks,
    /// so the contents are not asserted.
    const ORDERS_CSV: &str = "order_id,customer_id,amount_cents,order_date,status\n\
         O1,C1,200000,2026-06-01,active\n";

    /// `customers`'s rows - the counterpart table the fixture's relationship reaches.
    const CUSTOMERS_CSV: &str = "customer_id,segment\n\
         C1,retail\n";

    /// A directory this test owns and removes on every path out - see `served/okf.rs`'s `CatalogDir`
    /// for why it is a sibling of the settings directory (`written()` clears the settings directory).
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            std::fs::write(path.join("orders.csv"), ORDERS_CSV).expect("orders.csv is writable");
            std::fs::write(path.join("customers.csv"), CUSTOMERS_CSV).expect("customers.csv is writable");
            std::fs::write(path.join("token"), "pat-under-test\n").expect("the token file is writable");
            Self(path)
        }

        fn token_file(&self) -> PathBuf {
            self.0.join("token")
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings an `openmetadata`-kind deployment needs: `catalogs[].dir`/`data_dir` are the
    /// two non-empty path fields every kind requires (unread by this kind, the same stated limit
    /// `served/datahub.rs` notes), and `sources.<name>.data_dir` is where the CSVs actually are.
    fn settings(server: &FakeServer, data: &DataDir) -> String {
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"openmetadata\"\n    \
                 dir: \"/unused-for-openmetadata\"\n    \
                 data_dir: \"/unused-for-openmetadata\"\n    \
                 version: \"{VERSION}\"\n    \
                 endpoint: \"{endpoint}\"\n    \
                 token_file: \"{token_file}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{data_dir}\"\n    \
                 posture: \"shared-service-user\"\n",
            endpoint = server.endpoint(),
            token_file = data.token_file().display(),
            data_dir = data.0.display(),
        )
    }

    /// An `openmetadata`-kind deployment boots from the served binary and lists the bundle it
    /// measures: zero certified metrics (the metric is reported-not-defined), the declared version,
    /// and a digest.
    #[test]
    fn an_openmetadata_catalog_boots_and_lists_from_the_served_binary() {
        let case = "openmetadata-boot-and-list";
        let data = DataDir::prepared(case);
        // The composition root loads the catalog TWICE at boot - once for `catalog::load` (so the
        // engine can be opened for the sources the catalog names) and once inside
        // `LocalService::start_composed`, which loads the catalog it serves. Each `read()` fetches
        // two pages, so the fake must answer FOUR connections: `happy_path_answers()` twice.
        let mut answers = sutura_catalog_openmetadata::test_support::happy_path_answers();
        answers.extend(sutura_catalog_openmetadata::test_support::happy_path_answers());
        let server = FakeServer::start(answers);
        let deployment = start_configured(case, &settings(&server, &data));

        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["metrics"].as_array().map(Vec::len),
            Some(0),
            "an OpenMetadata metric with an expression string is reported-not-defined, so a served \
             listing must carry none: {}",
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

        // **`token_file` actually reaches the wire.** All four page requests must carry the bearer -
        // `finish()` joins the (already-exited) fake thread and returns every request's
        // `authorization` header.
        let authorizations = server.finish();
        assert_eq!(authorizations.len(), 4, "two reads of two pages each");
        assert!(
            authorizations
                .iter()
                .all(|seen| seen.authorization() == Some("Bearer pat-under-test")),
            "every OpenMetadata page request must carry the token_file's bearer, got: {authorizations:?}"
        );
    }
}

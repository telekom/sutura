//! `catalog.kind: rdbms`, served for real: a composed deployment boots over a live Postgres
//! documentation schema and lists what it measures - issue #970's "a served-binary test per kind
//! that boots and lists" acceptance, for `rdbms`.
//!
//! The existing live-reader test (`crates/sutura-catalog-rdbms/tests/provisioned.rs`) exercises the
//! reader directly; #972's own thread calls out that it is not a served/MCP feature-on cell. This
//! is that cell: the COMPOSED BINARY opens the catalog, reads the dictionary, and serves `/catalog`.
//!
//! # Why this asks nothing, and lists rather than answers
//!
//! `sutura-catalog-rdbms` provides `Structure` and may-provide `Descriptions`, `Relationships`,
//! `ColumnTypes` and `ColumnDescriptions`, and declares no measure, no grain and no anchor. A
//! bundle it produces therefore has zero certified metrics, so there is nothing here to ask - the
//! honest coverage, exactly like `served/okf.rs` and `served/openmetadata.rs`, is that the process
//! boots and the `/catalog` route describes the bundle: zero metrics, the right version, and a
//! digest.
//!
//! # Why a `postgres` source, not `files`
//!
//! The rdbms catalog's dictionary rows carry a required `schema_name`, so `convert_table_address`
//! always builds a `QualifiedTable` (e.g. `public.orders`). The `files` engine refuses a qualified
//! model at boot (`crates/sutura-cli/src/serve/files.rs`), so the `source_alias` must name a
//! `postgres` source. `PostgresWarehouse` takes the port's default `preflight` (`NotAsked`), so
//! the table's absence from the tier is a `NotReported` notice at boot rather than a refusal, and
//! the deployment starts without the physical table loaded - which is why this cell needs no
//! `FixtureLoadGuard` and no example CSV load.
//!
//! # The documentation-schema fixture
//!
//! A per-run schema holds a `columns` table with two rows describing `public.orders` (model
//! `orders`, one `order_id` primary-key column and one `amount` column), `environment: test`,
//! `is_deleted: false` - the documented fixed schema `PostgresReader` reads, the same shape
//! `crates/sutura-catalog-rdbms/tests/provisioned.rs` installs.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: drop every table description the reader decodes
//! (the killing patch at
//! `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_from_the_served_binary.patch` filters
//! `table_description` to `None` in the shared `documentation::Assembly::decode` the Postgres reader
//! hands every row to), so the deployment boots and
//! `/catalog` responds but the served digest no longer matches the digest [`expected`] builds
//! independently from the same fixture rows - the cell's own `assert_eq!` fires, RED. GREEN is
//! this file as written. The implementation is already on main (#1105), so this is a claim cell:
//! the commit carries a `Claim-Cell:` trailer and the patch above is the mechanism that holds it.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_domain::pinned::PinnedDefinitions;

    use crate::common::{CATALOG, ENVIRONMENT, SOURCE, install_documentation_schema};
    use crate::harness::{
        DiscoveredTier, LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, discover_tier, source_entry,
        start_configured, v1,
    };

    /// A directory this test owns and removes on every path out - see `served/okf.rs`'s `CatalogDir`
    /// for why it is a sibling of the settings directory (`written()` clears the settings directory).
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            Self(path)
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings an `rdbms`-kind deployment needs: one `catalog.kind: rdbms` entry reading the
    /// documentation schema over the tier's loopback TLS connection, and one `kind: postgres`
    /// source named for the catalog's `source_alias` reaching the same tier - the `sources:` half
    /// built by [`source_entry`], the same helper `harness/postgres.rs`'s own fixtures use, so this
    /// cell cannot drift from how a served cell reaches the tier.
    fn settings(data: &DataDir, tier: &DiscoveredTier, documentation_schema: &str) -> String {
        let password_file = data.0.join("password");
        std::fs::write(&password_file, format!("{}\n", tier.password)).expect("the password file is writable");
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"rdbms\"\n    \
                 version: \"{VERSION}\"\n    \
                 environment: \"{ENVIRONMENT}\"\n    \
                 source_alias: \"{SOURCE}\"\n    \
                 dictionary_schema: \"{documentation_schema}\"\n    \
                 connection:\n      \
                   host: \"127.0.0.1\"\n      \
                   port: {port}\n      \
                   database: \"{database}\"\n      \
                   user: \"{user}\"\n      \
                   password_file: \"{password_file}\"\n      \
                   transport_mode: \"verified\"\n      \
                   transport_anchors: \"{anchor}\"\n\
             sources:\n{source_entry}",
            port = tier.port,
            database = tier.database,
            user = tier.user,
            password_file = password_file.display(),
            anchor = tier.anchor,
            source_entry = source_entry(SOURCE, tier.port, &tier.database, &tier.user, &password_file, &tier.anchor),
        )
    }

    /// The bundle this cell expects - see [`crate::common::expected`].
    fn expected() -> PinnedDefinitions {
        crate::common::expected(VERSION)
    }

    /// An `rdbms`-kind deployment boots from the served binary and lists the bundle it measures:
    /// zero certified metrics (the dictionary carries no measure), the declared version, and a
    /// digest that matches [`expected`]'s independently built one.
    #[test]
    fn an_rdbms_catalog_boots_and_lists_from_the_served_binary() {
        let case = "rdbms-boot-and-list";
        let Some(tier) = discover_tier() else {
            return;
        };
        let data = DataDir::prepared(case);
        let documentation_schema = install_documentation_schema(&tier.config);
        let deployment = start_configured(case, &settings(&data, &tier, &documentation_schema));

        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["metrics"].as_array().map(Vec::len),
            Some(0),
            "an RDBMS dictionary carries no measure, so a served listing must carry none: {}",
            reply.body
        );
        assert_eq!(
            body["provenance"]["definition_version"], VERSION,
            "the served bundle must be stamped with the declared version: {}",
            reply.body
        );
        assert_eq!(
            body["provenance"]["definition_digest"],
            expected().digest().as_str(),
            "the served RDBMS bundle differs from the independently built fixture digest: {}",
            reply.body
        );
    }
}

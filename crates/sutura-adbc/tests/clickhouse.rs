#![forbid(unsafe_code)]
//! The mounted `ClickHouse` driver, opened by path the way a deployment that mounts one opens it:
//! that it carries the two upstream changes `vendor/adbc_clickhouse` holds, and - on the tier - that it
//! loads and answers a query.
//!
//! The library is `SUTURA_CLICKHOUSE_ADBC_DRIVER`, which the dev shell and every nix check set from
//! this flake's own build. The two cells that need a server skip loudly where no tier runs, and fail
//! where one is required (`sutura_dev::provisioned::here`); the patch cell needs no server.
//!
//! **A connection with no TLS option needs a platform root store.** The driver builds its client from it
//! and panics when there is none, over `http://` as well. A nix build sandbox has none
//! (`SSL_CERT_FILE=/no-cert-file.crt`), so the checks that run these cells point `SSL_CERT_FILE` at a CA
//! bundle. A cell cannot declare a root instead: a declared TLS setting requires an `https` URI.

// `cfg(test)` for the reason `crates/sutura-sql/tests/adversarial_findings.rs` gives: clippy's
// `allow-*-in-tests` reach only code inside a `#[cfg(test)]` item.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use adbc_core::error::Status;
    use adbc_core::options::{AdbcVersion, OptionConnection, OptionDatabase, OptionValue};
    use adbc_core::{Connection as _, Database as _, Driver as _, Optionable as _, Statement as _};
    use adbc_driver_manager::{ManagedDatabase, ManagedDriver};
    use arrow_array::RecordBatch;
    use arrow_array::cast::AsArray as _;
    use arrow_array::types::Int64Type;
    use sutura_dev::provisioned::{self, Provisioned};

    /// The driver's own spelling of the options the two vendored changes add.
    const TLS_ROOT_CERTS: &str = "clickhouse.client.tls_root_certs";
    const MTLS_CERT_CHAIN: &str = "clickhouse.client.mtls_cert_chain";
    const MTLS_PRIVATE_KEY: &str = "clickhouse.client.mtls_private_key";
    const QUERY_TIMEOUT: &str = "clickhouse.client.query_timeout";

    fn mounted() -> ManagedDriver {
        let library = std::env::var("SUTURA_CLICKHOUSE_ADBC_DRIVER")
            .expect("the dev shell and every nix check set SUTURA_CLICKHOUSE_ADBC_DRIVER");
        sutura_adbc::mounted_clickhouse_driver(&library).expect("the mounted ClickHouse driver initialises")
    }

    /// A database on the tier's server as its fixture user, or `None` where no tier runs.
    fn tier_database(case: &str) -> Option<ManagedDatabase> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("clickhouse::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let variable =
            |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is unset, so the tier has no fixture credential"));
        let uri = format!("http://{}:{}", endpoint.host(), endpoint.port());
        let database = mounted()
            .new_database_with_opts([
                (OptionDatabase::Uri, uri.into()),
                (OptionDatabase::Username, variable("SUTURA_CLICKHOUSE_TIER_USER").into()),
                (OptionDatabase::Password, variable("SUTURA_CLICKHOUSE_TIER_PASSWORD").into()),
            ])
            .expect("the tier database opens");
        Some(database)
    }

    /// A driver built from the unpatched source answers `InvalidArguments` to every option here, so
    /// each assertion is on what only the patched driver does: it accepts a PEM bundle and gives it
    /// back, it never gives a private key back, and it holds a query timeout a connection reads back.
    #[test]
    fn the_mounted_driver_carries_the_tls_and_query_timeout_patches() {
        let pair = rcgen::generate_simple_self_signed([String::from("tier.invalid")]).expect("a self-signed pair generates");
        let (bundle, key) = (pair.cert.pem(), pair.signing_key.serialize_pem());
        let mut driver = mounted();
        let mut database = driver.new_database().expect("a database opens");
        database
            .set_option(OptionDatabase::Uri, "https://tier.invalid:8443".into())
            .expect("an https URI is accepted");
        let root_certs = || OptionDatabase::Other(TLS_ROOT_CERTS.to_owned());

        database
            .set_option(root_certs(), bundle.clone().into())
            .expect("a PEM bundle is accepted");
        assert_eq!(
            database.get_option_string(root_certs()).expect("the bundle reads back"),
            bundle
        );
        let malformed = database
            .set_option(root_certs(), "not a certificate".into())
            .expect_err("malformed PEM is refused");
        assert_eq!(malformed.status, Status::InvalidArguments, "{malformed:?}");

        let private_key = || OptionDatabase::Other(MTLS_PRIVATE_KEY.to_owned());
        database
            .set_option(OptionDatabase::Other(MTLS_CERT_CHAIN.to_owned()), bundle.into())
            .expect("a PEM certificate chain is accepted");
        database
            .set_option(private_key(), key.into())
            .expect("a PEM private key is accepted");
        let hidden = database
            .get_option_string(private_key())
            .expect_err("a private key is never read back");
        assert_eq!(hidden.status, Status::NotFound, "{hidden:?}");

        let mut connection = database.new_connection().expect("a connection opens");
        let timeout = || OptionConnection::Other(QUERY_TIMEOUT.to_owned());
        connection
            .set_option(timeout(), "7".into())
            .expect("a whole number of seconds is accepted");
        assert_eq!(connection.get_option_string(timeout()).expect("the timeout reads back"), "7");
        let zero = connection
            .set_option(timeout(), "0".into())
            .expect_err("zero seconds is refused");
        assert_eq!(zero.status, Status::InvalidArguments, "{zero:?}");
    }

    /// The mounted library exports the entrypoint the linked route declares, `AdbcClickhouseInit`.
    ///
    /// What this does not hold: that `mounted_clickhouse_driver` PASSES that name. The library also
    /// defines the generic `AdbcDriverInit`, which the loader falls back to, so a load with no
    /// entrypoint opens the same driver and no cell over this library can tell the two apart (a
    /// mutation that drops the name stays green). The passing is held by review alone.
    #[test]
    fn the_mounted_library_exports_the_entrypoint_the_linked_route_declares() {
        let library = std::env::var("SUTURA_CLICKHOUSE_ADBC_DRIVER")
            .expect("the dev shell and every nix check set SUTURA_CLICKHOUSE_ADBC_DRIVER");
        ManagedDriver::load_dynamic_from_filename(Path::new(&library), Some(b"AdbcClickhouseInit"), AdbcVersion::default())
            .expect("the library defines AdbcClickhouseInit");
    }

    /// A database at `uri` (none, where `uri` is `None`) with one declared TLS root or, when
    /// `identity`, a client identity instead.
    fn database_declaring_tls(uri: Option<&str>, identity: bool) -> ManagedDatabase {
        let pair = rcgen::generate_simple_self_signed([String::from("tier.invalid")]).expect("a self-signed pair generates");
        let (certificate, key) = (pair.cert.pem(), pair.signing_key.serialize_pem());
        let mut options: Vec<(OptionDatabase, OptionValue)> =
            uri.map(|uri| (OptionDatabase::Uri, uri.into())).into_iter().collect();
        if identity {
            options.push((OptionDatabase::Other(MTLS_CERT_CHAIN.to_owned()), certificate.into()));
            options.push((OptionDatabase::Other(MTLS_PRIVATE_KEY.to_owned()), key.into()));
        } else {
            options.push((OptionDatabase::Other(TLS_ROOT_CERTS.to_owned()), certificate.into()));
        }
        mounted().new_database_with_opts(options).expect("the options are accepted")
    }

    /// A declared TLS root, or a declared client identity, is refused on an `http` URI - with a typed
    /// error that names the setting - and on no URI at all, where the default is `http`.
    #[test]
    fn a_declared_tls_setting_is_refused_on_an_http_uri() {
        for (identity, setting) in [(false, TLS_ROOT_CERTS), (true, MTLS_CERT_CHAIN)] {
            let database = database_declaring_tls(Some("http://127.0.0.1:1"), identity);
            let Err(refused) = database.new_connection() else {
                panic!("{setting} declared over http must be refused when the connection is created");
            };
            assert_eq!(refused.status, Status::InvalidArguments, "{refused:?}");
            assert!(refused.message.contains("requires an https URI"), "{refused:?}");
            assert!(refused.message.contains(setting), "{refused:?}");
        }
    }

    /// The refusal holds with no URI at all, where the client's default is `http`.
    #[test]
    fn a_declared_tls_setting_is_refused_with_no_uri() {
        let database = database_declaring_tls(None, false);
        let Err(refused) = database.new_connection() else {
            panic!("{TLS_ROOT_CERTS} declared with no URI must be refused when the connection is created");
        };
        assert_eq!(refused.status, Status::InvalidArguments, "{refused:?}");
        assert!(refused.message.contains("requires an https URI"), "{refused:?}");
        assert!(refused.message.contains(TLS_ROOT_CERTS), "{refused:?}");
    }

    /// The control for the refusal: the same declared root on an `https` URI builds the connection and
    /// goes on to dial, so the refusal above is the scheme and not the root.
    #[test]
    fn a_declared_tls_root_on_an_https_uri_reaches_the_dial() {
        let database = database_declaring_tls(Some("https://127.0.0.1:1"), false);
        let mut connection = database.new_connection().expect("an https URI with a declared root connects");
        let mut statement = connection.new_statement().expect("a statement opens");
        statement.set_sql_query("SELECT 1").expect("the query is accepted");
        let Err(failed) = statement.execute() else {
            panic!("nothing listens on port 1, so no query is answered");
        };
        assert!(!failed.message.contains("requires an https URI"), "{failed:?}");
        assert_ne!(failed.status, Status::InvalidArguments, "{failed:?}");
    }

    /// The mounted path is parsed before the loader sees it, as the `DuckDB` one is.
    #[test]
    fn a_relative_mounted_clickhouse_path_is_refused_rather_than_resolved() {
        let Err(refused) = sutura_adbc::mounted_clickhouse_driver("lib/libadbc_driver_clickhouse.so") else {
            panic!("a relative driver path must be refused, never opened against this process's cwd");
        };
        assert_eq!(refused.status, Status::InvalidArguments, "{refused:?}");
    }

    /// The driver loads from its mounted library and answers a query with Arrow columns, typed as
    /// `ClickHouse` typed them: an `Int64` and a `String` the driver reports as UTF-8.
    #[test]
    fn the_mounted_driver_answers_a_query_on_the_tier() {
        let Some(database) = tier_database("query") else { return };
        let mut connection = database.new_connection().expect("a connection opens");
        let mut statement = connection.new_statement().expect("a statement opens");
        statement
            .set_sql_query("SELECT toInt64(number) AS n, toString(number) AS s FROM numbers(3) ORDER BY n")
            .expect("the query is accepted");
        let batches: Vec<RecordBatch> = statement
            .execute()
            .expect("the query executes")
            .collect::<Result<_, _>>()
            .expect("every batch reads");
        let rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(rows, 3, "{batches:?}");
        let [batch] = batches.as_slice() else {
            panic!("one batch, got {}", batches.len());
        };
        assert_eq!(batch.column(0).as_primitive::<Int64Type>().values(), &[0, 1, 2]);
        let words = batch
            .column(1)
            .as_string_opt::<i32>()
            .unwrap_or_else(|| panic!("a UTF-8 column, got {}", batch.column(1).data_type()));
        assert_eq!(words.iter().collect::<Vec<_>>(), [Some("0"), Some("1"), Some("2")]);
    }

    /// A query the server would take three seconds over is cut off at the connection's one-second
    /// timeout and reported as a timeout - not left to run, and not reported as a failed query.
    #[test]
    fn a_query_past_the_connection_timeout_is_cut_off_as_a_timeout() {
        let Some(database) = tier_database("timeout") else { return };
        let mut connection = database.new_connection().expect("a connection opens");
        connection
            .set_option(OptionConnection::Other(QUERY_TIMEOUT.to_owned()), "1".into())
            .expect("a one-second timeout is accepted");
        let mut statement = connection.new_statement().expect("a statement opens");
        statement.set_sql_query("SELECT sleep(3)").expect("the query is accepted");
        let started = Instant::now();
        let Err(cut_off) = statement.execute() else {
            panic!("a three-second query must not finish inside a one-second timeout");
        };
        assert_eq!(cut_off.status, Status::Timeout, "{cut_off:?}");
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "the timeout did not cut the wait short: {:?}",
            started.elapsed()
        );
    }
}

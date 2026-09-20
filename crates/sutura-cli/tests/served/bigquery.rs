//! What a `bigquery` deployment's own startup does with the ADBC driver, on the COMPOSED BINARY.
//!
//! **This exists because the in-process suite could not hold it.** `serve/bigquery.rs` claims the
//! boot "reads the FILE and not only the variable" and stops the process on a driver it cannot
//! load. Review measured that claim: deleting the probe statement at BOTH composition roots left
//! the whole suite green at exit 0 with clippy clean, because every cell that reaches that code
//! path stops at the *variable is unset* refusal one line above it.
//!
//! **A spawned binary and not an in-process cell, and that is forced rather than preferred.**
//! `std::env::set_var` is `unsafe` on edition 2024 and `unsafe_code` is `forbid` here, so a test
//! cannot put `SUTURA_BIGQUERY_ADBC_DRIVER` into its own process. A child takes it through
//! `Command::env`, which is safe - and the harness strips every `SUTURA_*` variable first, so the
//! one this suite sets is the only one the deployment sees.
//!
//! # The pair, and why one of them alone proves nothing
//!
//! Two cases over one fixture, differing only in that variable:
//!
//! * **unset** - the refusal names the variable. This is the case every existing cell reaches, and
//!   it passes whether or not a probe exists.
//! * **set, to a path holding no driver** - the refusal names the driver. Only a boot that OPENS
//!   the file can produce it, so this is the cell that dies when the probe goes.
//!
//! The two are asserted to be DIFFERENT refusals rather than each matched in isolation: a boot that
//! answered *the variable is not set* to both would satisfy a lone substring check on the first.
//!
//! # What it does not establish
//!
//! That a real driver loads. The path here names nothing, so what is shown is that the process
//! reached the loader and declined to serve - `just bigquery-driver-check` is the venue that loads
//! a real `.so`, and it runs on `x86_64-linux` only.

#[cfg(test)]
mod tests {
    use sutura_config::Environment;

    use crate::harness::{
        LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, derived_catalog, example_root, files_source, refused_to_start, settings_over,
        written,
    };

    /// The source alias the moved model is pointed at.
    const BQ_SOURCE: &str = "warehouse";

    /// The one model this fixture moves off the `files` source and onto the `bigquery` one.
    ///
    /// `daily_usage.md` for `harness::two_kind`'s recorded reason: it is the one model the example
    /// catalog declares with no `via` relationship, so moving it changes which adapter answers and
    /// never how many legs a plan has.
    const MOVED_MODEL: &str = "daily_usage.md";

    /// A `bigquery` source entry, with every key `sutura_config` requires of one.
    ///
    /// The credential file names nothing and that is deliberate: the ADBC driver authenticates
    /// itself, so a boot that read this path would be reading a file no transport in this build
    /// wants - which is a second thing these cells would catch.
    fn bigquery_entry() -> String {
        format!(
            "  {BQ_SOURCE}:\n    \
               kind: \"bigquery\"\n    \
               billing_project: \"acme-analytics\"\n    \
               dataset: \"warehouse\"\n    \
               credential_file: \"/nonexistent/sutura-test-bigquery.json\"\n    \
               max_bytes_billed: 1073741824\n    \
               posture: \"shared-service-user\"\n"
        )
    }

    /// A deployment whose catalog puts one model on a `bigquery` source, so the boot opens one.
    fn settings(case: &str) -> String {
        let example = example_root();
        let data = example.join("data");
        let catalog = derived_catalog(case, &example.join("catalog"), MOVED_MODEL, BQ_SOURCE);
        settings_over(
            &catalog,
            &data,
            LOOPBACK,
            &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
            &format!("{}{}", files_source(LOCAL_SOURCE, &data), bigquery_entry()),
        )
    }

    /// What the composed binary said when it declined to serve this deployment.
    fn refusal(case: &str, driver: &[(&str, &str)]) -> String {
        refused_to_start(Environment::Development, written(case, &settings(case)), driver).join("\n")
    }

    #[test]
    fn a_driver_path_naming_no_driver_stops_the_process_rather_than_the_first_question() {
        // **THE CELL THE PROBE IS HELD BY.** `SUTURA_BIGQUERY_ADBC_DRIVER` is set, so the variable
        // check one line above the probe passes and the boot has to open the file to fail. Delete
        // the `AdbcBigQuery::probe` call at either composition root and this reads the *variable is
        // unset* refusal instead - which is the mutation review found nothing catching.
        let said = refusal(
            "bigquery-driver-unloadable",
            &[("SUTURA_BIGQUERY_ADBC_DRIVER", "/nonexistent/libadbc_driver_bigquery.so")],
        );
        assert!(
            said.contains("cannot load"),
            "the refusal must say the driver could not be LOADED, not that a variable is unset:\n{said}"
        );
        assert!(said.contains(BQ_SOURCE), "the refusal must name the source:\n{said}");
        assert!(
            !said.contains("is not set"),
            "a boot that only read the variable cannot have opened the file:\n{said}"
        );
    }

    #[test]
    fn a_deployment_naming_no_driver_at_all_is_a_different_refusal() {
        // **THE CONTROL.** Without it the cell above passes over a boot that refuses every
        // `bigquery` deployment for any reason: both cases exit 1 and both name the source, so the
        // only thing separating them is WHICH sentence, and this is the half that fixes that
        // sentence to the case it belongs to.
        let said = refusal("bigquery-driver-unset", &[]);
        assert!(
            said.contains("SUTURA_BIGQUERY_ADBC_DRIVER") && said.contains("is not set"),
            "an unnamed driver is refused by the variable check:\n{said}"
        );
        assert!(
            !said.contains("cannot load"),
            "nothing was opened, so nothing can have failed to load:\n{said}"
        );
    }
}

//! The application-facing surface, re-exported. **The port itself is not declared here any more.**
//!
//! [`Surface`], its failure types and its one implementor [`LocalService`] live in
//! `sutura_app::surface`. They used to live in this file, and a review found the problem: this is a
//! transport adapter, the module comment said a future MCP transport would consume the same trait,
//! and a trait declared here is a trait that other transport would have to reach through an HTTP
//! crate. Nothing in this repository may depend on an adapter. The argument for where it went, and
//! for why deleting the one-implementation trait was the weaker of the two options, is in
//! `sutura_app::surface`'s own module documentation.
//!
//! What is left is a re-export, so the paths this crate already uses - `crate::surface::Surface` in
//! the request state, `crate::surface::SurfaceFailure` in the query route - keep resolving. **The
//! canonical path is `sutura_app::surface`**, and a second transport imports it from there without
//! naming this crate at all.
//!
//! The tests below stayed with the fakes rather than with the code: `crate::testing` holds the two
//! port doubles they need, and they exercise the re-exported types, so what they assert about the
//! erasure is unchanged.

pub use sutura_app::surface::{ErasedCause, LocalService, ServiceNotStarted, Surface, SurfaceFailure, cause_chain};

#[cfg(test)]
mod tests {
    use core::error::Error as _;

    use crate::testing::{CatalogUnreadable, FailingCatalog, FailingWarehouse, bundle, catalog_of, source};

    use super::{LocalService, ServiceNotStarted, Surface as _, cause_chain};

    #[test]
    fn a_catalog_that_cannot_be_read_keeps_its_cause_as_an_error_not_as_text() {
        // The reason the cause is boxed rather than rendered. Erasing the adapter's error TYPE must
        // not erase the error: an operator reading a failed boot needs the driver's own complaint,
        // and code responding to one needs to be able to ask what it was.
        let error = LocalService::start(&FailingCatalog, FailingWarehouse::new(source()))
            .expect_err("a catalog that fails every read starts no service");
        let ServiceNotStarted::Catalog { ref cause } = error else {
            panic!("expected a catalog failure, got {error:?}");
        };
        // `source()` reaches the adapter's error, which the previous shape could not: it held a
        // `String` and a `Vec<String>`, so this returned `None`.
        let source = error.source().expect("the outer error has a source");
        assert_eq!(source.to_string(), "the catalog directory could not be read");
        // And the adapter's own type is recoverable, which is the half a vector of display strings
        // makes impossible.
        assert!(
            cause.downcast_ref::<CatalogUnreadable>().is_some(),
            "the adapter's error type did not survive: {cause:?}"
        );
        // The chain is still available for a log line - computed where the line is written.
        assert_eq!(
            cause_chain(&error),
            vec![
                String::from("the catalog directory could not be read"),
                String::from("no such file: catalog/"),
            ]
        );
    }

    #[test]
    fn a_bundle_whose_anchors_cannot_run_starts_no_service() {
        // The readiness gate, at the only place it can be enforced: `Validated` has no other
        // constructor, so this is not a check that could be skipped by a caller who forgot it.
        let error = LocalService::start(&catalog_of(bundle()), FailingWarehouse::new(source()))
            .expect_err("a data system that answers nothing validates no bundle");
        assert!(matches!(error, ServiceNotStarted::NotValidated { .. }), "{error:?}");
    }

    #[test]
    fn a_data_system_that_will_not_answer_comes_back_as_a_typed_failure() {
        // The request-path half of the same property. The handler only needs the variant to pick a
        // status, but a failure whose cause has been rendered to text is one nothing else can ever
        // respond to differently - and this is the boundary where that decision is made once for
        // every future caller.
        use super::SurfaceFailure;
        use crate::testing::{StatementRejected, a_question, unanchored_bundle};

        let service = LocalService::start(&catalog_of(unanchored_bundle()), FailingWarehouse::new(source()))
            .expect("a bundle with no anchor validates against a warehouse that answers nothing");
        let failure = service
            .answer(&a_question())
            .expect_err("a warehouse that rejects every statement answers nothing");
        let SurfaceFailure::Warehouse { ref cause } = failure else {
            panic!("expected a warehouse failure, got {failure:?}");
        };
        assert!(
            cause.downcast_ref::<StatementRejected>().is_some(),
            "the adapter's error type did not survive: {cause:?}"
        );
        assert_eq!(
            cause_chain(&failure),
            vec![
                String::from("the data system rejected the statement"),
                String::from("connection refused"),
            ]
        );
    }

    #[test]
    fn a_service_that_started_serves_the_bundle_it_validated() {
        // The positive case, without which every assertion above is satisfied by refusing
        // everything.
        let service = LocalService::start(&catalog_of(bundle()), crate::testing::fake_warehouse())
            .expect("an anchored bundle over a warehouse that answers validates");
        assert_eq!(service.definitions().version().as_str(), "test-1");
        // The `Debug` impl names the bundle rather than printing it.
        let rendered = format!("{service:?}");
        assert!(rendered.contains("definition_digest"), "{rendered}");
    }
}

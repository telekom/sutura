//! A statement that is only a scan of one listed source. The optimizer drops a projection of bare
//! columns and a filter that folds to true, so nothing is left above the scan, and the scan runs
//! whole at its source as a projected one does.

use std::collections::BTreeSet;

use super::{Engines, UNBOUNDED, asked, named};

/// `statement` answered at `geo` alone, with and without federation, as one engine holding the
/// table answers it.
fn answered_at_geo(statement: &str) {
    for federation_allowed in [false, true] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, federation_allowed, UNBOUNDED, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{statement}: a scan of one answering source is answered: {answer:?}");
        };
        assert_eq!(rows, legs.as_one(statement), "{statement}");
        assert_eq!(legs.opened, [BTreeSet::from([named("geo")])], "{statement}");
        assert_eq!(legs.ran, [named("geo")], "{statement}");
    }
}

#[test]
fn a_bare_column_of_one_source_runs_whole_at_it() {
    answered_at_geo("select region from dim_region");
}

#[test]
fn every_column_of_one_source_runs_whole_at_it() {
    answered_at_geo("select * from dim_region");
}

#[test]
fn a_filter_that_folds_to_true_leaves_a_scan_that_runs_whole_at_its_source() {
    answered_at_geo("select region from dim_region where 0 < 1+1+1");
}

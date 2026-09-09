//! Explicit cross-resource venues, not part of ordinary `BigQuery` acceptance.
//!
//! `just bigquery-cross-dataset` writes per-run fixtures to two disposable datasets in one billing
//! project. `just bigquery-cross-project` only reads preprovisioned mirrors: all committed corpus
//! tables except `dim_customer` live in the default dataset; that dimension lives in the other
//! project, and its default-dataset shadow contains the committed `cross_resource_shadow.csv` rows.
//! The mirror must preserve those fixtures, including the differing shadow. Neither task provisions
//! a dataset, project or grant. Missing configuration fails; no early return means live success.
//!
//! Both require `SUTURA_BQ_CROSS_BILLING_PROJECT`, `SUTURA_BQ_DATASET`, and the complete protected
//! address `SUTURA_BQ_RLS_PROJECT` / `SUTURA_BQ_RLS_DATASET`, before credentials are read. The writable
//! venue additionally needs `SUTURA_BQ_CROSS_DATASET_PROJECT` / `SUTURA_BQ_CROSS_DATASET`; the read-only
//! venue needs `SUTURA_BQ_MIRROR_PROJECT` / `SUTURA_BQ_MIRROR_DATASET`. Resource values belong only in
//! the invoking environment. Under GitHub Actions, tasks register masks before output; the script
//! does not redact local output.
//!
//! The same logical question is compiled separately over equivalent unqualified local models and
//! qualified `BigQuery` models. `DataFusion` deliberately refuses qualified tables; these are NOT
//! identical physical plans. The compiler must produce one source-native join, not federation.
//! This is shared-credential adapter execution, not a shipped feature or impersonation proof.

#[cfg(test)]
mod cross_resource_fixture;
#[cfg(test)]
#[path = "cross_resource_fixture/live.rs"]
mod live;
#[cfg(test)]
#[path = "naming/naming.rs"]
mod naming;
#[cfg(test)]
#[path = "support/support.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::model::{DatasetName, ModelName, ProjectName, QualifiedTable, TableQualifier};

    use crate::cross_resource_fixture::{Failed, FixturePort, InvalidVenue, Layout, Mode, RawPlacement, RawVenue, run};

    #[test]
    fn suffixing_changes_only_the_table_not_its_dataset_or_project() {
        let committed = crate::live::bundle();
        let qualifiers = [
            None,
            Some(TableQualifier::in_dataset(
                DatasetName::parse("fixture_data").expect("fixture"),
            )),
            Some(TableQualifier::in_project(
                ProjectName::parse("example-project").expect("fixture"),
                DatasetName::parse("fixture_data").expect("fixture"),
            )),
        ];
        let observed: Vec<_> = qualifiers
            .iter()
            .map(|qualifier| {
                let original = crate::live::remap(&committed, |model| {
                    QualifiedTable::new(qualifier.clone(), model.table_name().clone())
                });
                let renamed = crate::naming::suffixed_bundle(&original, "run_a", "cross");
                original.definitions().models().iter().all(|(name, before)| {
                    let after = &renamed.definitions().models()[name];
                    after.table().qualifier() == before.table().qualifier()
                        && *after.table_name()
                            == crate::naming::suffixed_table(before.table_name(), "run_a", "cross").expect("fixture")
                })
            })
            .collect();
        assert_eq!(observed, [true, true, true], "unqualified, dataset, project+dataset");
    }

    #[derive(Default)]
    struct Counts {
        effects: [usize; 4],
        refuse_comparison: bool,
        refuse_load: bool,
    }

    impl FixturePort for Counts {
        fn open(&mut self, _layout: &Layout) -> Result<(), Failed> {
            self.effects[0] += 1;
            Ok(())
        }

        fn load(&mut self, _layout: &Layout) -> Result<(), Failed> {
            self.effects[1] += 1;
            if self.refuse_load { Err(Failed::Load) } else { Ok(()) }
        }

        fn compare(&mut self, _layout: &Layout) -> Result<(), Failed> {
            self.effects[2] += 1;
            if self.refuse_comparison { Err(Failed::Query) } else { Ok(()) }
        }

        fn drop_tables(&mut self, _layout: &Layout) -> Result<(), Failed> {
            self.effects[3] += 1;
            Ok(())
        }
    }

    fn expected() -> BTreeSet<ModelName> {
        ["fact", "dimension"]
            .into_iter()
            .map(|name| ModelName::parse(name).expect("fixture"))
            .collect()
    }

    fn raw() -> RawVenue<'static> {
        RawVenue {
            billing: "example-billing",
            default: ("example-billing", "fixtures"),
            protected: ("example-billing", "protected_rows"),
            absent: ("example-billing", "absent_rows"),
        }
    }

    fn placements<'a>(dimension: (&'a str, &'a str)) -> [RawPlacement<'a>; 2] {
        [
            RawPlacement {
                model: "fact",
                address: ("example-billing", "fixtures"),
            },
            RawPlacement {
                model: "dimension",
                address: dimension,
            },
        ]
    }

    #[test]
    fn every_destination_is_admitted_before_open_load_or_drop() {
        let cases = [
            (("", "dimension_rows"), InvalidVenue::Address),
            (("example-billing", " "), InvalidVenue::Address),
            (("example-billing", "protected_rows"), InvalidVenue::Protected),
            (("example-billing", "fixtures"), InvalidVenue::WritableScope),
            (("example-partner", "dimension_rows"), InvalidVenue::WritableScope),
        ];
        let observed: Vec<_> = cases
            .into_iter()
            .map(|(destination, expected_error)| {
                // The first mapping is valid: a later refusal must still precede ALL effects.
                let mut port = Counts::default();
                let result = run(
                    &raw(),
                    &placements(destination),
                    &expected(),
                    Mode::WritableDatasets,
                    &mut port,
                );
                (
                    matches!(result, Err(Failed::Configuration(error)) if error == expected_error),
                    port.effects,
                )
            })
            .collect();
        assert_eq!(observed, vec![(true, [0; 4]); 5]);
    }

    #[test]
    fn missing_protected_identity_and_incomplete_model_maps_refuse_without_effects() {
        let mut missing = raw();
        missing.protected = ("", "protected_rows");
        let mut blank = raw();
        blank.protected = ("example-billing", "");
        let mut wrong_billing = raw();
        wrong_billing.billing = "example-partner";
        let mut protected_default = raw();
        protected_default.default = protected_default.protected;
        let mut overlapping_negative = raw();
        overlapping_negative.absent = overlapping_negative.default;
        let ordinary = raw();
        let regular = placements(("example-billing", "dimension_rows"));
        let duplicate = [
            RawPlacement {
                model: "fact",
                address: regular[0].address,
            },
            RawPlacement {
                model: "fact",
                address: regular[1].address,
            },
        ];
        let cases = [
            (&missing, regular.as_slice(), InvalidVenue::Address),
            (&blank, regular.as_slice(), InvalidVenue::Address),
            (&wrong_billing, regular.as_slice(), InvalidVenue::Billing),
            (&protected_default, regular.as_slice(), InvalidVenue::Protected),
            (&overlapping_negative, regular.as_slice(), InvalidVenue::NegativeScope),
            (&ordinary, &regular[..1], InvalidVenue::Mapping),
            (&ordinary, duplicate.as_slice(), InvalidVenue::Mapping),
        ];
        let mut observed = Vec::new();
        for (venue, mapping, expected_error) in cases {
            for mode in [Mode::WritableDatasets, Mode::ReadOnlyProjects] {
                let mut port = Counts::default();
                let result = run(venue, mapping, &expected(), mode, &mut port);
                observed.push((
                    matches!(result, Err(Failed::Configuration(error)) if error == expected_error),
                    port.effects,
                ));
            }
        }
        assert_eq!(observed, vec![(true, [0; 4]); 14]);
    }

    #[test]
    fn read_only_project_addresses_are_distinct_and_never_load_or_drop() {
        let mut port = Counts::default();
        // Same dataset spelling as the protected address, but in a distinct project.
        let result = run(
            &raw(),
            &placements(("example-partner", "protected_rows")),
            &expected(),
            Mode::ReadOnlyProjects,
            &mut port,
        );
        result.expect("read-only destinations are admitted");
        assert_eq!(port.effects, [1, 0, 1, 0]);
        let mut refused = Counts::default();
        let result = run(
            &raw(),
            &placements(("example-billing", "dimension_rows")),
            &expected(),
            Mode::ReadOnlyProjects,
            &mut refused,
        );
        assert!(matches!(result, Err(Failed::Configuration(InvalidVenue::ReadOnlyScope))));
        assert_eq!(refused.effects, [0; 4]);
    }

    #[test]
    fn a_failed_writable_comparison_still_attempts_cleanup() {
        let mut port = Counts {
            refuse_comparison: true,
            ..Counts::default()
        };
        let result = run(
            &raw(),
            &placements(("example-billing", "dimension_rows")),
            &expected(),
            Mode::WritableDatasets,
            &mut port,
        );
        assert!(matches!(result, Err(Failed::Query)));
        assert_eq!(port.effects, [1, 1, 1, 1]);
    }

    #[test]
    fn a_partial_load_failure_still_attempts_cleanup_without_comparing() {
        let mut port = Counts {
            refuse_load: true,
            ..Counts::default()
        };
        let result = run(
            &raw(),
            &placements(("example-billing", "dimension_rows")),
            &expected(),
            Mode::WritableDatasets,
            &mut port,
        );
        assert!(matches!(result, Err(Failed::Load)));
        assert_eq!(port.effects, [1, 1, 0, 1]);
    }

    #[test]
    #[ignore = "requires two explicitly disposable datasets; writes only owned per-run fixtures"]
    fn join_across_datasets_matches_the_engine_and_refuses_an_absent_owned_name() {
        crate::live::execute(Mode::WritableDatasets).expect("the explicit cross-dataset venue must complete");
    }

    #[test]
    #[ignore = "requires preprovisioned cross-project mirrors and differing default shadow; read-only"]
    fn join_across_projects_matches_the_engine_and_refuses_an_absent_owned_name() {
        crate::live::execute(Mode::ReadOnlyProjects).expect("the explicit cross-project mirror must complete");
    }
}

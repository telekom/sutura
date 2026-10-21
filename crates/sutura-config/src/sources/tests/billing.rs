use super::super::billing::InvalidBilling;
use super::{InvalidSourceRegistry, RawSourceEntry, bigquery, impersonating, registry_billed};
use crate::raw::{RawBilling, RawBillingGroup};

fn refusal((entry, billing): (RawSourceEntry<'_>, Option<RawBilling>)) -> InvalidBilling {
    match registry_billed(&entry, billing.as_ref()) {
        Err(InvalidSourceRegistry::Billing { cause, .. }) => cause,
        other => panic!("expected a billing refusal, got {other:?}"),
    }
}

fn caller_run(billing: RawBilling) -> (RawSourceEntry<'static>, Option<RawBilling>) {
    let entry = RawSourceEntry {
        posture: "impersonation-at-source",
        acknowledged_because: None,
        ..bigquery("warehouse")
    };
    (entry, Some(billing))
}

fn shared(billing: RawBilling) -> (RawSourceEntry<'static>, Option<RawBilling>) {
    (bigquery("warehouse"), Some(billing))
}

fn group(group: &str, project: &str) -> RawBillingGroup {
    RawBillingGroup {
        group: String::from(group),
        project: String::from(project),
    }
}

#[test]
fn a_removed_billing_project_is_refused_naming_the_keys_that_replace_it() {
    let cause = refusal((
        RawSourceEntry {
            billing_project: Some("acme-analytics"),
            ..bigquery("warehouse")
        },
        None,
    ));
    assert_eq!(cause, InvalidBilling::BillingProjectRemoved);
    assert!(cause.to_string().contains("billing.by_group"), "{cause}");
}

#[test]
fn a_removed_dataset_is_refused_naming_the_qualified_table_path() {
    let cause = refusal((
        RawSourceEntry {
            dataset: Some("warehouse"),
            ..bigquery("warehouse")
        },
        None,
    ));
    assert_eq!(cause, InvalidBilling::DatasetRemoved);
    assert!(cause.to_string().contains("project.dataset.table"), "{cause}");
}

#[test]
fn an_unusable_group_project_is_refused_naming_its_entry() {
    let cause = refusal(caller_run(RawBilling {
        by_group: vec![group("sales", "sales-dev-1234"), group("ops", "1bad")],
        ..RawBilling::default()
    }));
    assert!(matches!(cause, InvalidBilling::GroupProject { index: 1, .. }), "{cause:?}");
    assert!(cause.to_string().contains("billing.by_group[1]"), "{cause}");
}

#[test]
fn a_group_listed_twice_is_refused() {
    let cause = refusal(caller_run(RawBilling {
        by_group: vec![group("sales", "sales-dev-1234"), group("sales", "ops-12345")],
        ..RawBilling::default()
    }));
    assert_eq!(
        cause,
        InvalidBilling::GroupTwice {
            group: String::from("sales")
        }
    );
}

#[test]
fn billing_on_a_source_that_is_not_bigquery_is_refused() {
    let cause = refusal((impersonating("local"), Some(RawBilling::default())));
    assert_eq!(cause, InvalidBilling::BillingOnOtherKind);
}

#[test]
fn caller_billing_on_a_shared_source_is_refused() {
    for (billing, key) in [
        (
            RawBilling {
                by_group: vec![group("sales", "sales-dev-1234")],
                service_account_project: Some(String::from("acme-analytics")),
                ..RawBilling::default()
            },
            "by_group",
        ),
        (
            RawBilling {
                caller_may_name: true,
                service_account_project: Some(String::from("acme-analytics")),
                ..RawBilling::default()
            },
            "caller_may_name",
        ),
    ] {
        assert_eq!(refusal(shared(billing)), InvalidBilling::CallerBillingOnSharedSource { key });
    }
}

#[test]
fn a_shared_source_without_its_service_account_project_is_refused() {
    assert_eq!(
        refusal(shared(RawBilling::default())),
        InvalidBilling::ServiceAccountProjectMissing
    );
    assert_eq!(
        refusal((bigquery("warehouse"), None)),
        InvalidBilling::ServiceAccountProjectMissing
    );
}

#[test]
fn a_service_account_project_on_a_caller_run_source_is_refused() {
    let cause = refusal(caller_run(RawBilling {
        service_account_project: Some(String::from("acme-analytics")),
        ..RawBilling::default()
    }));
    assert_eq!(cause, InvalidBilling::ServiceAccountProjectOnCallerRun);
}

//! What a `sources:` tree refuses, asserted on the typed variant rather than on the message.

use std::path::PathBuf;

use sutura_domain::model::SourceName;
use sutura_domain::source::{AcknowledgementReason, AnchorIdentity, ConflictingSourceIdentity, VerificationIdentity};

use super::{InvalidSourceRegistry, RawSourceEntry, SourceRegistry};
use crate::security::DeploymentIdentity;

/// The absolute directory every entry here points at, so a path refusal is the only reason a test
/// fails on a path.
const DATA: &str = "/srv/sutura/data";

fn alias(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test alias is a name")
}

fn single_user() -> DeploymentIdentity {
    DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
        .expect("a declared single-user mode parses")
}

/// A declared workload-identity block an impersonating source needs.
fn wif() -> crate::raw::RawWorkloadIdentity {
    crate::raw::RawWorkloadIdentity {
        audience: String::from(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
        ),
        scope: String::from("https://www.googleapis.com/auth/bigquery.readonly"),
    }
}

/// An entry with everything an impersonating source needs, so a test changes one field at a time.
fn impersonating(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "files",
        data_dir: Some(DATA),
        billing_project: None,
        dataset: None,
        credential_file: None,
        max_bytes_billed: None,
        posture: "impersonation-at-source",
        acknowledged_because: None,
        verification_identity: None,
        workload_identity: Some(wif()),
        host: None,
        unix_socket: None,
        port: None,
        database: None,
        user: None,
        password_file: None,
        transport_mode: None,
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    }
}

#[test]
fn a_duplicate_alias_is_refused_at_parse() {
    // REACHABLE, which is the reason the check exists rather than reading as belt-and-braces over a
    // map that cannot hold two of one key. A source name is TRIMMED when it is parsed - case is
    // preserved, whitespace is not - so `local` and `" local"` are two distinct keys in a YAML mapping
    // and one `SourceName`. Whichever entry lost would be the one nobody opened, with the deployment
    // configured as though it had been.
    let entries = [impersonating("local"), impersonating(" local")];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("two keys, one source");
    assert_eq!(
        error,
        InvalidSourceRegistry::DuplicateAlias {
            alias: alias("local"),
            written: String::from(" local"),
        }
    );

    // And the two-distinct-sources case, so this cannot be passing by refusing every pair.
    let entries = [impersonating("local"), impersonating("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("two names, two sources");
    assert_eq!(registry.count(), 2);
}

#[test]
fn an_alias_that_is_not_a_name_is_refused_and_names_what_was_written() {
    // The key ends up inside a quoted identifier nowhere - it is a routing name - but it is also what a
    // model's `source:` has to spell, so the two parsers have to agree or a source is unreachable by
    // the models that name it.
    let entries = [impersonating("not a name")];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a space is not in an identifier");
    let InvalidSourceRegistry::Alias { ref written, .. } = error else {
        panic!("an unparseable key is an alias refusal, not {error:?}");
    };
    assert_eq!(written, "not a name");
}

#[test]
fn a_missing_file_location_is_refused_at_parse() {
    // Refused rather than defaulted to `catalog.data_dir`: a source that inherited the catalog's data
    // directory would be a second source reading the first one's files, which is a configuration
    // nobody wrote and nothing would show.
    //
    // **What this deliberately does NOT check is whether the directory exists.** `CatalogSettings`
    // declines the same check for a reason that holds here unchanged - a directory that disappears
    // between reading the configuration and opening the engine makes an existence check a claim that is
    // already stale - so a missing FILE is refused at boot, by the composition root that tries to
    // attach it, and this variant is about an entry that named no location at all.
    for absent in [None, Some(""), Some("   ")] {
        let entries = [RawSourceEntry {
            data_dir: absent,
            billing_project: None,
            dataset: None,
            credential_file: None,
            ..impersonating("local")
        }];
        let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a source has to say where it is");
        assert_eq!(error, InvalidSourceRegistry::NoDataDirectory { alias: alias("local") });
    }
}

#[test]
fn a_relative_path_is_refused_at_parse() {
    // A relative path resolves against the process working directory - a different directory on every
    // host, and never the one the operator meant. A service's working directory is whatever its
    // supervisor chose, which is the difference from `catalog.data_dir` on a command line.
    for relative in ["data", "./data", "../elsewhere/data"] {
        let entries = [RawSourceEntry {
            data_dir: Some(relative),
            billing_project: None,
            dataset: None,
            credential_file: None,
            ..impersonating("local")
        }];
        let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a relative path is not a location");
        assert_eq!(
            error,
            InvalidSourceRegistry::RelativeDataDirectory {
                alias: alias("local"),
                path: PathBuf::from(relative),
            }
        );
    }
    // The absolute case, so the assertion above is not satisfied by refusing every path.
    let entries = [impersonating("local")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("an absolute path is a location");
    // Matched rather than read off a `data_dir()` accessor, because a `bigquery` source has no
    // directory - see `ConfiguredSource::placement`.
    assert_eq!(
        registry.get(&alias("local")).expect("the entry is there").placement(),
        &super::placement::SourcePlacement::Files {
            data_dir: PathBuf::from(DATA)
        }
    );
}

#[test]
fn a_kind_this_build_has_no_adapter_for_is_refused_at_parse() {
    // **Where "a source this build cannot open" is refused now.** The composition root used to compare
    // the catalog's source NAME against a hard-coded `local`, which refused a legitimate deployment -
    // an operator who holds their warehouse extract as a directory of files and calls that source
    // `warehouse` was told this build had no adapter for it, on the strength of the alias.
    //
    // The kind is what carries that fact instead, and it is a PARSE refusal because the vocabulary is
    // closed: the set of kinds is the set of adapters, so a word outside it is a value no build could
    // honour. Which adapter opens a declared kind is then an exhaustive match in the composition root,
    // so a second kind is a compile error there rather than an arm that falls through.
    //
    // **The word being refused used to be `bigquery`, and a fourth dialect made this test fail rather
    // than mislead** - the good direction. Two refusals now exist where this test used to describe
    // one, and they are not the same:
    //
    // - a word that is not a kind AT ALL - `snowflake` - is refused HERE, at the parse, because no
    //   build of this repository has such an adapter;
    // - a kind this repository HAS and this binary did not link - `bigquery` - parses fine and is
    //   refused by the composition root, which is the only place that knows what was linked.
    //
    // Conflating them is how an operator gets sent looking for a typo in a word that is spelled right.
    let entries = [RawSourceEntry {
        kind: "snowflake",
        ..impersonating("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("snowflake is not a kind of data system");
    let InvalidSourceRegistry::Kind {
        alias: ref refused,
        ref cause,
    } = error
    else {
        panic!("an unknown kind is a kind refusal, not {error:?}");
    };
    assert_eq!(refused.as_str(), "warehouse");
    // The message lists what this build CAN open, so an operator does not have to find it.
    let rendered = cause.to_string();
    assert!(rendered.contains("files"), "{rendered}");

    // And an alias that is not the built-in engine's name is accepted, which is the deployment the old
    // comparison refused. This is the positive half, and it is the whole point of the change.
    let entries = [impersonating("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("a declared files source is a files source");
    assert_eq!(
        registry
            .get(&alias("warehouse"))
            .map(super::ConfiguredSource::kind)
            .map(super::SourceKind::as_str),
        Some("files")
    );
}

#[test]
fn a_word_that_is_not_a_posture_is_refused_and_the_two_that_are_parse() {
    let entries = [RawSourceEntry {
        posture: "trusted",
        ..impersonating("local")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("`trusted` is not a posture");
    let InvalidSourceRegistry::Posture { ref alias, ref cause } = error else {
        panic!("an unknown word is a posture refusal, not {error:?}");
    };
    assert_eq!(alias.as_str(), "local");
    // The message lists both spellings, so an operator does not have to find them.
    let rendered = cause.to_string();
    assert!(rendered.contains("shared-service-user"), "{rendered}");
    assert!(rendered.contains("impersonation-at-source"), "{rendered}");

    // Both real postures parse, and a shared one in single-user mode borrows the mode's own reason as
    // its witness - which is what makes `examples/single-player` a first-class deployment rather than
    // one that has to write an acknowledgement per source to say the obvious.
    let entries = [
        impersonating("warehouse"),
        RawSourceEntry {
            posture: "shared-service-user",
            workload_identity: None,
            ..impersonating("local")
        },
    ];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("two postures parse");
    assert_eq!(
        registry
            .each()
            .map(|(name, source)| (
                name.as_str(),
                source.posture().map(sutura_domain::source::SourcePosture::as_str)
            ))
            .collect::<Vec<(&str, Option<&str>)>>(),
        vec![
            ("local", Some("shared-service-user")),
            ("warehouse", Some("impersonation-at-source"))
        ]
    );
}

#[test]
fn a_shared_source_with_no_witness_anywhere_parses_without_an_identity_so_the_refusal_can_list_it() {
    // The state a FILE can describe and a running deployment may not be in. It parses, deliberately:
    // the refusal for it is a `NotFitToServe`, which is a LIST `Settings::refusals` returns, and a
    // parse error here would collapse "these three sources are unacknowledged" into whichever one came
    // first.
    let entries = [RawSourceEntry {
        posture: "shared-service-user",
        workload_identity: None,
        ..impersonating("local")
    }];
    let registry = SourceRegistry::parse(&entries, Some(&DeploymentIdentity::SubjectPerRequest)).expect("it parses");
    let source = registry.get(&alias("local")).expect("the entry is there");
    assert!(
        source.identity().is_none(),
        "a shared source with no acknowledgement has no identity a deployment may be served with"
    );

    // With the acknowledgement written on its own entry, the witness exists and carries the reason.
    let entries = [RawSourceEntry {
        posture: "shared-service-user",
        acknowledged_because: Some("a read-only reporting replica every caller is entitled to see"),
        workload_identity: None,
        ..impersonating("local")
    }];
    let registry = SourceRegistry::parse(&entries, Some(&DeploymentIdentity::SubjectPerRequest)).expect("it parses");
    let identity = registry
        .get(&alias("local"))
        .and_then(super::ConfiguredSource::identity)
        .expect("an acknowledged shared source has an identity");
    let AnchorIdentity::TheSharedIdentity { declared } = identity.anchors_run_as() else {
        panic!("a shared source's anchors run as the shared identity");
    };
    assert_eq!(
        declared.reason(),
        &AcknowledgementReason::parse("a read-only reporting replica every caller is entitled to see")
            .expect("a test reason is a reason")
    );
}

#[test]
fn a_verification_identity_is_carried_on_an_impersonating_source_and_refused_on_a_shared_one() {
    // The pairing check, from the settings side. On a shared source the verification identity IS the
    // shared identity, so a name there would be read by nothing - and a declaration that does nothing
    // reads as a control that is in place.
    let entries = [RawSourceEntry {
        verification_identity: Some("sutura_anchor_reader"),
        workload_identity: Some(wif()),
        ..impersonating("warehouse")
    }];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("an impersonating source may declare one");
    let identity = registry
        .get(&alias("warehouse"))
        .and_then(super::ConfiguredSource::identity)
        .expect("it parsed");
    let AnchorIdentity::Declared { identity } = identity.anchors_run_as() else {
        panic!("a declared verification identity is what the anchor path would run as");
    };
    assert_eq!(identity.as_str(), "sutura_anchor_reader");

    let entries = [RawSourceEntry {
        posture: "shared-service-user",
        acknowledged_because: Some("a directory of CSVs this deployment owns"),
        verification_identity: Some("sutura_anchor_reader"),
        workload_identity: None,
        ..impersonating("local")
    }];
    let error =
        SourceRegistry::parse(&entries, Some(&DeploymentIdentity::SubjectPerRequest)).expect_err("nothing would read that key");
    let InvalidSourceRegistry::Conflict { ref alias, ref cause } = error else {
        panic!("a contradictory pairing is a conflict, not {error:?}");
    };
    assert_eq!(alias.as_str(), "local");
    assert_eq!(
        *cause,
        ConflictingSourceIdentity::VerificationIdentityOnASharedSource {
            at: self::alias("local"),
            key: VerificationIdentity::KEY,
        }
    );
}

#[test]
fn an_impersonating_source_that_declares_no_verification_identity_parses_and_says_so() {
    // Not a parse refusal, and that is the decision: a deployment may legitimately run an impersonating
    // source with no boot identity, as long as no metric reading it declares an anchor. Whether that
    // holds is a fact about the CATALOG, so the refusal is the composition root's - and this asserts
    // that what the parse hands it is the third variant rather than an absence a reader has to
    // interpret.
    let entries = [impersonating("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("declaring none parses");
    let identity = registry
        .get(&alias("warehouse"))
        .and_then(super::ConfiguredSource::identity)
        .expect("an impersonating source needs no witness");
    assert_eq!(identity.anchors_run_as(), AnchorIdentity::NoneDeclared);
}

#[test]
fn an_empty_tree_is_a_registry_and_not_a_refusal() {
    // Deliberate, and the reason is where the refusal for it belongs rather than a softening: a
    // deployment that declared no source cannot answer anything, and what refuses it is the composition
    // root finding the catalog naming a source with no entry. Refusing here would mean
    // `Settings::load` on the embedded defaults could not produce a `Settings` at all, and the defaults
    // are what the `prompt` command and every settings test read.
    let registry = SourceRegistry::parse(&[], None).expect("no sources is a registry");
    assert!(registry.is_empty());
    assert_eq!(registry.count(), 0);
    assert!(registry.get(&alias("local")).is_none());
}

#[test]
fn the_mode_decides_where_a_shared_witness_may_come_from() {
    // The direction that is load-bearing. The same entry, byte for byte, has a witness in single-user
    // mode and none in multi-user mode - because in single-user mode the one identity is the one user's
    // own and the operator wrote a reason for the mode, and in multi-user mode the acknowledgement has
    // to be written against the source that would be shared.
    let shared = [RawSourceEntry {
        posture: "shared-service-user",
        workload_identity: None,
        ..impersonating("local")
    }];
    assert!(
        SourceRegistry::parse(&shared, Some(&single_user()))
            .expect("it parses")
            .get(&alias("local"))
            .and_then(super::ConfiguredSource::identity)
            .is_some(),
        "single-user mode supplies the witness"
    );
    assert!(
        SourceRegistry::parse(&shared, Some(&DeploymentIdentity::SubjectPerRequest))
            .expect("it parses")
            .get(&alias("local"))
            .and_then(super::ConfiguredSource::identity)
            .is_none(),
        "multi-user mode does not, and no acknowledgement is inherited"
    );
    assert!(
        SourceRegistry::parse(&shared, None)
            .expect("it parses")
            .get(&alias("local"))
            .and_then(super::ConfiguredSource::identity)
            .is_none(),
        "and an undeclared mode supplies nothing either"
    );
}

// ------------------------------------------------------------------- per-kind fields ----

/// A `bigquery` entry with everything that kind needs, so a test changes one field at a time.
fn bigquery(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "bigquery",
        data_dir: None,
        billing_project: Some("acme-analytics"),
        dataset: Some("warehouse"),
        credential_file: Some("/etc/sutura/bigquery.json"),
        max_bytes_billed: Some(1024 * 1024 * 1024),
        posture: "shared-service-user",
        acknowledged_because: Some("one service account reaching the dataset for everybody who asks"),
        verification_identity: None,
        workload_identity: None,
        host: None,
        unix_socket: None,
        port: None,
        database: None,
        user: None,
        password_file: None,
        transport_mode: None,
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    }
}

#[test]
fn a_bigquery_source_declares_its_billing_project_and_dataset() {
    // The declaration this step exists to land: the billing project is a value an operator wrote,
    // one step before anything impersonates, so the per-subject step does not have to introduce it.
    let entries = [bigquery("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("a complete bigquery entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    assert_eq!(configured.kind(), super::SourceKind::BigQuery);
    match configured.placement() {
        super::placement::SourcePlacement::BigQuery {
            billing_project,
            dataset,
            credential_file,
            max_bytes_billed,
        } => {
            assert_eq!(billing_project.as_str(), "acme-analytics");
            assert_eq!(dataset.as_str(), "warehouse");
            assert_eq!(credential_file, std::path::Path::new("/etc/sutura/bigquery.json"));
            assert_eq!(*max_bytes_billed, 1024 * 1024 * 1024);
        }
        super::placement::SourcePlacement::Files { .. } | super::placement::SourcePlacement::Postgres { .. } => {
            panic!("the entry declared kind: bigquery");
        }
    }
}

#[test]
fn a_bigquery_source_missing_a_required_key_does_not_parse() {
    // Fail-closed, and it names the key. There is nothing to infer a billing project from: it is a
    // path segment of the request that submits a job.
    let without_project = RawSourceEntry {
        billing_project: None,
        ..bigquery("warehouse")
    };
    let without_dataset = RawSourceEntry {
        dataset: None,
        ..bigquery("warehouse")
    };
    // The two keys the composition root needs to OPEN the source, as opposed to the two that name
    // it: without a credential file there is no identity to reach the dataset as, and without a
    // ceiling there is no bound on what one question may be billed for.
    let without_credential = RawSourceEntry {
        credential_file: None,
        ..bigquery("warehouse")
    };
    let without_ceiling = RawSourceEntry {
        max_bytes_billed: None,
        ..bigquery("warehouse")
    };
    for (key, entry) in [
        ("billing_project", without_project),
        ("dataset", without_dataset),
        ("credential_file", without_credential),
        ("max_bytes_billed", without_ceiling),
    ] {
        let entries = [entry];
        let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a bigquery entry missing a key is refused");
        match error {
            InvalidSourceRegistry::MissingForKind { kind, key: named, .. } => {
                assert_eq!(kind, super::SourceKind::BigQuery);
                assert_eq!(named, key);
            }
            other => panic!("expected a missing-key refusal for {key}, got {other:?}"),
        }
    }
}

#[test]
fn a_key_that_means_nothing_for_this_kind_is_refused_rather_than_ignored() {
    // **Both directions**, because the second one is the one that would otherwise be a configuration
    // nobody can see: an operator who writes `billing_project` under a `files` source believes it is
    // in effect. `deny_unknown_fields` cannot catch it - the key IS known, just not to this kind.
    let entries = [RawSourceEntry {
        billing_project: Some("acme-analytics"),
        ..impersonating("local")
    }];
    let error =
        SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a billing project on a files source is refused");
    assert!(
        matches!(
            error,
            InvalidSourceRegistry::KeyNotForKind {
                kind: super::SourceKind::Files,
                key: "billing_project",
                ..
            }
        ),
        "{error:?}"
    );

    // And the other way: a data directory on a bigquery source names no files anybody reads.
    let entries = [RawSourceEntry {
        data_dir: Some(DATA),
        ..bigquery("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a data dir on a bigquery source is refused");
    assert!(
        matches!(
            error,
            InvalidSourceRegistry::KeyNotForKind {
                kind: super::SourceKind::BigQuery,
                key: "data_dir",
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_relative_credential_file_is_refused_and_the_refusal_names_the_key() {
    // The same argument `data_dir` makes and a different key, which is why the refusal names one: a
    // service's working directory is whatever its supervisor chose, so a relative path is a different
    // file on every host - and for a CREDENTIAL that is the difference between the identity an
    // operator declared and whatever happened to be beside the process.
    let entries = [RawSourceEntry {
        credential_file: Some("bigquery.json"),
        ..bigquery("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a relative credential file is refused");
    match error {
        InvalidSourceRegistry::RelativePath { key, ref path, .. } => {
            assert_eq!(key, "credential_file");
            assert_eq!(path, std::path::Path::new("bigquery.json"));
        }
        other => panic!("expected a relative-path refusal, got {other:?}"),
    }
}

#[test]
fn the_two_keys_that_open_a_bigquery_source_mean_nothing_on_a_files_source() {
    // The other direction for the two keys this step added, held to the same rule the first two are:
    // a key an operator wrote and a deployment reads past is a configuration nobody can see.
    for (key, entry) in [
        (
            "credential_file",
            RawSourceEntry {
                credential_file: Some("/etc/sutura/bigquery.json"),
                ..impersonating("local")
            },
        ),
        (
            "max_bytes_billed",
            RawSourceEntry {
                max_bytes_billed: Some(1024),
                ..impersonating("local")
            },
        ),
    ] {
        let entries = [entry];
        let error =
            SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a bigquery-only key on a files source is refused");
        match error {
            InvalidSourceRegistry::KeyNotForKind { kind, key: named, .. } => {
                assert_eq!(kind, super::SourceKind::Files);
                assert_eq!(named, key);
            }
            other => panic!("expected a wrong-kind refusal for {key}, got {other:?}"),
        }
    }
}

#[test]
fn an_unusable_resource_name_is_refused_at_the_key_that_carries_it() {
    // The newtype's own refusal, surfaced with the key beside it - so an operator is told which line
    // to fix rather than that "a name" is wrong. The value is NOT in the message: a project id is one
    // of the things this repository does not print.
    let entries = [RawSourceEntry {
        billing_project: Some("acme/../other"),
        ..bigquery("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a path-escaping project id is refused");
    match error {
        InvalidSourceRegistry::ResourceName { key, ref cause, .. } => {
            assert_eq!(key, "billing_project");
            assert!(!cause.to_string().contains("acme"), "{cause}");
        }
        other => panic!("expected a resource-name refusal, got {other:?}"),
    }
}

#[test]
fn bigquery_is_a_kind_the_vocabulary_names() {
    // The message an operator sees for a typo has to list the kinds, and the parser and that list
    // cannot disagree - `SourceKind::NAMES` is the one source for both.
    assert!(super::SourceKind::NAMES.contains(&"bigquery"));
    assert_eq!(
        super::SourceKind::parse("bigquery").expect("bigquery is a kind"),
        super::SourceKind::BigQuery
    );
    let err = super::SourceKind::parse("snowflake").expect_err("snowflake is not a kind this build has");
    let rendered = err.to_string();
    assert!(rendered.contains("bigquery"), "{rendered}");
    assert!(rendered.contains("files"), "{rendered}");
}

#[test]
fn a_workload_identity_block_belongs_to_impersonation_and_nowhere_else() {
    // The token-exchange setup is the impersonating shape's own: an impersonating source must name
    // the provider its subject's credential is exchanged against, and a non-impersonating source must
    // not carry one at all - a declaration nothing will hand a token to is a configuration nobody can
    // see.
    let missing = [RawSourceEntry {
        workload_identity: None,
        ..impersonating("warehouse")
    }];
    let error = SourceRegistry::parse(&missing, Some(&single_user())).expect_err("an impersonating source needs a provider");
    assert!(matches!(error, InvalidSourceRegistry::MissingWorkloadIdentity { .. }));

    let on_shared = [RawSourceEntry {
        posture: "shared-service-user",
        workload_identity: Some(wif()),
        ..impersonating("local")
    }];
    let error = SourceRegistry::parse(&on_shared, Some(&single_user())).expect_err("a shared source may not carry one");
    assert!(matches!(
        error,
        InvalidSourceRegistry::WorkloadIdentityNotImpersonating { .. }
    ));

    // And the good shape carries it, so the broker the composition root builds has somewhere to read
    // it from.
    let parsed = SourceRegistry::parse(&[impersonating("warehouse")], Some(&single_user()))
        .expect("an impersonating source with a workload identity parses");
    let source = parsed.get(&alias("warehouse")).expect("it was registered");
    let workload = source.workload_identity().expect("the block was carried");
    assert_eq!(workload.scope().as_str(), "https://www.googleapis.com/auth/bigquery.readonly");
}

// ----------------------------------------------------------------- per-kind: postgres ----

/// A `postgres` entry with everything that kind needs, over a unix socket, so a test changes one
/// field at a time. The socket path means this deploys like the nix tier - no network, no TLS.
fn postgres(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "postgres",
        data_dir: None,
        billing_project: None,
        dataset: None,
        credential_file: None,
        max_bytes_billed: None,
        posture: "shared-service-user",
        acknowledged_because: Some("one service role reaching the database for everybody who asks"),
        verification_identity: None,
        workload_identity: None,
        host: None,
        unix_socket: Some("/tmp/sutura-pg"),
        port: Some(5432),
        database: Some("sutura"),
        user: Some("sutura"),
        password_file: Some("/etc/sutura/pg-password"),
        transport_mode: Some("plaintext"),
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    }
}

#[test]
fn a_postgres_source_declares_its_connection_and_its_plaintext_choice() {
    let entries = [postgres("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("a complete postgres entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    assert_eq!(configured.kind(), super::SourceKind::Postgres);
    match configured.placement() {
        super::placement::SourcePlacement::Postgres {
            host,
            unix_socket,
            port,
            database,
            user,
            password_file,
            transport,
        } => {
            assert!(host.is_none());
            assert_eq!(unix_socket, &Some(std::path::PathBuf::from("/tmp/sutura-pg")));
            assert_eq!(*port, 5432);
            assert_eq!(database, "sutura");
            assert_eq!(user, "sutura");
            assert_eq!(password_file, std::path::Path::new("/etc/sutura/pg-password"));
            assert_eq!(*transport, super::transport::SourceTransport::Plaintext);
        }
        other => panic!("expected a postgres placement, got {other:?}"),
    }
}

#[test]
fn a_postgres_source_missing_a_required_key_does_not_parse() {
    // Fail-closed, and it names the key. A database, a role, a password file and a transport are
    // each facts an adapter needs that nothing here can guess.
    for (key, entry) in [
        (
            "database",
            RawSourceEntry {
                database: None,
                ..postgres("warehouse")
            },
        ),
        (
            "user",
            RawSourceEntry {
                user: None,
                ..postgres("warehouse")
            },
        ),
        (
            "password_file",
            RawSourceEntry {
                password_file: None,
                ..postgres("warehouse")
            },
        ),
        (
            "port",
            RawSourceEntry {
                port: None,
                ..postgres("warehouse")
            },
        ),
    ] {
        let error =
            SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a postgres source has to say where it connects");
        let InvalidSourceRegistry::MissingForKind { key: refused, .. } = error else {
            panic!("expected a missing-key refusal, got {error:?}");
        };
        assert_eq!(refused, key, "{error}");
    }
}

#[test]
fn a_postgres_source_must_declare_exactly_one_of_host_or_unix_socket() {
    // Both directions are refused. Neither says nothing about where an adapter comes up to; both
    // says a TCP dial and a socket dial cannot both be what the connection is.
    let neither = RawSourceEntry {
        host: None,
        unix_socket: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[neither], Some(&single_user())).expect_err("a source has to say where it is");
    assert!(
        matches!(error, InvalidSourceRegistry::MissingForKind { key, .. } if key == "host_or_unix_socket"),
        "{error}"
    );

    let both = RawSourceEntry {
        host: Some("127.0.0.1"),
        unix_socket: Some("/tmp/pg"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[both], Some(&single_user())).expect_err("a source is one dial or the other");
    assert!(
        matches!(error, InvalidSourceRegistry::KeyNotForKind { key, .. } if key == "host_and_unix_socket"),
        "{error}"
    );
}

#[test]
fn a_postgres_source_refuses_the_keys_of_other_kinds() {
    // A postgres source that also declares a data_dir or a billing_project is a configuration
    // nobody can see - an operator believed a key was in effect and nothing would read it.
    let files_key = RawSourceEntry {
        data_dir: Some("/srv/data"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[files_key], Some(&single_user())).expect_err("a directory on a dataset is refused");
    assert!(matches!(error, InvalidSourceRegistry::KeyNotForKind { key: "data_dir", .. }));
}

#[test]
fn a_postgres_source_asking_for_tls_with_no_anchors_is_refused_naming_the_source() {
    // Rule 2 of docs/adr/0010: the trust store is stated, not inherited. `TrustAnchors` has no
    // default, so a `verified` or `mutual` transport with no `transport_anchors` cannot load.
    let no_anchors = RawSourceEntry {
        transport_mode: Some("verified"),
        transport_anchors: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[no_anchors], Some(&single_user())).expect_err("TLS with no anchors is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");
}

#[test]
fn a_remote_host_without_tls_is_a_startup_refusal_naming_the_key() {
    // Issue 124's fail-closed rule, as a test: a source a network can reach must not be declared
    // `plaintext`, because a password and a whole result set would cross it in clear text. The
    // refusal names `transport_mode`, which is where an operator writes the remedy.
    let remote = RawSourceEntry {
        host: Some("db.example.com"),
        unix_socket: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[remote], Some(&single_user())).expect_err("a remote plaintext source is refused");
    assert!(
        matches!(error, InvalidSourceRegistry::RemoteWithoutTls { ref host, .. } if host == "db.example.com"),
        "{error}"
    );
    assert!(error.to_string().contains("transport_mode"), "{error}");

    // A loopback LITERAL is the same declaration and is allowed - the rule is about reachability,
    // not about whether TLS happens to be spelled out.
    let local = RawSourceEntry {
        host: Some("127.0.0.1"),
        unix_socket: None,
        ..postgres("warehouse")
    };
    SourceRegistry::parse(&[local], Some(&single_user())).expect("a loopback plaintext source parses");

    // And the same remote host WITH a declared TLS mode and anchors is what a deployment writes.
    let secured = RawSourceEntry {
        host: Some("db.example.com"),
        unix_socket: None,
        transport_mode: Some("verified"),
        transport_anchors: Some("/etc/sutura/ca.pem"),
        ..postgres("warehouse")
    };
    SourceRegistry::parse(&[secured], Some(&single_user())).expect("a remote verified source parses");
}

#[test]
fn a_partial_client_certificate_is_refused_at_load() {
    // A certificate with no key, or a key with no certificate, is the same class as
    // `server.tls_certificate` without `server.tls_key`. Silent refusal would start mTLS disabled.
    let cert_only = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: Some("/tls/c.pem"),
        client_key: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[cert_only], Some(&single_user())).expect_err("a certificate alone is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");

    let key_only = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: None,
        client_key: Some("/tls/k.pem"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[key_only], Some(&single_user())).expect_err("a key alone is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");
}

#[test]
fn a_mutual_transport_declares_its_anchors_and_identity() {
    let entry = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: Some("/tls/c.pem"),
        client_key: Some("/tls/k.pem"),
        ..postgres("warehouse")
    };
    let registry = SourceRegistry::parse(&[entry], Some(&single_user())).expect("a complete mutual entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    match configured.placement() {
        super::placement::SourcePlacement::Postgres { transport, .. } => match transport {
            super::transport::SourceTransport::Mutual { anchors, identity } => {
                assert_eq!(*anchors, super::transport::TrustAnchors::System);
                assert_eq!(identity.certificate(), std::path::Path::new("/tls/c.pem"));
                assert_eq!(identity.key(), std::path::Path::new("/tls/k.pem"));
            }
            other => panic!("expected mutual, got {other:?}"),
        },
        other => panic!("expected a postgres placement, got {other:?}"),
    }
}

#[test]
fn postgres_is_a_kind_the_vocabulary_names() {
    assert!(super::SourceKind::NAMES.contains(&"postgres"));
    assert_eq!(
        super::SourceKind::parse("postgres").expect("postgres is a kind"),
        super::SourceKind::Postgres
    );
    let rendered = super::SourceKind::parse("snowflake")
        .expect_err("snowflake is not a kind")
        .to_string();
    assert!(rendered.contains("postgres"), "{rendered}");
}

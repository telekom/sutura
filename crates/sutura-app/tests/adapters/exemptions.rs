//! Every way a registered data system may skip a cell of the golden matrix, and the one reader a
//! cell may skip on. Declared by `adapters.rs`, so it is shared by every test target that is.

use std::path::Path;

use super::{DataSystemUnderTest, registered};

/// What a registered data system is excused from HERE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exempt {
    /// [`DataSystemUnderTest::available`] answered `false`: nothing to execute against on this host.
    Unavailable,
    /// It takes the port's default `declared_key`, so it counts no declared join key at all.
    KeyProbe,
    /// Its transport declines a dry run, so `dry_run` answers `PreFlight::NotAsked` and asks nothing.
    DryRun,
}

/// One named exemption: which [`DataSystemUnderTest::NAME`], from what, the `just` task that runs
/// the excused cell instead (`None` where nothing does), and why.
struct Exemption {
    system: &'static str,
    from: Exempt,
    runs_in: Option<&'static str>,
    because: &'static str,
}

/// Why a tier-backed entry may skip on a host with no tier up.
const NO_TIER: &str = "no tier answered discovery on this host; every gate that provisions one sets \
                       `SUTURA_DEV_REQUIRE_TIER`, which makes the absence fatal before it can skip";

/// **Every way a registered data system may not execute a cell of this matrix, by name.** A skip
/// is a value here rather than an early `return`, so a data system that stops answering on some
/// host is red until somebody writes down why.
const EXEMPTIONS: &[Exemption] = &[
    Exemption {
        system: "postgres",
        from: Exempt::Unavailable,
        runs_in: Some("validate"),
        because: NO_TIER,
    },
    Exemption {
        system: "clickhouse",
        from: Exempt::Unavailable,
        runs_in: Some("validate"),
        because: NO_TIER,
    },
    Exemption {
        system: "bigquery",
        from: Exempt::Unavailable,
        runs_in: Some("bigquery-conformance"),
        because: "cloud-only: no tier can stand a dataset up here, so it executes where SUTURA_BQ_DATASET \
                  names the dataset the `bigquery-conformance` CI job provisioned",
    },
    Exemption {
        system: "bigquery",
        from: Exempt::KeyProbe,
        runs_in: None,
        because: "`BigQueryWarehouse` takes the port's default `declared_key`, so a declared join key on a \
                  dataset is unchecked; the cell asserts that default still answers, so this reddens the \
                  day a probe is implemented",
    },
    Exemption {
        system: "bigquery",
        from: Exempt::DryRun,
        runs_in: None,
        because: "the ADBC transport declines a dry run, so `dry_run` answers `NotAsked` after rendering \
                  and never reaches the dataset; the cell asserts that answer, so this reddens the day the \
                  transport prices a statement",
    },
    Exemption {
        system: "oracle",
        from: Exempt::Unavailable,
        runs_in: None,
        because: "no gate provisions an Oracle and no CSV importer attaches this corpus to one - see its \
                  impl; its acceptance cell runs one whole-plan case outside this matrix",
    },
];

/// Whether `W` executes HERE: `true`, or `false` after printing its named exemption.
///
/// **The one reader of [`DataSystemUnderTest::available`] a cell may skip on**: an unavailable data
/// system [`EXEMPTIONS`] does not name panics instead. **The limit:** nothing stops a cell calling
/// `available()` itself; review holds that.
pub(crate) fn runs_here<W>() -> bool
where
    W: DataSystemUnderTest,
{
    runs(W::NAME, W::available())
}

/// [`runs_here`]'s decision over a name, so the refusal is reachable by a system nothing registers.
fn runs(system: &str, available: bool) -> bool {
    if available {
        return true;
    }
    assert!(
        excused(system, Exempt::Unavailable),
        "{system} answered `available() == false` and `adapters::exemptions::EXEMPTIONS` names no exemption \
         for it, so every cell would skip it in silence - name one, or give it a venue"
    );
    false
}

/// Whether [`EXEMPTIONS`] excuses the data system named `system` from `from`, printing the reason
/// where it does.
pub(crate) fn excused(system: &str, from: Exempt) -> bool {
    let found = EXEMPTIONS.iter().find(|entry| entry.system == system && entry.from == from);
    if let Some(entry) = found {
        eprintln!("exempt: {system} from {from:?} - {}", entry.because);
    }
    found.is_some()
}

/// **The refusal itself, not only its trigger**: an unavailable system no entry names panics, an
/// available one runs, and a named one skips - so a weakened `assert!` in [`runs`] reddens here.
#[test]
fn an_unavailable_data_system_no_exemption_names_is_refused_rather_than_skipped() {
    assert!(runs("unregistered", true), "an available system runs");
    assert!(!runs("oracle", false), "a system its entry names skips");
    let refused = std::panic::catch_unwind(|| runs("unregistered", false))
        .expect_err("an unavailable system no entry names must not skip in silence");
    let said = refused.downcast_ref::<String>().expect("the refusal is a formatted message");
    assert!(said.contains("names no exemption"), "the refusal says why: {said}");
}

/// **An exemption is held against the tree**: a registered data system, and a `runs_in`
/// task the `justfile` declares. **Does NOT reach** whether that task runs the cell,
/// nor whether an entry is still needed - one for a tier that is up here is simply not consulted.
#[test]
fn every_exemption_names_a_registered_data_system_and_where_its_cell_runs_instead() {
    let mut registered: Vec<&str> = Vec::new();
    macro_rules! named {
        ($name:ident, $adapter:ty) => {
            registered.push(<$adapter as DataSystemUnderTest>::NAME);
        };
    }
    registered!(data_systems: named);
    let justfile =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../justfile")).expect("the justfile reads");
    for entry in EXEMPTIONS {
        assert!(
            registered.contains(&entry.system),
            "an exemption names {}, which `registered!(data_systems:)` does not",
            entry.system
        );
        if let Some(task) = entry.runs_in {
            assert!(
                justfile.lines().any(|line| line
                    .strip_prefix(task)
                    .is_some_and(|rest| rest.starts_with(':') || rest.starts_with(' '))),
                "{}'s {:?} exemption says `just {task}` runs the cell instead, and the justfile declares no such task",
                entry.system,
                entry.from
            );
        }
    }
}

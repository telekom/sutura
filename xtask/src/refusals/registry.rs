//! The enrolled subjects themselves, kept apart from the gate that reads them.
//!
//! Widening [`ENROLLED`] is a diff here and nowhere else - see `crate::refusals`'s own module doc
//! for the direction this list does NOT hold: a refusal-shaped enum nobody enrols is subject to
//! nothing. The `const _` assertion below is the one property enrolling a subject cannot violate
//! silently: two subjects sharing an allow file would share their exceptions.

use super::declared::{Subject, variants};

/// Where the enum is declared. A constant so a move fails this gate loudly rather than making it
/// check nothing. `pub(super)`: the query subject's own fixture in `refusals::tests` is built from
/// the same path, so the real and fixture declarations cannot drift apart unnoticed.
pub(super) const DECLARED_IN: &str = "crates/sutura-domain/src/query.rs";

/// Where a query variant with no name evidence is argued for. `pub(super)` for the same reason as
/// [`DECLARED_IN`].
pub(super) const ALLOW_FILE: &str = "devco/refusals-unprovoked-allow";

/// One refused question: a caller asked something this deployment does not answer.
const QUERY: Subject = Subject {
    name: "RefusalReason",
    declared_in: DECLARED_IN,
    allow_file: ALLOW_FILE,
    // 23: `github.com/telekom/sutura#777` added `TopNotFederated`, then replaced it with
    // `TopOverUncertifiedRows` once the two-case rule made the blanket refusal's arm unreachable -
    // a straight substitution, so the count was unchanged. 22 since `docs/adr/0040` DELETED
    // `LegsDecideIdentityDifferently`: a cross-posture federated answer is disclosed per leg
    // instead, so there is no question left for that variant to refuse. 23 since
    // `telekom/sutura#780` ADDED `CrossModelRatioNotExecutable`: a ratio term naming another fact
    // model loads and is refused at plan time rather than mis-planned against the wrong table.
    // 24 since `telekom/sutura#967` ADDED `FederationLinkCompound`: a relationship crossing into a
    // remote data system with more than one join key is refused under its own name rather than
    // under `FederationLinkAmbiguous`, which means two RELATIONSHIPS crossing at once and would
    // tell a caller something untrue about one correctly declared compound key. 26:
    // `github.com/telekom/sutura#968` added `MetricsSpanDifferentModels` and
    // `MultiMetricNotExecutable` for a question naming more than one metric. 27: this diff
    // removed `MultiMetricNotExecutable` (every multi-metric question that shares model, time,
    // grain and dimensions now executes as one grouped statement) and added `TooManyMetrics` (the
    // count over `MAX_METRICS`, checked before any agreement) and `DuplicateMetricName` (a metric
    // named twice, refused rather than de-duplicated) - a net of +1 over the 26 above. 28: this
    // diff also added `MultiMetricFederationNotExecutable` - a multi-metric question that also
    // reaches a remote dimension is refused by name rather than silently planned as a federated
    // answer over only the first metric named. 29: this diff also added
    // `MultiMetricTopNotExecutable` - `top` names no metric to rank by, and this build's two
    // rendering paths disagreed about which measure it meant once there was more than one.
    variants: variants(29),
};

/// One refused deployment: the settings are not fit to serve and the process does not start.
const STARTUP: Subject = Subject {
    name: "NotFitToServe",
    declared_in: "crates/sutura-config/src/settings/posture.rs",
    allow_file: "devco/startup-refusals-unprovoked-allow",
    variants: variants(16),
};

/// One refused deployment again, and from the other side of the boot: the settings were fit and the
/// pinned bundle is not validated, so `verify_and_validate` refuses and the process does not start.
///
/// **The asymmetry `github.com/telekom/sutura#428` is about.** A `RefusalReason` refuses one
/// question; this refuses the whole deployment, and until it was enrolled the only thing holding
/// its variants was whether an author happened to look. All seven are named by tests, which keeps
/// the enrolment free: an eighth cannot arrive unnamed.
const VALIDATION: Subject = Subject {
    name: "NotValidated",
    declared_in: "crates/sutura-domain/src/pinned.rs",
    allow_file: "devco/validation-refusals-unprovoked-allow",
    variants: variants(7),
};

/// One refused raw statement: `docs/adr/0013`'s tool, off by default, with its own narrower
/// vocabulary - a row cap, a data-system volume bound, a statement that did not complete, and the
/// data system refusing at the identity/authorization level. Never `RefusalReason`'s: that
/// vocabulary is keyed to a compiled plan, and a raw statement has none.
const RAW: Subject = Subject {
    name: "RawRefusalReason",
    declared_in: "crates/sutura-domain/src/raw.rs",
    allow_file: "devco/raw-refusals-unprovoked-allow",
    variants: variants(4),
};

/// One refused TLS configuration: the certificate and key `sutura_config::TlsMaterial` parsed are
/// not usable material, and the process does not bind the socket.
///
/// `github.com/telekom/sutura#654` is the enrolment this was missing. All seven were named only at
/// their construction sites, which `named_in` does not count - only a `#[cfg(test)]` region does.
/// `NoCertificate` now is, by an assertion added to an existing test rather than a new one.
/// `NoKey`, `Malformed` and `NotConfigurable` are excused in `devco/tls-refusals-unprovoked-allow`
/// instead of tested: each is provokable (a working test for every one was written and verified
/// red then green by hand), but a brand new `#[test] fn` naming one is a test over PRODUCTION
/// CODE THIS DIFF DID NOT CHANGE, and `xtask/src/causality.rs` correctly refuses that as coverage
/// rather than a regression test - the allow file's own header records the reasoning per variant.
const TLS: Subject = Subject {
    name: "TlsNotUsable",
    declared_in: "crates/sutura-http/src/tls.rs",
    allow_file: "devco/tls-refusals-unprovoked-allow",
    variants: variants(7),
};

/// Every enum this gate reads. Widening it is a diff here and nowhere else; see `crate::refusals`'s
/// module doc for the direction this list does NOT hold.
pub(super) const ENROLLED: [&Subject; 5] = [&QUERY, &STARTUP, &VALIDATION, &RAW, &TLS];

/// Two subjects sharing one allow file would share their exceptions, and
/// `startup_exceptions_are_validated_and_do_not_cross_enum_boundaries` is the rule that forbids
/// it - a rule nothing compared until now, and invisible while every allow file was empty. This
/// is the check that matters once one of them holds an entry, not the count of how many do. A
/// `const` block, so a duplicated path fails the build rather than a run.
#[expect(
    clippy::indexing_slicing,
    reason = "const-evaluated and guarded by the loop bound: an out-of-range index here is a compile error, not a panic in a run"
)]
const _: () = {
    let mut outer = 0_usize;
    while outer < ENROLLED.len() {
        let mut inner = outer + 1;
        while inner < ENROLLED.len() {
            assert!(
                !same_path(ENROLLED[outer].allow_file, ENROLLED[inner].allow_file),
                "two enrolled subjects share an allow file, so their exceptions would cross"
            );
            inner += 1;
        }
        outer += 1;
    }
};

/// Byte equality on two paths, in a `const` context.
#[expect(
    clippy::indexing_slicing,
    reason = "const-evaluated and guarded by the loop bound: an out-of-range index here is a compile error, not a panic in a run"
)]
const fn same_path(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0_usize;
    while at < left.len() {
        if left[at] != right[at] {
            return false;
        }
        at += 1;
    }
    true
}

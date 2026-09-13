//! Fixtures shared by this crate's own tests: the posture and leg credential every fake
//! executes under, and a generous deadline.
//!
//! Split out of `lib.rs` for that file's own `max-lines` reason.

use sutura_domain::identity::Presented;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::deadline::Deadline;

/// The posture this crate's own tests open the engine with.
///
/// One definition shared by four test files, so a fixture cannot drift from the capability the adapter
/// declares. Shared is the honest value rather than a convenient one: one process, one
/// operating-system identity, and `IMPERSONATION` says there is nowhere for a subject to arrive.
pub(crate) fn test_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("one process reading local files as one identity")
                .expect("a fixture reason is a reason"),
        ),
    }
}

/// What this crate's own tests execute a leg as.
///
/// The deployment's own identity for the source, carrying the same acknowledgement
/// [`test_posture`] declares - because that is the one shape this adapter can honour, and a fixture
/// that presented anything else would be testing the refusal rather than the execution. The refusal
/// has its own test.
pub(crate) fn test_leg() -> Presented {
    match test_posture() {
        SourcePosture::SharedServiceUser { declared } => Presented::SharedServiceUser { declared },
        SourcePosture::ImpersonationAtSource => panic!("the fixture posture is shared, one function above"),
    }
}

/// A generous deadline for this crate's own tests, so a slow machine does not turn an unrelated
/// assertion into a spent-budget refusal.
pub(crate) fn test_deadline() -> Deadline {
    use sutura_domain::warehouse::deadline::Budget;
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(std::time::Duration::from_secs(30)).expect("30s"),
    )
}

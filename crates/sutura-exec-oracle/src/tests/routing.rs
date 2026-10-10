//! `Routing::deliverable`'s second question: whether the presented leg agrees with how the source
//! was declared.

use sutura_domain::source::SourcePosture;

use crate::{Channel, Dial, OracleError, Routing, TokenSessions};

/// **A shared leg at an impersonating source is refused by how the source was declared.**
/// `session_for` picks the boot connection for a shared leg at any source, so this check is what
/// keeps an impersonating source from answering one there.
#[test]
fn a_shared_leg_at_an_impersonating_source_is_refused_by_its_declared_posture() {
    let sessions =
        TokenSessions::new(Dial::new("127.0.0.1", 2484, "FREEPDB1", Channel::Plaintext)).expect("a dial is usable unopened");
    let routing = Routing {
        source: sutura_conformance::corpus::source(),
        posture: SourcePosture::ImpersonationAtSource,
        per_caller: Some(sessions),
    };
    let shared = sutura_conformance::corpus::presented();
    let refused = routing.deliverable(&shared);
    assert!(
        matches!(refused, Err(OracleError::PresentedDisagreesWithPosture { .. })),
        "{refused:?}"
    );
}

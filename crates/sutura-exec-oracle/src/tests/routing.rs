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

#[test]
fn oracle_capture_uses_the_native_binding_convention() {
    use sutura_domain::calendar::Date;
    use sutura_domain::warehouse::ParamValue;
    use sutura_domain::warehouse::execution::{Binding, BindingIdentity, BindingValue, Execution};
    use sutura_domain::warehouse::schema::TypeDialect;
    let date = Date::parse("2026-06-01").expect("a date");
    let params = [ParamValue::Text(String::from("north")), ParamValue::Date(date)];
    let native = crate::parameters::bind(&params);
    let [crate::parameters::OracleParam::Text(text), crate::parameters::OracleParam::Date(timestamp)] = native.as_slice() else { panic!("a native string then timestamp"); };
    assert_eq!(text, "north");
    assert_eq!(timestamp.to_string(), "2026-06-01T00:00:00.000000000");
    assert_eq!(Execution::bindings(&params, TypeDialect::Oracle), [
        Binding::new(BindingIdentity::Position(0), BindingValue::OracleString(String::from("north"))),
        Binding::new(BindingIdentity::Position(1), BindingValue::OracleDate(date)),
    ]);
}

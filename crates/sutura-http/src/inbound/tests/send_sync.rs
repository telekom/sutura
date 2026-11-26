//! The bound `axum` needs and reports badly when it is missing: a layer's state must be `Send +
//! Sync`, and the error for a state that is not is an unresolved trait bound on a tuple type. This
//! is the assertion that names it instead.

use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{KeySet, KeySetCache};
use crate::inbound::token::TokenValidator;

#[test]
fn nothing_in_this_module_is_sendable_by_accident() {
    fn assert_send_sync<T>()
    where
        T: Send + Sync + 'static,
    {
    }
    assert_send_sync::<InboundGate>();
    assert_send_sync::<KeySetCache>();
    assert_send_sync::<TokenValidator>();
    assert_send_sync::<KeySet>();
    assert_send_sync::<crate::inbound::VerifiedCaller>();
}

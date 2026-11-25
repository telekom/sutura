//! What `initialize` answers when a caller's view could not be narrowed: one code per thing a
//! client can act on, and never the whole bundle.

use rmcp::model::ErrorCode;
use sutura_app::surface::{ErasedCause, SurfaceFailure};

use super::{Narrowing, initialize_error};

#[test]
fn each_narrowing_failure_is_its_own_initialize_error() {
    let cause = || ErasedCause::from("a cause");
    for (narrowing, code, word) in [
        (
            Narrowing::Failed(SurfaceFailure::NotLinked { cause: cause() }),
            super::NOT_LINKED,
            "identity_not_linked",
        ),
        (
            Narrowing::Failed(SurfaceFailure::Broker { cause: cause() }),
            super::IDENTITY_UNAVAILABLE,
            "identity_unavailable",
        ),
        (
            Narrowing::Failed(SurfaceFailure::Warehouse { cause: cause() }),
            super::UNAVAILABLE,
            "unavailable",
        ),
        (
            Narrowing::Failed(SurfaceFailure::Miswired { cause: cause() }),
            ErrorCode::INTERNAL_ERROR.0,
            "internal",
        ),
    ] {
        let error = initialize_error(&narrowing);
        assert_eq!(error.code, ErrorCode(code), "{narrowing:?}");
        assert!(error.message.starts_with(word), "{narrowing:?}: {}", error.message);
    }
}

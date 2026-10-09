//! The delegation port's one newtype.

use super::{RequestedAudience, UnusableAudience};

#[test]
fn a_requested_audience_is_kept_exactly_and_refused_when_unprintable() {
    assert_eq!(
        RequestedAudience::parse("https://workforce-pool.example.com")
            .expect("a test audience parses")
            .as_str(),
        "https://workforce-pool.example.com"
    );
    assert_eq!(RequestedAudience::parse(""), Err(UnusableAudience::Empty));
    assert_eq!(
        RequestedAudience::parse(&"a".repeat(RequestedAudience::MOST + 1)),
        Err(UnusableAudience::TooLong {
            found: RequestedAudience::MOST + 1,
            most: RequestedAudience::MOST
        })
    );
    assert_eq!(
        RequestedAudience::parse(" padded"),
        Err(UnusableAudience::Unprintable { at: 0 })
    );
    assert_eq!(
        RequestedAudience::parse("aud\u{7f}"),
        Err(UnusableAudience::Unprintable { at: 3 })
    );
    assert_eq!(
        RequestedAudience::parse("pool\u{e9}"),
        Err(UnusableAudience::Unprintable { at: 4 })
    );
}

//! The opt-in metadata platforms, read off the registry. Their own module because `compose.rs` is
//! at its line budget.
#![cfg(test)]

use super::expected_services;

#[test]
fn the_openmetadata_platform_starts_only_when_it_is_asked_for() {
    let default_set = expected_services(&[]);
    assert!(
        !default_set.contains(&"openmetadata"),
        "the default set must not include the metadata platform: {default_set:?}"
    );

    let with_openmetadata = expected_services(&["openmetadata"]);
    assert!(with_openmetadata.contains(&"openmetadata"), "{with_openmetadata:?}");
    // The two metadata platforms are independent costs: asking for one must not start the other.
    assert!(!with_openmetadata.contains(&"datahub"), "{with_openmetadata:?}");
}

//! Tests for the generated interface description.

use crate::constants::{API_V1_PREFIX, HEALTH_PATH, base_paths};

use super::{document, document_json};

#[test]
fn the_document_is_byte_stable_across_independent_builds() {
    // THE determinism check, and the one that matters. Each call builds a fresh document, so
    // each call builds fresh maps - which is what makes this sensitive to a `HashMap`-backed
    // extension rather than only to a process-wide seed. If it ever fails, the module
    // documentation says what to build.
    let first = document_json().expect("the document serializes");
    let second = document_json().expect("the document serializes");
    assert_eq!(first, second);
}

#[test]
fn the_document_describes_every_governed_route() {
    // The two are generated from one attribute per handler, so this asserts the wiring rather
    // than the generator: a handler registered on the router and not merged into the document
    // is the mistake it catches.
    //
    // **Presence only, and that is deliberate here.** The exhaustive claim - the document
    // describes the governed operations and NOTHING else - is proved over the public API at
    // `crates/sutura-http/tests/operations.rs`, so a liveness route or an administrative one
    // cannot ride along; this cell is the cheap wiring half.
    let paths = document().paths;
    for route in crate::capability::governed() {
        assert!(
            paths.paths.contains_key(route.route()),
            "{} is missing from the document: {:?}",
            route.route(),
            paths.paths.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn liveness_is_not_described_in_the_interface_description() {
    // The probe is mounted on its own router and deliberately left out of the document: it is a
    // property of the process rather than an operation of the API, and a client that turns the
    // document's operations into tools (the local chat demo does) must not be handed a probe to
    // call. The exhaustive set proof is `tests/operations.rs`.
    let paths = document().paths;
    assert!(
        !paths.paths.contains_key(HEALTH_PATH),
        "liveness is in the interface description, so a tool client would list it"
    );
}

/// **This crate's half of `both_transports_describe_the_same_tools`, in the DOCUMENT.**
///
/// `crate::capability::tests` asserts the route table covers `sutura_app::Capability::every()`.
/// This asserts the generated interface description names the same capabilities by the same
/// identifiers - so a client reading the document and an agent reading `tools/list` see one tool
/// set under one set of names.
///
/// It matters because the two are written down twice: the table is Rust, the `operation_id` is a
/// literal inside a `#[utoipa::path]` attribute, and an attribute cannot take a `const`. This is
/// the test that catches the pair drifting.
///
/// **Every path, not only the versioned ones.** The prefix filter this used to carry is gone
/// with the liveness route: the document is now the governed surface in full, so a route that
/// does not start with the prefix is a route nothing governs and the placeholder it pushes is a
/// failure rather than something skipped.
#[test]
fn both_transports_describe_the_same_tools() {
    let document = document();
    let mut documented: Vec<String> = Vec::new();
    for (route, item) in &document.paths.paths {
        for operation in [item.get.as_ref(), item.post.as_ref()].into_iter().flatten() {
            documented.push(
                operation
                    .operation_id
                    .clone()
                    .unwrap_or_else(|| format!("{route} declares no operation_id")),
            );
        }
    }
    documented.sort();
    let mut expected: Vec<String> = sutura_app::Capability::every()
        .map(|capability| String::from(capability.id()))
        .collect();
    expected.sort();
    assert_eq!(documented, expected, "{documented:?}");
}

#[test]
fn the_document_tells_a_reader_that_identity_is_not_access() {
    // The one thing somebody integrating reads, and in the document because it is their surface.
    // The notice is deployment-independent: whichever way a caller is authenticated, no question
    // runs as the asker.
    let rendered = document_json().expect("the document serializes");
    assert!(
        rendered.contains("NEITHER IS PER-CALLER ACCESS"),
        "the description lost the notice"
    );
    assert!(rendered.contains("WHO IS ASKING"), "the description lost the identity notice");
    assert!(
        rendered.contains("refusal is a RESULT"),
        "the description lost the refusal notice"
    );
}

#[test]
fn every_refusal_status_is_declared_on_the_query_operation() {
    // Read the serialized document a client consumes, not the generator's typed tree.
    let rendered = document_json().expect("the document serializes");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("the document is JSON");
    let responses = &parsed["paths"][format!("{API_V1_PREFIX}{}", base_paths::QUERY)]["post"]["responses"];
    for status in ["200", "403", "404", "409", "413", "422", "503"] {
        let declared = &responses[status];
        assert!(!declared.is_null(), "{status} is not declared on POST /v1/query: {responses}");
        let description = declared["description"].as_str().unwrap_or_default();
        assert!(!description.is_empty(), "{status} is declared with no meaning given");
    }
    for status in ["403", "404", "409", "413", "422"] {
        assert!(
            responses[status]["description"]
                .as_str()
                .unwrap_or_default()
                .contains("outcome: refusal"),
            "{status} does not tell a reader it carries a refusal"
        );
    }
}

#[test]
fn no_security_scheme_is_declared() {
    // A shared deployment secret is not a caller authentication model.
    assert!(
        document()
            .components
            .and_then(|components| { (!components.security_schemes.is_empty()).then_some(components.security_schemes.len()) })
            .is_none(),
        "a security scheme was declared without a per-caller identity behind it"
    );
}

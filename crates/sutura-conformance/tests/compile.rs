//! The binding that exercises the compile packs over the two hand-built fixture catalogs.
//!
//! [`compile_packs!`](sutura_conformance::compile_packs) binds one `#[test]` per behaviour to a
//! catalog type, exactly the way [`execute_packs!`](sutura_conformance::execute_packs) binds one to
//! an adapter: the bodies live in `crates/sutura-conformance/src/compile.rs`, written once against
//! [`SemanticCatalog`](sutura_domain::pinned::SemanticCatalog), and this file contributes only the
//! two registrations - one golden subject, one declaring subject - and the one off-axis test that
//! shows the fidelity cell's assertion actually bites.
//!
//! The `compile` feature is what makes the pack exist at all (it needs the compiler and the
//! renderer), so this whole target is gated on it: with the feature off the file is empty, which is
//! the default every execute binding compiles under.

#![cfg(feature = "compile")]

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::pinned::SemanticCatalog as _;

use sutura_conformance::compile::DeclaringSubject;

// The two registrations. A `golden` binding gets all six behaviours; a `declaring` one gets the
// universal two (Fidelity, Repeat) and nothing golden, held there by the [`GoldenCatalog`]
// marker bound on the golden cells and by the `KIND` const assert in the macro. The catalog is a
// fully-qualified path, for the same reason [`execute_packs!`](sutura_conformance::execute_packs)'
// binding convention demands one: a name the caller imported at file scope does not resolve inside
// the generated `mod`.
sutura_conformance::compile_packs! {
    adapter: golden,
    catalog: sutura_conformance::compile::GoldenSubject,
    golden,
}

sutura_conformance::compile_packs! {
    adapter: declaring,
    catalog: sutura_conformance::compile::DeclaringSubject,
    declaring,
}

// ------------------------------------------------------------------- what the fidelity cell asserts ----
//
// The universal cells compare real things and are not vacuous: `repeat_load_is_stable` loads twice
// and requires one digest, and `fidelity_holds` runs
// [`MetadataCapabilities::checked_against`] in both directions, so a declaration wider than the
// bundle it produced must redden. Neither failure is exercised on the happy path, which is why this
// off-axis test provokes the widened direction directly - the same shape
// `crates/sutura-app/tests/golden/catalogs.rs` uses to show its declaring-fidelity test bites.

// Its own `#[cfg(test)]` module, because `clippy::tests_outside_test_module` asks for one and the
// strict lints exempt what is inside it - the same reason `bound.rs`'s halves have one.

/// A declaration wider than the declaring subject's bundle is refuted, naming the kind it over-claimed.
///
/// The declaring subject declares structure, metrics and grains and produces exactly that. Declaring
/// one further kind the bundle does not carry must fail the *unprovided* direction of
/// `checked_against` - so a future edit that widened the declaration past the bundle would turn
/// [`fidelity_holds`](sutura_conformance::compile::fidelity_holds) red instead of silently green.
#[cfg(test)]
mod off_axis {
    use super::*;

    #[test]
    fn a_declaration_wider_than_the_bundle_is_refuted() {
        let pinned = DeclaringSubject.load().expect("the declaring fixture cannot fail to load");
        let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());

        // The faithful declaration first, so the pair reads as one fact: this bundle is declared
        // exactly, and a widened copy of it is refused.
        assert_eq!(DeclaringSubject::capabilities().checked_against(&produced), Ok(()));

        let widened = MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
                DefinitionKind::Anchors,
            ]),
            KnowledgeCapabilities::none(),
        );
        assert!(
            widened.checked_against(&produced).is_err(),
            "a declaration wider than the bundle must be refuted"
        );
    }
}

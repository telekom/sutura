//! The falsification sweep for the four wiring-only refusals.
//!
//! `super::the_four_refusals_only_a_dialect_layer_defect_can_produce` asserts their WIRING and
//! cites a measured sweep for the claim that no fragment reaches `Qualify`, `Unrenderable`,
//! `Render` or `RenderedDoesNotParse`. This makes that sweep a check. It is a falsification guard
//! over the allowlist and six argument shapes, not a proof over every fragment a catalog can write.

use std::collections::BTreeMap;

use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};

use super::super::compile;
use super::super::refusal::ExpressionError;
use super::super::vocabulary::ALLOWED_FUNCTION_NAMES;
use super::{columns, table};

/// No allowlisted function, in six argument shapes over the model's own columns, reaches a
/// wiring-only refusal. A refusal for a real reason (`NotAggregated`, `Refused`, ...) is fine.
#[test]
fn no_allowlisted_fragment_reaches_a_wiring_only_refusal() {
    let shapes = [
        |name: &str| format!("{name}(mrr_eur)"),
        |name: &str| format!("{name}(DISTINCT mrr_eur)"),
        |name: &str| format!("{name}(mrr_eur) OVER (PARTITION BY region)"),
        |name: &str| format!("{name}(CASE WHEN status = 'active' THEN mrr_eur END)"),
        |name: &str| format!("{name}(mrr_eur ORDER BY region)"),
        |name: &str| format!("{name}(mrr_eur, customer_key)"),
    ];
    let mut rendered = 0_usize;
    for &(name, _) in ALLOWED_FUNCTION_NAMES {
        for shape in &shapes {
            let raw = shape(name);
            // A fragment that does not parse as SQL is out of scope: it fails before any of the
            // four paths.
            let Ok(fragment) = SqlFragment::parse(&raw) else { continue };
            let authored =
                AuthoredSql::new(BTreeMap::from([(DialectTag::portable(), fragment)])).expect("one fragment is authored sql");
            match compile(&authored, &table(), &columns()) {
                Ok(_) => rendered += 1,
                Err(
                    ExpressionError::Qualify { .. }
                    | ExpressionError::Unrenderable { .. }
                    | ExpressionError::Render { .. }
                    | ExpressionError::RenderedDoesNotParse { .. },
                ) => panic!("{raw:?} reached a wiring-only refusal, which only a dialect-layer defect produces"),
                Err(_) => {}
            }
        }
    }
    assert!(
        rendered > 0,
        "no fragment in the sweep compiled, so the sweep reached no render path"
    );
}

//! The prompt states the row ceilings this deployment configured, not the compiled ones -
//! `github.com/telekom/sutura#828`.

use sutura_domain::plan::{FederatedRowCeiling, MAX_ROWS, RowCeiling, RowCeilings};

use super::{CatalogProse, PromptInputs, ScopedView, Tool, bundle, render};

/// The prompt over `ceilings`, as one run of words: it is wrapped, so a phrase can straddle a line.
fn flowed(ceilings: RowCeilings) -> (String, String) {
    let text = render(
        &ScopedView::everything(&bundle()),
        &PromptInputs::new(Tool::ALL, CatalogProse::Quoted, None).row_ceilings(ceilings),
    );
    let flowed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (text, flowed)
}

fn configured() -> RowCeilings {
    RowCeilings::new(
        RowCeiling::parse(300).expect("a row ceiling"),
        FederatedRowCeiling::parse(500).expect("a federated row ceiling"),
    )
}

#[test]
fn the_top_bullet_states_the_configured_top_ceiling() {
    let (text, flowed) = flowed(configured());
    assert!(flowed.contains("`n` may not exceed 300 "), "{text}");
    assert!(!flowed.contains(&format!("`n` may not exceed {MAX_ROWS} ")), "{text}");
}

#[test]
fn a_two_source_sentence_states_the_configured_federated_ceiling() {
    let (text, flowed) = flowed(configured());
    assert!(
        flowed.contains("A question spanning two data systems is held to 500 rows instead."),
        "{text}"
    );
}

#[test]
fn the_defaults_state_the_compiled_ceiling_and_add_no_two_source_sentence() {
    let (text, flowed) = flowed(RowCeilings::DEFAULT);
    assert!(flowed.contains(&format!("`n` may not exceed {MAX_ROWS} ")), "{text}");
    assert!(!flowed.contains("two data systems is held to"), "{text}");
}

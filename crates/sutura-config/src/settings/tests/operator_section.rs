//! The `operator` section rides in a served deployment's tree, so one directory configures both
//! `sutura serve` and `sutura-operator`.

use crate::settings::{Environment, Settings, Sources};

#[test]
fn a_served_tree_carrying_an_operator_section_loads() {
    let sources = Sources::defaults(Environment::Development).with_overlay("operator:\n  api_group: workers.example.com\n");
    Settings::load(&sources).expect("a served tree with an operator section loads");
}

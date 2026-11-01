//! The raw tool's three texts against this build's `sql` feature: a build that plans the statement
//! says so, and one that passes it through says it is unparsed.

use sutura_app::Capability;
use sutura_app::prompt::Tool;

#[test]
fn the_raw_tool_texts_say_planned_only_in_a_build_that_plans() {
    for text in [
        super::UNTRUSTED_RAW_NOTICE,
        crate::tool::description(Capability::RunSql),
        Tool::RunSql.summary(),
    ] {
        assert_eq!(text.contains("planned"), cfg!(feature = "sql"), "{text}");
        assert_eq!(text.contains("unparsed"), !cfg!(feature = "sql"), "{text}");
    }
}

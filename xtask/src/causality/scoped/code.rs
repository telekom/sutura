//! Single-line code extraction for [`super::function_name`].

/// `line` with comments and string interiors blanked out.
pub(super) fn code_only(line: &str) -> String {
    crate::serde_parse::scan::code_lines_blanking_all_strings(line)
        .into_iter()
        .next()
        .unwrap_or_default()
}

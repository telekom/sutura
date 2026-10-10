//! The bounds section: the numbers a question is held to, quoted from where they are enforced.

use sutura_domain::plan::{MAX_ROWS, RowCeilings};
use sutura_domain::query::{MAX_DIMENSIONS, MAX_FILTERS, MAX_RANGE_DAYS};

use super::wrap;

/// The bounds a question is held to, with the numbers read from the domain rather than typed - and,
/// for the two row ceilings, from the deployment's own configuration.
pub(super) fn bounds(row_ceilings: RowCeilings) -> String {
    // `checked_div` rather than `/`, because the restriction category bans a bare integer division
    // and the divisor is a literal that cannot be zero: the fallback is unreachable and is written
    // as a fallback rather than as an `expect`, which is denied outside tests.
    let years = MAX_RANGE_DAYS.checked_div(365).unwrap_or(0);
    let top_rows = row_ceilings.top().get();
    // A two-source answer has its own ceiling, which is only worth a sentence where it is not the
    // single-source cap already stated, so a deployment that configures nothing reads as it always did.
    let two_sources = match row_ceilings.federated().get() {
        MAX_ROWS => String::new(),
        rows => format!(" A question spanning two data systems is held to {rows} rows instead."),
    };
    let bullets = [
        format!("- **At most {MAX_DIMENSIONS} dimensions** in one question."),
        format!("- **At most {MAX_FILTERS} filters** in one question."),
        format!(
            "- **A period of at most {MAX_RANGE_DAYS} days**, which is {years} years at its \
             longest. Both ends are required. A longer span is refused rather than trimmed to fit, \
             because an answer about a different period than the one asked about is a wrong number \
             nothing downstream can detect."
        ),
        format!(
            "- **At most {MAX_ROWS} rows in a result.** A wider result is REFUSED, not truncated: \
             the remedy is to narrow the question, and the refusal says so. Do not plan on paging \
             through a large result, because there is no paging and no cursor.{two_sources}"
        ),
        format!(
            "- **`top: {{ n, by, direction }}`** bounds a wide group-by instead of asking for every \
             group: `by` is `metric` or `period`, `direction` is `desc` or `asc`, and `n` may not \
             exceed {top_rows} - a larger `n` is the same refusal as an unbounded question this \
             wide. On a question spanning two data systems `top` ranks the combined answer; it is \
             refused when that combined set had already gone over the row ceiling."
        ),
    ];
    let mut lines = vec![
        String::from("## The bounds a question is held to\n"),
        String::from("These are not soft limits and there is no way to raise one from the caller's side.\n"),
    ];
    lines.extend(bullets.iter().map(|bullet| wrap("- ", bullet, "  ")));
    lines.push(String::new());
    lines.push(wrap(
        "",
        "Ask the question you want rather than a wide one you intend to filter afterwards. There is \
         no post-filtering step here, and a wide question is the one that gets declined.",
        "",
    ));
    lines.join("\n")
}

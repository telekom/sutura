//! The sentences for a job refused because of who would pay for it.
//!
//! `403` for both: who pays is this deployment's setting, so asking again unchanged is refused again.

use sutura_domain::model::SourceName;

pub(super) fn not_billed(source: &SourceName) -> String {
    format!(
        "the source `{source}` does not bill the payer named in `Sutura-Bill-To`, so nothing ran; ask again \
         without it"
    )
}

pub(super) fn undecided(source: &SourceName) -> String {
    format!(
        "the source `{source}` could not decide who pays for this question: none of your groups is mapped \
         to a billing project, it reads tables in more than one project, and no payer was named, so \
         nothing ran"
    )
}

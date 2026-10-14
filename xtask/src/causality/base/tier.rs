//! Whether a failing test ended at its OWN tier lookup: the base run keeps the head run's
//! requirement, so a cell whose tier is absent there panics in `dev/src/provisioned.rs`.

/// Did one of `failed` end at its OWN tier lookup rather than at an assertion? Only that test's own
/// panic line, `thread '<its name>' panicked at dev/src/provisioned.rs:`, is a site, and only with
/// the requirement in the message under it: a copy quoted, or under another test's name, is not.
///
/// `claim`'s kill run forces the requirement too, and this one predicate answers both arms. The site
/// is a path literal, so moving that panic out of `dev/src/provisioned.rs` makes a tier failure read
/// as an ordinary one again - loud in both arms.
pub(in crate::causality) fn never_reached_its_tier(text: &str, failed: &[String]) -> bool {
    let lines: Vec<&str> = text.lines().map(str::trim_start).collect();
    failed.iter().filter_map(|one| one.rsplit(' ').next()).any(|test| {
        let own = format!("thread '{test}'");
        lines.iter().enumerate().any(|(at, line)| {
            line.strip_prefix(own.as_str())
                .and_then(|rest| rest.split_once(" panicked at "))
                .is_some_and(|(id, site)| (id.is_empty() || id.starts_with(" (")) && site.starts_with("dev/src/provisioned.rs:"))
                && lines
                    .iter()
                    .skip(at + 1)
                    .take_while(|next| !next.is_empty())
                    .any(|next| next.contains(sutura_dev::requirement::REQUIRED))
        })
    })
}

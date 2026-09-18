//! Does every crate a fuzz target actually names reach both hook surfaces?
//!
//! `github.com/telekom/sutura#867`. `check-fuzz` never read either hook file, so a target whose
//! `sutura_*` imports outgrew the `fuzz` pre-commit hook's `files:` regex or the "fuzzed tree" row
//! in [`crate::hook_coverage`] stayed invisible to every other gate: the target still builds,
//! still runs under `just fuzz-smoke`, and CI's own matrix still names it. Only the git-delta hook
//! and the coverage surface silently stopped reaching it.
//!
//! A target's crates are read straight from its source rather than from a declared list, for the
//! reason `#867`'s own scope gives: the mapping is already mechanical, and a second, hand-kept
//! table would be exactly the kind of copy that rotted here in the first place.

use std::collections::BTreeSet;

/// The `sutura_<name>` crates a fuzz target's source imports, as their crate DIRECTORY names
/// (`_` folded to `-`).
///
/// Text scan rather than a parse - `xtask` carries no syntax tree for its own reasons (see
/// `fuzz.rs`'s header) - so this reads every maximal run of `sutura_` followed by lowercase ASCII
/// letters or underscores, wherever it appears. Deliberately over-inclusive rather than
/// under-inclusive: a mention in a doc comment asks for more coverage, never less, and every
/// target in this tree today names only crates it truly imports (checked by hand once, at
/// `github.com/telekom/sutura#867`, over all six).
pub(super) fn target_crates(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find("sutura_") {
        let tail = rest.get(at.saturating_add("sutura_".len())..).unwrap_or("");
        let word_end = tail
            .find(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .unwrap_or(tail.len());
        let name = format!("sutura_{}", tail.get(..word_end).unwrap_or(""));
        found.insert(name.replace('_', "-"));
        rest = tail.get(word_end..).unwrap_or("");
    }
    found
}

/// Is `crate_dir` (e.g. `sutura-exec-bigquery`) absent from a `files:` pattern's raw text?
///
/// A raw substring, not a regex match: it holds "the crate's directory is mentioned somewhere in
/// the pattern", not "the pattern's regex actually reaches every file under it". A planted
/// `crates/<crate_dir>/nothing-real.rs` reference would read as present.
pub(super) fn missing_from_hook(crate_dir: &str, files: &str) -> bool {
    !files.contains(&format!("crates/{crate_dir}/"))
}

/// Is `crate_dir` absent from every glob a surface's `paths` declares?
pub(super) fn missing_from_surface(crate_dir: &str, paths: &[&str]) -> bool {
    let prefix = format!("crates/{crate_dir}/");
    !paths.iter().any(|path| path.starts_with(&prefix))
}

#[cfg(test)]
mod tests;

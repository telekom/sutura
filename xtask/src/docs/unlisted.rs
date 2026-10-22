//! `not_in_nav`: pages generated into the docs directory, which no nav entry can name.
//!
//! The API reference is written into `docs/api/` by the publish run and by `just api`, and is
//! committed nowhere. `mkdocs build --strict` fails on a page in no nav entry unless `not_in_nav`
//! lists it - measured, with `validation.omitted_files: warn` - so `mkdocs.yml` lists the
//! directory, and this gate must not call the same pages orphans on a machine that has run `just api`.
//!
//! Only the `/dir/` form is read, the one [`super::exclude`] resolves; any other line is refused,
//! because a pattern this cannot resolve is a page it would silently stop checking. **Limit:** a
//! directory listed here is exempt from the orphan rule ONLY. Its pages are still scanned for links
//! and still matched against `exclude_docs`, which keeps a basename exclusion from silently
//! dropping `api/index.md` as well.

use std::collections::BTreeSet;

use super::exclude::directory;
use super::{CONFIG, block, strip_comment};

/// Every directory `not_in_nav` names, docs-relative with a trailing `/`.
pub(super) fn directories(config: &str) -> Result<Vec<String>, String> {
    block(config, "not_in_nav")
        .iter()
        .map(|line| strip_comment(line).trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            directory(line).map(String::from).ok_or_else(|| {
                format!("{CONFIG} lists `{line}` under `not_in_nav`, and this gate reads only a whole directory as `/dir/`")
            })
        })
        .collect()
}

/// The pages under a listed directory: the orphan rule's blind spot, and nothing else's.
pub(super) fn pages_within(present: &BTreeSet<String>, directories: &[String]) -> BTreeSet<String> {
    present
        .iter()
        .filter(|page| directories.iter().any(|dir| page.starts_with(dir.as_str())))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{directories, pages_within};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_listed_directory_is_read_without_its_slashes_being_guessed() {
        let config = "site_name: x\nnot_in_nav: |\n  /api/\n  # a comment\nnav:\n  - index.md\n";
        assert_eq!(directories(config), Ok(vec![String::from("api/")]));
        assert_eq!(directories("site_name: x\n"), Ok(Vec::new()));
    }

    #[test]
    fn a_line_that_is_not_a_whole_directory_is_refused_rather_than_ignored() {
        for line in ["api/", "/api", "/api/*.md", "index.md", "!/api/"] {
            let config = format!("not_in_nav: |\n  {line}\n");
            let refusal = directories(&config).expect_err(line);
            assert!(refusal.contains(line), "{refusal}");
        }
    }

    #[test]
    fn only_pages_under_a_listed_directory_are_exempt_from_the_orphan_rule() {
        let present = set(&[
            "index.md",
            "api/index.md",
            "api/sutura-domain.md",
            "apiary.md",
            "notes/api/x.md",
        ]);
        let listed = pages_within(&present, &[String::from("api/")]);
        assert_eq!(listed, set(&["api/index.md", "api/sutura-domain.md"]));
    }

    #[test]
    fn a_listed_page_is_no_orphan_and_an_unlisted_one_still_is() {
        let nav = set(&["index.md"]);
        let present = set(&["index.md", "api/sutura-domain.md", "orphan.md"]);
        let listed = pages_within(&present, &[String::from("api/")]);
        let found = crate::docs::problems(&nav, &present, &listed, "docs");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("orphan.md"), "{found:?}");
    }
}

//! A link into the published site names a version.
//!
//! mike publishes `latest/`, `main/` and one `X.Y.Z/` directory per release, and the site root is
//! only a redirect. So `https://telekom.github.io/sutura/examples/` answers 404 while
//! `https://telekom.github.io/sutura/latest/examples/` answers the page. The bare root stays allowed.
//!
//! **Limits.** Reads every tracked `.md` file as text, code blocks included, and matches the
//! literal host and path prefix. A link in another file type, or one spelled another way (an
//! encoded slash, another host that redirects here), is not read.

use crate::repo;

const SITE: &str = "telekom.github.io/sutura/";

/// Whether the text after [`SITE`] starts with a version segment, or ends the link at the root.
pub(super) fn versioned(rest: &str) -> bool {
    let segment: &str = rest
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
        .next()
        .unwrap_or_default();
    segment.is_empty()
        || matches!(segment, "latest" | "main")
        || (segment.split('.').count() == 3
            && segment
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
}

/// One problem per unversioned link, or the census's refusal.
pub(super) fn unversioned() -> Vec<String> {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => return vec![format!("site links: {}", why.describe())],
    };
    let mut found = Vec::new();
    let inspected = census.inspect(&["docs/index.md"], super::is_markdown, |rel, bytes| {
        for (number, line) in String::from_utf8_lossy(bytes).lines().enumerate() {
            for (at, _) in line.match_indices(SITE) {
                let rest = line.get(at.saturating_add(SITE.len())..).unwrap_or_default();
                if !versioned(rest) {
                    found.push(format!(
                        "{rel}:{} links {SITE}{} without a version - write latest/, main/ or X.Y.Z/ after {SITE}, because the root only redirects",
                        number.saturating_add(1),
                        rest.split_whitespace().next().unwrap_or_default(),
                    ));
                }
            }
        }
    });
    if let Err(why) = inspected {
        found.push(format!("site links: {}", why.describe()));
    }
    found
}

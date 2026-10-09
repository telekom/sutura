//! The icon half of the docs gate: an SVG under `overrides/.icons` must carry nothing that runs or
//! fetches once it is part of a page.
//!
//! **Why this is a gate.** `pymdownx.emoji`'s `to_svg` pastes the file into the page's HTML, so an
//! icon is not an image: a `<script>` in it runs with the origin of the docs, and an `href` or a
//! `url(..)` in it makes the reader's browser fetch what the file names. An icon is copied from
//! another project, so nobody reads it as code.
//!
//! **What is refused**, each by the text of the file and not by a parse, so a malformed file is
//! judged by what a browser could still make of it:
//!
//! | Form | Why |
//! | --- | --- |
//! | `<script` | runs in the page |
//! | an `on*=` attribute | runs in the page |
//! | `<foreignObject` | embeds HTML, so everything above again |
//! | `<set` or `<animate*` | can assign an `href` or an event handler at run time |
//! | `href` or `xlink:href` to anything but a `#fragment` | fetches or navigates |
//! | `url(..)` or `@import` to anything but a `#fragment` | fetches |
//!
//! **The limits, next to the claim.** NOT covered: a `<style>` rule, which is global once inlined, so
//! an icon whose class names collide with another icon restyles it; an icon that is huge, which
//! only costs a reader bandwidth; and a file that is not `.svg`, which `to_svg` does not read.
//! Case is ignored. A file that cannot be read as text, a symlink and a configured directory that is
//! missing or holds no icon are findings, never a smaller total.

use std::path::Path;

/// Where the icons live, as `mkdocs.yml` names it under `custom_icons`.
const DIR: &str = "overrides/.icons";

/// What [`sweep`] read and what it refused.
pub(super) struct Swept {
    pub(super) scanned: usize,
    pub(super) problems: Vec<String>,
}

/// The forms `svg` carries that an inlined icon must not, by name.
fn refusals(svg: &str) -> Vec<&'static str> {
    let lower = svg.to_ascii_lowercase();
    let mut out = Vec::new();
    if has_element(&lower, "script") {
        out.push("a `<script` element");
    }
    if has_element(&lower, "foreignobject") {
        out.push("a `<foreignObject` element");
    }
    if has_element(&lower, "set") || lower.contains("<animate") {
        out.push("a `<set` or `<animate` element");
    }
    if event_attribute(&lower) {
        out.push("an `on*=` event attribute");
    }
    if leaves_the_document(&lower, "href", |rest| rest.trim_start().strip_prefix('=')) {
        out.push("an `href` to anything but a `#fragment`");
    }
    if leaves_the_document(&lower, "url(", Some) || lower.contains("@import") {
        out.push("a `url(..)` or `@import` to anything but a `#fragment`");
    }
    out
}

/// `<name` followed by something that ends a tag name, so `<set` is not `<settings`.
fn has_element(lower: &str, name: &str) -> bool {
    lower.match_indices(&format!("<{name}")).any(|(at, matched)| {
        lower
            .get(at.saturating_add(matched.len())..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(|next| next.is_ascii_whitespace() || matches!(next, '/' | '>'))
    })
}

/// An attribute named `on` and letters, then `=`, after a character that can end the one before.
fn event_attribute(lower: &str) -> bool {
    lower.match_indices("on").any(|(at, _)| {
        let after_a_boundary = lower
            .get(..at)
            .and_then(|head| head.chars().next_back())
            .is_some_and(|c| c.is_ascii_whitespace() || matches!(c, '/' | '"' | '\''));
        let rest = lower.get(at.saturating_add(2)..).unwrap_or_default();
        let letters = rest.chars().take_while(char::is_ascii_lowercase).count();
        after_a_boundary && letters > 0 && rest.get(letters..).is_some_and(|tail| tail.trim_start().starts_with('='))
    })
}

/// Whether any `marker` is followed, through `value`, by something other than a `#fragment`.
///
/// `value` strips what stands between the marker and its value (`= ` for an attribute, nothing for
/// `url(`) and answers `None` when the marker is not used as one - the word `href` in a comment.
fn leaves_the_document<'a>(lower: &'a str, marker: &str, value: impl Fn(&'a str) -> Option<&'a str>) -> bool {
    lower.match_indices(marker).any(|(at, matched)| {
        lower
            .get(at.saturating_add(matched.len())..)
            .and_then(&value)
            .is_some_and(|rest| {
                !rest
                    .trim_start()
                    .trim_start_matches(['"', '\''])
                    .trim_start()
                    .starts_with('#')
            })
    })
}

/// Every `.svg` under [`DIR`], judged. `config` is `mkdocs.yml`: a directory it names that is
/// missing or empty is a finding, because a rename would otherwise leave the rule reading nothing.
pub(super) fn sweep(root: &Path, config: &str) -> Swept {
    let mut swept = Swept {
        scanned: 0,
        problems: Vec::new(),
    };
    let dir = root.join(DIR);
    let named = config.contains(DIR);
    if !dir.is_dir() {
        if named {
            swept
                .problems
                .push(format!("`mkdocs.yml` names `{DIR}` and that directory does not exist"));
        }
        return swept;
    }
    visit(&dir, &mut swept);
    if named && swept.scanned == 0 && swept.problems.is_empty() {
        swept
            .problems
            .push(format!("`{DIR}` holds no `.svg`, so the icon rule read nothing"));
    }
    swept
}

fn visit(dir: &Path, swept: &mut Swept) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            swept.problems.push(format!("could not list `{}`: {error}", dir.display()));
            return;
        }
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            swept
                .problems
                .push(format!("could not read the type of `{}`", path.display()));
            continue;
        };
        if kind.is_symlink() {
            swept
                .problems
                .push(format!("`{}` is a symlink: an icon is a file in this tree", path.display()));
        } else if kind.is_dir() {
            visit(&path, swept);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("svg")) {
            match std::fs::read_to_string(&path) {
                Ok(svg) => {
                    swept.scanned = swept.scanned.saturating_add(1);
                    for form in refusals(&svg) {
                        swept.problems.push(format!(
                            "`{}` carries {form}, which runs or fetches once the icon is inlined in a page",
                            path.display()
                        ));
                    }
                }
                Err(error) => swept
                    .problems
                    .push(format!("`{}` is not readable as text: {error}", path.display())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DIR, refusals, sweep};

    const CLEAN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><defs><linearGradient id="a"/></defs><use href="#a" xlink:href="#a"/><path fill="url(#a)" d="M0 0"/><settings/></svg>"##;

    #[test]
    fn each_refused_form_is_named_and_a_clean_icon_is_not() {
        let forms: [(&str, &str); 11] = [
            ("<script", "<svg><script>alert(1)</script></svg>"),
            ("<SCRIPT", r#"<svg><SCRIPT SRC="x"></SCRIPT></svg>"#),
            ("on*=", r#"<svg onload="alert(1)"><path d="M0 0"/></svg>"#),
            ("on*= no space", "<svg/onload=alert(1)>"),
            ("<foreignObject", "<svg><foreignObject><div/></foreignObject></svg>"),
            ("href https", r#"<svg><use href="https://example.com/x.svg#a"/></svg>"#),
            (
                "xlink:href js",
                "<svg><a xlink:href = 'javascript:alert(1)'><path/></a></svg>",
            ),
            ("href data", r#"<svg><image href="data:image/png;base64,AA"/></svg>"#),
            ("url()", r#"<svg><path style="fill:url(https://example.com/x)"/></svg>"#),
            ("@import", r#"<svg><style>@import "https://example.com/x.css";</style></svg>"#),
            (
                "<set",
                r#"<svg><a><set attributeName="href" to="javascript:alert(1)"/></a></svg>"#,
            ),
        ];
        for (name, svg) in forms {
            assert!(!refusals(svg).is_empty(), "{name} must be refused: {svg}");
        }
        assert_eq!(
            refusals(CLEAN),
            Vec::<&str>::new(),
            "a fragment reference is an icon's own business"
        );
    }

    #[test]
    fn a_planted_bad_icon_is_a_finding_and_a_missing_or_empty_directory_is_too() {
        let root = std::env::temp_dir().join(format!("sutura-icons-{}", std::process::id()));
        let dir = root.join(DIR).join("brand");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok.svg"), CLEAN).unwrap();
        let clean = sweep(&root, DIR);
        assert_eq!((clean.scanned, clean.problems.len()), (1, 0), "{:?}", clean.problems);
        std::fs::write(dir.join("bad.svg"), r#"<svg onload="alert(1)"/>"#).unwrap();
        let planted = sweep(&root, DIR);
        assert_eq!(planted.scanned, 2);
        assert!(
            planted.problems.iter().any(|p| p.contains("bad.svg") && p.contains("on*=")),
            "{:?}",
            planted.problems
        );
        std::fs::write(dir.join("bin.svg"), [0xFF, 0xFE]).unwrap();
        assert!(sweep(&root, DIR).problems.iter().any(|p| p.contains("not readable")));
        std::fs::remove_dir_all(root.join(DIR)).unwrap();
        assert_eq!(
            sweep(&root, DIR).problems.len(),
            1,
            "a named directory that is gone is a finding"
        );
        assert!(
            sweep(&root, "theme: material").problems.is_empty(),
            "an unnamed absent directory is not"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

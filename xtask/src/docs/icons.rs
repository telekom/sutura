//! The icon half of the docs gate: an SVG under `overrides/.icons` may hold only plain SVG drawing
//! elements, and none of the forms below, once it is part of a page.
//!
//! **Why this is a gate.** `pymdownx.emoji`'s `to_svg` pastes the file into the page's HTML, so an
//! icon is not an image: what it holds is HTML in the page. An icon is copied from another project,
//! so nobody reads it as code.
//!
//! **What is refused**, each by the text of the file and not by a parse, so a malformed file is
//! judged by what a browser could still make of it:
//!
//! | Form | Why |
//! | --- | --- |
//! | an element outside [`ELEMENTS`] | script, embedded HTML, frames, forms and animation are not drawing |
//! | an `on*=`, `src`, `srcdoc`, `data`, `action` or `formaction` attribute | runs or fetches |
//! | `href` or `xlink:href` to anything but a `#fragment` | fetches or navigates |
//! | `url(..)`, `image-set(` or `@import` to anything but a `#fragment` | fetches |
//! | a `\` or an `&` anywhere | an escape (`u\72l(`, `@\69mport`, `u&#114;l(`) hides a form above from a text match |
//!
//! **The limits, next to the claim.** The check names forms and does not prove an icon inert: a form
//! it does not name passes. NOT covered: a `<style>` rule, which is global once inlined, so
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

/// Plain SVG drawing, the `style` rule and the `metadata` an editor leaves. An element with a `:` in
/// its name (`rdf:RDF`, `sodipodi:namedview`) is no HTML element, so it is inert metadata. Everything
/// else is refused by being absent: `script`, `foreignobject`, `set`, `animate`, `iframe`, `img`, `meta`
/// and the rest of HTML. Names are lower case, as [`refusals`] reads them.
const ELEMENTS: &[&str] = &[
    "svg",
    "g",
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "defs",
    "lineargradient",
    "radialgradient",
    "stop",
    "clippath",
    "mask",
    "title",
    "desc",
    "symbol",
    "use",
    "style",
    "metadata",
];

/// The forms `svg` carries that an inlined icon must not, by name.
fn refusals(svg: &str) -> Vec<String> {
    let lower = svg.to_ascii_lowercase();
    let mut out: Vec<String> = elements(&lower)
        .filter(|name| !ELEMENTS.contains(name) && !name.contains(':'))
        .map(|name| format!("a `<{name}` element (not plain SVG drawing)"))
        .collect();
    let runs_or_fetches = |name: &str| {
        name.strip_prefix("on").is_some_and(|rest| !rest.is_empty())
            || matches!(name, "src" | "srcdoc" | "data" | "action" | "formaction")
    };
    if attribute(&lower, runs_or_fetches) {
        out.push("an `on*=`, `src`, `srcdoc`, `data`, `action` or `formaction` attribute".to_owned());
    }
    if leaves_the_document(&lower, "href", |rest| rest.trim_start().strip_prefix('=')) {
        out.push("an `href` to anything but a `#fragment`".to_owned());
    }
    if leaves_the_document(&lower, "url(", Some) || lower.contains("@import") || lower.contains("image-set(") {
        out.push("a `url(..)`, `image-set(` or `@import` to anything but a `#fragment`".to_owned());
    }
    if lower.contains(['\\', '&']) {
        out.push("an escape sequence (a `\\` or an `&`)".to_owned());
    }
    out
}

/// The name after each `<` that opens an element: not a closing tag, a comment, a declaration or a
/// processing instruction.
fn elements(lower: &str) -> impl Iterator<Item = &str> {
    lower
        .split('<')
        .skip(1)
        .filter(|piece| piece.starts_with(|c: char| c.is_ascii_alphabetic()))
        .map(|piece| {
            piece
                .split(|c: char| c.is_ascii_whitespace() || matches!(c, '/' | '>'))
                .next()
                .unwrap_or_default()
        })
}

/// Whether `refused` names an attribute that appears: a name after whitespace, `/` or a quote, then `=`.
fn attribute(lower: &str, refused: impl Fn(&str) -> bool) -> bool {
    lower
        .match_indices(|c: char| c.is_ascii_whitespace() || matches!(c, '/' | '"' | '\''))
        .any(|(at, boundary)| {
            let rest = lower.get(at.saturating_add(boundary.len())..).unwrap_or_default();
            let name = rest
                .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')))
                .next()
                .unwrap_or_default();
            !name.is_empty() && rest.get(name.len()..).is_some_and(|tail| tail.trim_start().starts_with('=')) && refused(name)
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
    use std::path::Path;

    use super::{DIR, refusals, sweep};

    const CLEAN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><defs><linearGradient id="a"/></defs><use href="#a" xlink:href="#a"/><path fill="url(#a)" d="M0 0"/><rdf:RDF/></svg>"##;

    #[test]
    fn each_refused_form_is_named_and_a_clean_icon_is_not() {
        let forms: [(&str, &str); 30] = [
            ("<script", "<svg><script>alert(1)</script></svg>"),
            ("<SCRIPT", r#"<svg><SCRIPT SRC="x"></SCRIPT></svg>"#),
            ("on*=", r#"<svg onload="alert(1)"><path d="M0 0"/></svg>"#),
            ("on*= no space", "<svg/onload=alert(1)>"),
            ("<foreignObject", "<svg><foreignObject/></svg>"),
            ("href https", r#"<svg><use href="https://example.com/x.svg#a"/></svg>"#),
            (
                "xlink:href js",
                "<svg><a xlink:href = 'javascript:alert(1)'><path/></a></svg>",
            ),
            ("href data", r#"<svg><image href="data:image/png;base64,AA"/></svg>"#),
            ("url()", r#"<svg><path style="fill:url(https://example.com/x)"/></svg>"#),
            ("@import", r#"<svg><style>@import "https://example.com/x.css";</style></svg>"#),
            ("<set", r#"<svg><set attributeName="href" to="javascript:alert(1)"/></svg>"#),
            (
                "<animate",
                r#"<svg><animate attributeName="href" values="javascript:alert(1)"/></svg>"#,
            ),
            ("iframe javascript", r#"<svg><iframe src="javascript:alert(1)"/></svg>"#),
            ("iframe", "<svg><iframe/></svg>"),
            ("img", "<svg><img/></svg>"),
            ("meta", "<svg><meta/></svg>"),
            ("embed", "<svg><embed/></svg>"),
            ("object", "<svg><object/></svg>"),
            ("form", "<svg><form/></svg>"),
            ("src on a drawing element", r#"<svg><path src="x"/></svg>"#),
            ("srcdoc", r#"<svg><path srcdoc="x"/></svg>"#),
            ("data", r#"<svg><path data="x"/></svg>"#),
            ("action", r#"<svg><path action="x"/></svg>"#),
            ("image-set", r#"<svg><path style="fill:image-set('x' 1x)"/></svg>"#),
            ("formaction", r#"<svg><path formaction="x"/></svg>"#),
            (
                "u\\72l( escaped",
                r#"<svg><path style="fill:u\72l(https://example.com/x)"/></svg>"#,
            ),
            (
                "@\\69mport escaped",
                r#"<svg><style>@\69mport "https://example.com/x.css";</style></svg>"#,
            ),
            (
                "character reference",
                r#"<svg><path style="fill:u&#114;l(https://example.com/x)"/></svg>"#,
            ),
            ("on*= after a quote", r#"<svg a="b"onload="alert(1)"/>"#),
            ("on*= after a single quote", r#"<svg a='b'onload="alert(1)"/>"#),
        ];
        for (name, svg) in forms {
            assert!(!refusals(svg).is_empty(), "{name} must be refused: {svg}");
        }
        assert_eq!(
            refusals(CLEAN),
            Vec::<String>::new(),
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
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("ok.svg"), dir.join("link.svg")).unwrap();
            assert!(
                sweep(&root, DIR)
                    .problems
                    .iter()
                    .any(|p| p.contains("link.svg") && p.contains("is a symlink"))
            );
            std::fs::remove_file(dir.join("link.svg")).unwrap();
        }
        std::fs::remove_file(dir.join("ok.svg")).unwrap();
        std::fs::remove_file(dir.join("bad.svg")).unwrap();
        std::fs::remove_file(dir.join("bin.svg")).unwrap();
        let empty = sweep(&root, DIR);
        assert!(
            empty.problems.iter().any(|p| p.contains("holds no")),
            "an empty named directory is a finding: {:?}",
            empty.problems
        );
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

    #[test]
    fn every_shipped_logo_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let swept = sweep(root, DIR);
        assert!(swept.scanned >= 5 && swept.problems.is_empty(), "{:?}", swept.problems);
    }
}

//! The embed half of the docs gate: an `<iframe src>`, an `<img src>` or a `![alt](src)` must
//! reach a file in the docs directory.
//!
//! **Why this is a gate.** mkdocs copies the files it finds and says nothing about a `src` that
//! names none: the page builds, `--strict` passes, and the reader sees an empty frame or a broken
//! image. A moved asset or a typo is invisible until somebody opens the page.
//!
//! **Two bases, and the difference is the reason a plain path join is wrong.** mkdocs rewrites the
//! destination of a markdown link or image, so `![](a.png)` is relative to the SOURCE file. It does
//! not touch raw HTML. A raw `src` is relative to the URL the page is served at, and with the
//! default directory URLs `docs/architecture.md` is served at `architecture/`, so the file
//! `docs/assets/x.html` is `../assets/x.html` from it. [`reached`] applies the right base to each.
//!
//! **The limits, next to the claim.** A `src` is judged against the files under the docs
//! directory, which is what mkdocs copies. NOT covered:
//!
//! | Uncovered | What happens |
//! | --- | --- |
//! | A remote URL, `https://..` or `//..` | skipped: whether it loads is not a file this tree owns |
//! | A `data:` URI, a `#` fragment alone | skipped |
//! | A `srcset`, `<source>`, `<video>`, `<script src>` or a CSS `url(..)` | not read |
//! | A file under `exclude_docs`, or under a dot-prefixed directory | found on disk, absent from the site |
//! | A `src` that names a `.md` page | found on disk, and mkdocs publishes it as HTML |
//! | `use_directory_urls: false` | raw HTML is then relative to the source file: this assumes the default, which `mkdocs.yml` does not change |
//! | A `src` built by a template or a plugin | not read |
//! | A tag whose `>` sits inside a quoted attribute | the tag is cut short at that `>` |
//!
//! A `src` that starts with `/` is refused outright: it is relative to the host, and the site is
//! served under a version prefix, so it reaches a different file than the one on disk.

use crate::markdown;

/// One `src` and which base it is relative to.
pub(super) struct Embed {
    /// The value as written, without a title or a fragment.
    pub(super) src: String,
    /// `true` for raw HTML, relative to the served URL. `false` for the markdown image form,
    /// which mkdocs resolves against the source file.
    pub(super) html: bool,
}

/// Where an embed points.
pub(super) enum Reach {
    /// A path in the docs directory, with `/` separators.
    Docs(String),
    /// Not a file this tree owns: a remote URL, a `data:` URI or a fragment.
    Elsewhere,
    /// A shape this cannot resolve to a docs path. Carries why.
    Unresolvable(&'static str),
}

/// The `src` of every `<iframe>` and `<img>` tag in `text`, and nothing else.
fn html_sources(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in text.split('<').skip(1) {
        let cut = piece.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(piece.len());
        let name = piece.get(..cut).unwrap_or_default();
        if !(name.eq_ignore_ascii_case("iframe") || name.eq_ignore_ascii_case("img")) {
            continue;
        }
        let tag = piece.get(cut..).unwrap_or_default().split('>').next().unwrap_or_default();
        out.extend(src_attribute(tag).map(String::from));
    }
    out
}

/// The value of the `src` attribute in one tag's attribute text, quoted or bare.
///
/// `data-src` is not `src`: the attribute name has to start after whitespace.
fn src_attribute(tag: &str) -> Option<&str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0_usize;
    while let Some(found) = lower.get(from..)?.find("src") {
        let at = from.saturating_add(found);
        from = at.saturating_add(3);
        let named = tag.get(..at)?.ends_with(|c: char| c.is_ascii_whitespace());
        let value = tag.get(from..)?.trim_start().strip_prefix('=');
        let (true, Some(value)) = (named, value) else {
            continue;
        };
        let value = value.trim_start();
        return match value.chars().next()? {
            quote @ ('"' | '\'') => value.get(1..)?.split(quote).next(),
            _ => value.split_whitespace().next().map(|bare| bare.trim_end_matches('/')),
        };
    }
    None
}

/// The destination of a markdown image, without a title or an angle bracket.
fn image_source(destination: &str) -> &str {
    destination
        .strip_prefix('<')
        .and_then(|rest| rest.split_once('>'))
        .map_or_else(
            || destination.split_whitespace().next().unwrap_or_default(),
            |(inside, _)| inside,
        )
}

/// Every embed in `lines`, which [`markdown::prose`] has already cleared of code and comments.
pub(super) fn of(lines: &[String]) -> Vec<Embed> {
    let mut out: Vec<Embed> = html_sources(&lines.join("\n"))
        .into_iter()
        .map(|src| Embed { src, html: true })
        .collect();
    out.extend(
        markdown::destinations(lines)
            .into_iter()
            .filter(|destination| destination.image)
            .map(|destination| Embed {
                src: String::from(image_source(destination.text)),
                html: false,
            }),
    );
    out
}

/// The docs-relative file `embed` reaches from the page `page`, which is docs-relative too.
pub(super) fn reached(page: &str, embed: &Embed) -> Reach {
    let src = embed.src.trim();
    if src.is_empty() || src.starts_with('#') || src.starts_with("//") || src.contains("://") || src.starts_with("data:") {
        return Reach::Elsewhere;
    }
    if src.starts_with('/') {
        return Reach::Unresolvable("it starts with `/`, so it is relative to the host and not to the docs");
    }
    let path = src.split(['#', '?']).next().unwrap_or_default();
    let (parent, name) = page.rsplit_once('/').unwrap_or(("", page));
    let mut segments: Vec<&str> = parent.split('/').filter(|segment| !segment.is_empty()).collect();
    // A page is served from a directory named for it, except an index, which is served from its own.
    let stem = name.strip_suffix(".md").unwrap_or(name);
    if embed.html && stem != "index" {
        segments.push(stem);
    }
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Reach::Unresolvable("it climbs out of the docs directory");
                }
            }
            other => segments.push(other),
        }
    }
    Reach::Docs(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{Embed, Reach, html_sources, image_source, of, reached};

    fn lines(text: &str) -> Vec<String> {
        crate::markdown::prose(text).unwrap_or_else(|e| panic!("{e}"))
    }

    fn docs(page: &str, src: &str, html: bool) -> String {
        match reached(
            page,
            &Embed {
                src: String::from(src),
                html,
            },
        ) {
            Reach::Docs(path) => path,
            Reach::Elsewhere => String::from("<elsewhere>"),
            Reach::Unresolvable(why) => format!("<{why}>"),
        }
    }

    #[test]
    fn a_tag_spread_over_lines_gives_its_src_and_a_look_alike_does_not() {
        let text = "<iframe\n  src=\"../assets/a.html\"\n  title=\"x\"\n></iframe>\n<IMG SRC='b.png' alt=''>\n\
                    <img data-src=\"no.png\"> <image src=\"no.png\"> <img alt=\"no src\"> <img src=bare.png/>";
        assert_eq!(html_sources(text), ["../assets/a.html", "b.png", "bare.png"]);
    }

    #[test]
    fn an_image_is_read_and_a_link_and_a_code_span_are_not() {
        let found = of(&lines(
            "![a [b]](x.png \"t\") [link](y.md) `![c](z.png)` ![](<w v.png>)\n```\n![d](q.png)\n```\n",
        ));
        let srcs: Vec<&str> = found.iter().map(|e| e.src.as_str()).collect();
        assert_eq!(srcs, ["x.png", "w v.png"]);
        assert!(found.iter().all(|e| !e.html));
        assert_eq!(image_source("a.png 'title'"), "a.png");
    }

    #[test]
    fn raw_html_is_relative_to_the_served_url_and_a_markdown_image_to_the_source_file() {
        // `architecture.md` is served at `architecture/`, so the asset is one level up from it.
        assert_eq!(docs("architecture.md", "../assets/x.html", true), "assets/x.html");
        assert_eq!(docs("architecture.md", "assets/x.html", false), "assets/x.html");
        // An index is served from its own directory.
        assert_eq!(docs("index.md", "assets/x.png", true), "assets/x.png");
        assert_eq!(docs("integrations/index.md", "../assets/x.png", true), "assets/x.png");
        assert_eq!(
            docs("integrations/oracle.md", "../../assets/x.png?v=2#top", true),
            "assets/x.png"
        );
        assert_eq!(docs("integrations/oracle.md", "../assets/x.png", false), "assets/x.png");
        assert_eq!(docs("integrations/oracle.md", "x.png", false), "integrations/x.png");
    }

    #[test]
    fn a_remote_a_data_and_a_host_absolute_src_are_each_told_apart() {
        for remote in [
            "https://example.com/a.png",
            "//cdn.example.com/a.png",
            "data:image/png;base64,AA",
            "#frag",
            "",
        ] {
            assert_eq!(docs("index.md", remote, true), "<elsewhere>", "{remote:?}");
        }
        assert!(docs("index.md", "/assets/a.png", true).contains("relative to the host"));
        assert!(docs("index.md", "../a.png", true).contains("climbs out"));
    }
}

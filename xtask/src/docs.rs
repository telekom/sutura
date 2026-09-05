//! The docs gate: keep `mkdocs.yml` and the pages on disk in agreement.
//!
//! mkdocs renders only what `nav` links to, so both failure modes are silent:
//!
//! * a nav entry naming a page that was renamed or deleted - the reader gets a 404 where a rule
//!   was supposed to be;
//! * a page under the docs directory that no nav entry names - it is published nowhere while
//!   looking published to whoever wrote it. This is the one that matters, and the one a casual
//!   build does not catch: `mkdocs build` warns and exits 0 unless `--strict` is passed.
//!
//! So this checks BOTH directions, without needing a Python environment to do it - which is the
//! point, because these gates run from `nix build .#checks...hygiene` and from a git hook, and
//! neither has mkdocs on PATH.
//!
//! There is a THIRD state, and it is a state rather than an exemption: a page under the docs
//! directory that `exclude_docs` names is deliberately not part of the site, and mkdocs drops it
//! from the build entirely. That is what the implementation plans are - written for whoever is
//! building sutura, kept where every path citation in the repository already names them, and no
//! part of what a reader came for. It would otherwise be the orphan case above, so the rule is
//! that a page is in `nav` OR excluded, never neither and never both, and every exclusion names
//! a page that exists. An exclusion over nothing is the shape a stale one takes, and it reads as
//! a page being kept off the site while nothing is.
//!
//! **The limit, next to the claim.** `exclude_docs` is ignore-file syntax, and this gate
//! implements the two shapes it accepts rather than all of it - so a glob, a directory pattern or
//! a `!` negation is REFUSED, because a pattern this cannot resolve is an exclusion nothing
//! checks. The two it does implement match the way mkdocs matches them, [`exclude`] carries the
//! measurement of what happened when they did not, and a pattern reaching more than one page is
//! itself a finding.
//!
//! A LINK from a published page into an excluded one is the third thing this holds, and it is
//! here because `mkdocs build --strict` does NOT catch it: mkdocs logs that link at INFO and
//! exits 0, measured. [`links`] carries the rule, the measurement, the `--8<--` includes it
//! follows to find a page's real body, and the syntax it still does not read.
//!
//! The second half is the assets. `mkdocs.yml` names its own stylesheet, its logo and its
//! favicon by path, and mkdocs copies what it finds without complaining about what it does not:
//! a stylesheet whose path stopped resolving is a site that renders unstyled behind a green
//! build. Those are checked too, along with the rule that none of them may load from another
//! origin - the site is served from one host and fetches nothing off it.
//!
//! One of those asset rules is not about a path at all. Material's DEFAULT is to link
//! fonts.googleapis.com from every page, so `theme.font: false` is required rather than
//! preferred: without it the site cannot render behind a proxy with no direct egress, and every
//! reader ends up in a third party's logs. The insecure setting is the default one, so nothing
//! but a gate would ever mention it.
//!
//! The last part is smaller and blunter: the mdBook stack this replaced must not come back. A
//! stray `book.toml` or `SUMMARY.md` is dead configuration that reads as live.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

mod exclude;
mod links;

/// The site configuration. Paths inside it are relative either to this file's directory (the
/// repo root) or to the docs directory, and which is which is per key - see `asset_problems`.
const CONFIG: &str = "mkdocs.yml";
/// Where the pages live when `mkdocs.yml` does not say otherwise. mkdocs' own default.
const DEFAULT_DOCS_DIR: &str = "docs";

/// Configuration from the stack this replaced. Present means dead: mkdocs reads none of it, so
/// it can only mislead the next reader into editing a file nothing builds.
const SUPERSEDED: &[&str] = &[
    "docs/book.toml",
    "docs/src/SUMMARY.md",
    "docs/publish.sh",
    "docs/theme/head.hbs",
    "docs/theme/telekom.css",
];

/// Ways a page actually LOADS something from another origin.
///
/// A URL in a comment fetches nothing and an `<a href="...">` is navigation, so neither is here -
/// only the forms that make the browser go and get a resource.
const EXTERNAL_LOADS: &[&str] = &[
    "@import 'http",
    "@import \"http",
    "fetch('http",
    "fetch(\"http",
    "import('http",
    "import(\"http",
    "src='http",
    "src=\"http",
    "url('http",
    "url(\"http",
    "url(http",
];

/// The line with any YAML comment removed.
///
/// `#` opens a comment only at the start of a line or after whitespace, which is what keeps a
/// URL fragment like `page.md#section` intact. Stripping first is what stops a commented-out
/// nav entry, or prose in a comment that happens to name a file, from counting as a reference.
fn strip_comment(line: &str) -> &str {
    let trimmed_start = line.trim_start();
    if trimmed_start.starts_with('#') {
        return "";
    }
    line.find(" #").map_or(line, |at| line.get(..at).unwrap_or(line))
}

/// Is this line a top-level key, i.e. does it start in column zero with content?
fn is_top_level(line: &str) -> bool {
    !line.starts_with(' ') && !line.starts_with('\t') && !line.trim().is_empty()
}

/// The lines belonging to the top-level block introduced by `key:`, the key's own line excluded.
///
/// The block ends at the next top-level key. A top-level COMMENT does not end it - a comment is
/// not a key, and guessing either way is wrong - which is harmless because every line is passed
/// through `strip_comment` before anything is read out of it.
fn block<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let top = is_top_level(line);
        if inside {
            if top && !line.trim_start().starts_with('#') {
                break;
            }
            out.push(line);
            continue;
        }
        if top && line.split(':').next() == Some(key) {
            inside = true;
        }
    }
    out
}

/// The value of a top-level `key: value` pair.
fn top_level_scalar(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if !is_top_level(line) {
            continue;
        }
        let stripped = strip_comment(line);
        let (name, value) = stripped.split_once(':')?;
        if name == key {
            let value = value.trim().trim_matches(['"', '\'']).trim();
            if !value.is_empty() {
                return Some(String::from(value));
            }
        }
    }
    None
}

/// The value of `key:` at the OUTERMOST indentation of a block.
///
/// The depth restriction is the whole point: Material has both `theme.logo`, which is a file
/// path, and `theme.icon.logo`, which is an icon name. Reading the second as the first would
/// send this gate looking for a file nobody claimed existed.
fn nested_scalar(lines: &[&str], key: &str) -> Option<String> {
    let outermost = lines
        .iter()
        .map(|line| strip_comment(line))
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()?;
    for line in lines {
        let stripped = strip_comment(line);
        if stripped.trim().is_empty() || stripped.len() - stripped.trim_start().len() != outermost {
            continue;
        }
        if let Some((name, value)) = stripped.trim().split_once(':')
            && name == key
        {
            let value = value.trim().trim_matches(['"', '\'']).trim();
            if !value.is_empty() {
                return Some(String::from(value));
            }
        }
    }
    None
}

/// Is this link target a markdown page? Case-insensitive, because half of this repo is
/// developed on a filesystem that does not distinguish `.MD` from `.md`.
fn is_markdown(target: &str) -> bool {
    Path::new(target)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// The page a `nav:` line points at, if it points at one.
///
/// Handles the three shapes mkdocs allows - `- page.md`, `- Title: page.md` and a bare section
/// heading `- Title:` with children - and drops an external URL, which is a legitimate nav entry
/// and not a file this repo has to own.
fn nav_target(line: &str) -> Option<&str> {
    let trimmed = strip_comment(line).trim();
    if trimmed.is_empty() {
        return None;
    }
    let body = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    if body.contains("://") {
        return None;
    }
    let value = body.rsplit(':').next()?.trim().trim_matches(['"', '\'']).trim();
    if value.is_empty() || !is_markdown(value) {
        return None;
    }
    Some(value)
}

/// The path a `- entry` line in a YAML list points at.
fn list_entry(line: &str) -> Option<&str> {
    let body = strip_comment(line).trim().strip_prefix("- ")?;
    let value = body.trim().trim_matches(['"', '\'']).trim();
    if value.is_empty() { None } else { Some(value) }
}

/// Does the configuration declare this top-level key at all?
///
/// Separate from reading its value, because a key declared with nothing under it is a finding of
/// its own: `exclude_docs:` naming no page reads as pages being kept off the site while none is.
fn declares(text: &str, key: &str) -> bool {
    text.lines()
        .filter(|line| is_top_level(line))
        .any(|line| strip_comment(line).split(':').next() == Some(key))
}

/// Every line of the `exclude_docs` block, as written.
///
/// The block is a `|` string in ignore-file syntax rather than a YAML list, so there is no `- `
/// to strip - a line is the pattern.
fn exclusions(text: &str) -> Vec<String> {
    block(text, "exclude_docs")
        .iter()
        .map(|line| strip_comment(line).trim())
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// Every page under the docs directory, relative to it, with `/` separators.
fn pages(root: &Path, docs_dir: &str) -> BTreeSet<String> {
    let mut found = Vec::new();
    repo::collect_files(root, &root.join(docs_dir), &["md"], &mut found);
    found
        .iter()
        .filter_map(|rel| {
            let tail = rel.strip_prefix(docs_dir)?;
            tail.strip_prefix('/')
        })
        .map(String::from)
        .collect()
}

/// Both directions, as a list of problems. Pure, so the rule is testable without a repo.
///
/// `excluded` is the third state: a page mkdocs is told to leave out of the build is not an
/// orphan, and a page in neither set still is.
fn problems(nav: &BTreeSet<String>, present: &BTreeSet<String>, excluded: &BTreeSet<String>, docs_dir: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for target in nav {
        if !present.contains(target) {
            problems.push(format!(
                "{CONFIG} navigates to `{target}`, but `{docs_dir}/{target}` does not exist"
            ));
        }
    }
    for orphan in present.difference(nav).filter(|page| !excluded.contains(*page)) {
        problems.push(format!(
            "`{docs_dir}/{orphan}` is in no nav entry - a page mkdocs does not navigate to is published nowhere, so add it to the nav in {CONFIG}, exclude it with `exclude_docs`, or delete it"
        ));
    }
    problems
}

/// Every off-origin load in one file, as a list of problems.
fn external_loads(rel: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lowered = line.to_ascii_lowercase();
        // `<link>` is judged by the whole line rather than by a prefix: the `href` can sit
        // before or after `rel`, so there is no fixed pair of characters to look for.
        let loads_off_origin = EXTERNAL_LOADS.iter().any(|needle| lowered.contains(needle))
            || (lowered.contains("<link") && lowered.contains("http"));
        if loads_off_origin {
            problems.push(format!(
                "{rel}:{}: loads from another origin - the site is served from one host and must fetch nothing off it, so inline it or vendor it under the docs directory",
                i + 1
            ));
        }
    }
    problems
}

/// One asset the configuration names: where it was named, the path, and what it is relative to.
struct Asset {
    /// The configuration key, for the message.
    key: &'static str,
    /// The path as written.
    path: String,
    /// Relative to the docs directory (true) or to the repo root (false).
    under_docs: bool,
    /// Must be a directory rather than a file.
    directory: bool,
}

/// Every asset `mkdocs.yml` names.
///
/// `extra_css`, `extra_javascript`, `theme.logo` and `theme.favicon` are resolved against the
/// DOCS directory, because that is where mkdocs copies from. `theme.custom_dir` is resolved
/// against the configuration file instead - mkdocs treats it as a project path, and getting that
/// wrong is how an override directory silently does nothing.
fn assets(text: &str) -> Vec<Asset> {
    let mut out = Vec::new();
    for key in ["extra_css", "extra_javascript"] {
        for line in block(text, key) {
            if let Some(path) = list_entry(line) {
                out.push(Asset {
                    key,
                    path: String::from(path),
                    under_docs: true,
                    directory: false,
                });
            }
        }
    }
    let theme = block(text, "theme");
    for key in ["logo", "favicon"] {
        if let Some(path) = nested_scalar(&theme, key) {
            out.push(Asset {
                key,
                path,
                under_docs: true,
                directory: false,
            });
        }
    }
    if let Some(path) = nested_scalar(&theme, "custom_dir") {
        out.push(Asset {
            key: "custom_dir",
            path,
            under_docs: false,
            directory: true,
        });
    }
    out
}

/// Judge one asset's path without touching the filesystem: an absolute URL is a finding on its
/// own, because a site that fetches its own stylesheet from a CDN is not self-contained and no
/// amount of the file being present makes it so.
fn asset_url_problem(asset: &Asset) -> Option<String> {
    if asset.path.contains("://") {
        return Some(format!(
            "{CONFIG} points `{}` at `{}` - an absolute URL, so the page loads it from another origin. Vendor it under the docs directory instead",
            asset.key, asset.path
        ));
    }
    None
}

/// The asset half of the gate, filesystem included.
fn asset_problems(root: &Path, docs_dir: &str, found: &[Asset]) -> Vec<String> {
    let mut problems = Vec::new();
    for asset in found {
        if let Some(problem) = asset_url_problem(asset) {
            problems.push(problem);
            continue;
        }
        let base = if asset.under_docs {
            root.join(docs_dir)
        } else {
            root.to_path_buf()
        };
        let path = base.join(&asset.path);
        let shown = if asset.under_docs {
            format!("{docs_dir}/{}", asset.path)
        } else {
            asset.path.clone()
        };
        let there = if asset.directory { path.is_dir() } else { path.is_file() };
        if !there {
            let what = if asset.directory { "directory" } else { "file" };
            problems.push(format!(
                "{CONFIG} names `{}: {}` but there is no {what} at `{shown}` - mkdocs copies what it finds and says nothing about what it does not",
                asset.key, asset.path
            ));
            continue;
        }
        if !asset.directory
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            problems.extend(external_loads(&shown, &text));
        }
    }
    problems
}

/// Material fetches its webfont from Google unless told not to. Not a path check, but the same
/// failure: something the configuration pulls off the origin without saying so.
fn font_problem(text: &str) -> Option<String> {
    let theme = block(text, "theme");
    if nested_scalar(&theme, "name").as_deref() != Some("material") {
        return None;
    }
    if nested_scalar(&theme, "font").as_deref() == Some("false") {
        return None;
    }
    Some(format!(
        "{CONFIG} does not set `theme.font: false`, so Material links fonts.googleapis.com and preconnects to fonts.gstatic.com on every page - the site stops rendering behind a proxy with no direct egress, and every reader lands in a third party's logs"
    ))
}

/// The `pymdownx.snippets` extension, and where it resolves an include from.
///
/// A stub page's published body is whatever `--8<--` pulls in, so [`links`] has to follow the
/// include - and it resolves against the repo root, which is what `base_path: ["."]` means. That
/// makes the root an ASSUMPTION this gate depends on, so the assumption is gated rather than
/// written down: a different `base_path` is a finding here, not a silent disagreement between
/// the file the gate reads and the file mkdocs publishes.
fn snippet_problems(text: &str) -> (links::Snippets, Vec<String>) {
    let extensions = block(text, "markdown_extensions");
    let mut inside = None;
    let mut base = None;
    for line in &extensions {
        let stripped = strip_comment(line);
        if stripped.trim().is_empty() {
            continue;
        }
        let depth = stripped.len().saturating_sub(stripped.trim_start().len());
        if stripped.trim().starts_with("- pymdownx.snippets") {
            inside = Some(depth);
            continue;
        }
        let Some(opened) = inside else { continue };
        if depth <= opened {
            inside = None;
            continue;
        }
        if let Some((key, value)) = stripped.trim().split_once(':')
            && key == "base_path"
        {
            base = Some(String::from(value.trim()));
        }
    }
    let configured = extensions
        .iter()
        .any(|line| strip_comment(line).trim().starts_with("- pymdownx.snippets"));
    if !configured {
        return (links::Snippets::NotConfigured, Vec::new());
    }
    // `["."]` is mkdocs' own default written out, and the only value this gate can follow.
    let problems = match base.as_deref() {
        None | Some("[\".\"]" | "['.']" | "[.]") => Vec::new(),
        Some(other) => vec![format!(
            "{CONFIG} sets `pymdownx.snippets.base_path` to `{other}`, and this gate resolves a `--8<--` include against the repo root - so the file it reads for a stub page is not the file mkdocs publishes. Keep the base path at `[\".\"]`, or teach this gate the one you want"
        )],
    };
    (links::Snippets::Followed, problems)
}

/// Configuration from the replaced stack, still on disk.
fn superseded_problems(root: &Path) -> Vec<String> {
    SUPERSEDED
        .iter()
        .filter(|rel| root.join(rel).exists())
        .map(|rel| {
            format!(
                "`{rel}` is left over from mdBook, which nothing builds any more - mkdocs reads none of it, so it can only mislead whoever edits it next. Delete it"
            )
        })
        .collect()
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-docs: could not determine the repo root");
        return Verdict::Fail;
    };

    let config_path = root.join(CONFIG);
    let Ok(config) = std::fs::read_to_string(&config_path) else {
        // Not an error: a repo need not have a site. Pages with no configuration are.
        let orphans = pages(&root, DEFAULT_DOCS_DIR);
        if orphans.is_empty() {
            println!("xtask check-docs: ok - no site");
            return Verdict::Pass;
        }
        eprintln!("xtask check-docs: FAILED - pages exist under {DEFAULT_DOCS_DIR} but {CONFIG} does not");
        return Verdict::Fail;
    };

    let docs_dir = top_level_scalar(&config, "docs_dir").unwrap_or_else(|| String::from(DEFAULT_DOCS_DIR));
    let nav: BTreeSet<String> = block(&config, "nav")
        .iter()
        .filter_map(|line| nav_target(line))
        .map(String::from)
        .collect();
    let present = pages(&root, &docs_dir);
    let patterns = exclusions(&config);
    let exclude::Resolved {
        pages: excluded,
        problems: exclusion_problems,
    } = exclude::resolve(&patterns, &present, &nav, &docs_dir);

    let mut found = problems(&nav, &present, &excluded, &docs_dir);
    found.extend(exclusion_problems);
    let (snippets, snippet_problems) = snippet_problems(&config);
    found.extend(snippet_problems);
    let published: Vec<&String> = present.difference(&excluded).collect();
    let sweep = links::sweep(&root, &docs_dir, &published, &excluded, snippets);
    found.extend(sweep.problems);
    let linked = sweep.read;
    // FAIL CLOSED, TWICE, because the two ways the scan can read less than it claims are
    // different failures and one floor cannot see both.
    //
    // PER PAGE first: every published page has to be scanned end to end. A repo-wide link total
    // was the only floor this had, and `.take(1)` on the page loop satisfied it with 3 links off
    // one page while 51 of 52 went unread - the quantifier `sutura/gates` records as defending
    // nothing about WHICH row.
    if sweep.scanned != published.len() {
        found.push(format!(
            "scanned {} of {} published page(s) - a page nothing scanned is a page nothing checked, so the link rule is silently false for the rest. Any page this could not read is named above",
            sweep.scanned,
            published.len()
        ));
    }
    // PER TREE second, for the case the per-page count cannot see: every page opened and parsed
    // and not one link read means the link parser is broken, not that the tree agrees.
    if linked == 0 && !published.is_empty() {
        found.push(format!(
            "read no link to a page in `{docs_dir}` out of {} page(s) - the scan is broken, not the tree",
            published.len()
        ));
    }
    if declares(&config, "exclude_docs") && patterns.is_empty() {
        // Fail closed. A declared block naming nothing reads as pages being kept off the site,
        // and every page is then judged by the nav alone with no sign that the intent was wider.
        found.push(format!(
            "{CONFIG} declares `exclude_docs` and names no page - either name the pages that are not part of the site, or delete the key"
        ));
    }
    if nav.is_empty() {
        // An empty nav would make every page an orphan and every check above vacuous, so say
        // what is actually wrong instead of printing one problem per page.
        found.clear();
        found.push(format!(
            "{CONFIG} has no `nav:` entries naming a markdown page - the navigation is what mkdocs publishes, so without it nothing is reachable"
        ));
    }
    let declared = assets(&config);
    found.extend(asset_problems(&root, &docs_dir, &declared));
    found.extend(font_problem(&config));
    found.extend(superseded_problems(&root));

    if found.is_empty() {
        println!(
            "xtask check-docs: ok - {} nav entr(ies), {} page(s), {} excluded, every other page reachable, {} of {} published page(s) scanned, {linked} page link(s) land on one, {} asset(s) resolve",
            nav.len(),
            present.len(),
            excluded.len(),
            sweep.scanned,
            published.len(),
            declared.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-docs: FAILED");
    for problem in &found {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("The site is what a reader sees, so a page it does not reach is a rule nobody reads.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        block, declares, exclusions, list_entry, nav_target, nested_scalar, problems, snippet_problems, top_level_scalar,
    };

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    const CONFIG: &str = "\
site_name: sutura
docs_dir: docs
theme:
  name: material
  logo: assets/logo.svg
  icon:
    logo: material/library
    repo: fontawesome/brands/github
nav:
  - Introduction: index.md
  - Getting started: getting-started.md
  - Reference:
    - Overview: reference/index.md
    - Slides: https://example.com/deck.md
exclude_docs: |
  notes/why-the-order.md
extra_css:
  - css/telekom.css
extra:
  version:
    provider: mike
";

    #[test]
    fn reads_nav_targets_at_every_depth() {
        let targets: Vec<&str> = block(CONFIG, "nav").iter().filter_map(|l| nav_target(l)).collect();
        assert_eq!(targets, vec!["index.md", "getting-started.md", "reference/index.md"]);
    }

    #[test]
    fn a_nav_entry_can_be_a_bare_path_or_a_titled_one() {
        assert_eq!(nav_target("  - index.md"), Some("index.md"));
        assert_eq!(nav_target("  - Introduction: index.md"), Some("index.md"));
        assert_eq!(
            nav_target("  - 'Getting started': getting-started.md"),
            Some("getting-started.md")
        );
        // A section heading names no file, and neither does a non-markdown entry.
        assert_eq!(nav_target("  - Reference:"), None);
        assert_eq!(nav_target("  - Logo: assets/logo.svg"), None);
        assert_eq!(nav_target(""), None);
    }

    #[test]
    fn an_external_nav_entry_is_not_a_file_this_repo_owns() {
        // The real case: a link out to a talk. Reading it as a path made the gate demand a file
        // called `//example.com/deck.md`.
        assert_eq!(nav_target("    - Slides: https://example.com/deck.md"), None);
    }

    #[test]
    fn a_commented_out_nav_entry_is_not_an_entry() {
        assert_eq!(nav_target("  # - Draft: draft.md"), None);
        // And a trailing comment does not become part of the path.
        assert_eq!(nav_target("  - Intro: index.md # the front page"), Some("index.md"));
    }

    #[test]
    fn an_anchor_survives_comment_stripping() {
        // `#` opens a comment after whitespace, not inside a word: `page.md#section` is one path.
        assert_eq!(super::strip_comment("  - A: page.md#section").trim(), "- A: page.md#section");
    }

    #[test]
    fn a_block_ends_at_the_next_top_level_key() {
        let lines = block(CONFIG, "extra_css");
        assert_eq!(
            lines.iter().filter_map(|l| list_entry(l)).collect::<Vec<_>>(),
            vec!["css/telekom.css"]
        );
    }

    #[test]
    fn the_theme_logo_is_read_but_the_theme_icon_logo_is_not() {
        // Material has both. `theme.logo` is a file; `theme.icon.logo` is an icon name, and
        // reading the second as the first sends the gate looking for `docs/material/library`.
        let theme = block(CONFIG, "theme");
        assert_eq!(nested_scalar(&theme, "logo").as_deref(), Some("assets/logo.svg"));
        assert_eq!(nested_scalar(&theme, "name").as_deref(), Some("material"));
        assert_eq!(nested_scalar(&theme, "favicon"), None);
        assert_eq!(nested_scalar(&theme, "custom_dir"), None);
    }

    #[test]
    fn docs_dir_is_read_from_the_top_level_only() {
        assert_eq!(top_level_scalar(CONFIG, "docs_dir").as_deref(), Some("docs"));
        assert_eq!(top_level_scalar(CONFIG, "site_name").as_deref(), Some("sutura"));
        // `name: material` is nested under `theme:`, so it is not a top-level key.
        assert_eq!(top_level_scalar(CONFIG, "name"), None);
    }

    #[test]
    fn a_matching_pair_is_clean() {
        assert!(
            problems(
                &set(&["index.md", "gates.md"]),
                &set(&["index.md", "gates.md"]),
                &set(&[]),
                "docs"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_nav_entry_with_no_file_is_a_problem() {
        let found = problems(&set(&["index.md", "gone.md"]), &set(&["index.md"]), &set(&[]), "docs");
        assert_eq!(found.len(), 1);
        assert!(found.iter().any(|p| p.contains("does not exist")), "{found:?}");
    }

    #[test]
    fn a_page_in_no_nav_entry_is_a_problem() {
        // The direction a non-strict build does not catch: an orphan page renders and is read by
        // nobody.
        let found = problems(&set(&["index.md"]), &set(&["index.md", "orphan.md"]), &set(&[]), "docs");
        assert_eq!(found.len(), 1);
        assert!(found.iter().any(|p| p.contains("in no nav entry")), "{found:?}");
    }

    #[test]
    fn both_directions_are_reported_together() {
        let found = problems(
            &set(&["index.md", "gone.md"]),
            &set(&["index.md", "orphan.md"]),
            &set(&[]),
            "docs",
        );
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn an_asset_named_by_an_absolute_url_is_a_problem() {
        use super::{Asset, asset_url_problem};

        let remote = Asset {
            key: "extra_css",
            path: String::from("https://cdn.example/x.css"),
            under_docs: true,
            directory: false,
        };
        assert!(asset_url_problem(&remote).is_some_and(|p| p.contains("another origin")));
        let local = Asset {
            key: "extra_css",
            path: String::from("css/telekom.css"),
            under_docs: true,
            directory: false,
        };
        assert!(asset_url_problem(&local).is_none());
    }

    #[test]
    fn the_declared_assets_are_read_with_the_right_base() {
        use super::assets;

        let found = assets(CONFIG);
        let named: Vec<(&str, &str)> = found.iter().map(|a| (a.key, a.path.as_str())).collect();
        assert_eq!(named, vec![("extra_css", "css/telekom.css"), ("logo", "assets/logo.svg")]);
        assert!(found.iter().all(|a| a.under_docs), "both resolve under the docs directory");
    }

    #[test]
    fn a_material_theme_must_switch_the_google_font_off() {
        use super::font_problem;

        // The default is the leaky one, which is exactly why this is a gate and not a comment.
        assert!(
            font_problem(
                "theme:
  name: material
"
            )
            .is_some_and(|p| p.contains("fonts.googleapis.com"))
        );
        assert!(
            font_problem(
                "theme:
  name: material
  font: false
"
            )
            .is_none()
        );
        // A different theme has different defaults; this rule is Material's.
        assert!(
            font_problem(
                "theme:
  name: readthedocs
"
            )
            .is_none()
        );
    }

    #[test]
    fn an_off_origin_load_is_a_problem_and_a_local_one_is_not() {
        use super::external_loads;

        assert_eq!(external_loads("t.css", "@import \"https://fonts.example/f.css\";").len(), 1);
        assert_eq!(external_loads("t.css", "src: url(https://x/f.woff2);").len(), 1);
        assert_eq!(external_loads("t.js", "fetch(\"https://x/api\")").len(), 1);
        assert_eq!(
            external_loads("t.html", "<link rel=stylesheet href=\"http://x/y.css\">").len(),
            1
        );

        // Same-origin and relative loads are the point of the rule, not violations of it.
        assert!(external_loads("t.css", "@import \"variables.css\";").is_empty());
        assert!(external_loads("t.css", "  --md-primary-fg-color: #e20074;").is_empty());
        // A URL in prose or a comment fetches nothing, so it is none of this rule's business.
        assert!(external_loads("t.css", " * see https://example.com/why").is_empty());
    }

    #[test]
    fn the_exclusion_block_is_read_as_lines_rather_than_a_yaml_list() {
        assert_eq!(exclusions(CONFIG), vec![String::from("notes/why-the-order.md")]);
        assert!(declares(CONFIG, "exclude_docs"));
        // A key that is not there is not declared, which is what separates "no exclusions" from
        // "an exclusion block naming nothing".
        assert!(!declares(CONFIG, "not_in_nav"));
        assert!(exclusions("site_name: sutura\n").is_empty());
    }

    #[test]
    fn an_excluded_page_is_not_an_orphan() {
        // The whole point of the third state: the implementation plans are under the docs
        // directory, are in no nav entry, and are not a finding.
        let found = problems(
            &set(&["index.md"]),
            &set(&["index.md", "implementation-plan.md"]),
            &set(&["implementation-plan.md"]),
            "docs",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_exclusion_does_not_excuse_a_different_orphan() {
        // The filter is per page, not a switch that turns the orphan rule off.
        let found = problems(
            &set(&["index.md"]),
            &set(&["index.md", "orphan.md", "implementation-plan.md"]),
            &set(&["implementation-plan.md"]),
            "docs",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("orphan.md"), "{found:?}");
    }

    #[test]
    fn the_snippet_base_path_this_gate_depends_on_is_gated_rather_than_assumed() {
        // The gate follows `--8<--` against the REPO ROOT to find a stub page's real body, so a
        // different base path means it reads a different file from the one mkdocs publishes.
        let default = "markdown_extensions:
  - pymdownx.snippets:
      base_path: [\".\"]
      check_paths: true
";
        let (snippets, problems) = snippet_problems(default);
        assert!(matches!(snippets, super::links::Snippets::Followed));
        assert!(problems.is_empty(), "{problems:?}");

        let moved = default.replace("[\".\"]", "[\"docs\"]");
        let (_, problems) = snippet_problems(&moved);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems.first().is_some_and(|p| p.contains("base_path")), "{problems:?}");

        // With the extension absent, `--8<--` is text mkdocs publishes verbatim, so there is
        // nothing to follow and `links` says so about a page that carries one.
        let (snippets, problems) = snippet_problems("markdown_extensions:\n  - admonition\n");
        assert!(matches!(snippets, super::links::Snippets::NotConfigured));
        assert!(problems.is_empty(), "{problems:?}");
    }

    // NOTE: there is deliberately no test here that reads the real `mkdocs.yml` and the real
    // docs tree. That property is enforced by `cargo xtask check-docs`, which runs in the hooks
    // and in the `hygiene` flake check - and `hygiene` is given the whole repository as its
    // source.
    //
    // **This note used to give a reason that is no longer true, and the correction matters more
    // than the conclusion.** It said a unit test CANNOT read `docs/` because `checks.nextest`
    // gets crane's Cargo-only source filter. It does not any more: `flake.nix` passes that check
    // `wholeTree`, and says why - `xtask`'s tests read repo files by design, and three bug
    // reports came out of the filter hiding them. So such a test would work; the fixture stays
    // because the rule is a pure function of two sets and a parser, and a test over the real
    // tree would assert today's page list rather than the rule.
}

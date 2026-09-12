//! `versions` - a version written where nothing compares it to the pin.
//!
//! The rule this replaced compared a version against the pin it named, and only on a line that
//! named the pin FILE. That is a ratchet on one sentence: it held the compiler pin and declined
//! everything else, because "some other version in prose is not this check's business".
//!
//! It is this check's business now, and the reason is a measured instance rather than a
//! preference. `crates/sutura-http` carried a `tower-http` patch version in four comments that
//! was neither the constraint the workspace manifest declares nor the version the lock resolves.
//! Nobody bumped anything: the copy was never true, and it was checked by nothing that would
//! have said so. A version in a comment is not a pin; it is a second
//! copy of one, and a second copy with nothing comparing it rots and then misleads.
//!
//! **So the subject is a copy nobody compares, not a digit.** A version may be written where a
//! MECHANISM compares it to the pin, and may not be written anywhere a person has to remember
//! it. That is `AGENTS.md`'s rule about invariants applied to a number: held by a type, a lint, a
//! hook or a gate, never by recall. `check-crap` is the clearest case - it FAILS when
//! `docs/crap.md` stops naming the version `nix/crap.nix` pins, and has its own test - so that
//! page's copy is compared and this check has no quarrel with it.
//!
//! **What it keys on, and why not the shape.** Measured over this tree, a version-shaped token is
//! usually not a version: `1.5` and `101.5` are a wrong answer and a column total in a refusal
//! narrative, `v4` and `v6` are address families, `v1` and `v2` are API routes and cgroup
//! generations, `11.1` is an RFC section and also a CRAP timing, and `127.0.0.1` is an address.
//! Refusing by shape would redden every one of those, and a gate that reddens a correct tree is
//! worse than the rot it prevents. So a token is refused only when the token BESIDE it names
//! something this repo pins, read out of the manifests rather than listed here - which makes the
//! subject exactly the thing that has an authoritative home. The one exception is a
//! `nightly-<date>`, which cannot be anything but the compiler pin.
//!
//! **The trailing tag on a SHA-pinned action is out of scope BY DECISION**, not by evidence: the
//! owner ruled that dependabot maintains its own tags. Nobody here reproduced what dependabot
//! writes, and that is not what settles it. The implementation is structural rather than a named
//! carve-out - a `#` breaks adjacency, so nothing lists those lines and the rule stays one rule -
//! and **the limit of that is that it is BROADER than the ruling**: any version in a comment
//! whose nearest token is punctuation is spared, dependabot's or not.
//!
//! **What it therefore does not catch, stated before a reader trusts it.** A version separated
//! from its name by a clause ("`pkgs.stax` is 0.102.2 on nixpkgs"). A version of something this
//! repo does not pin, which has no home here to rot against. A bare version with no name beside
//! it, which is indistinguishable from data. A fourth dotted field, so that an address is never
//! refused. A bare `v<n>`, for the reason [`is_version`] gives. And in Rust, a `//` inside a
//! string literal that is not a URL scheme is read as a comment.

use std::collections::BTreeSet;
use std::path::Path;

use crate::guidance::has_ext;
use crate::repo;

/// Pages where a version is the content, or is already held against the pin by a mechanism.
///
/// Each entry is a mechanism or a record, never a preference - an exemption whose reason is
/// "it would be noisy" is the hole this gate exists to close.
const EXEMPT: &[&str] = &[
    // Mirrored upstream material describes ITS repo. Same reason the prose scope excludes them.
    "vendor/**",
    ".agents/skill-library/**",
    ".agents/skills/engineering/ms-rust/**",
    // The provenance of a vendored copy: the upstream version IS the record, which is why
    // `vendor/**` is excluded from formatting for the same reason.
    "VENDOR.md",
    // The release record, where a version is the primary key rather than a description.
    "CHANGELOG.md",
    // A decision record is dated and closed. Its versions are mostly of crates this workspace
    // does NOT depend on - the resolution that was rejected - so there is no home here for them
    // to rot against, and rewriting a record to drop them would make it less true, not more.
    "docs/adr/**",
    // `check-crap` fails when this page stops naming the version `nix/crap.nix` pins, and its
    // own test holds that. A compared copy is the one shape this check has no quarrel with.
    "docs/crap.md",
];

/// What the scan read. A gate over no names, or over no comments, is a gate over nothing -
/// `check-pins` records that failure mode for a text scan: an empty side passes by finding
/// nothing. Both are printed in the success line so a narrowed walk is visible when it is green.
pub(in crate::guidance) struct Scan {
    pub(in crate::guidance) names: usize,
    pub(in crate::guidance) comments: usize,
}

/// Below these the harvest has lost a source rather than found a clean tree.
///
/// Not a tally of what is there today - that would be one more number to keep true. A floor far
/// under it, which only a broken reader can trip.
const NAME_FLOOR: usize = 40;
const COMMENT_FLOOR: usize = 2_000;

/// Punctuation a sentence puts around a token, none of which is part of it.
const EDGE: &[char] = &[
    '.', ',', ';', ':', '(', ')', '"', '\'', '`', '[', ']', '!', '?', '*', '<', '>', '|', '#', '-',
];

/// A token with the sentence's own punctuation and a possessive removed.
///
/// The trim runs twice around the possessive: `0.9.143's` ends in `s`, so the first pass stops
/// there, and treating that as a different version from `0.9.143` is how a correct doc gets
/// flagged. `-` is trimmed at the edges only, so `tower-http` survives whole.
fn bare(token: &str) -> &str {
    let trimmed = token.trim_matches(|c: char| c.is_whitespace() || EDGE.contains(&c));
    let owned = trimmed.strip_suffix("'s").unwrap_or(trimmed);
    owned.trim_matches(|c: char| EDGE.contains(&c))
}

/// The compiler pin's own shape, which cannot be anything else.
fn nightly(token: &str) -> bool {
    let Some(date) = token.strip_prefix("nightly-") else {
        return false;
    };
    let mut fields = date.split('-');
    let (Some(year), Some(month), Some(day), None) = (fields.next(), fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    [(year, 4), (month, 2), (day, 2)]
        .iter()
        .all(|(field, width)| field.len() == *width && field.bytes().all(|b| b.is_ascii_digit()))
}

/// Is this token a version?
///
/// Two or three dotted numbers, with an optional `v`. Three shapes are deliberately NOT one:
///
/// * **Four fields**, because semver has no fourth and `127.0.0.1` and `10.0.0.7` are all over
///   this tree's comments.
/// * **A bare integer**, because `17` is a count far more often than a version.
/// * **A bare `v<n>`**, MEASURED: `v1` and `v2` are an API route and a cgroup generation here,
///   and a planted `the duckdb v1 route` was the one false positive the probes found. Dropping
///   the shape cost one refusal in the whole tree - `mimalloc v2`, which names a major we do not
///   ship - and a route prefix beside a crate name is the likelier sentence.  So `v7.0.1` is
///   refused and `v17` is not read as a version at all.
fn is_version(token: &str) -> bool {
    if nightly(token) {
        return true;
    }
    let body = token.strip_prefix('v').unwrap_or(token);
    if body.is_empty() || body.starts_with('.') || body.ends_with('.') || body.contains("..") {
        return false;
    }
    if !body.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return false;
    }
    matches!(body.split('.').count(), 2 | 3)
}

/// Every token in `line`, with the byte it starts at.
///
/// Split on a space rather than on whitespace, because the offset is what says whether a token
/// is inside the comment, and `split_whitespace` does not give one.
fn tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    for raw in line.split(' ') {
        if !raw.is_empty() {
            out.push((at, raw));
        }
        at = at.saturating_add(raw.len()).saturating_add(1);
    }
    out
}

/// Where the comment starts on this line, or `None` if there is not one.
///
/// A whole Markdown line is prose, so the answer is 0 there. Everywhere else a version sits in a
/// VALUE legitimately - that value is the pin - so only the comment is read, which is what makes
/// `Cargo.toml`, `rust-toolchain.toml` and a pinned `uses:` need no exemption.
fn comment_start(rel: &str, line: &str) -> Option<usize> {
    let extension = std::path::Path::new(rel).extension().and_then(std::ffi::OsStr::to_str)?;
    if extension.eq_ignore_ascii_case("md") {
        return Some(0);
    }
    if extension.eq_ignore_ascii_case("rs") {
        // `://` is a URL in a string literal, not a comment. The rest of that class is the
        // stated limit: this reads a `//` inside any other literal as a comment.
        let mut rest = line;
        let mut at = 0_usize;
        loop {
            let found = rest.find("//")?;
            let after = at.saturating_add(found).saturating_add(2);
            let scheme = rest.split_at_checked(found).is_some_and(|(before, _)| before.ends_with(':'));
            if !scheme {
                return Some(after);
            }
            rest = rest.get(found.saturating_add(2)..)?;
            at = after;
        }
    }
    if ["nix", "yml", "yaml", "sh", "toml"]
        .iter()
        .any(|k| extension.eq_ignore_ascii_case(k))
    {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if line.trim_start().starts_with('#') {
            return Some(indent.saturating_add(1));
        }
        return line.find(" #").map(|at| at.saturating_add(2));
    }
    None
}

/// The names of everything this repo pins, read out of the files that pin it.
///
/// Six sources because the pins live in six places, and a seventh list written here would be the
/// second copy this whole module is about. `rust`, `rustc` and `cargo` are named because the
/// toolchain's pin file holds a channel rather than a tool name.
fn pinned_names(root: &Path, files: &[String]) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = ["rust", "rustc", "cargo"].iter().map(|n| String::from(*n)).collect();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    for rel in files {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        if base == "Cargo.toml" {
            names.extend(manifest_dependencies(&read(rel)));
        }
        // A service is pinned by its image tag, so the image name is the thing that has a home.
        if has_ext(base, &["yml", "yaml"]) {
            names.extend(image_names(&read(rel)));
        }
        // Each nix module is named after the tool it pins.
        if let Some(tool) = rel.strip_prefix("nix/").and_then(|n| n.strip_suffix(".nix")) {
            names.insert(tool.to_ascii_lowercase());
        }
    }
    let flake = read("flake.nix");
    names.extend(crate::pins::flake_apps(&flake).iter().map(|n| n.to_ascii_lowercase()));
    names.extend(flake_inputs(&flake));
    // Every dependency TABLE, where `check-pins` reads exactly `[dependencies]`: that gate is
    // about the nix/pixi overlap, and this one is about anything with an authoritative home, so
    // the docs toolchain under `[feature.docs.dependencies]` counts here and not there.
    names.extend(pixi_dependencies(&read("pixi.toml")));
    names
}

/// Every dependency named by one manifest, from a table header or a key.
///
/// A `[...dependencies.<name>]` sub-table contributes its header's last segment and NONE of its
/// keys, which is what keeps `version`, `features` and `path` out of the name set.
fn manifest_dependencies(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut keys_count = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            let last = header.rsplit('.').next().unwrap_or(header);
            let is_table = header.contains("dependencies");
            keys_count = is_table && last.ends_with("dependencies");
            if is_table && !keys_count {
                names.insert(last.to_ascii_lowercase());
            }
            continue;
        }
        if !keys_count || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim().trim_matches('"');
            if !key.is_empty() {
                names.insert(key.to_ascii_lowercase());
            }
        }
    }
    names
}

/// Every flake input name.
fn flake_inputs(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("inputs = {") {
            inside = true;
            continue;
        }
        if inside && trimmed == "};" {
            break;
        }
        if !inside {
            continue;
        }
        if let Some((left, _)) = trimmed.split_once('=')
            && let Some(name) = left.trim().split(['.', ' ']).next()
            && !name.is_empty()
        {
            names.insert(name.to_ascii_lowercase());
        }
    }
    names
}

/// Every key in any pixi dependency table.
fn pixi_dependencies(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            inside = header.ends_with("dependencies");
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                names.insert(key.to_ascii_lowercase());
            }
        }
    }
    names
}

/// Every container image name a compose file pins by tag.
fn image_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(reference) = trimmed.strip_prefix("image:")
            && let Some((repository, tag)) = reference.trim().rsplit_once(':')
            && !tag.is_empty()
            && let Some(name) = repository.rsplit('/').next()
            && !name.is_empty()
        {
            names.insert(name.to_ascii_lowercase());
        }
    }
    names
}

/// The versions this line writes where nothing compares them, as `(version, name)`.
///
/// Adjacency is strictly the token before or after, and punctuation BREAKS it rather than being
/// skipped: measured over this tree, widening to two tokens turns a Markdown table row
/// (`| sutura-domain | 11.1 s |`), an example invocation and a timing beside a crate name into
/// failures, and every real instance is written name-then-version with nothing between.
fn refused<'line>(line: &'line str, start: usize, names: &BTreeSet<String>) -> Vec<(&'line str, &'line str)> {
    let mut found = Vec::new();
    let words = tokens(line);
    for (index, (at, raw)) in words.iter().enumerate() {
        if *at < start {
            continue;
        }
        let version = bare(raw);
        if !is_version(version) {
            continue;
        }
        if nightly(version) {
            found.push((version, "the compiler pin"));
            continue;
        }
        let neighbour = [index.checked_sub(1), index.checked_add(1)]
            .into_iter()
            .flatten()
            .filter_map(|other| words.get(other))
            .map(|(_, raw)| bare(raw))
            .find(|word| !word.is_empty() && names.contains(&word.to_ascii_lowercase()));
        if let Some(name) = neighbour {
            found.push((version, name));
        }
    }
    found
}

/// Every comment and prose line in the tree that writes a version nothing compares.
pub(in crate::guidance) fn comment_versions(root: &Path, files: &[String]) -> (Vec<String>, Scan) {
    let names = pinned_names(root, files);
    let mut problems = Vec::new();
    let mut comments = 0_usize;
    for rel in files {
        if repo::matches_any(EXEMPT, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        let markdown = has_ext(rel, &["md"]);
        let mut fenced = false;
        for (number, line) in text.lines().enumerate() {
            if markdown && line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            // A fence holds a transcript or an example invocation - pasted output, not a claim.
            if fenced {
                continue;
            }
            let Some(start) = comment_start(rel, line) else {
                continue;
            };
            comments = comments.saturating_add(1);
            for (version, name) in refused(line, start, &names) {
                problems.push(format!(
                    "{rel}:{}: writes `{version}` beside {name}, and nothing compares it to the pin\n      {}",
                    number.saturating_add(1),
                    line.trim()
                ));
            }
        }
    }
    // FAIL CLOSED, for `check-pins`' reason: a harvest that reads nothing refuses nothing, and
    // that reads as a clean tree.
    if names.len() < NAME_FLOOR {
        problems.push(format!(
            "read {} pinned name(s) out of the manifests, under a floor of {NAME_FLOOR} - the \
             harvest is broken, not the tree",
            names.len()
        ));
    }
    if comments < COMMENT_FLOOR {
        problems.push(format!(
            "read {comments} comment line(s), under a floor of {COMMENT_FLOOR} - the scan stopped \
             reading rather than the tree being clean"
        ));
    }
    (
        problems,
        Scan {
            names: names.len(),
            comments,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{bare, comment_start, is_version, refused};
    use std::collections::BTreeSet;

    fn names() -> BTreeSet<String> {
        ["tower-http", "duckdb", "prek", "rustc", "mimalloc"]
            .iter()
            .map(|n| String::from(*n))
            .collect()
    }

    /// The fixtures carry no comment marker of their own: a `//` in a string literal here would
    /// be read as a comment by the scan that reads this very file, and the gate would report
    /// itself. `start` says where the comment begins instead.
    fn fires(text: &str) -> bool {
        !refused(text, 0, &names()).is_empty()
    }

    #[test]
    fn a_pinned_name_beside_a_version_is_refused() {
        assert!(fires("Pinned `tower-http` 0.7.0 implements it as"));
        assert!(fires("MEASURED against prek 0.4.14 rather than taken from its documentation"));
        assert!(fires("a real `DuckDB` 1.5.5 answers it with"));
        assert!(fires("the mimalloc 3.5.0 win. Enumerated rather"));
    }

    #[test]
    fn a_measured_figure_with_no_pinned_name_is_content() {
        // The class that defeats a shape rule outright: a wrong answer and a column total, in a
        // refusal narrative, numerically identical to a version.
        assert!(!fires("this group answered `1.5` for a column totalling `101.5`"));
        assert!(!fires(
            "that cell passed in 5.37 s, because its ceiling was the fake's own cap"
        ));
        assert!(!fires(
            "RFC 9110 section 11.1 makes an authentication scheme name insensitive"
        ));
        assert!(!fires("`0.1_f32` as an `f64` prints as 0.10000000149011612"));
    }

    #[test]
    fn something_version_shaped_that_is_not_a_version_is_not_one() {
        // Every class measured in this tree, and `inversion` for the substring that is not a
        // token at all.
        assert!(!is_version("2026-09-12"), "a bare date is a date");
        assert!(!is_version("0.17s"), "a timing carries its unit");
        assert!(!is_version("127.0.0.1"), "a fourth field is an address");
        assert!(!is_version("10.0.0.7"));
        assert!(!is_version("1000"), "a bare integer is a count");
        assert!(
            !is_version("v17"),
            "a bare `v<n>` is a route or a generation as often as a version"
        );
        assert!(is_version("v1.7.0"), "a `v` on a dotted version is still one");
        assert!(!is_version("inversion"));
        assert!(!is_version("ADR-0009"));
        assert!(!is_version("3x32-core"));
        assert!(!is_version(""));
        assert!(is_version("0.9.143"));
        assert!(is_version("1.98"));
        assert!(is_version("nightly-2026-09-08"));
        assert!(!is_version("nightly-2026-09"), "a short date is not the channel");
    }

    #[test]
    fn an_address_family_or_a_route_is_not_refused() {
        // `v4`, `v6`, `v1` and `v2` are the shapes a bare `v<n>` actually has in this tree.
        assert!(!fires("a v4 block does not contain a v6 address, and the canonical"));
        assert!(!fires("A `v2` does not delegate to a `v1`"));
        // THE MEASURED FALSE POSITIVE, and the reason a bare `v<n>` is not a version: a route
        // prefix written directly after the name of something this repo pins.
        assert!(!fires(
            "Measured 2026-09-12 on the duckdb v1 route, an inversion of the prefix"
        ));
        // And a `v` on a DOTTED version is still refused, so dropping the bare shape did not
        // drop the prefix with it.
        assert!(fires("the pinned duckdb v1.7.0 answers it"));
    }

    #[test]
    fn punctuation_between_the_name_and_the_version_breaks_adjacency() {
        // A Markdown table row of a crate name and a timing, which is the false positive a
        // two-token window produced here.
        assert!(!fires("| `duckdb` | 11.1 s | 50.8 s |"));
        // And the sentence's own punctuation around the version does NOT break it.
        assert!(fires("the pinned `tower-http` **0.7.0**, which seeds it from"));
        assert!(fires("Measured on prek 0.4.14:"));
        assert!(fires("`prek` 0.4.14's REAL commit-stage output"));
    }

    #[test]
    fn the_compiler_channel_needs_no_name_beside_it() {
        // The one shape that cannot be anything else, so it is refused on its own - which is
        // what carries the old rule's only subject into this one.
        let empty = BTreeSet::new();
        let found = refused("the pin is at nightly-2026-09-08 - authoritative rather than", 0, &empty);
        assert_eq!(found, vec![("nightly-2026-09-08", "the compiler pin")]);
    }

    #[test]
    fn only_the_comment_is_read() {
        // Just past the marker: `//` starts at 4, so the comment starts at 6.
        assert_eq!(comment_start("x.rs", "    // the pinned hook runner prints"), Some(6));
        assert_eq!(comment_start("x.rs", "let url = \"https://example.com/prek/0.4.14\";"), None);
        assert_eq!(comment_start("x.yml", "  # prek 0.4.14"), Some(3));
        assert!(comment_start("x.yml", "    uses: a/b@1234567 # v7.0.1").is_some());
        assert_eq!(comment_start("x.toml", "prek = \">=0.4.14,<0.5\""), None);
        assert_eq!(comment_start("x.md", "prek 0.4.14 prints"), Some(0));
        assert_eq!(comment_start("x.lock", "prek 0.4.14"), None);
    }

    #[test]
    fn a_trailing_tag_on_a_pinned_action_is_not_adjacent_to_it() {
        // Dependabot rewrites that comment when it moves the SHA, which is the mechanism this
        // check defers to - and `#` breaking adjacency is what makes that automatic rather than
        // an exemption. Stated as a test so a widening that reaches it is a red, not a surprise.
        let names: BTreeSet<String> = BTreeSet::from([String::from("cachix")]);
        // A DOTTED tag, deliberately: `# v17` is not a version token at all now, so a bare one
        // would make this test pass without exercising the adjacency break it is about.
        let line = "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v7.0.1";
        let start = comment_start("ci.yml", line).expect("the trailing comment");
        assert_eq!(refused(line, start, &names), Vec::new());
    }

    #[test]
    fn a_possessive_and_a_backtick_are_not_part_of_the_token() {
        assert_eq!(bare("`tower-http`"), "tower-http");
        assert_eq!(bare("0.9.143's"), "0.9.143");
        assert_eq!(bare("**0.7.0**,"), "0.7.0");
        assert_eq!(bare("(v7.0.1)"), "v7.0.1");
        assert_eq!(bare("#"), "");
    }

    #[test]
    fn a_version_outside_the_comment_is_not_read() {
        // The boundary itself, and the reason it is a test rather than only a probe: `refused`
        // takes the comment's offset, and ignoring it would read a version out of a Rust STRING
        // as though someone had written it in prose.
        let line = "const PROBE: &str = \"duckdb 9.9.9\";";
        assert_eq!(refused(line, line.len(), &names()), Vec::new());
        // And with the offset at the front, the same line IS refused - so the empty answer above
        // is the offset working rather than the walk failing to find anything.
        assert_eq!(refused(line, 0, &names()), vec![("9.9.9", "duckdb")]);
    }

    #[test]
    fn a_harvest_that_reads_nothing_refuses_itself() {
        // THE FLOOR'S REFUSAL, not just its predicate. Over a directory with no manifest in it
        // the harvest finds nothing to key on, so every real instance would pass - which is the
        // failure mode `check-pins` records for a text scan and the one a green run hides.
        let dir = std::env::temp_dir().join(format!("sutura-versions-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        std::fs::create_dir_all(&dir).expect("a fixture directory");
        let (problems, scan) = super::comment_versions(&dir, &[]);
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        assert_eq!(scan.comments, 0, "no file was offered, so no comment was read");
        assert!(
            problems.iter().any(|p| p.contains("the harvest is broken, not the tree")),
            "an empty name set must refuse itself: {problems:#?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("the scan stopped reading")),
            "a scan over no comments must refuse itself: {problems:#?}"
        );
    }

    #[test]
    fn the_tree_writes_no_version_a_mechanism_does_not_compare() {
        // The assertion the sweep earns, and the one that is RED against a tree that still
        // carries the copies. Over the real repo, for the reason the pin check's own tree-wide
        // test existed: a rule nobody runs over the tree is a rule about a fixture.
        let (root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not list the repo");
        let (problems, scan) = super::comment_versions(&root, &files);
        assert!(
            problems.is_empty(),
            "{} version(s) written where nothing compares them:\n{}",
            problems.len(),
            problems.join("\n")
        );
        assert!(scan.names >= super::NAME_FLOOR, "the name harvest read {} names", scan.names);
        assert!(
            scan.comments >= super::COMMENT_FLOOR,
            "the scan read {} comment lines",
            scan.comments
        );
    }
}

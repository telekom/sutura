//! Has a vendored crate's reason expired? `devco/vendor-expiry` names the version each
//! `vendor/` child was copied at; this asks `index.crates.io` whether a newer one has shipped.
//!
//! # WHY IT IS NOT A HYGIENE GATE
//!
//! It needs egress. `just validate`'s nix checks are hermetic and must stay that way, and a gate
//! that reddens whichever branch happens to be open at the moment upstream publishes is worse
//! than the rot it closes - the finding is about the repository, not about that diff. So this is
//! `Kind::Standalone`, invoked by `just vendor-expiry` and by `just update`, which is the moment
//! somebody is already deciding about versions. The hermetic half - *this file names every
//! vendored child and nothing else* - lives in `check-workflows`, beside the rules that already
//! require a `REUSE.toml` block and a `VENDOR.md` record for the same child.
//!
//! # THE SPARSE INDEX, NOT THE API
//!
//! `crates.io/api/v1` answers 403 without a user agent, measured. `index.crates.io` answers 200
//! and returns one JSON object per line, oldest publish first. Fetched with `curl` rather than an
//! HTTP crate: the parse and the comparison are what can be wrong, and both are pure functions
//! below with tests; adding a client to `xtask` to save one `Command` would put a TLS stack in the
//! gate tree for nothing.
//!
//! # WHAT IT DOES NOT SAY
//!
//! A newer release is not proof the reason expired - it says re-read the row. **No newer release
//! is not proof the reason still holds**, which is the weaker direction and the one worth naming:
//! upstream can abandon a fix, or ship it under a scheme [`Release`] refuses to compare. A
//! prerelease or a build-metadata suffix is skipped rather than ordered, so a fix that exists only
//! in `0.6.0-rc.1` reads as nothing published.
//!
//! # WHAT IT DOES NOT COVER
//!
//! Vendored CRATES only, because the subject is a registry release. The one known patched
//! third-party thing that is not a crate stays outside this gate by construction:
//! `nix/actionlint.nix` pins a commit and carries no version at all, so [`Release`] could not
//! represent it even if it were given a row - and nothing else watches it either, which is the
//! gap this gate does NOT close. A second source kind, a GitHub release tag, is where it would go,
//! and it is not built. [`DECLARATION`]'s own "WHAT NEITHER COVERS" paragraph states the same
//! exclusion; an earlier version of this note claimed it was *"said in both headers"* while
//! neither header actually named it, which is the shape of overstatement this repo treats as the
//! defect rather than the omission.

use std::process::Command;

use crate::{Verdict, repo};

/// The declaration. Its own header carries the argument; this is the only reader.
pub(crate) const DECLARATION: &str = "devco/vendor-expiry";

/// A crate name that can be put in a URL path.
///
/// **The refusal is the point, not the parse.** A row is hand-written text that becomes a path
/// segment, so `../../` in that position would walk the index host's URL space, and a name with a
/// `?` or a newline would splice the request. crates.io itself allows only ASCII alphanumerics,
/// `-` and `_`, so nothing legitimate is lost by making that the type's invariant rather than a
/// check somebody remembers at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrateName(String);

impl CrateName {
    /// Parse, or say which character refused it.
    fn parse(raw: &str) -> Result<Self, RowError> {
        if raw.is_empty() {
            return Err(RowError::EmptyCrate);
        }
        raw.chars()
            .find(|c| !c.is_ascii_alphanumeric() && *c != '-' && *c != '_')
            .map_or_else(
                || Ok(Self(String::from(raw))),
                |bad| {
                    Err(RowError::CrateCharacter {
                        name: String::from(raw),
                        bad,
                    })
                },
            )
    }

    /// The name as declared, so neither a report nor a test reaches into the newtype's field.
    /// The anchor test asserts on the CRATE rather than the `vendor/` child, because two crates
    /// share one child here and the child cannot witness both.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Where the sparse index keeps this crate, per cargo's own bucketing rule: one directory for
    /// a one- or two-character name, `3/<first>` for three, and `<first two>/<next two>`
    /// otherwise. Lowercased, because the index is.
    fn index_path(&self) -> String {
        let lower = self.0.to_lowercase();
        let chars: Vec<char> = lower.chars().collect();
        let head = |from: usize, len: usize| -> String { chars.iter().skip(from).take(len).collect() };
        match chars.len() {
            1 => format!("1/{lower}"),
            2 => format!("2/{lower}"),
            3 => format!("3/{}/{lower}", head(0, 1)),
            _ => format!("{}/{}/{lower}", head(0, 2), head(2, 2)),
        }
    }
}

/// A release this gate is willing to ORDER: dot-separated decimal components and nothing else.
///
/// Refusing a prerelease rather than ranking it is deliberate. `0.6.0-rc.1` sorts below `0.6.0`
/// under semver and above it under a naive string compare, and getting that backwards in the
/// permissive direction would announce an expiry that has not happened. A gate that cries wolf
/// gets disabled, so the unorderable case is skipped and the module header says so.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Release(Vec<u64>);

impl Release {
    fn parse(raw: &str) -> Option<Self> {
        let parts: Option<Vec<u64>> = raw.split('.').map(|part| part.parse::<u64>().ok()).collect();
        parts.filter(|parts| !parts.is_empty()).map(Self)
    }

    /// The version as written, so a report quotes a version rather than a vector.
    fn render(&self) -> String {
        self.0.iter().map(u64::to_string).collect::<Vec<String>>().join(".")
    }
}

/// What a malformed row is, so the report can name the fault instead of the line.
#[derive(Debug, PartialEq, Eq)]
enum RowError {
    /// Fewer than the five positions the header declares.
    Fields {
        found: usize,
    },
    EmptyCrate,
    CrateCharacter {
        name: String,
        bad: char,
    },
    /// The version position is there but not orderable.
    Version {
        raw: String,
    },
}

impl std::fmt::Display for RowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fields { found } => write!(
                f,
                "holds {found} field(s), not the five the header declares (<vendor/ child> <crate> <version> <ISO date> <what a newer release means>) - all five or it is not a decision"
            ),
            Self::EmptyCrate => write!(f, "names no crate"),
            Self::CrateCharacter { name, bad } => write!(
                f,
                "names the crate `{name}`, which holds {bad:?} - a crate name becomes a path segment in the index URL, and only ASCII alphanumerics, `-` and `_` are accepted"
            ),
            Self::Version { raw } => write!(
                f,
                "states the version `{raw}`, which is not dot-separated decimals - a prerelease or a build suffix is refused rather than ordered, because ordering it wrong in the permissive direction announces an expiry that has not happened"
            ),
        }
    }
}

/// One declared vendored crate.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// One-based line in [`DECLARATION`], so a report can send a reader to it.
    pub(crate) line: usize,
    /// The immediate child of `vendor/` this crate's source sits under.
    pub(crate) child: String,
    pub(crate) krate: CrateName,
    pub(crate) vendored: Release,
}

impl Row {
    fn parse(line: usize, text: &str) -> Result<Self, RowError> {
        let mut fields = text.split_whitespace();
        let (Some(child), Some(krate), Some(version), Some(_date), Some(_reason)) =
            (fields.next(), fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(RowError::Fields {
                found: text.split_whitespace().count(),
            });
        };
        Ok(Self {
            line,
            child: String::from(child),
            krate: CrateName::parse(krate)?,
            vendored: Release::parse(version).ok_or_else(|| RowError::Version {
                raw: String::from(version),
            })?,
        })
    }
}

/// What reading the declaration yields: the rows that parsed, and the faults of those that did not.
///
/// A named type rather than a tuple because both halves are consumed by two readers - `run` here
/// and `check-workflows`' completeness rule - and a pair of `Vec`s at a call site says nothing
/// about which is which.
pub(crate) struct Declared {
    pub(crate) rows: Vec<Row>,
    pub(crate) malformed: Vec<String>,
}

/// Every row in `text`, with the malformed ones kept as their own fault.
///
/// `pub(crate)` because `check-workflows` reads the same declaration for the hermetic half. One
/// parser, so the two halves cannot disagree about what a row is.
pub(crate) fn rows(text: &str) -> Declared {
    let mut parsed = Vec::new();
    let mut problems = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match Row::parse(line, trimmed) {
            Ok(row) => parsed.push(row),
            Err(error) => problems.push(format!("{DECLARATION}:{line}: {error}")),
        }
    }
    Declared {
        rows: parsed,
        malformed: problems,
    }
}

/// The newest orderable, non-yanked release in a sparse-index response body.
///
/// Reads every line rather than taking the last: the index is in publish order, and a yanked or
/// prerelease tail would otherwise be read as the newest thing available.
fn newest(body: &str) -> Option<Release> {
    body.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| entry.get("yanked").and_then(serde_json::Value::as_bool) != Some(true))
        .filter_map(|entry| entry.get("vers").and_then(serde_json::Value::as_str).and_then(Release::parse))
        .max()
}

/// Ask the index. `Err` is a fetch that did not happen, which is never a pass.
fn fetch(krate: &CrateName) -> Result<String, String> {
    let url = format!("https://index.crates.io/{}", krate.index_path());
    let output = Command::new("curl")
        .args(["--silent", "--show-error", "--fail", "--location", "--max-time", "30", &url])
        .output()
        .map_err(|error| format!("could not run curl for {url}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl failed for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("{url} returned non-UTF-8: {error}"))
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    const NAME: &str = "xtask check-vendor-expiry";

    let Some(root) = repo::root() else {
        eprintln!("{NAME}: FAILED - could not determine the repo root");
        return Verdict::Fail;
    };
    let Ok(text) = std::fs::read_to_string(root.join(DECLARATION)) else {
        // FAIL CLOSED. An absent declaration is not "no vendored crate to check": `vendor/` is
        // recorded, load-bearing third-party source, and a reader that finds nothing must not be
        // how the rule goes unchecked again.
        eprintln!(
            "{NAME}: FAILED - could not read {DECLARATION}, so whether a vendored crate's reason has expired is unread rather than clean"
        );
        return Verdict::Fail;
    };

    let Declared {
        rows,
        malformed: mut problems,
    } = rows(&text);
    if rows.is_empty() && problems.is_empty() {
        eprintln!(
            "{NAME}: FAILED - {DECLARATION} declares no vendored crate; an empty declaration is a file that should have been deleted, not a clean bill"
        );
        return Verdict::Fail;
    }

    let mut checked = 0_usize;
    for row in &rows {
        match fetch(&row.krate) {
            Ok(body) => match newest(&body) {
                Some(published) if published > row.vendored => problems.push(format!(
                    "{DECLARATION}:{}: `{}` is vendored at {} and crates.io now publishes {} - re-read the row's reason and either retire vendor/{} or re-vendor on the new release",
                    row.line,
                    row.krate.as_str(),
                    row.vendored.render(),
                    published.render(),
                    row.child,
                )),
                Some(_) => checked += 1,
                None => problems.push(format!(
                    "{DECLARATION}:{}: the index returned no orderable, non-yanked release for `{}` - that is an input this gate could not read, not a crate with nothing newer",
                    row.line, row.krate.as_str()
                )),
            },
            Err(error) => problems.push(format!("{DECLARATION}:{}: {error}", row.line)),
        }
    }

    if !problems.is_empty() {
        eprintln!(
            "{NAME}: FAILED - {} finding(s) over {} declared vendored crate(s)",
            problems.len(),
            rows.len()
        );
        for problem in &problems {
            eprintln!("  {problem}");
        }
        return Verdict::Fail;
    }
    // NAMES WHAT IT READ, as a pair from two places: one number cannot witness itself, and a
    // broken row filter and an empty declaration both read zero.
    println!(
        "{NAME}: ok - {checked} of {} declared vendored crate(s) still at the newest published release",
        rows.len()
    );
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::{CrateName, DECLARATION, Release, Row, RowError, newest, rows};

    #[test]
    fn the_index_bucket_follows_cargos_own_rule() {
        let path = |name: &str| CrateName::parse(name).expect("a legal crate name").index_path();
        assert_eq!(path("a"), "1/a");
        assert_eq!(path("ab"), "2/ab");
        assert_eq!(path("abc"), "3/a/abc");
        assert_eq!(path("libmimalloc-sys"), "li/bm/libmimalloc-sys");
        assert_eq!(path("Mimalloc"), "mi/ma/mimalloc", "the index is lowercase");
    }

    #[test]
    fn a_crate_name_that_would_escape_the_index_path_is_refused() {
        // THE REFUSAL, not the predicate: `../../` in the row's crate position becomes a path
        // segment in the URL this gate fetches. Neutralising the character filter with `&& false`
        // while keeping the field read turns this cell red; `dead_code` sees nothing.
        assert_eq!(
            CrateName::parse(".."),
            Err(RowError::CrateCharacter {
                name: String::from(".."),
                bad: '.'
            })
        );
        assert!(matches!(CrateName::parse("a/b"), Err(RowError::CrateCharacter { .. })));
        assert!(matches!(CrateName::parse("a?b"), Err(RowError::CrateCharacter { .. })));
        assert_eq!(CrateName::parse(""), Err(RowError::EmptyCrate));
    }

    #[test]
    fn a_release_orders_by_component_and_not_by_string() {
        let release = |raw: &str| Release::parse(raw).expect("an orderable release");
        assert!(release("0.1.10") > release("0.1.9"), "a string compare gets this backwards");
        assert!(release("0.5.6") > release("0.5.5"));
        assert_eq!(release("0.5.6").render(), "0.5.6");
    }

    #[test]
    fn a_version_this_gate_cannot_order_is_refused_rather_than_guessed() {
        assert_eq!(Release::parse("0.6.0-rc.1"), None);
        assert_eq!(Release::parse("1.0.0+build"), None);
        assert_eq!(Release::parse(""), None);
        assert_eq!(Release::parse("v1.0"), None);
    }

    #[test]
    fn a_row_needs_all_five_positions() {
        assert_eq!(
            Row::parse(7, "child crate 1.0.0 2026-01-01"),
            Err(RowError::Fields { found: 4 })
        );
        let row = Row::parse(7, "child crate 1.0.0 2026-01-01 because upstream has not shipped it").expect("a row");
        assert_eq!(row.line, 7);
        assert_eq!(row.child, "child");
        assert_eq!(row.vendored, Release::parse("1.0.0").expect("the version"));
    }

    #[test]
    fn comments_and_blank_lines_are_not_rows_and_a_malformed_one_is_kept_as_a_fault() {
        let read = rows("# a comment\n\n  \nchild crate 1.0.0 2026-01-01 why\nbroken row\n");
        assert_eq!(read.rows.len(), 1);
        let malformed = read.malformed;
        assert_eq!(malformed.len(), 1);
        assert!(
            malformed.iter().any(|p| p.contains(&format!("{DECLARATION}:5"))),
            "{malformed:?}"
        );
    }

    #[test]
    fn the_newest_release_skips_a_yanked_and_an_unorderable_tail() {
        let body = concat!(
            "{\"name\":\"c\",\"vers\":\"0.5.5\",\"yanked\":false}\n",
            "{\"name\":\"c\",\"vers\":\"0.5.6\",\"yanked\":false}\n",
            "{\"name\":\"c\",\"vers\":\"0.5.7\",\"yanked\":true}\n",
            "{\"name\":\"c\",\"vers\":\"0.6.0-rc.1\",\"yanked\":false}\n",
        );
        assert_eq!(newest(body), Release::parse("0.5.6"));
        assert_eq!(newest(""), None, "an empty body is not a crate with nothing newer");
    }

    #[test]
    fn this_repos_declaration_parses_and_names_every_vendored_child() {
        // An anchor over the real file rather than a fixture, so a row added by hand in the shape
        // a reader guesses at fails here rather than at the next `just update`.
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(DECLARATION)).expect("the declaration");
        let read = rows(&text);
        assert_eq!(read.malformed, Vec::<String>::new());
        // BOTH crates under the one vendored child, because they version separately: a row for
        // the wrapper alone would miss a sys-only release, and asserting on the CHILD name only
        // would not notice if one of the two rows were dropped.
        for krate in ["mimalloc", "libmimalloc-sys"] {
            assert!(
                read.rows.iter().any(|row| row.krate.as_str() == krate),
                "`{krate}` is not declared in {DECLARATION}"
            );
        }
        assert!(
            read.rows.iter().all(|row| row.child == "mimalloc_rust"),
            "a row names a vendored child this repo no longer has"
        );
    }
}

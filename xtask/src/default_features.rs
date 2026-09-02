//! Every package that SHIPS compiles and lints at the feature set it ships with.
//!
//! **The lane no other gate can see, and it was found by shipping a red branch through it.** Every
//! compiling gate in this repository passes `--all-features`: `just lint`, `just test`,
//! `just check-changed` and the `clippy` and `nextest` nix checks alike. `nix/shipped.nix`, though,
//! builds each published binary with cargo's DEFAULT features - `--package` and `--target`, no
//! `--features` - because a non-optional networked adapter would cross-compile `ureq`, rustls and
//! `ring` for two musl triples for a binary that links none of them. So a `#[cfg(feature = "...")]`
//! that a developer only ever compiled with the feature ON can be a hard error in the exact
//! configuration a release publishes, and every gate a developer runs before pushing is green.
//!
//! That happened: `sutura-serve`'s boot pre-flight landed with three items reachable only from a
//! `bigquery` arm, `dead_code = "deny"` made all three errors with the feature off, and it reached
//! review as a green branch. The four `cross` link checks WOULD have caught it - they build
//! `.#sutura-serve-<triple>-ci` at the default set on every pull request - but they are
//! `needs: [ci]`, and `ci` had failed on something else, so they never ran. That is a sequencing
//! fact rather than coverage, and a gate a developer can run is the fix for it. The same review then
//! measured the LINT half of the same lane still red on a pre-existing `doc_markdown` in a
//! `#[cfg(not(feature = "bigquery"))]` doc comment, which is why this runs clippy as well as check:
//! the two commands see different code, and neither is `just lint`'s.
//!
//! **NOT a hygiene gate, for `check-attribution-current`'s reason.** It invokes cargo, so it needs a
//! resolvable registry and a target directory; the nix sandbox `hygiene` runs in has neither. So it
//! lives in `just gates`, which is where every gate that shells out to cargo lives.
//!
//! **What CI runs is still nothing, and that is the limit to read before trusting a green run here.**
//! `ci.yml` reaches every gate as a `nix build .#checks.*` or a `nix run .#<app>`, so this one wants
//! an app warmed the way `apps.causality` is - otherwise it compiles the dependency graph a third
//! time in the same job. Until then the developer lane is gated and the CI lane is not: what CI has
//! is the four `cross` link builds for the COMPILE half and nothing at all for the LINT half.
//!
//! **The package list is DERIVED and not written here**, which is the single-owner rule: it is every
//! `package = "..."` inside `nix/shipped.nix`'s `binaries` list, the same declaration
//! `check-shipped-binaries` compares the release literals against. A binary added there is covered by
//! this gate without anybody remembering to add it, and a package renamed in one place fails rather
//! than silently dropping out. FAIL CLOSED on parsing none, for that gate's own stated reason: a
//! parser that silently sees half a file is worse than no parser.
//!
//! **What it does NOT do**, stated because a green run invites the wider reading: it compiles the
//! default set only. A feature declared and never compiled by anything is still uncovered here - the
//! `--all-features` gates are what reach those - and this says nothing about a package that does not
//! ship. Nor does it link: `cargo check` and `cargo clippy` both stop at metadata, which is what
//! keeps it affordable and is also why the `cross` builds stay the authority on a musl link.

use crate::Verdict;
use crate::repo;

/// The declaration the package list is read out of.
const SOURCE: &str = "nix/shipped.nix";

/// Every `package = "..."` inside `nix/shipped.nix`'s `binaries = [ ... ]`, in declaration order.
///
/// Scoped to that list rather than grepping the file, and found anywhere on the line rather than at
/// its start - both for the reasons `shipped::declared` states at length beside its own parser: the
/// key appears elsewhere in that file, and `{ bin = "sutura"; package = "sutura-cli"; }` is one legal
/// record on one line. Duplicates are dropped, keeping first appearance, so two binaries out of one
/// package are one compile rather than two.
fn shipped_packages(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(open) = indent else {
            if trimmed.starts_with("binaries = [") {
                indent = Some(line.len().saturating_sub(line.trim_start().len()));
            }
            continue;
        };
        if trimmed == "];" && line.len().saturating_sub(line.trim_start().len()) == open {
            break;
        }
        for found in packages_in(line) {
            let owned = String::from(found);
            if !names.contains(&owned) {
                names.push(owned);
            }
        }
    }
    names
}

/// Every `package = "..."` value on one line, left to right.
///
/// The character before the key must not be part of a name, so a hypothetical `subPackage = "x"` is
/// not read as one. An unterminated quote yields nothing rather than the rest of the file.
fn packages_in(line: &str) -> impl Iterator<Item = &str> {
    const KEY: &str = "package = \"";
    let mut rest = line;
    core::iter::from_fn(move || {
        loop {
            let at = rest.find(KEY)?;
            let is_key = rest
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');
            let tail = rest.get(at.saturating_add(KEY.len())..)?;
            let end = tail.find('"')?;
            let value = tail.get(..end)?;
            rest = tail.get(end.saturating_add(1)..)?;
            if is_key && !value.is_empty() {
                return Some(value);
            }
        }
    })
}

/// One cargo invocation, named for the question it answers.
struct Pass {
    /// What this pass is called in the output.
    what: &'static str,
    /// The cargo subcommand and its arguments, before `--package`.
    lead: &'static [&'static str],
    /// Arguments after `--package`, which is where a lint level goes.
    tail: &'static [&'static str],
}

/// The two passes, and they are two because they see different code.
///
/// `check` answers *does the shipped configuration compile*; clippy answers *is it clean under this
/// workspace's lint set*, which includes the whole `restriction` category and `-D warnings`. A
/// `doc_markdown` on a `#[cfg(not(feature = ...))]` item is invisible to the first and to every
/// `--all-features` run, which is the measured case that put both here.
const PASSES: &[Pass] = &[
    Pass {
        what: "check",
        lead: &["check", "--all-targets"],
        tail: &[],
    },
    Pass {
        what: "clippy",
        lead: &["clippy", "--all-targets"],
        tail: &["--", "-D", "warnings"],
    },
];

/// `cargo xtask check-default-features` - the shipped feature set compiles and lints.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-default-features: could not determine the repo root");
        return Verdict::Fail;
    };
    let path = root.join(SOURCE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-default-features: could not read {}: {error}", path.display());
            return Verdict::Fail;
        }
    };
    let packages = shipped_packages(&text);
    if packages.is_empty() {
        eprintln!("xtask check-default-features: FAILED - parsed no package out of {SOURCE}");
        eprintln!("  A list this gate reads as empty checks nothing and passes, which is the one");
        eprintln!("  failure it must not have. `binaries = [` and `package = \"...\";` are the two");
        eprintln!("  shapes it looks for.");
        return Verdict::Fail;
    }
    println!(
        "xtask check-default-features: {} shipped package(s) from {SOURCE}: {}",
        packages.len(),
        packages.join(", ")
    );
    println!("  cargo's DEFAULT feature set - the one `nix/shipped.nix` publishes and no other gate compiles.");
    let mut failed: Vec<String> = Vec::new();
    for package in &packages {
        for pass in PASSES {
            println!("\n=== {} {package} ===", pass.what);
            let mut command = std::process::Command::new("cargo");
            command
                .current_dir(&root)
                .args(pass.lead)
                .args(["--package", package])
                .args(pass.tail);
            match command.status() {
                Ok(status) if status.success() => {}
                Ok(_) => failed.push(format!("{} {package}", pass.what)),
                Err(error) => {
                    eprintln!("xtask check-default-features: could not run cargo: {error}");
                    return Verdict::Fail;
                }
            }
        }
    }
    if failed.is_empty() {
        println!(
            "\nxtask check-default-features: ok - {} package(s) compile and lint at their default features",
            packages.len()
        );
        return Verdict::Pass;
    }
    eprintln!(
        "\nxtask check-default-features: FAILED - {}: {}",
        failed.len(),
        failed.join(", ")
    );
    eprintln!("  `just lint` and `just test` pass --all-features and cannot see this, and neither can");
    eprintln!("  `just check-changed`. What ships is what this compiled.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::shipped_packages;

    #[test]
    fn every_package_in_the_binaries_list_is_read_in_declaration_order() {
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "    }\n",
            "    {\n",
            "      bin = \"sutura-serve\";\n",
            "      package = \"sutura-serve\";\n",
            "    }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli", "sutura-serve"]);
    }

    #[test]
    fn a_record_written_on_one_line_is_still_a_record() {
        // The failure `shipped::declared`'s own tests forced: `nixpkgs-fmt`'s shape is not the only
        // legal one, and a parser that reads fewer packages than are declared passes by checking
        // less - which is the one failure mode this gate may not have.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_longer_key_ending_in_package_is_not_a_package() {
        let nix = "  binaries = [\n    { subPackage = \"decoy\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_package_outside_the_list_is_not_a_declaration() {
        // The scoping reason: this key appears elsewhere in that file, and a whole-file grep would
        // compile packages nothing publishes.
        let nix = concat!(
            "  someOther = { package = \"not-shipped\"; };\n",
            "  binaries = [\n",
            "    { package = \"sutura-cli\"; }\n",
            "  ];\n",
            "  after = { package = \"also-not-shipped\"; };\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn two_binaries_out_of_one_package_are_one_compile() {
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"one\"; package = \"sutura-cli\"; }\n",
            "    { bin = \"two\"; package = \"sutura-cli\"; }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_list_this_parser_cannot_find_reads_as_empty_so_the_gate_can_fail_closed() {
        // `run` turns this into a FAILURE rather than a pass, which is the whole of why the parser
        // is allowed to answer nothing.
        assert!(shipped_packages("nothing that looks like a binaries list").is_empty());
    }
}

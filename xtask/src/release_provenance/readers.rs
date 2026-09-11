//! The guard that `image-digests.txt` keeps ONE reader: no consumer may re-split the grammar.
//!
//! The grammar itself lives in [`super::image_digests`]. What this module holds is that a *new*
//! consumer cannot quietly re-derive it - the class `telekom/sutura#598` names, where five shell
//! readers survived only because `grep '^leaf '` skips a comment header as a side effect. A seventh
//! reader that iterates every line was a green pull request; it is a refusal here.
//!
//! **Fail closed, and over a walk that cannot be narrowed.** The walk is [`repo::all_files`] and the
//! loop is [`repo::Census::inspect`]'s, so a consumer surface that is missing, unreadable or empty
//! is a refusal rather than a pass over silence - and the [`repo::Scope`] is a bare `fn` with
//! nothing captured, so it cannot count its subjects or react to a failed read. The parser of record
//! is the walk's anchor, so deleting it is a refusal rather than an exemption nobody claims.
//!
//! # The limit, next to the claim
//!
//! **This holds DELEGATION, not correctness.** It cannot prove a shell reader parses a record
//! correctly - only that no consumer re-implements the split; grammar correctness is
//! [`super::image_digests`]'s tests. It reaches the surfaces declared in [`in_scope`]: the
//! workflows and actions under `.github/`, the shared shell under `nix/`, and the Rust under
//! `xtask/src/`. A naive reader in another crate is outside it, and nothing implausible reads a
//! release artefact from one. The needles are [lexical](https://www.howtocodeit.com) text, so a
//! reader that splits the file through an unlisted spelling is a review finding rather than a
//! refusal - stated here rather than implied by a green line.

use crate::Verdict;
use crate::repo;

/// The parser of record: the one file allowed to split the grammar. It is the walk's anchor, so a
/// deleted parser is [`repo::Refusal::NotJudged`] rather than an exemption nothing exercises.
const PARSER: &str = "xtask/src/release_provenance/image_digests.rs";

/// This module defines the rule and holds the needles; it is not a consumer.
const GUARD: &str = "xtask/src/release_provenance/readers.rs";

/// The collector reads `image-digests.txt` and must reach its records through the parser, so a
/// field split in this file is a re-implementation whatever else it names.
const COLLECTOR: &str = "xtask/src/release_provenance.rs";

/// The surfaces a consumer of `image-digests.txt` can live in, each judged whole.
const SURFACES: &[&str] = &[".github/workflows/", ".github/actions/", "nix/", "xtask/src/"];

/// The shell spellings of the digests input: the step env var, the action input, and the path.
const INPUTS: &[&str] = &["IMAGE_DIGESTS", "image-digests.txt", "digests-file"];

/// The single delegation: the parser of record invoked as a task. A line carrying both a
/// [`INPUTS`] spelling and this is reaching the records through the one reader, not around it.
const DELEGATION: &str = "image-digests ";

/// The shell constructs that take fields out of a line, or that feed the file to something which
/// will. Each is a way a consumer could re-derive the split instead of asking
/// [`super::image_digests`].
const SHELL_SPLITS: &[&str] = &[
    "grep",
    "awk",
    "sed",
    "cut",
    "while read",
    "read -r",
    "mapfile",
    "IFS=",
    "done <",
    "$(",
];

/// The Rust constructs that split a line into fields.
const RUST_SPLITS: &[&str] = &["split_whitespace(", "split_ascii_whitespace(", "splitn(", "split(' ')"];

/// Is `rel` a file this guard judges?
///
/// A bare `fn`, so it is a [`repo::Scope`] with nowhere to keep a counter and no content to react
/// to - the two narrowings [`repo::Census`] exists to make unexpressible.
pub(crate) fn in_scope(rel: &str) -> bool {
    SURFACES.iter().any(|surface| rel.starts_with(surface))
}

/// Walk every consumer surface and refuse a file that re-splits the grammar.
pub(crate) fn enforce() -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask image-digests: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let mut findings: Vec<String> = Vec::new();
    let mut judged = 0_usize;
    let scope: repo::Scope = in_scope;
    let anchored = census.inspect(&[PARSER], scope, |rel, bytes| {
        judged = judged.saturating_add(1);
        if rel == PARSER || rel == GUARD {
            return;
        }
        let text = String::from_utf8_lossy(bytes);
        findings.extend(scan(rel, &text));
    });
    let counted = match anchored {
        Ok(counted) => counted,
        Err(why) => {
            eprintln!("xtask image-digests: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    if findings.is_empty() {
        println!(
            "xtask image-digests: ok - {judged} consumer subject(s) judged, none re-splits the \
             grammar; {}",
            counted.verdict()
        );
        return Verdict::Pass;
    }
    eprintln!(
        "xtask image-digests: FAILED - {} line(s) re-split the image-digests grammar",
        findings.len()
    );
    for finding in &findings {
        eprintln!("  {finding}");
    }
    eprintln!();
    eprintln!("A consumer reaches the records through `cargo xtask image-digests <file> <kind>`,");
    eprintln!("never by grep/awk/read over the file itself.");
    Verdict::Fail
}

/// Every line of `rel` that re-derives the grammar, as a reader-facing sentence.
fn scan(rel: &str, text: &str) -> Vec<String> {
    match std::path::Path::new(rel).extension().and_then(std::ffi::OsStr::to_str) {
        Some("rs") => rust_findings(rel, text),
        Some("yml" | "yaml" | "sh") => shell_findings(rel, text),
        _ => Vec::new(),
    }
}

/// A shell or workflow line that mentions the digests input and splits it itself.
fn shell_findings(rel: &str, text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        // A `#` line is YAML or shell prose, not a reader.
        if raw.trim_start().starts_with('#') {
            continue;
        }
        if !INPUTS.iter().any(|needle| raw.contains(needle)) {
            continue;
        }
        if raw.contains(DELEGATION) {
            continue;
        }
        if SHELL_SPLITS.iter().any(|needle| raw.contains(needle)) {
            findings.push(describe(rel, index));
        }
    }
    findings
}

/// A Rust file that mentions the digests file in code - or the collector itself - and splits fields
/// anywhere in the file.
///
/// File-scoped rather than line-scoped because a Rust reader separates the read from the split: the
/// original defect read every line and split each one two statements later. `//` lines are skipped,
/// so a doc comment that merely names the file is not a reader.
fn rust_findings(rel: &str, text: &str) -> Vec<String> {
    // The collector is a declared consumer: its production code reaches the records through
    // `image_subjects`, and never spells the file's name, so a field split here is a
    // re-implementation whether or not it names the file.
    let mut mentions = rel == COLLECTOR;
    let mut splits = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        if raw.trim_start().starts_with("//") {
            continue;
        }
        if raw.contains("image-digests") {
            mentions = true;
        }
        if RUST_SPLITS.iter().any(|needle| raw.contains(needle)) {
            splits.push(describe(rel, index));
        }
    }
    if mentions { splits } else { Vec::new() }
}

/// One finding, naming the file and the line a reader opens.
fn describe(rel: &str, index: usize) -> String {
    let line = index.saturating_add(1);
    format!("{rel}:{line}: splits the image-digests grammar itself")
}

#[cfg(test)]
mod tests {
    use super::{in_scope, scan};

    #[test]
    fn a_shell_reader_that_re_splits_the_file_is_refused() {
        let filtered = "grep -E '^leaf ' \"$IMAGE_DIGESTS\" > leaves.txt\n";
        assert_eq!(scan(".github/workflows/release.yml", filtered).len(), 1);
        let looped = "while read -r kind name ref < \"$IMAGE_DIGESTS\"; do echo \"$ref\"; done\n";
        assert_eq!(scan(".github/workflows/release.yml", looped).len(), 1);
        let field = "ref=\"$(awk -v want=glibc '$1 == \"list\" && $2 == want { print $3 }' \"$IMAGE_DIGESTS\")\"\n";
        assert_eq!(scan(".github/actions/attest-and-sign/action.yml", field).len(), 1);
    }

    #[test]
    fn a_loop_that_feeds_the_file_to_read_is_refused_even_across_lines() {
        let reading = "while IFS= read -r line; do\n  printf '%s\\n' \"$line\"\ndone < \"$IMAGE_DIGESTS\"\n";
        assert_eq!(scan(".github/workflows/release.yml", reading).len(), 1);
    }

    #[test]
    fn delegating_and_non_parsing_uses_are_accepted() {
        let empty = Vec::<String>::new();
        let delegating = "nix run .#xtask -- image-digests \"$IMAGE_DIGESTS\" leaf > leaf.txt\n";
        assert_eq!(scan(".github/workflows/release.yml", delegating), empty);
        let subject = "raw=\"$(nix run .#xtask -- image-digests \"$IMAGE_DIGESTS\" subject glibc)\"\n";
        assert_eq!(scan(".github/actions/attest-and-sign/action.yml", subject), empty);
        let display = "cat dist/image-digests.txt\n";
        assert_eq!(scan(".github/workflows/release.yml", display), empty);
        let presence = "if [ -f dist/image-digests.txt ]; then\n";
        assert_eq!(scan(".github/workflows/release.yml", presence), empty);
        let comment = "# see image-digests.txt for the digests\n";
        assert_eq!(scan(".github/workflows/release.yml", comment), empty);
    }

    #[test]
    fn a_rust_reader_that_reads_then_splits_is_refused() {
        let reader = "let text = std::fs::read_to_string(\"image-digests.txt\")?;\n\
                      for line in text.lines() {\n\
                          let fields: Vec<_> = line.split_whitespace().collect();\n\
                      }\n";
        let findings = scan("xtask/src/example.rs", reader);
        assert_eq!(findings.len(), 1, "the split is named even when the read is a line above");
    }

    #[test]
    fn a_rust_file_that_does_not_mention_the_file_is_not_a_reader() {
        let unrelated = "let fields: Vec<_> = line.split_whitespace().collect();\n";
        assert_eq!(scan("xtask/src/example.rs", unrelated), Vec::<String>::new());
    }

    #[test]
    fn the_collector_must_not_split_even_without_naming_the_file() {
        let naive = "let fields: Vec<_> = line.split_whitespace().collect();\n";
        assert_eq!(scan("xtask/src/release_provenance.rs", naive).len(), 1);
    }

    #[test]
    fn the_surfaces_are_declared_rather_than_self_derived() {
        assert!(in_scope(".github/workflows/release.yml"));
        assert!(in_scope(".github/actions/attest-and-sign/action.yml"));
        assert!(in_scope("nix/run-gate.sh"));
        assert!(in_scope("xtask/src/release_provenance.rs"));
        assert!(!in_scope("crates/sutura-domain/src/lib.rs"));
    }
}

//! Does every reader of `image-digests.txt` anchor on the record kind? A gate that says it must.
//!
//! The release path writes one small file naming every image it pushed, and its grammar is:
//! a `#` comment header, then records of three whitespace-separated fields - `leaf` or `list`,
//! a variant name, and a reference that is a repository and a digest with NO tag between them.
//!
//! **Six readers re-implement that grammar across `grep`, `awk` and Rust, and nothing related
//! them.** Five anchor on the record kind, so they skip the header *as a side effect*; the sixth
//! required every line to be a record, refused on line 1 with `malformed image record`, and cost a
//! release. That reader shipped in `github.com/telekom/sutura#505` and `release.yml` fires only on
//! a tag, so the first tag it ever ran on was its first execution anywhere.
//! `github.com/telekom/sutura#596` fixed it. Five survivors that agree by coincidence are not a
//! mechanism, and `check-action-shell` and `check-workflows`'s other rules lint shell FORM and
//! reference SHAPE - neither knows this file has a grammar. So a SEVENTH consumer that iterates
//! every line was a green pull request, with the defect that cost a release available unchanged.
//!
//! # The rule
//!
//! A non-comment line in anything CI reads that REACHES the file - by its path, by the
//! `IMAGE_DIGESTS` environment name the steps bind, or by the `digests-file` input that fills it -
//! must be one of three things: a write by the producer, a read that carries a record-kind
//! [`ANCHORS`] form, or one of the [`DECLARED`] whole-file uses that take no field at all.
//! Anything else is refused, and the seventh naive reader is exactly *anything else*.
//!
//! [`DECLARED`] is held in BOTH directions, `check-skills`-style: an entry that matches no line is
//! a stale exemption and fails too, so a declaration cannot outlive the step it was written for.
//!
//! # Why a gate rather than one reader the others delegate to
//!
//! That was the first proposal, and it was measured and declined. Delegation needs the five shell
//! steps to call `xtask` for a field split, which puts a Rust build in the middle of the signing
//! sequence, and **it is a change to a path no gate in this tree can exercise** - `release.yml`
//! runs on a tag. Trading five working readers for five untested delegating ones, on the venue
//! whose first-run-on-a-tag property caused the original defect, is the trade `docs/adr/0025`
//! exists to refuse. And delegation alone stops nothing: a seventh naive reader is just as green
//! after the refactor, because what forbids it is a gate either way. So the gate is the whole
//! mechanism, and it runs on every pull request.
//!
//! # What this does NOT hold, stated next to the claim
//!
//! * **It cannot prove a shell reader parses correctly** - only that it anchors, and therefore
//!   that it cannot see the comment header as a record. Whether `grep '^list '` then splits the
//!   fields right is held by review; the Rust reader's correctness is held by its own unit tests
//!   in `crate::release_provenance`.
//! * **It refuses a new way to REACH the file, not a new way to parse one already reached.** A
//!   step that binds `inputs.digests-file` to some other environment name is refused at the
//!   binding, so the reader below it cannot be introduced silently - but the reading line itself
//!   is outside this scan, and a Rust caller handed the path as an argument is outside it
//!   entirely.
//! * **The header was only half the defect.** The same replay found that a `list` reference is
//!   written untagged, and that half is NOT what [`ANCHORS`] holds. It has two sites -
//!   `attest-and-sign`'s `${repo%:*}` and `release_provenance`'s `rsplit_once(':')` - and they are
//!   held against each other by the exact subject-set equality in `crate::release_provenance`,
//!   which only fires on a tag. Anchoring on the record kind would not have caught it and does not
//!   claim to.

use std::path::Path;

use super::sources;

/// How a line REACHES the image-record file. Any of these, on a non-comment line, is in scope.
///
/// Three spellings because the file is reached three ways: `release.yml` names the path, the
/// composite action's steps read `"$IMAGE_DIGESTS"`, and that name is filled from the action's
/// `digests-file` input.
///
/// **The path entry carries its DIRECTORY, and that is what separates a read from a mention.** A
/// release note or a verification page writes the bare file name, so `image-digests.txt` alone
/// matched a markdown table this rule has nothing to say about - and a rule that fired on the prose
/// explaining it would make its own header unwritable, which is `codegen`'s recorded trap. A
/// path-shaped handle needs no comment or backtick exemption to tell the two apart. The cost is
/// that MOVING the file leaves the scan, which is why [`problems`] refuses a walk that finds no
/// write: the producer is the one site that must always match.
///
/// A reader that reaches the file by some FOURTH route - a path assembled from pieces, or a second
/// environment name bound from a variable rather than from the input - is this rule's stated limit,
/// and the routes above are the only ones the release path has ever used.
const HANDLES: [&str; 3] = ["dist/image-digests.txt", "IMAGE_DIGESTS", "digests-file"];

/// The record-kind anchors a field read may use.
///
/// **This list IS the grammar's "anchor on the record kind"**, so a correct read written a
/// spelling this list does not hold is refused until the spelling is added - the safe direction,
/// and the opposite of `codegen::CODEGEN_BACKEND`'s exact-name limit. Both kinds are present
/// although only three of the four forms are live: a two-kind grammar that permits an anchor on
/// one kind and not the other reads as an oversight rather than as a decision.
const ANCHORS: [&str; 4] = ["'^leaf '", "'^list '", "$1 == \"leaf\"", "$1 == \"list\""];

/// Whole-file uses: the line reaches the file and takes no field out of it.
///
/// EXACT trimmed lines, not substrings, and that is the fail-closed half - `cat FILE` as a
/// substring would admit `cat FILE | awk '{ print $3 }'`, which is the seventh naive reader
/// wearing an exemption. Trimmed, so indentation is free; changing the line itself is not, which
/// is the point: a step that reaches a signed format gets re-declared when it is rewritten.
const DECLARED: [(&str, &str); 6] = [
    ("digests-file:", "the composite action's input declaration"),
    ("default: dist/image-digests.txt", "that input's default value"),
    (
        "IMAGE_DIGESTS: ${{ inputs.digests-file }}",
        "a step's env binding - it names the file, it does not read it",
    ),
    (
        "cat dist/image-digests.txt",
        "copied verbatim into the step summary, so nothing is parsed",
    ),
    ("if [ -f dist/image-digests.txt ]; then", "an existence test"),
    (
        "nix run .#xtask -- collect-provenance subjects.sha256 \"$IMAGE_DIGESTS\" \\",
        "handed to the reader of record, whose own tests hold its parse",
    ),
];

/// Every refusal in this module, over one repository root.
///
/// Over all three places [`sources::ci_sources`] walks, `nix/*.sh` included - unlike
/// `codegen::problems`, which deliberately skips it. A read of this file moving into shared shell
/// would be the same defect in a third directory, and a line cap is what moves steps.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let Some(sources) = sources::ci_sources(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut writes = 0usize;
    let mut anchored = 0usize;
    let mut used = [false; DECLARED.len()];
    for source in &sources {
        let found = scan(&source.label, &source.text, &mut used);
        writes = writes.saturating_add(found.writes);
        anchored = anchored.saturating_add(found.anchored);
        out.extend(found.problems);
    }
    // PASSING OVER NOTHING is the failure mode a text scan is most prone to, and a renamed path is
    // how this one would get there: the producer's three lines are the only sites that carry the
    // path spelling, so no write means the file moved and five release.yml readers left the scan in
    // silence. No anchored read means the same for the action's half.
    if writes == 0 || anchored == 0 {
        out.push(format!(
            "the scan found {writes} write(s) and {anchored} anchored read(s) of the image-record file - the release path produces that file and reads it, so this is the scan being broken rather than the workflows. Update the routes it knows, which are {}",
            HANDLES.join(", ")
        ));
    }
    for (index, (form, why)) in DECLARED.iter().enumerate() {
        if !used.get(index).copied().unwrap_or(false) {
            out.push(format!(
                "the declared whole-file use `{form}` ({why}) matches no line CI reads - a stale exemption is a hole waiting for a line to fall into it, so it is removed rather than kept"
            ));
        }
    }
    out
}

/// What one file contributed.
struct Found {
    /// Lines whose every occurrence is a redirect target: the producer writing the file.
    writes: usize,
    /// Lines that take fields and carry an [`ANCHORS`] form.
    anchored: usize,
    problems: Vec<String>,
}

/// Classify every line of one workflow, action or script.
///
/// A LINE rule, for `codegen::codegen_backend`'s reason: the file is reached from an `env:`
/// mapping and from inside a `run:` body, and no step-shaped scanner sees both. `used` records
/// which [`DECLARED`] entries this file consumed, so the staleness half above can be answered over
/// the whole walk rather than per file.
fn scan(label: &str, text: &str, used: &mut [bool; DECLARED.len()]) -> Found {
    let mut found = Found {
        writes: 0,
        anchored: 0,
        problems: Vec::new(),
    };
    for (index, line) in text.lines().enumerate() {
        let code = line.trim();
        if code.starts_with('#') {
            continue;
        }
        let (occurrences, redirects) = occurrences(line);
        if occurrences == 0 {
            continue;
        }
        // The producer. Every occurrence has to be a redirect target, not just one of them: a line
        // that writes the file AND reads it unanchored would otherwise pass on the write.
        if occurrences == redirects {
            found.writes = found.writes.saturating_add(1);
            continue;
        }
        if ANCHORS.iter().any(|anchor| line.contains(anchor)) {
            found.anchored = found.anchored.saturating_add(1);
            continue;
        }
        if let Some((at, _)) = DECLARED.iter().enumerate().find(|(_, (form, _))| *form == code) {
            if let Some(slot) = used.get_mut(at) {
                *slot = true;
            }
            continue;
        }
        found.problems.push(format!(
            "{label}:{}  reads the image-record file without anchoring on a record kind. `image-digests.txt` carries a `#` comment header INSIDE the signed asset, so a reader that treats every line as a record refuses on line 1 - which is what cost a release. Anchor the read on `leaf`/`list` with one of {}, or declare it in xtask/src/workflows/image_records.rs as a use that takes no field",
            index.saturating_add(1),
            ANCHORS.join(", ")
        ));
    }
    found
}

/// How many times one line reaches the image-record file, and how many of those are writes.
///
/// A write is an occurrence whose text is preceded by a redirection operator - `> FILE` or
/// `>> FILE` - which is the producer appending a record. Read off the head of the line rather than
/// from a shell parse, because that is all it takes to tell *writing this file* from *reading it*,
/// and a redirect cannot be spelled without the operator immediately before the target.
fn occurrences(line: &str) -> (usize, usize) {
    let mut reached = 0usize;
    let mut redirects = 0usize;
    for handle in HANDLES {
        for (at, _) in line.match_indices(handle) {
            reached = reached.saturating_add(1);
            if line.get(..at).is_some_and(|head| head.trim_end().ends_with('>')) {
                redirects = redirects.saturating_add(1);
            }
        }
    }
    (reached, redirects)
}

#[cfg(test)]
mod tests {
    /// No declaration consumed: the rows below assert on refusals, and staleness is answered over
    /// the whole walk by `the_committed_tree_anchors_every_reader`.
    fn scan(text: &str) -> super::Found {
        let mut used = [false; super::DECLARED.len()];
        super::scan("release.yml", text, &mut used)
    }

    #[test]
    fn a_seventh_reader_that_iterates_every_line_is_refused() {
        // THE FALSIFIER. Each row is a reader that takes the three fields off every line, which is
        // the shape that refused on the comment header and cost a release. `check-action-shell`
        // and the other `check-workflows` rules accept all of them.
        for (why, line) in [
            (
                "a redirect into read, the exact shape of the original defect",
                "              while read -r kind name ref; do echo \"$ref\"; done < dist/image-digests.txt\n",
            ),
            ("an unanchored awk field", "          awk '{ print $3 }' \"$IMAGE_DIGESTS\"\n"),
            (
                "a command substitution over every line",
                "          refs=\"$(cut -d' ' -f3 dist/image-digests.txt)\"\n",
            ),
            (
                "a pipe out of an exempt whole-file use",
                "            cat dist/image-digests.txt | awk '{ print $3 }'\n",
            ),
            (
                "a second env name bound from the same input",
                "        DIGESTS: ${{ inputs.digests-file }}\n",
            ),
        ] {
            let found = scan(line);
            assert!(!found.problems.is_empty(), "not refused - {why}: {line}");
            let problem = found.problems.first().expect("the refusal");
            assert!(problem.contains("without anchoring on a record kind"), "{problem}");
            assert_eq!(found.anchored, 0, "counted as an anchored read - {why}");
            assert_eq!(found.writes, 0, "counted as a write - {why}");
        }
    }

    #[test]
    fn an_anchored_read_and_a_write_pass() {
        // THE OTHER DIRECTION, and what keeps this from passing by refusing everything: the five
        // live shell readers and the three producer lines, verbatim from the tree.
        let live = concat!(
            "        grep -E '^leaf ' \"$IMAGE_DIGESTS\" > leaves.txt\n",
            "        grep -E '^list ' \"$IMAGE_DIGESTS\" > lists.txt\n",
            "          ref=\"$(awk -v want=\"$variant\" '$1 == \"list\" && $2 == want { print $3 }' \"$IMAGE_DIGESTS\")\"\n",
            "              grep '^list ' dist/image-digests.txt | while read -r _ name ref; do\n",
            "              grep '^leaf ' dist/image-digests.txt | while read -r _ key ref; do\n",
            "          } > dist/image-digests.txt\n",
            "            printf 'leaf %s %s@%s\\n' \"$key\" \"$IMAGE\" \"$digest\" >> dist/image-digests.txt\n",
            "              >> dist/image-digests.txt\n",
        );
        let found = scan(live);
        assert!(found.problems.is_empty(), "{:#?}", found.problems);
        assert_eq!(found.anchored, 5, "the five anchored readers");
        assert_eq!(found.writes, 3, "the three producer lines");
    }

    #[test]
    fn a_comment_or_a_bare_file_name_in_prose_is_not_a_read() {
        // Both halves are load-bearing. Comments are how the decision is documented - including in
        // this module - and the bare file name is how the release notes and the verification page
        // name the format. A gate that refused its own explanation would be unwritable, and the
        // third row is the release-notes table line that `image-digests.txt` alone did refuse.
        for line in [
            "          # `dist/image-digests.txt` records the list under that name\n",
            "          # digests from dist/image-digests.txt - so the notes cannot drift\n",
            "            echo \"| the licence documents, \\`image-digests.txt\\` | attached | yes |\"\n",
        ] {
            let found = scan(line);
            assert!(found.problems.is_empty(), "over-fired on: {line}\n{:#?}", found.problems);
            assert_eq!(found.writes, 0, "counted as a write: {line}");
            assert_eq!(found.anchored, 0, "counted as a read: {line}");
        }
    }

    #[test]
    fn the_committed_tree_anchors_every_reader() {
        // THE LIVE ASSERTION: every reader in the tree anchors, every exemption is still matched by
        // a line, and the producer's writes were found. Red the moment a seventh naive reader lands.
        let Some(root) = crate::repo::root() else { return };
        let problems = super::problems(&root);
        assert!(problems.is_empty(), "{problems:#?}");
    }
}

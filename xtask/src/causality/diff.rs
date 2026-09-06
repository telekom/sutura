//! `git diff` into added lines that know where they sit.
//!
//! Split out of `causality.rs` for the file-length gate, and the seam is a real one: everything
//! here reads a diff, and nothing here decides anything about a test.

use std::process::Command;

use super::provenance::Commit;
use super::regions::AddedLine;

/// A changed file and the lines the diff added to it, each with its post-image line number.
///
/// The line numbers are load-bearing rather than informational: whether an added line is test
/// code is decided by WHERE IT SITS, and a bare list of added lines cannot answer that. See
/// [`super::regions`] for the defect that shape produced.
#[derive(Debug)]
pub(crate) struct ChangedFile {
    /// Repo-relative, with forward slashes, as git prints it.
    pub(crate) path: String,
    /// The lines this diff added, in file order.
    pub(crate) added: Vec<AddedLine>,
    /// The lines this diff REMOVED, in file order.
    ///
    /// **Additions alone cannot say what a REVERT restores**, which is why this exists at all:
    /// `super::reverted` asks whether putting a file back at base could change anything the
    /// measured tests execute, and a change that only DELETES lines - a wrong early return taken
    /// out of a production function - adds nothing at all. Reading additions only would have read
    /// that as *this file changed no program*, which is the one direction that arm may not be
    /// wrong in.
    pub(crate) removed: Vec<RemovedLine>,
}

/// One removed line: the text the diff took out, and a POST-image line NEXT TO the gap it left.
///
/// **THE ANCHOR IS NOT A POSITION, IT IS ONE SIDE OF A GAP, and which side depends on the hunk.**
/// A removed line has a PRE-image number while every region this gate computes is read off the
/// POST-image, so the two are not comparable; what is comparable is where the removal landed in
/// the file as it stands. Measured against git rather than assumed, on `-U0`:
///
/// | the hunk | its header | what the anchor is |
/// | --- | --- | --- |
/// | delete lines 3-4 of five | `@@ -3,2 +2,0 @@` | **2** - the last surviving line BEFORE the gap |
/// | replace lines 3-4 | `@@ -3,2 +3,2 @@` | **3** - the FIRST replacement line, one past the gap |
///
/// So a consumer may not read it as *the removal was here*. `super::reverted` asks the whole
/// neighbourhood instead, and its own header says why one side was not enough.
///
/// **The first version of this doc claimed the wrong direction**, and review falsified it
/// end-to-end: it said a hunk that deleted a whole region is *wrong towards `Behaviour`*, the safe
/// side. It is wrong towards the EXCUSE. `regions::cfg_test_regions` builds a range that includes
/// the region's own last line, so a deletion whose preceding surviving line is a closing brace
/// anchored INSIDE the region it sits after - and a pure deletion of production code read as test
/// code. That was reachable at a shape this tree writes in several places: a
/// `#[cfg(test)] mod x;` followed immediately by a production item.
#[derive(Debug)]
pub(crate) struct RemovedLine {
    /// A 1-based POST-image line beside the gap - see the table above for which side. Zero when
    /// the hunk emptied the file's first lines, which no region contains.
    pub(crate) anchor: usize,
    /// The removed text, without the diff's leading `-`.
    pub(crate) text: String,
}

/// Added lines per changed file, from `git diff`, each carrying its post-image line number.
///
/// A [`Commit`] and not a ref, for the reason `super::provenance` states: a ref that has moved
/// puts commits this branch never made into the diff, and every consumer downstream then treats
/// them as the change under test.
pub(crate) fn changed_with_additions(base: &Commit) -> Option<Vec<ChangedFile>> {
    let out = Command::new("git")
        .args(["diff", "--unified=0", "--no-color", base.as_str(), "--"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_diff(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse a unified diff into added lines with their post-image line numbers.
///
/// The counter is advanced by CONTEXT and ADDED lines and not by removed ones, which is the
/// general rule for any `-U`; the gate asks for `-U0`, where the rule degenerates to "the added
/// lines are exactly the hunk's new-side range", but writing the general form means a change of
/// context width cannot silently shift every number by a few lines.
///
/// A `diff --git` header closes the previous file, so a DELETED file - whose `+++` is
/// `/dev/null` rather than `b/<path>` - cannot leave the previous file's entry open and collect
/// stray lines into it.
fn parse_diff(text: &str) -> Vec<ChangedFile> {
    let mut files: Vec<ChangedFile> = Vec::new();
    let mut current: Option<ChangedFile> = None;
    let mut next_line = 0_usize;
    // INSIDE A HUNK, `---` IS CONTENT AND NOT A HEADER, and reading it as one silently DROPPED a
    // removed line - a `--` SQL comment at column 0 of a raw string is the shape, in a repository
    // whose subject is generated SQL. A file whose only change vanished that way parsed to an
    // EMPTY change set, which `super::reverted` read as *nothing changed*. Headers occur only
    // before a file's first `@@`, so that is where they are read.
    let mut in_hunk = false;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            if let Some(done) = current.take() {
                files.push(done);
            }
            in_hunk = false;
            continue;
        }
        // Guarded by `in_hunk` for the same reason: an ADDED line spelling `++ b/x` arrives here
        // as `+++ b/x` and would otherwise open a file entry of its own.
        if let Some(rest) = (!in_hunk).then(|| line.strip_prefix("+++ b/")).flatten() {
            if let Some(done) = current.take() {
                files.push(done);
            }
            current = Some(ChangedFile {
                path: String::from(rest),
                added: Vec::new(),
                removed: Vec::new(),
            });
            continue;
        }
        if let Some(start) = hunk_start(line) {
            next_line = start;
            in_hunk = true;
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        // `---`/`+++` are headers only OUTSIDE a hunk; inside one they are ordinary content that
        // happens to begin with the marker. `\ No newline at end of file` is a note either way,
        // and no image line can be taken for it, because an image line begins with its own marker.
        if (!in_hunk && (line.starts_with("---") || line.starts_with("+++"))) || line.starts_with('\\') {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            file.added.push(AddedLine::new(next_line, added));
            next_line += 1;
            continue;
        }
        if let Some(removed) = line.strip_prefix('-') {
            // The counter does NOT advance: a removed line occupies no line of the new image, so
            // every removal in one run sits at the same gap.
            file.removed.push(RemovedLine {
                anchor: next_line,
                text: String::from(removed),
            });
            continue;
        }
        // Context, blank or not, occupies a line of the new image too.
        next_line += 1;
    }
    if let Some(done) = current.take() {
        files.push(done);
    }
    files
}

/// The new-side starting line of a hunk header, `@@ -a,b +c,d @@`.
fn hunk_start(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = rest.split_whitespace().find_map(|field| field.strip_prefix('+'))?;
    let count = plus.split_once(',').map_or(plus, |(start, _)| start);
    count.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_diff;

    #[test]
    fn a_diff_carries_the_post_image_line_number_of_every_added_line() {
        // Real `-U0` headers from the branch that found the defect: additions at 530 in one file,
        // and in the other a hunk that REPLACES a line - where the new-side number comes from the
        // header and not from counting the removals.
        let diff = concat!(
            "diff --git a/crates/x/src/commands.rs b/crates/x/src/commands.rs\n",
            "index 0296213..f20b073 100644\n",
            "--- a/crates/x/src/commands.rs\n",
            "+++ b/crates/x/src/commands.rs\n",
            "@@ -529,0 +530,2 @@ mod tests {\n",
            "+    #[test]\n",
            "+    fn added() {}\n",
            "diff --git a/crates/y/src/main.rs b/crates/y/src/main.rs\n",
            "--- a/crates/y/src/main.rs\n",
            "+++ b/crates/y/src/main.rs\n",
            "@@ -449 +450,2 @@ mod tests {\n",
            "-    use old::Thing;\n",
            "+    use new::Thing;\n",
            "+    use other::Thing;\n",
        );
        let files = parse_diff(diff);
        assert_eq!(files.len(), 2);
        let numbers: Vec<Vec<usize>> = files
            .iter()
            .map(|file| file.added.iter().map(|line| line.number).collect())
            .collect();
        assert_eq!(numbers, vec![vec![530, 531], vec![450, 451]]);
        assert_eq!(files.first().map(|file| file.path.as_str()), Some("crates/x/src/commands.rs"));
    }

    #[test]
    fn context_advances_the_counter_and_a_removal_does_not() {
        // `-U0` is what the gate asks for, so this shape does not arrive today. It is asserted
        // anyway: a later `-U3` would otherwise shift every number by the context width, and the
        // failure would be silent misclassification rather than a broken parse.
        let diff = concat!(
            "diff --git a/a.rs b/a.rs\n",
            "--- a/a.rs\n",
            "+++ b/a.rs\n",
            "@@ -8,4 +8,4 @@ fn outer() {\n",
            " let kept = 1;\n",
            "-let gone = 2;\n",
            "+let fresh = 2;\n",
            " let also_kept = 3;\n",
        );
        let files = parse_diff(diff);
        let numbers: Vec<usize> = files
            .first()
            .map(|file| file.added.iter().map(|line| line.number).collect())
            .unwrap_or_default();
        // Line 8 is context, so the replacement lands on 9 - not on 8, and not on 10.
        assert_eq!(numbers, vec![9]);
    }

    #[test]
    fn a_removed_line_is_anchored_where_it_sat_and_not_at_its_hunk_s_header() {
        // WHY THE REMOVALS ARE READ AT ALL: `super::super::reverted` asks whether a revert can
        // reach the tests in scope, and reading only the additions would see NOTHING in the first
        // hunk - so a wrong line taken out of a production function would read as a file that
        // changed no program.
        //
        // The second hunk carries CONTEXT, which is where the anchor choice becomes visible: the
        // counter has walked past the header by the time the removal arrives. Anchoring the whole
        // hunk at its header would put that removal three lines earlier than it sat, which is a
        // different answer to *is this line inside a test region*.
        let diff = concat!(
            "diff --git a/crates/x/src/a.rs b/crates/x/src/a.rs\n",
            "--- a/crates/x/src/a.rs\n",
            "+++ b/crates/x/src/a.rs\n",
            "@@ -12 +11,0 @@ fn guard() {\n",
            "-    if wrong { return Err(e); }\n",
            "@@ -40,5 +39,5 @@ mod tests {\n",
            " let kept = 1;\n",
            " let also_kept = 2;\n",
            " let still_kept = 3;\n",
            "-    assert!(old);\n",
            "-    assert!(also_old);\n",
            "+    assert_eq!(fresh, 1);\n",
            "+    assert_eq!(fresh, 2);\n",
        );
        let files = parse_diff(diff);
        let file = files.first().expect("one changed file");
        let anchors: Vec<usize> = file.removed.iter().map(|line| line.anchor).collect();
        // 11 for the first: the gap the deleted line left, not its pre-image number 12. Then 42
        // for both of the second hunk's - the header says 39 and three context lines walked the
        // counter to 42, which is where those two lines sat.
        assert_eq!(anchors, vec![11, 42, 42]);
        assert_eq!(
            file.removed.first().map(|line| line.text.trim()),
            Some("if wrong { return Err(e); }")
        );
        // The additions still carry their own post-image numbers, unmoved by the removals.
        let numbers: Vec<usize> = file.added.iter().map(|line| line.number).collect();
        assert_eq!(numbers, vec![42, 43]);
    }

    #[test]
    fn a_removed_line_that_begins_like_a_header_is_content_and_not_dropped() {
        // THE SILENT DROP. `---` opens a file header OUTSIDE a hunk and is ordinary content inside
        // one, and reading it as a header everywhere deleted the line from the parsed change set.
        // A `--` SQL comment at column 0 of a raw string is that shape, in a repository whose
        // subject is generated SQL - and a file whose WHOLE change was such a line then arrived at
        // `super::super::reverted` with an empty change set, which read as *nothing changed*.
        let diff = concat!(
            "diff --git a/crates/x/src/render.rs b/crates/x/src/render.rs\n",
            "--- a/crates/x/src/render.rs\n",
            "+++ b/crates/x/src/render.rs\n",
            "@@ -12 +11,0 @@ fn statement() {\n",
            "--- restricted to the caller's own rows\n",
        );
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1, "{files:?}");
        let file = files.first().expect("one changed file");
        assert_eq!(
            file.removed.iter().map(|line| line.text.as_str()).collect::<Vec<&str>>(),
            vec!["-- restricted to the caller's own rows"],
            "the removal is content, not a second `---` header"
        );
        // And the real headers are still headers: one file entry, not three.
        assert_eq!(file.path, "crates/x/src/render.rs");
    }

    #[test]
    fn a_deleted_file_does_not_collect_the_previous_file_s_lines() {
        // A deletion's `+++` is `/dev/null`, which opens no entry. Without the `diff --git`
        // header closing the previous one, its hunk lines would be attributed to that file.
        let diff = concat!(
            "diff --git a/a.rs b/a.rs\n",
            "--- a/a.rs\n",
            "+++ b/a.rs\n",
            "@@ -1,0 +2 @@\n",
            "+fn added() {}\n",
            "diff --git a/b.rs b/b.rs\n",
            "deleted file mode 100644\n",
            "--- a/b.rs\n",
            "+++ /dev/null\n",
            "@@ -1,2 +0,0 @@\n",
            "-fn gone() {}\n",
            "-fn also_gone() {}\n",
        );
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files.first().map(|file| file.added.len()), Some(1));
    }
}

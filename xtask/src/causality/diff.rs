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

/// One removed line: the text the diff took out, and the PRE-image line it sat on.
///
/// **THE NUMBER IS THE PRE-IMAGE'S, because that is the only image the line exists in.** An earlier
/// version carried a POST-image anchor beside the gap and asked `super::reverted` to check its
/// neighbourhood; review falsified that end to end. A pure deletion between two `#[cfg(test)]`
/// declarations has both surviving neighbours inside test regions while the line it removed was
/// production code that is simply GONE from the post-image - so the excuse was not imprecise, it
/// was false, and false in the direction that suppresses a FAILED. Measured on a two-package
/// workspace whose `check(5)` flips from false to true when the deleted `use` is restored:
/// `FAILED` exit 1 became `INCONCLUSIVE` exit 3.
///
/// The old-side numbers are in the hunk header the parser already reads, so the exact question -
/// *was this line inside a test region of the tree it was deleted from* - is answerable and
/// nothing has to be approximated. `super::reverted` reads the BASE image to ask it.
#[derive(Debug)]
pub(crate) struct RemovedLine {
    /// The 1-based line this text occupied in the PRE-image - the tree the revert restores.
    pub(crate) before: usize,
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
    // The OLD-side counter, advanced by context and by removals and not by additions - the mirror
    // of `next_line`, and what gives a removed line the only number it really has.
    let mut old_line = 0_usize;
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
        if let Some((old_start, new_start)) = hunk_bounds(line) {
            old_line = old_start;
            next_line = new_start;
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
            // Only the OLD counter advances: a removed line occupies a line of the pre-image and
            // none of the post-image.
            file.removed.push(RemovedLine {
                before: old_line,
                text: String::from(removed),
            });
            old_line += 1;
            continue;
        }
        // Context, blank or not, occupies a line of BOTH images.
        next_line += 1;
        old_line += 1;
    }
    if let Some(done) = current.take() {
        files.push(done);
    }
    files
}

/// The old-side and new-side starting lines of a hunk header, `@@ -a,b +c,d @@`.
///
/// BOTH, because a removed line and an added one are numbered in different images, and only the
/// header says where each run begins.
fn hunk_bounds(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let start = |marker: char| -> Option<usize> {
        let field = rest.split_whitespace().find_map(|part| part.strip_prefix(marker))?;
        field.split_once(',').map_or(field, |(at, _)| at).parse().ok()
    };
    Some((start('-')?, start('+')?))
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
    fn a_removed_line_carries_its_pre_image_number_and_an_added_one_its_post_image_number() {
        // WHY THE REMOVALS ARE READ AT ALL: `super::super::reverted` asks whether a revert can
        // reach the tests in scope, and reading only the additions would see NOTHING in the first
        // hunk - so a wrong line taken out of a production function would read as a file that
        // changed no program.
        //
        // THE TWO SIDES ARE NUMBERED IN DIFFERENT IMAGES and the second hunk is where that shows:
        // three context lines walk BOTH counters, then two removals walk only the old one and two
        // additions only the new one. A removed line's number is the line it sat on in the tree
        // the revert restores, which is the only tree it exists in.
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
        let before: Vec<usize> = file.removed.iter().map(|line| line.before).collect();
        // 12 for the first - its own pre-image line, not the 11 the gap left behind. Then 43 and
        // 44: the header says 40 and three context lines walked the old counter to 43.
        assert_eq!(before, vec![12, 43, 44]);
        assert_eq!(
            file.removed.first().map(|line| line.text.trim()),
            Some("if wrong { return Err(e); }")
        );
        // The additions carry post-image numbers, walked by context and by themselves only.
        let numbers: Vec<usize> = file.added.iter().map(|line| line.number).collect();
        assert_eq!(numbers, vec![42, 43]);
    }

    #[test]
    fn an_added_line_that_looks_like_a_file_header_opens_no_second_file() {
        // The mirror of the removal case below, and the half that had no test until review said
        // so: an added line spelling `++ b/x` reaches the parser as `+++ b/x`, so reading a file
        // header inside a hunk would split one changed file into two and lose the line.
        let diff = concat!(
            "diff --git a/crates/x/src/render.rs b/crates/x/src/render.rs\n",
            "--- a/crates/x/src/render.rs\n",
            "+++ b/crates/x/src/render.rs\n",
            "@@ -7,0 +8 @@ fn patch() {\n",
            "+++ b/not-a-header.rs\n",
        );
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1, "{files:?}");
        let file = files.first().expect("one changed file");
        assert_eq!(file.path, "crates/x/src/render.rs");
        assert_eq!(
            file.added.iter().map(|line| line.text.as_str()).collect::<Vec<&str>>(),
            vec!["++ b/not-a-header.rs"],
            "the addition is content, not a second file header"
        );
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

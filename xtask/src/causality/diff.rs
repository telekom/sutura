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

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            if let Some(done) = current.take() {
                files.push(done);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ b/") {
            if let Some(done) = current.take() {
                files.push(done);
            }
            current = Some(ChangedFile {
                path: String::from(rest),
                added: Vec::new(),
            });
            continue;
        }
        if let Some(start) = hunk_start(line) {
            next_line = start;
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        // `---`/`+++` are headers, `\ No newline at end of file` is a note; neither is a line of
        // either image.
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with('\\') {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            file.added.push(AddedLine::new(next_line, added));
            next_line += 1;
            continue;
        }
        if !line.starts_with('-') {
            // Context, blank or not, occupies a line of the new image too.
            next_line += 1;
        }
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

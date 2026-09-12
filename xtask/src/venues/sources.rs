//! What the TREE holds, as against what the page says about it.
//!
//! The third side of the split `page` and this module's parent already draw: `page` says what a
//! venues row or a verdict word IS, the parent says what the page must HOLD, and this says what is
//! true of the workspace the page describes - which tests exist, and which tasks CI actually runs.
//!
//! Split out when `venues.rs` crossed the unexemptable 1000-line gate a second time, and split at
//! this seam because these items are the only ones there that read the TREE rather than the page.
//! The split MOVED no assertion, which is what `.agents/skills/sutura/gates` prescribes: a file
//! that adds an assertion is never reverted, so moving the harness orphans nothing. A rule written
//! here since then is asserted here, beside the escape it closes.
//!
//! # The reading that was wrong, twice, and both times in the direction that passes
//!
//! [`invoked`] used to scan each non-comment line of a CI source for the substring `just ` or
//! `nix run .#`. A comment was the only prose it excluded - so
//! `echo to run this leg locally use nix run .#bigquery-two-principals please` resolved that name,
//! and a venue no job runs could claim a wiring at exit 0 with a summary byte-identical to the
//! honest one. [`starts_a_command`] reads a command instead.
//!
//! That fix then split the raw line on [`SEGMENTS`] to find where a command starts, so every one of
//! those tokens written inside somebody's quoted sentence was a boundary and the same prose
//! resolved one layer down. [`command_spans`] tracks the quoting, and its documentation carries the
//! five shapes measured on the merged tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Every test function in the workspace, by name.
///
/// A test rather than any function, because the page's claim is that these names ARE the standing
/// tests: a helper renamed into one of them would satisfy an existence check and prove nothing.
pub(super) fn test_names(root: &Path, files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let rust = files
        .iter()
        .filter(|f| Path::new(f).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs")));
    for rel in rust {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if !(trimmed.starts_with("#[") && trimmed.contains("test")) {
                continue;
            }
            for ahead in lines.iter().skip(i.saturating_add(1)).take(4) {
                let candidate = ahead.trim().trim_start_matches("pub ").trim_start_matches("async ");
                if let Some(rest) = candidate.strip_prefix("fn ")
                    && let Some(name) = rest.split('(').next()
                {
                    out.insert(name.trim().to_owned());
                    break;
                }
            }
        }
    }
    out
}

/// The task or app name one invocation names, or `None` when the span is not an invocation.
///
/// `just bigquery-two-principals` and `nix run .#bigquery-two-principals` both resolve to
/// `bigquery-two-principals`, which is what makes a page citing the TASK comparable against a
/// workflow invoking the APP. That the two names coincide is this repository's convention and not a
/// derived fact - a task whose flake app is named differently would not be seen, and that is a
/// false NEGATIVE, which is the direction this check can afford.
pub(super) fn invocation(span: &str) -> Option<&str> {
    let rest = span
        .strip_prefix("nix run .#just -- ")
        .or_else(|| span.strip_prefix("nix run .#"))
        .or_else(|| span.strip_prefix("just "))?;
    let name: &str = rest.split_whitespace().next()?;
    let name = name.trim_end_matches(&[',', '.', ';', ':'][..]);
    let shaped = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    shaped.then_some(name)
}

/// The tokens that end one shell command and begin another, so what follows one is a COMMAND.
///
/// A vocabulary rather than a parser, for `acceptance::shape::traces`' reason: what is needed is
/// *does a command start here*, and the shapes that start one in this tree are a line, a pipe, a
/// conjunction, a subshell, a `run:` key and the two verbs that prefix a command without being one.
///
/// **Every one of them separates only where the shell would let it**, which is [`command_spans`]:
/// splitting the raw line on this list is what made a `;`, an `&&` or a `(` written inside somebody's
/// quoted sentence a command boundary.
const SEGMENTS: &[&str] = &["&&", "||", "|", ";", "(", ")", "{", "}", "run:", "&"];

/// The quoting one byte of a line sits in. Only `Bare` lets a [`SEGMENTS`] token separate.
#[derive(Clone, Copy, PartialEq)]
enum Quoting {
    /// Outside every quote: a separator separates, and a ` #` begins a comment.
    Bare,
    /// Inside `'…'`, where the shell expands and separates nothing at all.
    Single,
    /// Inside `"…"`, where no separator separates and a `$(` still opens a command.
    Double,
}

/// What one token of a line does to the scan: how far it reaches, whether a command begins after
/// it, and the quoting it leaves behind. `None` ends the line, which is a comment.
type Step = Option<(usize, bool, Quoting)>;

/// Every span of one line that a command could begin at.
///
/// **The reading this replaces, and it was wrong in the direction that passes.** [`invoked`] split
/// each line on [`SEGMENTS`] and read the head of every piece, so a token inside a quoted sentence
/// was a command boundary. Measured on the merged tree at `d5bd307`, one line appended to `ci.yml`
/// at a time: a backticked name in a single-quoted `echo`, a `;`, an `&&`, a `(` and a `printf` of
/// a markdown table cell each resolved `bigquery-two-principals` and each moved the count from 18
/// to 19 - which refuses the honest `unrun` cell and instructs `wired`, at exit 0.
///
/// So the quoting is tracked. A `'…'` span contributes nothing; a `"…"` span separates nothing but
/// still opens a command at a `$(` or a backtick, because both really do run one there; and a `#`
/// after whitespace ends the line, which subsumes the whole-line-comment rule `invoked` used to
/// carry and closes the trailing-comment form of it.
///
/// **A bare backtick is PROSE here, deliberately**, and it is the one place this departs from
/// shell: an unquoted `` `x` `` is a legacy substitution that `shellcheck` refuses (SC2006) and
/// that no body in this tree writes, while `` `just test` `` inside a YAML `name:` or a heredoc'd
/// markdown bullet is ordinary writing. Read the other way round, every such sentence resolves.
///
/// **What it still does not reach**, since a false negative fails closed and a false positive does
/// not: an apostrophe in unquoted prose opens a `Single` span that swallows the rest of the line,
/// which drops invocations rather than inventing them. A heredoc body used to be read as commands
/// here - see [`outside_heredocs`], which is where that one went, and why the reasoning above it
/// does not excuse a false positive on this gate.
pub(super) fn command_spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let (mut start, mut at, mut state) = (0usize, 0usize, Quoting::Bare);
    // The quoting each open `$(` or backtick was written in, so its close returns to it.
    let mut opened: Vec<(Quoting, bool)> = Vec::new();
    while let Some(rest) = line.get(at..).filter(|rest| !rest.is_empty()) {
        let head = rest.chars().next().unwrap_or(' ');
        // One character, so an advance always lands on a boundary and the slices below resolve.
        let one = head.len_utf8();
        let step: Step = match (state, head) {
            (Quoting::Single, '\'') | (Quoting::Double, '"') => Some((one, false, Quoting::Bare)),
            (Quoting::Single, _) | (Quoting::Bare, '\'') => Some((one, false, Quoting::Single)),
            (Quoting::Bare, '"') => Some((one, false, Quoting::Double)),
            // A backslash inside `"…"` makes the NEXT byte a literal, so an ESCAPED backtick is
            // one somebody printed and not a substitution that runs something. Without this arm,
            // an `echo` that quotes a task name in escaped backticks resolved that task - the
            // same prose hole one escape deeper, and the shape a recipe's own `echo` uses.
            (Quoting::Double, '\\') => Some((
                one.saturating_add(rest.chars().nth(1).map_or(0, char::len_utf8)),
                false,
                Quoting::Double,
            )),
            // A substitution runs a command wherever it is written, so it opens one - and its
            // close puts the scan back in the quoting the opener was written in.
            (Quoting::Bare | Quoting::Double, '$') if rest.starts_with("$(") => {
                opened.push((state, false));
                Some((2, true, Quoting::Bare))
            }
            // This expansion is one path atom, not a brace group. Do not skip arbitrary `${...}`:
            // a parameter expansion can itself contain a real command substitution.
            (Quoting::Bare, '$') if rest.starts_with("${RUNNER_TEMP}") => Some(("${RUNNER_TEMP}".len(), false, Quoting::Bare)),
            (Quoting::Double, '`') => {
                opened.push((state, true));
                Some((one, true, Quoting::Bare))
            }
            (Quoting::Bare, '`') if opened.last().is_some_and(|(_, backtick)| *backtick) => {
                Some((one, true, opened.pop().map_or(Quoting::Bare, |(outer, _)| outer)))
            }
            (Quoting::Bare, ')') => Some((
                one,
                true,
                opened
                    .pop_if(|(_, backtick)| !*backtick)
                    .map_or(Quoting::Bare, |(outer, _)| outer),
            )),
            // The rest of the line is a comment, in a workflow's YAML and in a shell body alike.
            (Quoting::Bare, '#') if line.get(..at).is_none_or(begins_a_word) => None,
            (Quoting::Bare, _) => Some(
                SEGMENTS
                    .iter()
                    .find(|token| rest.starts_with(**token))
                    .map_or((one, false, Quoting::Bare), |token| (token.len(), true, Quoting::Bare)),
            ),
            (Quoting::Double, _) => Some((one, false, Quoting::Double)),
        };
        let Some((width, separates, next)) = step else {
            break;
        };
        if separates {
            spans.push(line.get(start..at).unwrap_or_default());
        }
        at = at.saturating_add(width);
        if separates {
            start = at;
        }
        state = next;
    }
    spans.push(line.get(start..at).unwrap_or_default());
    spans
}

/// Would the next character start a word? A `#` there begins a comment; one glued to a word - the
/// `.#` of `nix run .#app`, or a fragment in a URL - does not.
fn begins_a_word(before: &str) -> bool {
    before.is_empty() || before.ends_with(char::is_whitespace)
}

/// The words that may sit in front of a command without making it text.
const PREFIXES: &[&str] = &["- ", "exec ", "sudo ", "time ", "then ", "else ", "do ", "eval "];

/// Commands that RUN another command, with the real one after their argument terminator.
///
/// **A false negative this closes, measured rather than imagined.** The first hardening resolved 18
/// names where the substring scan resolved 19, and the one it lost was real:
/// `devenv shell -- just setup` in `nix/container-setup.sh`. That is an invocation, and losing it
/// would have been the gate quietly forgetting a task CI runs.
///
/// **Handled here rather than by splitting lines on `--`**, and the difference is the whole point
/// of this module's rewrite: splitting would make ` just x` its own segment inside
/// `echo "use -- just x"`, which is the prose hole all of this exists to close. A wrapper is
/// recognised as the segment's OWN command, so the text it wraps is still one segment.
const WRAPPERS: &[(&str, &str)] = &[
    ("devenv shell", " -- "),
    ("nix develop", " --command "),
    ("nix-shell", " --run "),
];

/// Is this segment's own command an invocation, rather than a word inside somebody's argument?
///
/// **The hole this closes, measured:** `invoked` used to scan each non-comment line for the
/// substring `nix run .#`, so
/// `echo to run this leg locally use nix run .#bigquery-two-principals please` resolved that name
/// and a venue no job runs could say `wired` at exit 0, with a summary byte-identical to the honest
/// one. A print's ARGUMENT is prose - the same distinction `acceptance::shape::emits_file` draws
/// one file over, where reading the verb rather than the line was also the fix.
///
/// **The direction it is wrong in, on purpose.** A command this cannot see is a false NEGATIVE, and
/// the three rules built on it each fail CLOSED on one: `wired` is refused, a built venue naming no
/// resolvable invocation is refused, and only `unrun` survives - which is the state that claims
/// nothing. **That direction still has to be MEASURED rather than trusted:** the first version of
/// this reader silently lost `devenv shell -- just setup`, which is why [`WRAPPERS`] exists and why
/// the count this gate prints is worth reading on a change to either.
pub(super) fn starts_a_command(segment: &str) -> Option<&str> {
    invocation(command_of(segment))
}

/// The command one span actually runs, with the prefixes and wrappers in front of it removed.
///
/// Split out of [`starts_a_command`] so that *what does this span run* has ONE reader. The second
/// caller is [`test_tasks`], which asks whether a recipe's own command runs this workspace's
/// tests; asking that of the raw line instead would have read the three `echo` lines every venue
/// recipe opens with, which is the prose hole this module exists to close.
fn command_of(segment: &str) -> &str {
    let mut command = segment.trim_start();
    loop {
        if let Some(rest) = PREFIXES.iter().find_map(|word| command.strip_prefix(word)) {
            command = rest.trim_start();
            continue;
        }
        // A wrapper's own argument terminator, and only when the wrapper is what this segment
        // starts with - so the terminator inside somebody's printed sentence is not one.
        let unwrapped = WRAPPERS.iter().find_map(|(wrapper, terminator)| {
            command
                .strip_prefix(wrapper)
                .and_then(|rest| rest.split_once(terminator))
                .map(|(_, after)| after)
        });
        match unwrapped {
            Some(rest) => command = rest.trim_start(),
            None => break,
        }
    }
    command
}

/// Every `just` task and `nix run .#` app CI invokes, by bare name.
///
/// **Read out of the same three places `crate::workflows` scans, through the same walk**, because
/// the recorded way a reference leaves a gate's sight is a step moving out of a workflow - and a
/// second walk here would be a second answer to *where does CI invoke things from*.
///
/// A comment is not an invocation, matching `crate::workflows::collect`'s own rule: `ci.yml` and
/// `docs.yml` both discuss tasks in prose, and a gate that read those would refuse an `unrun` cell
/// because somebody explained the job in a comment. **Neither is a word inside a command's
/// arguments** - see [`starts_a_command`] - and **neither is a word inside somebody's QUOTES**,
/// which is [`command_spans`] and the finding one review later: the comment rule was a whole-line
/// `starts_with('#')` while five other prose shapes went on resolving.
/// Workflow and action sources contribute only their raw `run:` bodies, through
/// [`crate::workflows::step::shell`]; shared scripts contribute their whole text. The indentation
/// reader's YAML limits and this module's heredoc limits remain, so this is not an execution proof.
///
/// `None` only when the scan itself is broken, which its caller turns into a failure rather than an
/// empty set: a set that found nothing would make every `unrun` cell pass, and *a scan that passes
/// by finding nothing* is the failure mode a text gate is most prone to here.
pub(super) fn invoked(root: &Path) -> Option<BTreeSet<String>> {
    let read = crate::workflows::sources::ci_sources(root)?;
    let mut out = BTreeSet::new();
    for source in &read {
        let lines: Vec<&str> = source.text.lines().collect();
        let bodies = if source.label.starts_with("nix/") {
            lines
        } else {
            crate::workflows::step::shell(&lines)
        };
        for line in outside_heredocs(&bodies) {
            for span in command_spans(line) {
                if let Some(name) = starts_a_command(span) {
                    out.insert(name.to_owned());
                }
            }
        }
    }
    Some(out)
}

/// The lines of a shell body that are COMMANDS, with every heredoc body dropped.
///
/// **A false positive on this gate publishes a false claim about identity, which is why the
/// "a false negative fails closed" argument in [`command_spans`] did not reach this one.**
/// `github.com/telekom/sutura#401` measured six prose shapes against the merged tree; five were
/// closed by tracking quoting, and this was the sixth still live. One bullet added to `ci.yml`:
///
/// ```yaml
/// - run: |
///     cat <<'EOF'
///     - just bigquery-two-principals (by hand)
///     EOF
/// ```
///
/// resolved that task, which REFUSES the honest `unrun` cell at exit 1 and whose remedy text
/// instructs `wired` - so a sentence saying a leg is not wired created the wiring, on
/// `docs/where-identity-is-proven.md`. A heredoc body is data the shell hands to a command's
/// stdin; it is not a command, and `- ` in front of it is a markdown bullet rather than
/// [`PREFIXES`]' YAML dash.
///
/// The OPENING line is still scanned - `cat <<'EOF' > out.sh` runs `cat` - and the body and its
/// terminator are not.
///
/// **What this does not reach, and both directions are stated because the gate's whole subject is
/// an overstated claim.** The terminator is matched on a trimmed line, because a YAML block scalar
/// indents the whole script and the shell sees it dedented, so a body line that is exactly the
/// delimiter word closes the heredoc early and the rest of the body is read as commands again -
/// which is today's behaviour, not a new hole. Only the FIRST heredoc a line opens is tracked, so
/// `cmd <<A <<B` reads B's body as commands. And a real invocation written INSIDE a heredoc - a
/// script generated by `cat <<EOF > run.sh` - is now invisible, which is the false-negative
/// direction: `wired` is refused, a built venue naming no resolvable invocation is refused, and
/// only `unrun` survives. That direction is MEASURED rather than trusted, the way [`WRAPPERS`]
/// had to be: the count this gate prints is 20 with and without this function on the tree that
/// introduced it.
fn outside_heredocs<'l>(lines: &[&'l str]) -> Vec<&'l str> {
    let mut commands = Vec::new();
    let mut terminator: Option<String> = None;
    for line in lines {
        if let Some(word) = terminator.as_deref() {
            if line.trim() == word {
                terminator = None;
            }
            continue;
        }
        commands.push(*line);
        terminator = heredoc_word(line);
    }
    commands
}

/// The delimiter a line opens a heredoc with, if it opens one.
///
/// `<<<` is a here-string rather than a heredoc, and its operand is one word on the same line, so
/// it opens nothing. `<<-` strips leading tabs from the body and its terminator, which the trimmed
/// comparison in [`outside_heredocs`] already tolerates.
fn heredoc_word(line: &str) -> Option<String> {
    let after = line.split_once("<<")?.1;
    if after.starts_with('<') {
        return None;
    }
    let word = after.strip_prefix('-').unwrap_or(after).trim_start();
    let quoted = |quote: char| {
        word.strip_prefix(quote)
            .and_then(|rest| rest.split_once(quote))
            .map(|(name, _)| name)
    };
    let bare = || word.split([' ', '\t', ';', '|', '&', ')', '>', '<']).next();
    let name = quoted('\'').or_else(|| quoted('"')).or_else(bare)?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// The file every task this page cites is defined in.
const JUSTFILE: &str = "justfile";

/// The commands that RUN this workspace's tests.
///
/// Whole commands rather than words, and read through [`command_of`] rather than off the raw line,
/// because a venue recipe opens with three `echo` lines that name tasks in prose. Two entries are
/// enough for the one question asked of this list - *does this task run a test at all* - and a
/// task running neither, transitively, runs none.
const TEST_COMMANDS: &[&str] = &["cargo nextest run", "cargo test"];

/// One `just` recipe, reduced to the two things the anchor rule asks of it.
struct Recipe {
    /// Does its own body start a command from [`TEST_COMMANDS`]?
    runs_tests: bool,
    /// Every task it invokes: its header's dependencies, and the invocations in its body.
    delegates: BTreeSet<String>,
}

/// A recipe's header line, reduced to the two things [`recipes`] needs from it.
struct Header {
    /// The recipe's name.
    name: String,
    /// The tasks it depends on, which `just` runs before its body.
    deps: BTreeSet<String>,
}

/// Is this line a recipe header, and if so its name and its dependencies?
///
/// A header sits at column zero and its name is followed by parameters, then a `:`. The two shapes
/// deliberately excluded are an assignment (`x := y`, which contains the `:` a header ends with)
/// and a `set`/`export` setting - both are column-zero lines with a colon and neither runs
/// anything.
///
/// **A parameter DEFAULT is why the name is taken off the head rather than by pattern**, measured
/// against `just --list` over this repository's own justfile: six recipes carry a default such as
/// `base="origin/main"`, and a reader that refused an `=` before the colon lost all six.
fn header(line: &str) -> Option<Header> {
    if line.starts_with(char::is_whitespace) || line.starts_with('#') || line.contains(":=") {
        return None;
    }
    let (head, deps) = line.split_once(':')?;
    let name = head.split_whitespace().next()?;
    let shaped = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !shaped || ["set", "export", "alias", "import"].contains(&name) {
        return None;
    }
    let deps = deps
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    Some(Header {
        name: name.to_owned(),
        deps,
    })
}

/// Every recipe [`JUSTFILE`] defines, by name.
fn recipes(text: &str) -> BTreeMap<String, Recipe> {
    let mut out: BTreeMap<String, Recipe> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(Header { name, deps }) = header(line) {
            out.insert(
                name.clone(),
                Recipe {
                    runs_tests: false,
                    delegates: deps,
                },
            );
            current = Some(name);
            continue;
        }
        // A body line is indented, and a column-zero line that is not a header ends the recipe.
        if !line.trim().is_empty() && !line.starts_with(char::is_whitespace) {
            current = None;
            continue;
        }
        let Some(recipe) = current.as_ref().and_then(|name| out.get_mut(name)) else {
            continue;
        };
        for span in command_spans(line) {
            let command = command_of(span);
            if TEST_COMMANDS.iter().any(|verb| command.starts_with(verb)) {
                recipe.runs_tests = true;
            }
            if let Some(name) = invocation(command) {
                recipe.delegates.insert(name.to_owned());
            }
        }
    }
    out
}

/// Every task that RUNS a test, through the tasks it delegates to as well as its own body.
///
/// **What this is the anchor for.** A `Reached by` cell earns its venue a citable verdict by naming
/// a task CI invokes - and CI invokes lints, builds and doc jobs too. Measured and recorded on the
/// page before this existed: pointing the two-keys row at a real, CI-invoked, entirely unrelated
/// lint and moving its cell passed at exit 0. A task that runs no test cannot be what reaches a
/// venue whose venue-hood is a test, so this is resolved beside `invoked` and both are required.
///
/// **The direction it is wrong in.** A task this cannot see runs no test as far as the gate is
/// concerned, so the verdict is REFUSED - a false negative, which is the direction the rules built
/// on this can afford. The two shapes it cannot see: a task whose test run happens inside a `nix`
/// derivation rather than in its own recipe (`just validate` is one), and a flake app with no
/// same-named recipe. Both are refusals of an honest cell rather than acceptances of a false one.
///
/// **What it does NOT reach, stated where the claim is.** It holds that the cited task runs tests,
/// never that it runs THIS venue's tests. Pointing a row at another venue's test task still
/// passes, and that residue is review's - narrowed from *any CI-invoked task* to *any CI-invoked
/// task that runs tests*.
///
/// `None` when the justfile cannot be read or defines no test task at all, which its caller turns
/// into a failure: a set that found nothing would refuse every citable cell, and a scan that
/// passes - or fails - by finding nothing is what a text gate is most prone to.
pub(super) fn test_tasks(root: &Path) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(root.join(JUSTFILE)).ok()?;
    let found = test_tasks_in(&text);
    (!found.is_empty()).then_some(found)
}

/// [`test_tasks`] over the justfile's TEXT, so the reader has a venue of its own.
fn test_tasks_in(text: &str) -> BTreeSet<String> {
    let defined = recipes(text);
    let mut out: BTreeSet<String> = defined
        .iter()
        .filter(|(_, recipe)| recipe.runs_tests)
        .map(|(name, _)| name.clone())
        .collect();
    // Delegation is transitive, and the fixpoint is bounded by the recipe count: each pass adds at
    // least one name or stops.
    loop {
        let grown: Vec<String> = defined
            .iter()
            .filter(|(name, recipe)| !out.contains(name.as_str()) && recipe.delegates.iter().any(|task| out.contains(task)))
            .map(|(name, _)| name.clone())
            .collect();
        if grown.is_empty() {
            break;
        }
        out.extend(grown);
    }
    out
}

/// The task and app names a `Reached by` cell cites, by bare name.
///
/// Backticked spans only, because that is how the page writes an invocation and because the cell's
/// prose ("a GitHub environment, on demand") would otherwise contribute words.
pub(super) fn cited_invocations(reached: &str) -> BTreeSet<String> {
    reached
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|span| invocation(span.trim()))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{command_spans, starts_a_command};

    #[test]
    fn invoked_reads_yaml_run_bodies_and_shell_scripts_but_not_yaml_prose() {
        let root = std::env::temp_dir().join(format!("sutura-venue-invocation-{}", std::process::id()));
        for directory in [".github/workflows", ".github/actions/fixture", "nix"] {
            std::fs::create_dir_all(root.join(directory)).expect("fixture directory");
        }
        std::fs::write(
            root.join(".github/workflows/fixture.yml"),
            concat!(
                "jobs:\n  ci:\n    steps:\n",
                "      - name: Optional; just workflow-prose\n",
                "        run: just workflow-run\n",
                "      - run: |\n          just nameless-run\n",
            ),
        )
        .expect("workflow fixture");
        std::fs::write(
            root.join(".github/actions/fixture/action.yml"),
            concat!(
                "name: Optional; just action-prose\n",
                "runs:\n  using: composite\n  steps:\n",
                "    - run: just action-run\n      shell: bash\n",
            ),
        )
        .expect("action fixture");
        std::fs::write(root.join("nix/fixture.sh"), "#!/bin/sh\njust script-run\n").expect("script fixture");
        let found = super::invoked(&root);
        std::fs::remove_dir_all(&root).expect("remove fixture");
        let expected = ["workflow-run", "nameless-run", "action-run", "script-run"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn a_heredoc_body_is_data_rather_than_commands() {
        // `github.com/telekom/sutura#401`, the one prose shape still live after quoting was
        // tracked: a markdown bullet inside a heredoc resolved as an invocation, which refuses the
        // honest `unrun` cell and instructs `wired` on the identity page. The line after the
        // terminator is a REAL invocation, so this fails in both directions - a skip that never
        // ends loses it, and no skip at all resolves the bullet.
        let root = std::env::temp_dir().join(format!("sutura-venue-heredoc-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".github/workflows")).expect("fixture directory");
        std::fs::write(
            root.join(".github/workflows/fixture.yml"),
            concat!(
                "jobs:\n  ci:\n    steps:\n",
                "      - run: |\n",
                "          cat <<'EOF'\n",
                "          - just heredoc-prose (by hand)\n",
                "          just also-prose\n",
                "          EOF\n",
                "          just after-the-terminator\n",
            ),
        )
        .expect("workflow fixture");
        let found = super::invoked(&root);
        std::fs::remove_dir_all(&root).expect("remove fixture");
        let expected = std::collections::BTreeSet::from([String::from("after-the-terminator")]);
        assert_eq!(found, Some(expected));
    }

    /// The task name every shape below tries to resolve, spelled once.
    const TASK: &str = "bigquery-two-principals";

    /// Does any span of this line begin a command that invokes `TASK`?
    fn resolves(line: &str) -> bool {
        command_spans(line)
            .into_iter()
            .any(|span| starts_a_command(span) == Some(TASK))
    }

    #[test]
    fn a_test_command_inside_a_recipe_s_own_echo_is_not_a_test_run() {
        // The anchor rule asks the justfile *does this task run a test*, and the recipes it asks
        // that of open with three `echo` lines that name tasks and commands in prose - including
        // in ESCAPED backticks, which is the shape this file's own quoting reader gained an arm
        // for. So the same command reader `invoked` uses is what answers here.
        let justfile = concat!(
            "# A lint, which CI really does invoke.\n",
            "lint-workflows:\n",
            "    echo \"this is NOT a gate - run \\`cargo nextest run\\` for the whole suite\"\n",
            "    nix run .#actionlint\n",
            "\n",
            "two-principals:\n",
            "    echo \"CI runs it through \\`nix run .#two-principals\\`, in its own job.\"\n",
            "    cargo nextest run -p sutura-exec-bigquery -E 'binary(two_principals)'\n",
            "\n",
            "gates: two-principals\n",
            "    nix build .#checks\n",
        );
        let found = super::test_tasks_in(justfile);
        // The real command counts, wherever in the body it is.
        assert!(found.contains("two-principals"), "{found:?}");
        // A dependency runs before the body, so a task that depends on a test task runs tests.
        assert!(found.contains("gates"), "{found:?}");
        // And the one the whole rule exists for: a lint that only TALKS about a test run is not a
        // venue anything is proven in. Without the escaped-quote arm this line reads as a test.
        assert!(!found.contains("lint-workflows"), "{found:?}");
    }

    #[test]
    fn a_separator_inside_somebody_s_quoted_sentence_is_not_a_command_boundary() {
        // The first five moved the invocation count from 18 to 19 on the merged tree, which refuses
        // the honest `unrun` cell and instructs `wired` - measured one appended `ci.yml` line at a
        // time. The backtick pair is the one that reads most like an explanation of the gap.
        for prose in [
            "      - run: echo 'not wired yet - run `just bigquery-two-principals` by hand.'",
            "      - run: echo \"the leg is not wired; just bigquery-two-principals is the task\"",
            "      - run: echo \"not wired && just bigquery-two-principals is how you run it\"",
            "      - run: echo \"run it by hand (just bigquery-two-principals) for now\"",
            "      - run: printf '| a venue | just bigquery-two-principals |\\n'",
            "      - run: echo 'nothing runs `nix run .#bigquery-two-principals` yet'",
            // An ESCAPED backtick inside double quotes is a printed backtick, not a substitution -
            // and it is the shape a recipe's own `echo` lines are written in.
            "      - run: echo \"run \\`just bigquery-two-principals\\` by hand for now\"",
            "      - run: nix build .#checks.x86_64-linux.hygiene # just bigquery-two-principals",
            "      # just bigquery-two-principals is the task",
            "        name: Run `just bigquery-two-principals`",
        ] {
            assert!(!resolves(prose), "{prose}");
        }
    }

    #[test]
    fn a_command_still_resolves_wherever_the_shell_would_run_one() {
        // The other direction, which is the one that gets a gate deleted: a false negative refuses
        // a correct `wired` cell. A substitution inside double quotes really does run its command,
        // and `devenv shell -- just <task>` is the real invocation the first hardening lost.
        for command in [
            "      - run: just bigquery-two-principals",
            "      - run: set -eu; just bigquery-two-principals",
            "      - run: nix run .#just -- bigquery-two-principals",
            "      - run: echo \"$(just bigquery-two-principals)\"",
            "      - run: echo \"two `just bigquery-two-principals` keys\"",
            "      - run: devenv shell -- just bigquery-two-principals",
            "          if [ -n \"$X\" ]; then just bigquery-two-principals; fi",
        ] {
            assert!(resolves(command), "{command}");
        }
    }
}

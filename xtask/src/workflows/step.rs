//! One job of a workflow, and the one step in it that reaches a flake output.
//!
//! **A gate asserting that a lane still invokes it cannot use `contains` over the file.** A
//! commented-out step satisfies a substring while nothing runs, and a step that has been moved to
//! a job nobody requires satisfies it too - both of them the dead-check shape rather than a
//! hypothetical. [`super::collect`] already refuses the first for this repository's own scan, and
//! `a_comment_is_not_a_reference` is the test that says it must; this module adds the second half
//! by narrowing the text to one job before collecting, and hands back the step's own lines so a
//! caller can ask what else that step declares.
//!
//! WHAT IT DOES NOT REACH. Whether the job is a REQUIRED context is a property of the repository's
//! branch ruleset and not of any file here, so no gate in this tree can read it. And a step's
//! condition is returned rather than judged - "does this `if:` ever evaluate true" is a question
//! about an event, which a text scan may not pretend to answer.

// Both only reachable from [`app_step`], which is `#[cfg(test)]` for the reason stated there.
#[cfg(test)]
use super::{Kind, collect};

/// One job's own lines, from its header to the next thing at the same indentation.
///
/// Two spaces is where a job's name sits and four is where its keys do, so a comment block
/// introducing the NEXT job - which `ci.yml` writes at two spaces - ends the block rather than
/// joining it.
///
/// Here rather than in `venues::acceptance`, which is where both readers were written: that gate
/// reads the acceptance job's properties, this one reads a step's, and two copies of the
/// indentation contract is how one of them stops matching the file after a reindent.
pub(crate) fn job<'a>(text: &'a str, name: &str) -> Option<Vec<&'a str>> {
    keyed_block(text, "  ", name)
}

/// The lines under `name:` written at `indent`, up to the next line no deeper than that key.
///
/// One reader for two scopes, because the second one is what `venues::acceptance`'s
/// `configures_tracing` was missing: column zero is the WORKFLOW's own mapping, where `defaults:`
/// and `env:` hold keys that decide how a job's shells start. A block is a block at either
/// indentation, so the depth is an argument rather than a second function that can disagree with
/// this one.
pub(crate) fn keyed_block<'a>(text: &'a str, indent: &str, name: &str) -> Option<Vec<&'a str>> {
    let header = format!("{indent}{name}:");
    let deeper = format!("{indent}  ");
    let mut lines = text.lines().skip_while(|line| *line != header);
    lines.next()?;
    Some(
        lines
            .take_while(|line| line.trim().is_empty() || line.starts_with(&deeper))
            .collect(),
    )
}

/// The step of `job` that reaches the flake app `app`, as its own lines.
///
/// `None` where no such job exists, where nothing in it reaches the app, or where the reference is
/// not inside a step this can delimit - each of which is a caller's failure rather than its pass,
/// for the reason [`super::declared_block`] gives about a parse that has desynchronised.
///
/// `#[cfg(test)]` for `crate::tasks::recipe_body`'s reason, stated there: the property is asserted
/// per gate - TWO of them now, the two halves of the default-feature lane, which is the second
/// copy of one assertion and therefore the argument for generalising it. The general form wants a
/// payload on `Kind::Standalone` naming the lane so the registry makes every new gate declare its
/// wiring. Widen this attribute when that lands - a run-time gate holding *every standalone gate is
/// invoked by the lane it declares* is the same shape as `check-workflows` reading this file
/// already.
#[cfg(test)]
pub(crate) fn app_step<'a>(text: &'a str, job_name: &str, app: &str) -> Option<Vec<&'a str>> {
    let lines = job(text, job_name)?;
    let mut references = Vec::new();
    collect(&lines.join("\n"), job_name, &mut references);
    let at = references
        .iter()
        .find(|reference| reference.kind == Kind::Runnable && reference.name == app)?
        .line;
    let start = (0..at)
        .rev()
        .find(|index| lines.get(*index).is_some_and(|line| line.trim_start().starts_with("- ")))?;
    let depth = indent(lines.get(start)?);
    let end = lines
        .iter()
        .enumerate()
        .skip(start.saturating_add(1))
        .find(|(_, line)| !line.trim().is_empty() && indent(line) <= depth)
        .map_or(lines.len(), |(index, _)| index);
    lines.get(start..end).map(<[&str]>::to_vec)
}

/// How deep a line is indented.
#[cfg(test)]
fn indent(line: &str) -> usize {
    line.len().saturating_sub(line.trim_start().len())
}

#[cfg(test)]
mod tests {
    /// Two jobs naming the same app, a parked reference above the live one, and the next job
    /// introduced by a comment at a job's own indentation - the three shapes of the real file.
    const WORKFLOW: &str = concat!(
        "jobs:\n",
        "  ci:\n",
        "    steps:\n",
        "      - name: Tests\n",
        "        run: nix build .#checks.x86_64-linux.nextest -L\n",
        "\n",
        "      # was: nix run .#default-feature-tests\n",
        "      - name: The shipped feature set runs its tests\n",
        "        if: steps.classify.outputs.rust == 'true'\n",
        "        run: nix run .#default-feature-tests\n",
        "\n",
        "      - name: After\n",
        "        run: nix run .#crap\n",
        "\n",
        "  # A comment introducing the next job, written at two spaces.\n",
        "  elsewhere:\n",
        "    steps:\n",
        "      - name: A copy nobody requires\n",
        "        continue-on-error: true\n",
        "        run: nix run .#default-feature-tests\n",
    );

    const APP: &str = "default-feature-tests";

    #[test]
    fn a_jobs_block_ends_where_the_next_job_is_introduced() {
        let ci = super::job(WORKFLOW, "ci").expect("the ci job");
        assert!(ci.iter().any(|line| line.contains("name: After")));
        assert!(
            !ci.iter().any(|line| line.contains("A copy nobody requires")),
            "a comment at two spaces introduces the next job and must end this one: {ci:?}"
        );
        assert!(super::job(WORKFLOW, "no-such-job").is_none());
    }

    #[test]
    fn the_step_returned_is_the_one_that_runs_the_app() {
        let step = super::app_step(WORKFLOW, "ci", APP).expect("the step that runs the app");
        let block = step.join("\n");
        assert!(block.contains("name: The shipped feature set runs its tests"), "{block}");
        assert!(block.contains("if: steps.classify.outputs.rust == 'true'"), "{block}");
        // The step ends where the next one begins, or a `continue-on-error:` two steps away would
        // read as this step's own.
        assert!(!block.contains("name: After"), "{block}");
        assert!(!block.contains("name: Tests"), "{block}");
    }

    #[test]
    fn a_parked_reference_is_not_a_step_and_neither_is_one_in_another_job() {
        // The whole reason this module exists: `contains` over the file passes on both of these.
        let parked = WORKFLOW.replace(
            "        run: nix run .#default-feature-tests",
            "      #  run: nix run .#default-feature-tests",
        );
        assert!(
            super::app_step(&parked, "ci", APP).is_none(),
            "a commented-out step invokes nothing"
        );
        let moved = super::app_step(WORKFLOW, "elsewhere", APP).expect("the copy in the other job");
        assert!(
            moved.join("\n").contains("continue-on-error: true"),
            "the other job's copy is what a whole-file scan would have found"
        );
        assert!(super::app_step(WORKFLOW, "ci", "not-an-app").is_none());
    }
}

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

/// One key of a step, whether it is written on the `-` line or below it.
///
/// Reading only the leading-key form hid `- run:` from the acceptance checks, and
/// `- continue-on-error: true` from the failure-downgrade check.
pub(crate) fn step_key(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed.strip_prefix('-').map_or(trimmed, str::trim_start)
}

/// The raw shell of every `run:` block, including its key and any shell comments.
///
/// A body ends at a nonempty line no deeper than the KEY's column, not the list marker's.
/// Otherwise an `env:` sibling after `- run: |` becomes shell. Expressions are left unchanged:
/// acceptance must reject even one written inside a shell comment.
///
/// This is an indentation reader, not a YAML parser. A `run:`-shaped line inside a prose block
/// scalar can still be collected; quoting, anchors and folded scalars are not evaluated.
pub(crate) fn shell<'a>(block: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside: Option<usize> = None;
    for line in block {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if let Some(depth) = inside {
            if !line.trim().is_empty() && indent <= depth {
                inside = None;
            } else {
                out.push(*line);
                continue;
            }
        }
        let key = step_key(line);
        if key.starts_with("run:") {
            inside = Some(line.len().saturating_sub(key.len()));
            out.push(key);
        }
    }
    out
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
    mod causality {
        //! Execute the real CI step against divergent main and topic histories, without running Nix.

        use std::process::Command;

        #[test]
        fn pr_causality_measures_the_topic_and_restores_the_merge_for_every_verdict() {
            let root = crate::repo::root().expect("the repository root");
            let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("the workflow");
            let step = crate::workflows::step::app_step(&workflow, "ci", "causality").expect("the live step");
            let body = crate::workflows::step::shell(&step)
                .into_iter()
                .skip(1)
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            let scratch = std::env::temp_dir().join(format!("sutura-pr-causality-{}", std::process::id()));
            #[expect(clippy::create_dir, reason = "exclusive creation refuses stale fixture state")]
            std::fs::create_dir(&scratch).expect("a new isolated fixture directory");
            let output = Command::new("bash")
                .args(["--noprofile", "--norc", "-c", HISTORY])
                .current_dir(&scratch)
                .env_remove("BASH_ENV")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_COMMON_DIR")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("CAUSALITY_STEP", body)
                .output()
                .expect("the workflow shell executes");
            std::fs::remove_dir_all(&scratch).expect("remove only this fixture");
            assert!(
                output.status.success(),
                "the real step chose the wrong tree, scope or status:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        /// The event's stale base predates the fork; main and the topic then add disjoint files.
        /// The fake app observes Git state and arguments, rather than matching the workflow's source.
        const HISTORY: &str = r#"
set -eu
git init --quiet --initial-branch=main --template= .
git config user.name Fixture
git config user.email user@example.com
git config commit.gpgsign false
git config core.hooksPath /dev/null
printf root > common.txt
git add common.txt
git commit --quiet -m root
stale=$(git rev-parse HEAD)
printf fork > fork.txt
git add fork.txt
git commit --quiet -m fork
fork=$(git rev-parse HEAD)
git checkout --quiet -b topic
printf topic > topic.txt
git add topic.txt
git commit --quiet -m topic
pr_head=$(git rev-parse HEAD)
git checkout --quiet main
printf main > main-only.txt
git add main-only.txt
git commit --quiet -m main
git merge --quiet --no-ff --no-edit topic
merge_head=$(git rev-parse HEAD)

nix() {
  [ "$#" -eq 5 ] && [ "$1" = run ] && [ "$2" = .#causality ] && [ "$3" = -- ] && [ "$4" = --since ] || return 97
  git rev-parse HEAD > seen-head
  printf '%s' "$5" > seen-base
  git diff --name-only "$5" HEAD > seen-paths
  return "$GATE_EXIT"
}
export -f nix
export EVENT PR_HEAD BASE GATE_EXIT
GITHUB_STEP_SUMMARY="$PWD/summary"
export GITHUB_STEP_SUMMARY

run_step() {
  local expected_exit=$1 expected_head=$2 expected_base=$3 expected_paths=$4
  local original_head actual_exit
  original_head=$(git rev-parse HEAD)
  printf not-called > seen-head
  : > seen-base
  : > seen-paths
  : > summary
  # A conditional around the shell would suppress its `set -e`; capture the status afterward.
  set +e
  bash --noprofile --norc -c "$CAUSALITY_STEP"
  actual_exit=$?
  set -e
  printf 'event=%s gate=%s step=%s\n' "$EVENT" "$GATE_EXIT" "$actual_exit"
  [ "$actual_exit" -eq "$expected_exit" ] || { echo wrong-exit; exit 1; }
  [ "$(git rev-parse HEAD)" = "$original_head" ] || { echo merge-not-restored; exit 1; }
  [ "$(cat seen-head)" = "$expected_head" ] || { echo wrong-head; cat seen-head; exit 1; }
  [ "$(cat seen-base)" = "$expected_base" ] || { echo wrong-base; cat seen-base; exit 1; }
  [ "$(cat seen-paths)" = "$expected_paths" ] || { echo wrong-paths; cat seen-paths; exit 1; }
}

for EVENT in pull_request push merge_group; do
  PR_HEAD=$pr_head
  if [ "$EVENT" = pull_request ]; then
    BASE=$stale
    expected_head=$pr_head
    expected_paths=topic.txt
  else
    BASE=$fork
    expected_head=$merge_head
    expected_paths=$(printf 'main-only.txt\ntopic.txt')
  fi
  for GATE_EXIT in 0 1 3; do
    expected_exit=$GATE_EXIT
    [ "$GATE_EXIT" -ne 3 ] || expected_exit=0
    run_step "$expected_exit" "$expected_head" "$fork" "$expected_paths"
    if [ "$GATE_EXIT" -eq 3 ]; then
      [ -s summary ] || { echo inconclusive-was-hidden; exit 1; }
    else
      [ ! -s summary ] || { echo invented-inconclusive; exit 1; }
    fi
  done
done

# PR causality derives its own fork even if ordinary classification had no usable base.
EVENT=pull_request
GATE_EXIT=0
BASE=
run_step 0 "$pr_head" "$fork" topic.txt

# A wrong event head or a non-merge checkout must refuse before invoking the app.
PR_HEAD=$fork
run_step 1 not-called '' ''
PR_HEAD=
run_step 1 not-called '' ''
PR_HEAD=$pr_head
git checkout --quiet --detach "$pr_head"
run_step 1 not-called '' ''
extra_merge=$(git commit-tree "$merge_head^{tree}" -p "$merge_head^1" -p "$pr_head" -p "$fork" -m extra-parent)
git checkout --quiet --detach "$extra_merge"
run_step 1 not-called '' ''
git checkout --quiet --detach "$merge_head"

# A change common to both trees could otherwise be carried silently through checkout.
printf dirty > common.txt
run_step 1 not-called '' ''
[ "$(cat common.txt)" = dirty ] || { echo discarded-dirty-work; exit 1; }
git add common.txt
run_step 1 not-called '' ''
[ "$(git show :common.txt)" = dirty ] || { echo discarded-staged-work; exit 1; }
printf root > common.txt
git add common.txt

# The existing empty-base policy remains unchanged for non-PR events.
for EVENT in push merge_group; do
  run_step 0 not-called '' ''
done
"#;
    }

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

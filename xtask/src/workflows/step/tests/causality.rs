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

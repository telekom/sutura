"""Supplemental named assertions; this does not run or replace the causality gate."""

from contextlib import contextmanager
import os
from pathlib import Path
import re
import tempfile

from oracle_raw_proof import digest, execute, git, manifest, record, state, tree

SOURCE = "dfdf267d7cd7f087bf9c58419654971e8afcc9c6"
TREE = "3808acc5e879ebddaf7e79af8d8da5efb1542cc1"
NOTES_BASE = "4edf3716ef854b233f45a28a21d139ea71abe1b9"
NOTES_BASE_TREE = "d78bcdf5f9ae062f393c8e272b713ad47bcc8445"

BASE = "745bf195f8b4e7598b1efd5b294664bf79b7bf6d"
BASE_TREE = "8c6bc9155482e9b3cc8adcec2cc4fd530111e9ca"
PERF = "xtask/src/workflows/performance_release.rs"
PROV = "xtask/tests/release_provenance.rs"
SHIPPED = "xtask/src/shipped.rs"
ORACLE = "crates/sutura-exec-datafusion/src/combine/statement/oracle.rs"
ORACLE_TEST = "crates/sutura-exec-datafusion/src/combine/statement/tests/oracle.rs"
ACTION = ".github/actions/push-images/action.yml"
ALIAS = "an_oracle_part_preserves_quoted_aliases_and_fetch_limits_alone_and_in_a_federation"
LISTS = "the_optimised_images_never_move_latest"
GUARD = "a_publish_from_the_performance_tag_is_refused"
SIX = "release_provenance_requires_both_sql_lists_and_all_six_exact_names"
REAL = "the_real_tree_agrees_with_itself"
NOTES = "the_notes_name_the_image_tags_that_were_pushed"
PACKAGE = {ALIAS: "sutura-exec-datafusion", LISTS: "xtask", GUARD: "xtask", SIX: "xtask", REAL: "xtask", NOTES: "xtask"}
LAUNCH = '''set -euo pipefail
unset BASH_ENV CARGO_NET_OFFLINE CARGO_PROFILE_DEV_CODEGEN_BACKEND CARGO_UNSTABLE_CODEGEN_BACKEND
export CARGO_TERM_COLOR=never NO_COLOR=1
export CARGO_HOME="$PWD/target/supplemental-cargo-home"
export CARGO_TARGET_DIR="$PWD/target/supplemental-target"
mkdir -p "$CARGO_HOME"
test ! -e "$CARGO_HOME/config.toml"
cargo() {
  test "$1" = nextest
  printf 'supplemental_cargo_context=%s\n' "$PWD" >&2
  nix run --no-write-lock-file .#cargo -- "$@"
}
export -f cargo
just test-only "$1"
'''


def run(cell, log):
    selected = f"package(={PACKAGE[cell]}) & test(/(^|::){cell}$/)"
    status, text = execute(
        ["nix", "shell", "--inputs-from", ".", "nixpkgs#just", "nixpkgs#cargo-nextest",
         "nixpkgs#clang", "nixpkgs#lld", "nixpkgs#pkg-config", "--command",
         "bash", "--noprofile", "--norc", "-c", LAUNCH, "supplemental", selected], log,
    )
    assert f"supplemental_cargo_context={Path.cwd()}\n" in text
    return status, text


def verdict(text, cell, status):
    assert re.search(rf"^\s*{status}\s+\[[^\]]+\].*\s(?:\S*::)?{re.escape(cell)}\s*$", text, re.MULTILINE)


def green(cell, log):
    status, text = run(cell, log)
    assert status == 0
    verdict(text, cell, "PASS")
    assert re.search(r"Summary.*1 test run: 1 passed", text)


def own_red(status, text, cell, test, marker, needles):
    assert status != 0
    verdict(text, cell, "FAIL")
    source = Path(test).read_text()
    start = source.index("fn " + cell + "(")
    line = source[:source.index(marker, start)].count("\n") + 1
    assert f"panicked at {test}:{line}:" in text
    assert all(needle in text for needle in needles)


@contextmanager
def overlay(out, label, path, content, expected_tree):
    before = state(out, label + "-before")
    file = Path(path)
    saved = (file.read_bytes(), file.stat().st_mode & 0o777) if file.exists() else None
    index_path = Path(git("rev-parse", "--git-path", "index").decode().strip()).resolve()
    index = index_path.read_bytes()
    stage = git("ls-files", "--stage", "-z")
    expected_stage = stage
    try:
        file.write_bytes(content)
        if saved is None:
            file.chmod(0o644)
            git("add", "-N", path)
        expected_stage = git("ls-files", "--stage", "-z")
        changed = state(out, label + "-overlay")
        assert tree() == expected_tree
        yield
        assert manifest() == changed and tree() == expected_tree
    finally:
        if saved is None:
            file.unlink(missing_ok=True)
        else:
            file.write_bytes(saved[0])
            file.chmod(saved[1])
        observed = digest(index_path.read_bytes())
        observed_stage = git("ls-files", "--stage", "-z")
        index_path.write_bytes(index)
        record(out / (label + "-index-restored.json"), {
            "before": digest(index), "observed": observed, "after": digest(index_path.read_bytes()),
            "stage_before": digest(stage), "stage_expected": digest(expected_stage), "stage_observed": digest(observed_stage),
        })
        assert observed_stage == expected_stage
        assert git("ls-files", "--stage", "-z") == stage
        assert state(out, label + "-restored") == before
        assert not git("status", "--porcelain")


def main():
    assert os.environ["GITHUB_ACTIONS"] == "true"
    assert git("rev-parse", "HEAD").decode().strip() == SOURCE
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert not git("status", "--porcelain")
    root = Path.cwd()
    index_path = Path(git("rev-parse", "--git-path", "index").decode().strip()).resolve()
    index = index_path.read_bytes()
    stage = git("ls-files", "--stage", "-z")
    out = root / "target/oracle-supplemental-proof"
    out.mkdir(parents=True, exist_ok=False)
    baseline = state(out, "head-before")
    files = {path: Path(path).read_bytes() for path in [PERF, PROV, SHIPPED, ORACLE, ACTION]}
    record(out / "inputs.json", {"source": SOURCE, "tree": TREE, "base": BASE, "base_tree": BASE_TREE,
                                 "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"], "payload_sha256": digest(Path(__file__).read_bytes()),
                                 "raw_payload_sha256": digest(Path(__file__).with_name("oracle_raw_proof.py").read_bytes()),
                                 "notes_base": NOTES_BASE, "notes_base_tree": NOTES_BASE_TREE})
    try:
        for cell in [LISTS, GUARD, SIX, REAL, NOTES]:
            green(cell, out / (cell + "-head.log"))
        base_root = Path(tempfile.mkdtemp(prefix="supplemental-base-", dir=root / "target"))
        git("worktree", "add", "--detach", str(base_root), BASE)
        os.chdir(base_root)
        assert tree() == BASE_TREE
        try:
            with overlay(out, "performance-base", PERF, files[PERF], "e63cd9dd83de2bee5d5657a1d2e34bffbd47bf9b"):
                own_red(*run(LISTS, out / "lists-base.log"), LISTS, PERF,
                        "assert_eq!(lists.len(), 4,", ["left: 2", "right: 4"])
                status, text = run(GUARD, out / "guard-base.log")
                guard_passes_base = status == 0
                if guard_passes_base:
                    verdict(text, GUARD, "PASS")
                else:
                    own_red(status, text, GUARD, PERF, "assert!(!run.succeeded,", ["assertion failed"])
                record(out / "guard-base-result.json", {"passed": guard_passes_base, "exception_accepted": False})
            with overlay(out, "provenance-base", PROV, files[PROV], "feda4a0587082cb756a4f8d3b31b99f5776f41a8"):
                own_red(*run(SIX, out / "provenance-base.log"), SIX, PROV,
                        "assert_eq!(result.status.code(),", ["complete:"])
            with overlay(out, "shipped-base", SHIPPED, files[SHIPPED], "693cdc1f4332a7b4b213142965344e8aeac00361"):
                own_red(*run(REAL, out / "shipped-base.log"), REAL, SHIPPED,
                        "assert_eq!(\n            references,", ["the set of declarations referencing the shipped set has changed"])
        finally:
            assert not git("status", "--porcelain")
            os.chdir(root)
            git("worktree", "remove", str(base_root))
        notes_root = Path(tempfile.mkdtemp(prefix="supplemental-notes-base-", dir=root / "target"))
        git("worktree", "add", "--detach", str(notes_root), NOTES_BASE)
        os.chdir(notes_root)
        assert tree() == NOTES_BASE_TREE
        try:
            with overlay(out, "notes-base", PERF, files[PERF], "de925809dc901edd5d5fb3a3e9dd8549b590e7bc"):
                own_red(*run(NOTES, out / "notes-base.log"), NOTES, PERF,
                        "assert_eq!(named, pushed,", ['left: ["ghcr.io/example/sutura:0.6.1-performance", "ghcr.io/example/sutura:0.6.1-performance-musl"]', 'right: ["ghcr.io/example/sutura:0.6.1-performance", "ghcr.io/example/sutura:0.6.1-performance-musl", "ghcr.io/example/sutura:0.6.1-performance-sql", "ghcr.io/example/sutura:0.6.1-performance-sql-musl"]'])
        finally:
            assert not git("status", "--porcelain")
            os.chdir(root)
            git("worktree", "remove", str(notes_root))
        for cell in [LISTS, GUARD, SIX, REAL, NOTES]:
            green(cell, out / (cell + "-restored.log"))
        if guard_passes_base:
            guard = next(line for line in files[ACTION].splitlines(keepends=True)
                         if b"*-performance) echo" in line and b"an optimised release" in line)
            with overlay(out, "refguard", ACTION, files[ACTION].replace(guard, b"", 1), "bf619caa27bdcc0632143c9f4edf21f930fc772e"):
                own_red(*run(GUARD, out / "refguard-red.log"), GUARD, PERF,
                        "assert!(!run.succeeded,", ["assertion failed"])
            green(GUARD, out / "refguard-restored.log")
        green(ALIAS, out / "alias-head.log")
        old = b"alias.explicit = false;"
        assert files[ORACLE].count(old) == 1
        with overlay(out, "alias", ORACLE, files[ORACLE].replace(old, b"alias.explicit = true;", 1),
                     "334fc610e75b4481a8d40b87feb9c9203a1876a4"):
            own_red(*run(ALIAS, out / "alias-red.log"), ALIAS, ORACLE_TEST,
                    r'assert!(!text.contains("AS \"r\""),', ['a table alias has no AS:'])
        green(ALIAS, out / "alias-restored.log")
    finally:
        os.chdir(root)
        observed = digest(index_path.read_bytes())
        observed_stage = git("ls-files", "--stage", "-z")
        index_path.write_bytes(index)
        record(out / "head-index-restored.json", {"before": digest(index), "observed": observed,
                                                   "after": digest(index_path.read_bytes()), "stage": digest(stage)})
        assert observed_stage == stage and git("ls-files", "--stage", "-z") == stage
        assert state(out, "head-restored") == baseline
        assert not git("status", "--porcelain")
        assert git("rev-parse", "HEAD").decode().strip() == SOURCE
        assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE


if __name__ == "__main__":
    main()

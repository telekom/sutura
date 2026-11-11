"""Focused planner rows on exact head and a single-test overlay over current main."""

import os
from pathlib import Path
import re
import tempfile

from oracle_raw_proof import SOURCE, TREE, digest, execute, git, manifest, named, record, red, state, tree

BASE = "745bf195f8b4e7598b1efd5b294664bf79b7bf6d"
BASE_TREE = "c4c88a8b180dbcf96db0a99f1c56f0d00ab54a84"
TESTS = "crates/sutura-exec-datafusion/src/combine/statement/tests.rs"
ROW = "combine::statement::tests::an_ordered_source_limit_keeps_its_offset_and_fetch_after_pushdown"
ORACLE = "combine::statement::tests::oracle::an_oracle_part_preserves_quoted_aliases_and_fetch_limits_alone_and_in_a_federation"


def run(cell: str, log: Path) -> tuple[int, str]:
    return execute(
        ["nix", "run", "--no-write-lock-file", ".#cargo", "--", "test", "--locked", "--profile", "ci",
         "-p", "sutura-exec-datafusion", "--all-features", "--lib", cell, "--", "--exact", "--nocapture"],
        log,
    )


def main() -> None:
    assert os.environ["GITHUB_ACTIONS"] == "true"
    assert git("rev-parse", "HEAD").decode().strip() == SOURCE
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert not git("status", "--porcelain")
    git("merge-base", "--is-ancestor", BASE, SOURCE)
    root = Path.cwd()
    out = root / "target/oracle-plan-proof"
    out.mkdir(parents=True, exist_ok=False)
    record(out / "inputs.json", {"source": SOURCE, "tree": TREE, "base": BASE, "base_overlay_tree": BASE_TREE,
                                 "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"], "payload_sha256": digest(Path(__file__).read_bytes()),
                                 "raw_payload_sha256": digest(Path(__file__).with_name("oracle_raw_proof.py").read_bytes())})
    baseline = state(out, "head-before")
    index_path = Path(git("rev-parse", "--git-path", "index").decode().strip()).resolve()
    index = index_path.read_bytes()
    stage = git("ls-files", "--stage", "-z")
    source = Path(TESTS).read_text(encoding="utf-8")
    start = source.index("#[test]\nfn " + ROW.rsplit("::", 1)[-1] + "(")
    end = source.index("\n}\n", start) + 3
    test = source[start:end] + "\n"
    try:
        status, text = run(ORACLE, out / "oracle-head.log")
        assert status == 0 and "test result: ok. 1 passed; 0 failed;" in text
        named(text, ORACLE, "ok")
        assert manifest() == baseline
        base_root = Path(tempfile.mkdtemp(prefix="oracle-plan-base-", dir=root / "target"))
        git("worktree", "add", "--detach", str(base_root), BASE)
        os.chdir(base_root)
        before = state(out, "base-before")
        base_index_path = Path(git("rev-parse", "--git-path", "index").decode().strip()).resolve()
        base_index = base_index_path.read_bytes()
        base_stage = git("ls-files", "--stage", "-z")
        original = Path(TESTS).read_bytes()
        mode = Path(TESTS).stat().st_mode & 0o777
        try:
            anchor = "/// Only a query plans: no definition, write, session statement, explanation or description, and no\n"
            old = original.decode("utf-8")
            assert old.count(anchor) == 1 and "mod oracle;" not in old
            assert ROW.rsplit("::", 1)[-1] not in old
            overlay = old.replace(anchor, test + anchor)
            Path(TESTS).write_text(overlay, encoding="utf-8")
            state(out, "base-test-only")
            assert tree() == BASE_TREE
            marker = "    assert_eq!(rows, legs.as_one(statement));"
            position = overlay.index(marker, overlay.index("fn " + ROW.rsplit("::", 1)[-1]))
            line = overlay[:position].count("\n") + 1
            status, text = run(ROW, out / "supported-base.log")
            red(status, text, ROW, TESTS, line, ["assertion `left == right` failed"])
            assert "test result: FAILED. 0 passed; 1 failed;" in text
            rows = re.search(r"\n\s*left: (.+)\n\s*right: (.+)", text)
            assert rows is not None
            assert 'rows: [[Text("north")], [Text("south")]]' in rows.group(1)
            assert 'rows: [[Text("north")]]' in rows.group(2)
            assert manifest() == {**before, TESTS: [digest(overlay.encode()), False, mode]}
            assert git("ls-files", "--stage", "-z") == base_stage
        finally:
            Path(TESTS).write_bytes(original)
            Path(TESTS).chmod(mode)
            assert state(out, "base-restored") == before
            assert not git("status", "--porcelain")
            assert git("ls-files", "--stage", "-z") == base_stage
            observed = digest(base_index_path.read_bytes())
            base_index_path.write_bytes(base_index)
            record(out / "base-index-restored.json", {"before": digest(base_index), "observed": observed,
                                                      "after": digest(base_index_path.read_bytes()), "stage": digest(base_stage)})
            os.chdir(root)
            git("worktree", "remove", str(base_root))
        status, text = run(ROW, out / "supported-head.log")
        assert status == 0 and "test result: ok. 1 passed; 0 failed;" in text
        named(text, ROW, "ok")
    finally:
        os.chdir(root)
        assert state(out, "head-restored") == baseline
        assert not git("status", "--porcelain")
        assert git("rev-parse", "HEAD").decode().strip() == SOURCE
        assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE
        assert git("ls-files", "--stage", "-z") == stage
        observed = digest(index_path.read_bytes())
        index_path.write_bytes(index)
        record(out / "head-index-restored.json", {"before": digest(index), "observed": observed,
                                                  "after": digest(index_path.read_bytes()), "stage": digest(stage)})


if __name__ == "__main__":
    main()

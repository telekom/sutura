"""One missing base-side configuration cause, without replaying head evidence."""

import importlib.util
import json
import os
import re
import shlex
import sys
from pathlib import Path


def main():
    assert os.environ["GITHUB_ACTIONS"] == "true"
    root = Path.cwd()
    frozen = json.loads(Path(sys.argv[1]).read_text())
    spec = importlib.util.spec_from_file_location(
        "dictionary_runtime", "/tmp/native-dictionary-runtime.py"
    )
    runtime = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runtime)
    assert runtime.sha(Path(spec.origin).read_bytes()) == frozen["runtime_sha256"]
    runtime.ROOT = root
    runtime.OUT = root / "target/native-config-cause"
    runtime.FROZEN = frozen
    runtime.LISTED = {}
    output = runtime.OUT
    output.mkdir(parents=True, exist_ok=False)
    git = runtime.git
    assert os.environ["SOURCE_SHA"] == frozen["head"]
    assert os.environ["SOURCE_TREE"] == frozen["head_tree"]
    assert os.environ["BASE_SHA"] == frozen["base"]
    assert git(root, "rev-parse", "HEAD").decode().strip() == frozen["head"]
    assert git(root, "rev-parse", "HEAD^{tree}").decode().strip() == frozen["head_tree"]
    assert (
        git(root, "rev-parse", frozen["base"] + "^{tree}").decode().strip()
        == frozen["base_tree"]
    )
    assert not git(root, "status", "--porcelain")
    git(root, "merge-base", "--is-ancestor", frozen["base"], frozen["head"])
    root_index = Path(
        git(root, "rev-parse", "--git-path", "index").decode().strip()
    ).resolve()
    original_index = root_index.read_bytes()
    original_stages = git(root, "ls-files", "--stage", "-z")
    original_files = runtime.inventory(root)
    runtime.record(
        output / "inputs.json",
        {
            "frozen": frozen,
            "helper": os.environ["GITHUB_WORKFLOW_SHA"],
            "payload_sha256": runtime.sha(Path(__file__).read_bytes()),
            "files": original_files,
        },
    )
    try:
        body = Path(runtime.program("causality")).read_text()
        assert (
            runtime.sha(git(root, "show", frozen["head"] + ":flake.nix"))
            == frozen["flake_sha256"]
        )
        cuts = list(re.finditer(frozen["wrapper_cut_before_regex"], body, re.MULTILINE))
        assert len(cuts) == 1
        prefix = "set -euo pipefail\n" + body[: cuts[0].start()]
        just = shlex.quote(runtime.program("just"))
        runtime.record(
            output / "wrapper.json",
            {
                "body_sha256": runtime.sha(body.encode()),
                "prefix_sha256": runtime.sha(prefix.encode()),
                "source": frozen["head"],
                "limit": "Final-source pinned tool environment; only the disposable base test overlay executes.",
            },
        )
        venue = output / "nextest-venue.sh"
        venue.write_text(
            prefix
            + '\nif [ "$1" = --list ]; then\n  mkdir -p target\n  cargo nextest list --workspace --all-features --message-format json > target/native-dictionary-runner-list.json\n  exit 0\nfi\n'
            + just
            + ' test-only "$1"\n'
        )
        (output / "fixture-clear.sh").write_text(prefix + "\n" + just + " dev-clear\n")
        (output / "fixture-cleanup.sh").write_text(
            prefix + "\n" + just + " dev-down\n" + just + " dev-clear\n"
        )
        runtime.NEXTEST = venue
        with runtime.worktree(frozen["base"], "base-config-unit") as base:
            before = runtime.inventory(base)
            path = frozen["overlay_path"]
            assert runtime.sha((base / path).read_bytes()) == frozen["base_file_sha256"]
            exact_final = git(root, "show", frozen["head"] + ":" + path)
            assert runtime.sha(exact_final) == frozen["overlay_sha256"]
            (base / path).write_bytes(exact_final)
            git(base, "add", "--", path)
            after = runtime.inventory(base)
            assert {
                name
                for name in set(before) | set(after)
                if before.get(name) != after.get(name)
            } == {path}
            assert git(base, "write-tree").decode().strip() == frozen["overlay_tree"]
            (output / "base-unit-overlay.patch").write_bytes(
                git(base, "diff", "--cached", "--binary")
            )
            runtime.record(output / "base-unit-overlay-files.json", after)
            runtime.assert_cell(base, frozen["cell"], "base-config-unit", frozen["red"])
        runtime.record(
            output / "summary.json",
            {
                "base_own_cause_witnesses": 1,
                "head_replayed": False,
                "native_replayed": False,
                "claim_replayed": False,
                "limits": "One raw named base-unit failure at its reviewed own expect site and cause only. Prior named unit head PASS remains attributed to846-associated generated-merge538 in its ordinary failed-CRAP run; no exact645 runtime claim.",
            },
        )
    finally:
        root_index.write_bytes(original_index)
        after_files = runtime.inventory(root)
        after_stages = git(root, "ls-files", "--stage", "-z")
        status = git(root, "status", "--porcelain").decode()
        runtime.record(
            output / "root-final.json",
            {
                "files_before": original_files,
                "files_after": after_files,
                "index_before_sha256": runtime.sha(original_index),
                "index_after_sha256": runtime.sha(root_index.read_bytes()),
                "stages_before_sha256": runtime.sha(original_stages),
                "stages_after_sha256": runtime.sha(after_stages),
                "status": status,
            },
        )
        assert after_files == original_files and after_stages == original_stages
        assert root_index.read_bytes() == original_index and not status


if __name__ == "__main__":
    main()

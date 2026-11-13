"""Hosted dictionary evidence. Frozen inputs must be reviewed after main integration."""

import contextlib
import hashlib
import json
import os
import re
import secrets
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

os.environ["GIT_OPTIONAL_LOCKS"] = "0"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args, data=None):
    return subprocess.check_output(["git", "-C", str(root), *args], input=data)


def record(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def inventory(root):
    result = {}
    for name in git(root, "ls-files", "-z").decode().split("\0"):
        if name:
            path = root / name
            link = path.is_symlink()
            data = os.fsencode(os.readlink(path)) if link else path.read_bytes()
            result[name] = [sha(data), link, path.lstat().st_mode & 0o777]
    return result


def execute(root, command, label):
    with tempfile.TemporaryFile(mode="w+") as stream:
        status = subprocess.run(
            ["timeout", "--kill-after=30s", "90m", *command],
            cwd=root,
            stdout=stream,
            stderr=subprocess.STDOUT,
            check=False,
        ).returncode
        stream.seek(0)
        text = stream.read()
    masks = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    masks += [os.environ.get("SUTURA_DEV_PASSWORD", "")]
    for value in sorted(filter(None, masks), key=len, reverse=True):
        text = text.replace(value, "***")
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    (OUT / (label + ".log")).write_text(text)
    record(
        OUT / (label + ".json"),
        {"status": status, "command": command, "source_root": str(root)},
    )
    print(text, end="", flush=True)
    return status, text


def assert_cell(root, cell, label, red=None):
    before = git(root, "diff", "--binary", "HEAD")
    stages = git(root, "ls-files", "--stage", "-z")
    if cell["venue"] == "oracle":
        command = ["bash", str(ORACLE), cell["name"]]
    else:
        if root not in LISTED:
            status, _ = execute(root, ["bash", str(NEXTEST), "--list"], label + "-list")
            assert status == 0
            LISTED[root] = json.loads(
                (root / "target/native-dictionary-runner-list.json").read_text()
            )
        suites = LISTED[root]["rust-suites"].values()
        assert (
            sum(
                s["package-name"] == cell["package"]
                and s["binary-name"] == cell["binary"]
                and cell["name"] in s["testcases"]
                for s in suites
            )
            == 1
        ), cell
        expression = (
            f"package(={cell['package']}) & binary(={cell['binary']}) "
            f"& test(/^{re.escape(cell['name'])}$/)"
        )
        command = ["bash", str(NEXTEST), expression]
    status, text = execute(root, command, label)
    assert git(root, "diff", "--binary", "HEAD") == before, (label, "source changed")
    assert git(root, "ls-files", "--stage", "-z") == stages, (label, "stages changed")
    if red is None:
        assert status == 0
        if cell["venue"] == "oracle":
            assert (
                f"test {cell['name']} ... ok" in text and "1 passed; 0 failed" in text
            )
        else:
            assert re.search(
                r"PASS\s+\[.*\].* " + re.escape(cell["name"]) + r"\s*$",
                text,
                re.MULTILINE,
            )
            assert re.search(r"Summary.*1 test run: 1 passed", text)
    else:
        assert status != 0
        assert (
            f"test {cell['name']} ... FAILED" in text
            if cell["venue"] == "oracle"
            else re.search(
                r"FAIL\s+\[.*\].* " + re.escape(cell["name"]) + r"\s*$",
                text,
                re.MULTILINE,
            )
        )
        assert any(
            f"panicked at {site['path']}:{site['line']}:" in text
            for site in red["sites"]
        )
        assert all(needle in text for needle in red["needles"]), (label, red)
        if "last_probe" in red:
            probes = re.findall(
                r"^native dictionary refusal probe: (.+)$", text, re.MULTILINE
            )
            assert probes and probes[-1] == red["last_probe"], (label, probes)
        assert "0 tests run" not in text


@contextlib.contextmanager
def worktree(revision, label):
    path = Path(tempfile.mkdtemp(prefix=label + "-", dir=ROOT / "target"))
    git(ROOT, "worktree", "add", "--detach", str(path), revision)
    before = inventory(path)
    index = Path(git(path, "rev-parse", "--git-path", "index").decode().strip())
    if not index.is_absolute():
        index = path / index
    saved_index = index.read_bytes()
    saved_stages = git(path, "ls-files", "--stage", "-z")
    try:
        assert (
            execute(
                path, ["bash", str(OUT / "fixture-clear.sh")], label + "-clear-before"
            )[0]
            == 0
        )
        record(
            OUT / (label + "-input.json"),
            {
                "revision": revision,
                "tree": git(path, "rev-parse", "HEAD^{tree}").decode().strip(),
                "files": before,
            },
        )
        yield path
    finally:
        cleanup_status, _ = execute(
            path, ["bash", str(OUT / "fixture-cleanup.sh")], label + "-fixture-cleanup"
        )
        current = inventory(path)
        for name in git(path, "ls-files", "-z").decode().split("\0"):
            if name and name not in before:
                (path / name).unlink()
        for name, (_, link, mode) in before.items():
            if current.get(name) == before[name]:
                continue
            target = path / name
            original = git(path, "show", revision + ":" + name)
            if target.is_symlink():
                target.unlink()
            if link:
                target.unlink(missing_ok=True)
                target.symlink_to(os.fsdecode(original))
            else:
                target.write_bytes(original)
                target.chmod(mode)
        index.write_bytes(saved_index)
        assert inventory(path) == before and not git(path, "status", "--porcelain")
        assert git(path, "ls-files", "--stage", "-z") == saved_stages
        index.write_bytes(saved_index)
        assert index.read_bytes() == saved_index
        record(
            OUT / (label + "-restored.json"),
            {
                "files": before,
                "stages_sha256": sha(saved_stages),
                "index_before_sha256": sha(saved_index),
                "index_after_sha256": sha(index.read_bytes()),
            },
        )
        # Existing tier tasks own teardown; remove only our disposable worktree.
        git(ROOT, "worktree", "remove", str(path))
        assert cleanup_status == 0


def program(app):
    attr = f"{ROOT}#apps.x86_64-linux.{app}.program"
    path = (
        subprocess.check_output(
            ["nix", "eval", "--no-write-lock-file", "--raw", attr], cwd=ROOT
        )
        .decode()
        .strip()
    )
    context = json.loads(
        subprocess.check_output(
            [
                "nix",
                "eval",
                "--no-write-lock-file",
                "--json",
                "--apply",
                "builtins.getContext",
                attr,
            ],
            cwd=ROOT,
        )
    )
    assert len(context) == 1 and all(key.endswith(".drv") for key in context)
    subprocess.run(["nix-store", "--realise", *context], cwd=ROOT, check=True)
    return path


def wrappers():
    just = program("just")
    for app, key in [("causality", "nextest"), ("oracle-acceptance", "oracle")]:
        executable = program(app)
        body = Path(executable).read_text()
        pin = FROZEN["wrappers"][key]
        assert (
            sha(git(ROOT, "show", FROZEN["head"] + ":flake.nix"))
            == FROZEN["flake_sha256"]
        )
        cuts = list(re.finditer(pin["cut_before_regex"], body, re.MULTILINE))
        assert len(cuts) == 1
        prefix = "set -euo pipefail\n" + body[: cuts[0].start()]
        record(
            OUT / (key + "-wrapper.json"),
            {
                "head": FROZEN["head"],
                "program": executable,
                "program_sha256": sha(body.encode()),
                "prefix_sha256": sha(prefix.encode()),
                "limit": "H venue wrapper; cargo compiles the current worktree source",
            },
        )
        suffix = (
            (
                '\nif [ "$1" = --list ]; then\n'
                "  mkdir -p target\n"
                "  cargo nextest list --workspace --all-features --message-format json > target/native-dictionary-runner-list.json\n"
                "  exit 0\nfi\n"
                "command -v sutura-postgres-tier\nexport SUTURA_DEV_REQUIRE_TIER=1\n"
                + shlex.quote(just)
                + ' test "$1"\n'
            )
            if key == "nextest"
            else (
                "\nexport SUTURA_DEV_REQUIRE_TIER=1\n"
                'cargo test --locked --profile ci -p sutura-cli --all-features --test served "$1" -- --exact --ignored --nocapture\n'
            )
        )
        path = OUT / (key + "-venue.sh")
        path.write_text(prefix + suffix)
        if key == "nextest":
            (OUT / "fixture-cleanup.sh").write_text(
                prefix
                + "\n"
                + shlex.quote(just)
                + " dev-down\n"
                + shlex.quote(just)
                + " dev-clear\n"
            )
            (OUT / "fixture-clear.sh").write_text(
                prefix + "\n" + shlex.quote(just) + " dev-clear\n"
            )
    return OUT / "nextest-venue.sh", OUT / "oracle-venue.sh"


def main():
    global ROOT, OUT, FROZEN, NEXTEST, ORACLE, LISTED
    assert os.environ["GITHUB_ACTIONS"] == "true"
    ROOT = Path.cwd()
    FROZEN = json.loads(Path(sys.argv[1]).read_text())
    LISTED = {}
    assert FROZEN["status"] == "frozen-final"
    assert os.environ["SOURCE_SHA"] == FROZEN["head"]
    assert os.environ["SOURCE_TREE"] == FROZEN["head_tree"]
    assert os.environ["BASE_SHA"] == FROZEN["base"]
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == FROZEN["head"]
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == FROZEN["head_tree"]
    assert not git(ROOT, "status", "--porcelain")
    git(ROOT, "merge-base", "--is-ancestor", FROZEN["base"], FROZEN["head"])
    assert (
        git(ROOT, "rev-parse", FROZEN["base"] + "^{tree}").decode().strip()
        == FROZEN["base_tree"]
    )
    OUT = ROOT / "target/native-dictionary-proof"
    OUT.mkdir(parents=True, exist_ok=False)
    original = inventory(ROOT)
    for revision in [FROZEN["head"], FROZEN["base"]]:
        for name, digest in FROZEN["locks"][revision].items():
            assert sha(git(ROOT, "show", revision + ":" + name)) == digest
    record(
        OUT / "inputs.json",
        {
            "frozen": FROZEN,
            "helper": os.environ["GITHUB_WORKFLOW_SHA"],
            "payload_sha256": sha(Path(__file__).read_bytes()),
            "files": original,
        },
    )
    os.environ["SUTURA_DEV_PASSWORD"] = secrets.token_hex(12)
    NEXTEST, ORACLE = wrappers()
    cells = FROZEN["cells"]
    assert len(cells) == 9 and sum(c["venue"] == "oracle" for c in cells) == 1
    assert (
        execute(ROOT, ["bash", str(OUT / "fixture-clear.sh")], "head-clear-before")[0]
        == 0
    )
    try:
        for number, cell in enumerate(cells):
            assert_cell(ROOT, cell, f"head-{number}")
    finally:
        assert (
            execute(
                ROOT, ["bash", str(OUT / "fixture-cleanup.sh")], "head-fixture-cleanup"
            )[0]
            == 0
        )
    with worktree(FROZEN["base"], "base") as base:
        baseline = inventory(base)
        for name, digest in FROZEN["overlays"].items():
            data = git(ROOT, "show", FROZEN["head"] + ":" + name)
            assert sha(data) == digest
            (base / name).parent.mkdir(parents=True, exist_ok=True)
            (base / name).write_bytes(data)
            if name not in baseline:
                git(base, "add", "-N", "--", name)
        function = FROZEN["reader_function"]
        path = base / function["path"]
        old, new = function["old"].encode(), function["new"].encode()
        assert path.read_bytes().count(old) == 1
        path.write_bytes(path.read_bytes().replace(old, new))
        assert set(FROZEN["overlays"]) == {
            "crates/sutura-config/tests/native_dictionary.rs",
            "crates/sutura-cli/tests/served/rdbms.rs",
            "crates/sutura-cli/tests/served/native_oracle_dictionary.rs",
            "crates/sutura-cli/tests/served.rs",
        }
        assert function["path"] == "crates/sutura-cli/src/serve/tests/rdbms_served.rs"
        assert new in git(ROOT, "show", FROZEN["head"] + ":" + function["path"])
        allowed = set(FROZEN["overlays"]) | {function["path"]}
        git(base, "add", "--", *sorted(allowed))
        after = inventory(base)
        assert {
            n for n in set(baseline) | set(after) if baseline.get(n) != after.get(n)
        } == allowed
        assert git(base, "write-tree").decode().strip() == FROZEN["base_overlay_tree"]
        (OUT / "base-overlay.patch").write_bytes(
            git(base, "diff", "--cached", "--binary")
        )
        record(OUT / "base-overlay-files.json", after)
        for number, cell in enumerate(cells):
            assert_cell(base, cell, f"base-{number}", cell["base_red"])
    assert len(FROZEN["mutations"]) == 5
    for mutation in FROZEN["mutations"]:
        with worktree(FROZEN["head"], mutation["name"]) as mutant:
            for number, witness in enumerate(mutation["witnesses"]):
                assert_cell(
                    mutant, witness["cell"], f"{mutation['name']}-{number}-head"
                )
            patch = mutation["patch"].encode()
            assert sha(patch) == mutation["patch_sha256"]
            git(mutant, "apply", "--check", "-", data=patch)
            git(mutant, "apply", "-", data=patch)
            changed = {
                n for n, value in inventory(mutant).items() if original.get(n) != value
            }
            assert changed == {mutation["path"]}
            assert mutation["path"].startswith(
                ("crates/sutura-catalog-rdbms/src/", "crates/sutura-domain/src/")
            )
            (OUT / (mutation["name"] + ".patch")).write_bytes(patch)
            for number, witness in enumerate(mutation["witnesses"]):
                assert_cell(
                    mutant,
                    witness["cell"],
                    f"{mutation['name']}-{number}-red",
                    witness["red"],
                )
            git(mutant, "apply", "--reverse", "-", data=patch)
            assert inventory(mutant) == original
            for number, witness in enumerate(mutation["witnesses"]):
                assert_cell(
                    mutant, witness["cell"], f"{mutation['name']}-{number}-restored"
                )
    assert inventory(ROOT) == original and not git(ROOT, "status", "--porcelain")
    record(
        OUT / "summary.json",
        {
            "stable_cells": 9,
            "head_green": 9,
            "base_own_assertion_red": 9,
            "mutations": len(FROZEN["mutations"]),
            "source_restored": True,
            "limits": "Five native claims use existing singleton workflow separately; no final CI/validate credit.",
        },
    )


if __name__ == "__main__":
    root_index = Path(
        git(Path.cwd(), "rev-parse", "--git-path", "index").decode().strip()
    ).resolve()
    original_index = root_index.read_bytes()
    try:
        main()
    finally:
        root_index.write_bytes(original_index)
        assert root_index.read_bytes() == original_index
        if "OUT" in globals() and OUT.exists():
            record(
                OUT / "root-index-restored.json",
                {"before": sha(original_index), "after": sha(root_index.read_bytes())},
            )

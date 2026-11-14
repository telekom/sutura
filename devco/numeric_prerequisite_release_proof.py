"""Hosted diagnostic of index writes through the sixth scoped claim kill."""

import base64
import copy
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

import tomllib

BASE = "b8bce349b79fc16891f8bfd466ba7187cafdf019"
BASE_TREE = "2191984dba94ae674e39f101b48d07a619d72255"
PREPARED_TREE = "ef25185218ea0ac728ea9903abbdffd091e830a9"
PATCH_SHA256 = "66e9374f267ead8454c43b081d5388478fda7035b37f80782e0a24dfb88d66c6"
REQUEST_ID = "60782fd2-5a25-44d2-87eb-7166c77e56eb"
REF = "refs/heads/ci/numeric-prerequisite-release-proof"
PAYLOAD_HASHES = {
    "source-manifest.json": "f5c8aa0331ce3db86aef63701b37ab4309499f0dd7775cd1893c5214223032fd",
    "claims.json": "bf46193691ec1aecaea518abe79df6de73bcce9a72e3e3b1f5bd572c36be7bb3",
    "numeric-followup-plan.json": "e254f3794b744ad14a4fc5c9d441e3a832ae879f86eff653c38e2d6dfde1a584",
    "input-bindings.json": "cf7401dc294fad442648075bb07b10a2a5b11b0925f970333b84d090504e87d7",
}
RETAINED_LOCK_SHA256 = (
    "506947e68501e0afb6c8dc83b6cbaed0c1645630d7cb0526e8dae81bba9d6596"
)
REVIEW_SHA256 = "10a2b0f6c23696b55f9e1981766a917552bbe8adc92e452b166b435fa029fec5"
EDGES = ("sutura-exec-node", "sutura-http", "sutura-mcp", "tonic")
TEST_FILE = "crates/sutura-app/src/federated/tests/split_test.rs"
TEST_PREFIX = "federated::tests::split_test::"
ROWS = 'Object {"Rows": Object {"limit": Number(999)}}'
VOLUME = 'String("Volume")'


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write(out: Path, name: str, value: object) -> None:
    (out / name).write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", "--no-optional-locks", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
    ).stdout


def index(root: Path) -> bytes:
    path = (
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    return Path(path).read_bytes()


def capture_index(
    out: Path, label: str, details: dict, *, expected: bytes | None
) -> bytes | None:
    record = {
        "phase": label,
        "details": details,
        "request_id": REQUEST_ID,
        "helper_commit": os.environ["GITHUB_SHA"],
        "helper_tree": os.environ["HELPER_TREE"],
        "run_id": os.environ["GITHUB_RUN_ID"],
        "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "source_tree": PREPARED_TREE,
        "optional_locks": os.environ.get("GIT_OPTIONAL_LOCKS"),
        "started_ns": time.time_ns(),
        "started_monotonic_ns": time.monotonic_ns(),
        "stable": False,
    }
    raw = None
    try:
        path = Path((out / "scratch-index-path.txt").read_text(encoding="utf-8"))
        record["path"] = str(path)
        before = path.stat()
        with path.open("rb") as stream:
            fd_before = os.fstat(stream.fileno())
            raw = stream.read()
            fd_after = os.fstat(stream.fileno())
        after = path.stat()
        record["stats"] = {
            name: {
                key: getattr(value, key)
                for key in (
                    "st_dev",
                    "st_ino",
                    "st_mode",
                    "st_size",
                    "st_mtime_ns",
                    "st_ctime_ns",
                )
            }
            for name, value in (
                ("path_before", before),
                ("fd_before", fd_before),
                ("fd_after", fd_after),
                ("path_after", after),
            )
        }
        record["stable"] = (
            stat.S_ISREG(fd_before.st_mode)
            and len(raw) == fd_before.st_size
            and all(
                value == record["stats"]["fd_before"]
                for value in record["stats"].values()
            )
        )
    except OSError as error:
        record["error"] = str(error)
    record["finished_ns"] = time.time_ns()
    record["finished_monotonic_ns"] = time.monotonic_ns()
    record["size"] = None if raw is None else len(raw)
    record["sha256"] = None if raw is None else digest(raw)
    record["expected_sha256"] = None if expected is None else digest(expected)
    if expected is None or raw != expected or not record["stable"]:
        if raw is not None:
            (out / f"{label}-index.bin").write_bytes(raw)
        write(out, f"{label}-index.json", record)
    return raw if record["stable"] else None


def manifest(root: Path) -> dict:
    result = {}
    for name in git(root, "ls-files", "-z").decode().split("\0"):
        if not name:
            continue
        path = root / name
        data = os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
        result[name] = {
            "sha256": digest(data),
            "size": len(data),
            "mode": oct(stat.S_IMODE(path.lstat().st_mode)),
            "symlink": path.is_symlink(),
        }
    return result


def execute(
    root: Path,
    out: Path,
    label: str,
    argv: list[str],
    *,
    saved_index: bytes | None = None,
) -> tuple[int, str]:
    print(f"{label}: starting", flush=True)
    log_path = out / f"{label}.log"
    with log_path.open("wb") as log:
        if saved_index is not None:
            require(
                capture_index(
                    out, label + "-before-cargo", {"argv": argv}, expected=None
                )
                == saved_index,
                "scratch index bytes changed before Cargo",
            )
        status = None
        try:
            status = subprocess.run(
                argv, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=False
            ).returncode
        finally:
            if saved_index is not None:
                require(
                    capture_index(
                        out,
                        label + "-after-cargo",
                        {"argv": argv, "exit": status},
                        expected=None,
                    )
                    == saved_index,
                    "scratch index bytes changed during Cargo",
                )
    raw = log_path.read_bytes()
    write(
        out, f"{label}.json", {"argv": argv, "exit": status, "log_sha256": digest(raw)}
    )
    print(f"{label}: exit {status}", flush=True)
    return status, re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", raw.decode(errors="replace"))


def immutable(tools: Path, out: Path) -> None:
    require(
        git(tools, "rev-parse", "HEAD").decode().strip() == BASE, "tools base drift"
    )
    require(
        git(tools, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "tools tree drift",
    )
    require(
        not git(tools, "status", "--porcelain", "--untracked-files=all"),
        "dirty tools checkout",
    )
    expected = json.loads((out / "tools-manifest.json").read_text(encoding="utf-8"))
    require(manifest(tools) == expected, "immutable tools bytes changed")


def exact(
    root: Path, out: Path, tools: Path, expected: dict, saved_index: bytes
) -> None:
    require(
        git(root, "rev-parse", "HEAD").decode().strip() == BASE, "scratch HEAD changed"
    )
    require(manifest(root) == expected, "source/lock byte or mode drift")
    require(
        capture_index(out, f"strict-{time.time_ns()}", {}, expected=saved_index)
        == saved_index,
        "scratch index bytes changed",
    )
    require(
        not git(root, "ls-files", "--others", "--exclude-standard"),
        "unexpected nonignored source",
    )
    immutable(tools, out)


def normalized_lock(data: dict) -> dict:
    result = copy.deepcopy(data)
    for package in result["package"]:
        if "dependencies" in package:
            package["dependencies"].sort()
    result["package"].sort(
        key=lambda package: (
            package["name"],
            package["version"],
            package.get("source", ""),
        )
    )
    return result


def lock_delta(before: bytes, after: bytes) -> dict:
    require(before != after, "retained root lock has no release delta")
    expected = tomllib.loads(before.decode())
    apps = [
        package for package in expected["package"] if package["name"] == "sutura-app"
    ]
    require(
        len(apps) == 1 and apps[0]["version"] == "0.14.0", "wrong release app record"
    )
    dependencies = apps[0]["dependencies"]
    require(
        not set(EDGES).intersection(dependencies), "requested app edges already present"
    )
    dependencies.extend(EDGES)
    actual = tomllib.loads(after.decode())
    require(
        normalized_lock(actual) == normalized_lock(expected),
        "unexpected semantic lock delta",
    )
    return {
        "package": "sutura-app",
        "version": "0.14.0",
        "added": EDGES,
        "all_other_records_equal": True,
    }


def verdict(
    text: str, status: int, cell: dict, failure_line: int | None
) -> dict | None:
    name = TEST_PREFIX + cell["name"]
    require(
        re.findall(r"^running (\d+) tests?$", text, re.MULTILINE) == ["1"],
        "not exactly one test",
    )
    require(
        len(re.findall(rf"^test {re.escape(name)} \.\.\. ", text, re.MULTILINE)) == 1,
        "wrong named test",
    )
    expected = "FAILED" if failure_line else "ok"
    require(
        re.search(
            rf"^{expected}$|^test {re.escape(name)} \.\.\. {expected}$",
            text,
            re.MULTILINE,
        )
        is not None,
        "missing test result",
    )
    summary = (
        "FAILED. 0 passed; 1 failed; 0 ignored;"
        if failure_line
        else "ok. 1 passed; 0 failed; 0 ignored;"
    )
    require(
        len(re.findall(r"^test result:", text, re.MULTILINE)) == 1, "wrong result count"
    )
    require(
        re.search(
            rf"^test result: {re.escape(summary)} 0 measured; \d+ filtered out; finished in .+$",
            text,
            re.MULTILINE,
        )
        is not None,
        "wrong test summary",
    )
    require(status == (101 if failure_line else 0), "wrong process exit")
    panics = re.findall(
        r"^thread '([^'\n]+)'(?: \([^\n]*\))? panicked at ([^\n]+):(\d+):\d+:\n",
        text,
        re.MULTILINE,
    )
    if failure_line is None:
        require(not panics, "panic in passing cell")
        return None
    require(
        panics == [(name, TEST_FILE, str(failure_line))],
        "not the intended own assertion",
    )
    panic = text[text.index("thread '") :].split("\n\n", 1)[0]
    require("assertion `left == right` failed" in panic, "not the comparison failure")
    for side, value in (("left", ROWS), ("right", VOLUME)):
        require(
            re.search(rf"^\s*{side}: {re.escape(value)}$", panic, re.MULTILINE)
            is not None,
            "wrong assertion values",
        )
    return {"cell": name, "site": f"{TEST_FILE}:{failure_line}", "panic": panic}


def parser_check(out: Path, cell: dict) -> None:
    # ponytail: hosted parser check only; synthetic logs earn no product credit.
    name = TEST_PREFIX + cell["name"]
    line = cell["own_assertion_line"]
    prefix = f"running 1 test\ntest {name} ... "
    green = (
        prefix
        + "ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    )
    red = (
        prefix
        + f"\nthread '{name}' (1) panicked at {TEST_FILE}:{line}:1:\nassertion `left == right` failed\n  left: {ROWS}\n right: {VOLUME}\n\nFAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    )
    verdict(green, 0, cell, None)
    verdict(red, 101, cell, line)
    rejected = [
        ("error: could not compile\n", 101, line),
        (green.replace("running 1 test", "running 0 tests"), 0, None),
        (green.replace("0 ignored", "1 ignored"), 0, None),
        (red.replace(name, "tests::another_cell"), 101, line),
        (red.replace(f":{line}:1:", f":{line + 1}:1:"), 101, line),
        (red.replace("assertion `left == right` failed", "setup failed"), 101, line),
        (red.replace(ROWS, 'String("unexpected")'), 101, line),
        (red, 0, line),
        (green, 101, None),
    ]
    for text, status, failure_line in rejected:
        try:
            verdict(text, status, cell, failure_line)
        except RuntimeError:
            continue
        raise RuntimeError("parser accepted invalid evidence")
    write(
        out,
        "parser-self-check.json",
        {"positive": 2, "rejected": len(rejected), "product_credit": False},
    )


def cell_test(
    root: Path,
    out: Path,
    label: str,
    cell: dict,
    failure: bool = False,
    *,
    saved_index: bytes | None = None,
) -> None:
    argv = [
        "cargo",
        "test",
        "--locked",
        "--profile",
        "ci",
        "--all-features",
        "-p",
        "sutura-app",
        "--lib",
        TEST_PREFIX + cell["name"],
        "--",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]
    status, text = execute(root, out, label, argv, saved_index=saved_index)
    try:
        own = verdict(
            text, status, cell, cell["own_assertion_line"] if failure else None
        )
    except RuntimeError as error:
        write(
            out,
            label + "-verdict.json",
            {"cell": cell["name"], "status": "REFUSED", "reason": str(error)},
        )
        raise
    write(
        out,
        label + "-verdict.json",
        {
            "cell": cell["name"],
            "status": "own comparison failed" if own else "pass",
            "own_assertion": own,
        },
    )


def claims(
    root: Path,
    out: Path,
    tools: Path,
    payload: Path,
    generated: dict,
    saved_index: bytes,
) -> None:
    cells = json.loads((payload / "claims.json").read_text(encoding="utf-8"))
    require(
        len(cells) == 10 and len({cell["name"] for cell in cells}) == 10,
        "wrong claim inventory",
    )
    parser_check(out, cells[0])
    source = (root / TEST_FILE).read_bytes()
    targets = ("crates/sutura-app/src/federated/leg.rs", "crates/sutura-app/src/lib.rs")
    originals = {name: (root / name).read_bytes() for name in targets}
    completed = []
    for number, cell in enumerate(cells[:6], 1):
        label = f"claim-{number:02d}-{cell['name']}"
        exact(root, out, tools, generated, saved_index)
        cell_test(root, out, label + "-baseline", cell)
        exact(root, out, tools, generated, saved_index)
        patch = root / cell["mutation_path"]
        require(digest(patch.read_bytes()) == cell["mutation_sha256"], "mutation drift")
        touched = git(root, "apply", "--numstat", str(patch)).decode().splitlines()
        require(len(touched) == 1, "mutation must touch exactly one source file")
        target = touched[0].split("\t")[-1]
        require(target in targets, "mutation touches unexpected source")
        try:
            git(root, "apply", "--check", str(patch))
            git(root, "apply", str(patch))
            mutated = manifest(root)
            expected = dict(generated)
            expected[target] = mutated[target]
            require(
                expected[target] != generated[target],
                "mutation did not change its target",
            )
            exact(root, out, tools, expected, saved_index)
            require(
                (root / TEST_FILE).read_bytes() == source, "mutation changed own tests"
            )
            write(
                out,
                label + "-mutation.json",
                {
                    "cell": cell,
                    "target": target,
                    "manifest": expected,
                    "index_sha256": digest(saved_index),
                },
            )
            diff_argv = [
                "git",
                "--no-optional-locks",
                "-C",
                str(root),
                "diff",
                "--binary",
                "--full-index",
            ]
            if number == 6:
                require(
                    capture_index(
                        out, label + "-before-diff", {"argv": diff_argv}, expected=None
                    )
                    == saved_index,
                    "scratch index bytes changed before diff",
                )
            diff_exit = None
            try:
                (out / (label + "-applied.patch")).write_bytes(
                    git(root, "diff", "--binary", "--full-index")
                )
                diff_exit = 0
            except subprocess.CalledProcessError as error:
                diff_exit = error.returncode
                raise
            finally:
                if number == 6:
                    require(
                        capture_index(
                            out,
                            label + "-after-diff",
                            {"argv": diff_argv, "exit": diff_exit},
                            expected=None,
                        )
                        == saved_index,
                        "scratch index bytes changed during diff",
                    )
            cell_test(
                root,
                out,
                label + "-kill",
                cell,
                True,
                saved_index=saved_index if number == 6 else None,
            )
            exact(root, out, tools, expected, saved_index)
        finally:
            (root / target).write_bytes(originals[target])
            (root / target).chmod(int(generated[target]["mode"], 8))
            if number == 6:
                restored_index = capture_index(
                    out, label + "-after-restore", {"target": target}, expected=None
                )
            exact(root, out, tools, generated, saved_index)
            if number == 6:
                require(
                    restored_index == saved_index,
                    "missing, unstable or changed restored index capture",
                )
            if number < 6:
                cell_test(root, out, label + "-restored", cell)
            exact(root, out, tools, generated, saved_index)
            write(
                out,
                label + "-restored-state.json",
                {
                    "manifest_sha256": digest(
                        (out / "generated-manifest.json").read_bytes()
                    ),
                    "index_sha256": digest(saved_index),
                    "pristine": True,
                    "restored_test_run": number < 6,
                },
            )
        if number < 6:
            completed.append(cell["name"])
    write(
        out,
        "claims-verdict.json",
        {
            "status": "DIAGNOSTIC_ONLY_NO_PREREQUISITE_PASS",
            "completed_baseline_own_kill_restored": completed,
            "count": len(completed),
            "sixth_baseline_and_kill_only": cells[5]["name"],
            "claim_phase_cap": 17,
            "remaining_cells_unrun": [cell["name"] for cell in cells[6:]],
            "limit": "Opaque loop kill reaches its first case; baseline/restored cover both cases. No ordinary causality or full-suite credit.",
        },
    )


def checks(root: Path, out: Path, tools: Path, payload: Path) -> None:
    generated = json.loads(
        (out / "generated-manifest.json").read_text(encoding="utf-8")
    )
    saved_index = (out / "generated-index.bin").read_bytes()
    exact(root, out, tools, generated, saved_index)
    resolved = {}
    for tool in ("cargo", "rustc", "clang", "ld.lld", "just", "jscpd"):
        path = shutil.which(tool)
        require(
            path is not None and path.startswith("/nix/store/"),
            f"unpinned tool: {tool}",
        )
        resolved[tool] = path
        require(
            execute(root, out, f"version-{tool}", [tool, "--version"])[0] == 0, tool
        )
    write(
        out,
        "environment.json",
        {
            "tools": resolved,
            "cargo_home": os.environ["CARGO_HOME"],
            "target_dir": os.environ["CARGO_TARGET_DIR"],
        },
    )
    for label, argv in (
        (
            "cargo-check",
            [
                "cargo",
                "check",
                "--locked",
                "-p",
                "sutura-app",
                "-p",
                "sutura-exec-node",
                "--all-targets",
                "--all-features",
            ],
        ),
        ("just-lint", ["just", "lint"]),
    ):
        status, _ = execute(root, out, label, argv)
        exact(root, out, tools, generated, saved_index)
        require(status == 0, f"{label} failed; no later phase attempted")
    claims(root, out, tools, payload, generated, saved_index)
    exact(root, out, tools, generated, saved_index)


def main() -> None:
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    root = Path.cwd().resolve()
    helper = Path(__file__).resolve().parents[1]
    tools = Path(os.environ["TOOLS_ROOT"]).resolve()
    out = Path(os.environ["RUNNER_TEMP"]) / (
        "numeric-prerequisite-release-proof-" + REQUEST_ID
    )
    payload = helper / "devco/numeric-prerequisite-release-proof"
    require(
        os.environ.get("GITHUB_ACTIONS") == "true"
        and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch",
        "hosted dispatch only",
    )
    require(os.environ.get("GITHUB_REPOSITORY") == "telekom/sutura", "wrong repository")
    require(
        os.environ.get("GITHUB_REF") == REF
        and os.environ.get("REQUEST_ID") == REQUEST_ID,
        "wrong ref/request",
    )
    require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "spent request; no reruns")
    require(
        re.fullmatch("[0-9a-f]{40}", os.environ.get("HELPER_TREE", "")) is not None,
        "bad helper tree",
    )
    require(
        git(helper, "rev-parse", "HEAD").decode().strip() == os.environ["GITHUB_SHA"],
        "wrong helper commit",
    )
    require(
        git(helper, "rev-parse", "HEAD^{tree}").decode().strip()
        == os.environ["HELPER_TREE"],
        "wrong helper tree",
    )
    require(
        not git(helper, "status", "--porcelain", "--untracked-files=all"),
        "dirty helper",
    )
    require(len({root, helper, tools}) == 3, "checkouts must be separate")
    require(
        git(root, "rev-parse", "HEAD").decode().strip() == BASE, "wrong scratch base"
    )
    require(
        git(root, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "wrong scratch tree",
    )
    for name, expected in PAYLOAD_HASHES.items():
        require(
            digest((payload / name).read_bytes()) == expected, "wrong payload: " + name
        )
    if sys.argv[1:] == ["checks"]:
        checks(root, out, tools, payload)
        return
    require(not sys.argv[1:], "no configurable operation")
    require(
        not git(root, "status", "--porcelain", "--untracked-files=all"),
        "dirty source scratch",
    )
    require(
        git(tools, "rev-parse", "HEAD").decode().strip() == BASE, "wrong tools base"
    )
    require(
        git(tools, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "wrong tools tree",
    )
    require(
        not git(tools, "status", "--porcelain", "--untracked-files=all"), "dirty tools"
    )
    out.mkdir(exist_ok=False)
    (out / "scratch-index-path.txt").write_text(
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip(),
        encoding="utf-8",
    )
    write(out, "verdict.json", {"status": "INCOMPLETE", "tests": "UNRUN"})
    write(
        out,
        "request.json",
        {
            "ref": REF.removeprefix("refs/heads/"),
            "inputs": {
                "helper_tree": os.environ["HELPER_TREE"],
                "request_id": REQUEST_ID,
            },
        },
    )
    write(
        out,
        "provenance.json",
        {
            "base": BASE,
            "base_tree": BASE_TREE,
            "prepared_tree": PREPARED_TREE,
            "helper_commit": os.environ["GITHUB_SHA"],
            "helper_tree": os.environ["HELPER_TREE"],
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
            "payload_hashes": PAYLOAD_HASHES,
        },
    )
    write(out, "tools-manifest.json", manifest(tools))
    write(out, "base-manifest.json", manifest(root))
    (out / "base-index.bin").write_bytes(index(root))
    for name in PAYLOAD_HASHES:
        shutil.copyfile(payload / name, out / name)
    inputs = json.loads((payload / "input-bindings.json").read_text(encoding="utf-8"))
    for name, binding in inputs["release_locks"].items():
        require(
            digest((root / name).read_bytes()) == binding["sha256"],
            "wrong release lock input",
        )
    original_lock = (root / "Cargo.lock").read_bytes()
    (out / "release-Cargo.lock").write_bytes(original_lock)
    shutil.copyfile(root / "fuzz/Cargo.lock", out / "release-fuzz-Cargo.lock")
    patch_data = base64.b64decode(
        b"".join((payload / "source.patch.b64").read_bytes().splitlines()),
        validate=True,
    )
    require(digest(patch_data) == PATCH_SHA256, "wrong source patch")
    patch = out / "accepted-source.patch"
    patch.write_bytes(patch_data)
    generated = None
    generated_index = None
    success = False
    try:
        git(root, "apply", "--check", "--index", "--binary", str(patch))
        git(root, "apply", "--index", "--binary", str(patch))
        require(
            git(root, "write-tree").decode().strip() == PREPARED_TREE,
            "wrong prepared tree",
        )
        git(root, "diff", "--exit-code")
        prepared = manifest(root)
        write(out, "prepared-manifest.json", prepared)
        accepted = json.loads(
            (payload / "source-manifest.json").read_text(encoding="utf-8")
        )
        require(
            accepted["base"] == BASE and accepted["prepared_tree"] == PREPARED_TREE,
            "wrong source binding",
        )
        for name, expected in accepted["files"].items():
            require(
                all(
                    prepared[name][key] == expected[key]
                    for key in ("sha256", "size", "mode")
                ),
                name,
            )
        for name, expected in inputs["cargo_manifests"].items():
            require(
                prepared[name]["sha256"] == expected["sha256"], "Cargo manifest drift"
            )
        for name, expected in inputs["locks"].items():
            require(
                prepared[name]["sha256"] == expected["sha256"],
                "prepared lock differs from accepted binding",
            )
        for key in tuple(os.environ):
            if key.startswith(("CARGO_", "RUSTUP_")) or key in (
                "RUSTC",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "RUSTFLAGS",
                "RUSTDOCFLAGS",
                "DUCKDB_LIB_DIR",
                "DUCKDB_INCLUDE_DIR",
                "BASH_ENV",
            ):
                os.environ.pop(key)
        os.environ["CARGO_HOME"] = str(
            out.parent / ("numeric-release-cargo-" + REQUEST_ID)
        )
        os.environ["CARGO_TARGET_DIR"] = str(
            out.parent / ("numeric-release-target-" + REQUEST_ID)
        )
        os.environ["CARGO_TERM_COLOR"] = "never"
        os.environ["NO_COLOR"] = "1"
        Path(os.environ["CARGO_HOME"]).mkdir(exist_ok=False)
        Path(os.environ["CARGO_TARGET_DIR"]).mkdir(exist_ok=False)
        require(
            digest((root / "Cargo.lock").read_bytes()) == RETAINED_LOCK_SHA256,
            "wrong accepted retained root lock",
        )
        require(
            inputs["retained_lock_review_sha256"] == REVIEW_SHA256,
            "wrong retained-lock review binding",
        )
        shutil.copyfile(root / "Cargo.lock", out / "generated-Cargo.lock")
        write(out, "retained-lock-manifest.json", prepared)
        write(
            out,
            "lock-semantic-delta.json",
            lock_delta(original_lock, (root / "Cargo.lock").read_bytes()),
        )
        immutable(tools, out)
        git(root, "diff", "--exit-code")
        require(
            git(root, "write-tree").decode().strip() == PREPARED_TREE,
            "retained source index drift",
        )
        generated_tree = git(root, "write-tree").decode().strip()
        generated = manifest(root)
        generated_index = index(root)
        write(out, "generated-manifest.json", generated)
        (out / "generated-index.bin").write_bytes(generated_index)
        generated_patch = git(
            root, "diff", "--binary", "--full-index", BASE, generated_tree
        )
        (out / "generated-source.patch").write_bytes(generated_patch)
        write(
            out,
            "generated-source.json",
            {
                "base": BASE,
                "prepared_tree": PREPARED_TREE,
                "generated_tree": generated_tree,
                "lock_origin": "accepted retained generation; no generator run",
                "retained_lock_review_sha256": REVIEW_SHA256,
                "root_lock_sha256": digest((root / "Cargo.lock").read_bytes()),
                "fuzz_lock_sha256": digest((root / "fuzz/Cargo.lock").read_bytes()),
                "patch_sha256": digest(generated_patch),
                "manifest_sha256": digest(
                    (out / "generated-manifest.json").read_bytes()
                ),
                "claims_sha256": PAYLOAD_HASHES["claims.json"],
                "followup_sha256": PAYLOAD_HASHES["numeric-followup-plan.json"],
            },
        )
        status, _ = execute(
            root,
            out,
            "check-and-claim-environment",
            [
                "nix",
                "develop",
                "--impure",
                "--file",
                str(helper / "devco/numeric_prerequisite_release_proof.nix"),
                "--argstr",
                "base",
                str(tools),
                "--command",
                "python3",
                "-I",
                str(Path(__file__).resolve()),
                "checks",
            ],
        )
        exact(root, out, tools, generated, generated_index)
        require(status == 0, "hosted check/lint/claim phase failed")
        success = True
    finally:
        final_entry = capture_index(out, "final-entry", {}, expected=None)
        final = manifest(root)
        write(out, "final-manifest.json", final)
        (out / "final-index.bin").write_bytes(index(root))
        shutil.copyfile(root / "Cargo.lock", out / "final-Cargo.lock")
        shutil.copyfile(root / "fuzz/Cargo.lock", out / "final-fuzz-Cargo.lock")
        write(
            out,
            "final-status.json",
            {
                "status": git(
                    root, "status", "--porcelain=v2", "--untracked-files=all"
                ).decode()
            },
        )
        final_after_status = capture_index(out, "final-after-status", {}, expected=None)
        intact = (
            None
            if generated is None
            else (
                final == generated
                and index(root) == generated_index
                and final_entry == generated_index
                and final_after_status == generated_index
            )
        )
        write(
            out,
            "verdict.json",
            {
                "status": "DIAGNOSTIC_ONLY_NO_PREREQUISITE_PASS"
                if success and intact
                else "FAILED",
                "source_and_index_intact": intact,
                "diagnostic_only": True,
                "claim_phase_cap": 17,
                "ordinary_causality": "UNRUN",
                "validate": "UNRUN",
                "ship_check": "UNRUN",
                "numeric_followup": "UNRUN",
                "Oracle_focused": "UNRUN",
                "final_actual_main_union": "RECHECK REQUIRED",
                "limit": "Frozen release base and exact generated source only; no full-suite, admission, publication, queue or final-main credit.",
            },
        )
        require(
            intact is not False, "source/index not restored; final evidence retained"
        )
        require(
            final_entry is not None and final_after_status is not None,
            "missing or unstable final index capture",
        )
        immutable(tools, out)


if __name__ == "__main__":
    main()

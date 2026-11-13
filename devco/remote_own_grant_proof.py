"""Twelve exact-source native checks; numeric promotion and ordinary gates stay separate."""

import base64
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import uuid
import zlib
from pathlib import Path

import tomllib


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(root), *args])


def write(name: str, value: object) -> None:
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def redact(text: str) -> str:
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets.extend(
        value
        for key, value in os.environ.items()
        if re.search(r"PASSWORD|TOKEN|SECRET|CREDENTIAL", key)
    )
    for secret in sorted(filter(None, secrets), key=len, reverse=True):
        text = text.replace(secret, "***")
    return re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)


def execute(label: str, command: list[str], root: Path, env: dict) -> tuple[int, str]:
    print(label, flush=True)
    with tempfile.TemporaryFile() as output:
        result = subprocess.run(
            command,
            cwd=root,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            check=False,
        )
        output.seek(0)
        text = redact(output.read().decode(errors="replace"))
    (OUT / (label + ".log")).write_text(text, encoding="utf-8")
    write(
        label + "-process.json",
        {
            "command": command,
            "exit": result.returncode,
            "log_sha256": digest(text.encode()),
        },
    )
    return result.returncode, re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)


def file_state(path: Path) -> dict:
    mode = path.lstat().st_mode
    if stat.S_ISLNK(mode):
        git_mode, data = "120000", os.fsencode(os.readlink(path))
    else:
        assert stat.S_ISREG(mode), "unsupported source entry"
        git_mode = "100755" if mode & 0o111 else "100644"
        data = path.read_bytes()
    return {
        "mode": git_mode,
        "permissions": stat.S_IMODE(mode),
        "blob": hashlib.sha1(
            b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False
        ).hexdigest(),
        "sha256": digest(data),
    }


def state(root: Path) -> dict:
    return {
        str(path.relative_to(root)): file_state(path)
        for path in sorted(root.rglob("*"))
        if path.is_symlink() or not path.is_dir()
    }


def verify(label: str, expected: dict) -> None:
    observed = state(EXECUTION)
    write(
        label + "-source.json",
        {
            "expected_tree": expected["tree"],
            "manifest": observed,
            "expected_manifest_equal": observed == expected["files"],
            "locks": {name: observed.get(name) for name in SPEC["lock_paths"]},
        },
    )
    assert observed == expected["files"], (
        "full source bytes, permissions, entries or locks differ"
    )


def checkout_state(root: Path) -> dict:
    names = git(root, "ls-files", "-z").decode().rstrip("\0").split("\0")
    index = Path(
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    return {
        "head": git(root, "rev-parse", "HEAD").decode().strip(),
        "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
        "tracked": {name: file_state(root / name) for name in names},
        "index_sha256": digest(index.read_bytes()),
        "staged_entries_sha256": digest(git(root, "ls-files", "--stage", "-z")),
        "tracked_status": git(
            root, "status", "--porcelain", "--untracked-files=no"
        ).decode(),
    }


def guard_sources() -> None:
    for name, root in CHECKOUT_ROOTS.items():
        assert checkout_state(root) == CHECKOUTS[name], (
            "source/helper checkout or index changed: " + name
        )


def remove_owned() -> None:
    assert (
        EXECUTION.parent == SCRATCH
        and EXECUTION.name == "execution"
        and not EXECUTION.is_symlink()
    )
    if EXECUTION.exists():
        shutil.rmtree(EXECUTION)
    EXECUTION.mkdir()


def materialize_base(label: str) -> None:
    remove_owned()
    archive = git(SOURCE, "archive", "--format=tar", SPEC["source_base_sha"])
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        stream.extractall(EXECUTION, filter="data")
    for name, entry in SPEC["states"]["actual-base"]["files"].items():
        if entry["mode"] != "120000":
            (EXECUTION / name).chmod(entry["permissions"])
    assert not (EXECUTION / ".git").exists()
    verify(label, SPEC["states"]["actual-base"])
    guard_sources()


def apply(label: str, patch: dict, env: dict) -> None:
    data = patch["text"].encode()
    assert digest(data) == patch["sha256"]
    path = OUT / (label + ".patch")
    path.write_bytes(data)
    command = ["git", "apply", "--no-index", "--whitespace=error"]
    for suffix, arguments in (("check", ["--check"]), ("apply", [])):
        status, _ = execute(
            label + "-" + suffix, [*command, *arguments, str(path)], EXECUTION, env
        )
        assert status == 0, "patch setup is not behavior evidence"


def snapshot() -> dict:
    return {
        name: (
            entry,
            os.fsencode(os.readlink(EXECUTION / name))
            if entry["mode"] == "120000"
            else (EXECUTION / name).read_bytes(),
        )
        for name, entry in state(EXECUTION).items()
    }


def restore(original: dict, label: str) -> None:
    remove_owned()
    for name, (entry, data) in original.items():
        path = EXECUTION / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if entry["mode"] == "120000":
            path.symlink_to(os.fsdecode(data))
        else:
            path.write_bytes(data)
            path.chmod(entry["permissions"])
    verify(label, SPEC["states"]["head"])
    guard_sources()


def own_result(status: int, text: str, row: dict) -> dict:
    cell = row["cell"]
    assert re.findall(r"^running (\d+) tests?$", text, re.MULTILINE) == ["1"], (
        "not exactly one executed cell"
    )
    assert len(re.findall(rf"^test {re.escape(cell)} \.\.\. ", text, re.MULTILINE)) == 1
    failed = row["exit"] == 101
    word = "FAILED" if failed else "ok"
    summary = (
        "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
        if failed
        else "test result: ok. 1 passed; 0 failed; 0 ignored;"
    )
    assert re.search(
        rf"^{word}$|^test {re.escape(cell)} \.\.\. {word}$", text, re.MULTILINE
    )
    assert (
        len(re.findall(r"^test result: ", text, re.MULTILINE)) == 1 and summary in text
    )
    assert status == row["exit"], "compile/setup/timeout/other exit is not a witness"
    assert "NOT RUN" not in text and "requires a provisioned service tier" not in text
    if failed:
        site = row["own_site"]
        assert len(re.findall(r"^thread [^\n]+ panicked at ", text, re.MULTILINE)) == 1
        panic = re.search(
            rf"^thread '{re.escape(cell)}'(?: \([^\n]*\))? panicked at {re.escape(site['path'])}:{site['line']}:\d+:\n([^\n]+)",
            text,
            re.MULTILINE,
        )
        assert panic and site["message"] in panic[1], (
            "not the selected cell's exact first own assertion"
        )
        return {"accepted": True, "selected": 1, "own_panic": panic[0]}
    return {"accepted": True, "selected": 1}


def run_row(row: dict, env: dict) -> dict:
    label = row["name"]
    expected = SPEC["states"][row["state"]]
    guard_sources()
    verify(label + "-before", expected)
    cargo = str(Path(env["REMOTE_GRANT_TOOLCHAIN"], "bin/cargo"))
    status, _ = execute(
        label + "-freshness",
        [
            "timeout",
            "--kill-after=30s",
            "5m",
            cargo,
            "clean",
            "--locked",
            "--package",
            "sutura-exec-datafusion",
        ],
        EXECUTION,
        env,
    )
    assert status == 0, "crate artifact cleanup failed; no stale binary may count"
    verify(label + "-after-clean", expected)
    command = [
        "timeout",
        "--kill-after=30s",
        "30m",
        cargo,
        "test",
        "--locked",
        "--all-features",
        "--package",
        "sutura-exec-datafusion",
        "--lib",
        row["cell"],
        "--",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]
    status, text = execute(label, command, EXECUTION, env)
    result = {
        "row": row,
        "exit": status,
        "accepted": False,
        "actual_base_sha": SPEC["source_base_sha"],
        "candidate_tree": SPEC["candidate_tree"],
        "executed_tree": expected["tree"],
    }
    write(label + "-result.json", result)
    try:
        result.update(own_result(status, text, row))
    except AssertionError as error:
        result["reason"] = redact(str(error))
    verify(label + "-after", expected)
    guard_sources()
    write(label + "-result.json", result)
    return result


def proof(env: dict) -> list[dict]:
    materialize_base("initial-actual-base")
    apply("prepared-source", SPEC["patches"]["source"], env)
    verify("prepared-head", SPEC["states"]["head"])
    original = snapshot()
    results = []
    try:
        for row in SPEC["rows"]:
            verify("independent-" + row["name"], SPEC["states"]["head"])
            try:
                if row["state"] == "base-overlay":
                    materialize_base(row["name"] + "-actual-base")
                    apply(
                        row["name"] + "-overlay",
                        SPEC["patches"]["base-test-overlay"],
                        env,
                    )
                elif row["state"] != "head":
                    apply(row["name"] + "-mutation", SPEC["patches"][row["state"]], env)
                result = run_row(row, env)
                results.append(result)
                write("rows.json", results)
                if row["exit"] == 0:
                    assert result["accepted"], (
                        "pristine/restored head failed; stop the batch"
                    )
            finally:
                if row["state"] != "head":
                    restore(original, "restoration-after-" + row["name"])
        assert len(results) == 12
    finally:
        restore(original, "final-restoration")
    return results


if __name__ == "__main__":
    assert sys.version_info >= (3, 12), "source extraction requires the data filter"
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    HELPER = Path(__file__).resolve().parent.parent
    SOURCE = HELPER / "base-source"
    SPEC_PATH = HELPER / "devco/remote-own-grant-proof/spec.json"
    SPEC = json.loads(SPEC_PATH.read_text(encoding="utf-8"))
    assert os.environ["GITHUB_REF_NAME"] == SPEC["ref"]
    assert (
        os.environ["REQUEST_ID"]
        == SPEC["request_id"]
        == str(uuid.UUID(os.environ["REQUEST_ID"]))
    )
    assert (
        git(HELPER, "rev-parse", "HEAD").decode().strip()
        == os.environ["GITHUB_WORKFLOW_SHA"]
    )
    assert (
        git(HELPER, "rev-parse", "HEAD^{tree}").decode().strip()
        == os.environ["HELPER_TREE"]
    )
    assert (
        git(SOURCE, "rev-parse", "HEAD").decode().strip()
        == os.environ["BASE_SHA"]
        == SPEC["source_base_sha"]
    )
    assert (
        git(SOURCE, "rev-parse", "HEAD^{tree}").decode().strip()
        == os.environ["BASE_TREE"]
        == SPEC["source_base_tree"]
    )
    assert os.environ["CANDIDATE_TREE"] == SPEC["candidate_tree"]
    assert (
        SPEC["states"]["head"]["tree"] == SPEC["candidate_tree"]
        and SPEC["numeric_promotion"] is False
    )
    raw = zlib.decompress(base64.b85decode(SPEC["head_manifest"]["data"]))
    assert (
        len(raw) == SPEC["head_manifest"]["raw_bytes"]
        and digest(raw) == SPEC["head_manifest"]["raw_sha256"]
    )
    head = json.loads(raw)
    assert len(head) == SPEC["head_manifest"]["entries"]
    for expected in SPEC["states"].values():
        compact = head | expected["changed_files"]
        for name in expected["removed_files"]:
            del compact[name]
        expected["files"] = {
            name: {
                "mode": mode,
                "permissions": {"100644": 0o644, "100755": 0o755, "120000": 0o777}[
                    mode
                ],
                "blob": blob,
                "sha256": sha256,
            }
            for name, (mode, blob, sha256) in compact.items()
        }
    assert len(SPEC["rows"]) == 12
    CHECKOUT_ROOTS = {"helper": HELPER, "base-source": SOURCE}
    CHECKOUTS = {name: checkout_state(root) for name, root in CHECKOUT_ROOTS.items()}
    assert all(not entry["tracked_status"] for entry in CHECKOUTS.values())
    OUT = HELPER / "target/remote-own-grant-proof"
    OUT.mkdir(parents=True, exist_ok=False)
    request = {
        "ref": SPEC["ref"],
        "inputs": {
            "base_sha": os.environ["BASE_SHA"],
            "base_tree": os.environ["BASE_TREE"],
            "candidate_tree": os.environ["CANDIDATE_TREE"],
            "helper_tree": os.environ["HELPER_TREE"],
            "request_id": SPEC["request_id"],
        },
    }
    write("request.json", request)
    write("checkout-initial.json", CHECKOUTS)
    write("verdict.json", {"status": "INCOMPLETE", "accepted_12_rows": False})
    write(
        "provenance.json",
        {
            "request_id": SPEC["request_id"],
            "request_sha256": digest(json.dumps(request, indent=2).encode() + b"\n"),
            "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
            "helper_tree": os.environ["HELPER_TREE"],
            "actual_base_sha": SPEC["source_base_sha"],
            "actual_base_tree": SPEC["source_base_tree"],
            "prepared_source_tree": SPEC["candidate_tree"],
            "payload_sha256": digest(Path(__file__).read_bytes()),
            "spec_sha256": digest(SPEC_PATH.read_bytes()),
            "workflow_sha256": digest(
                (HELPER / ".github/workflows/format.yml").read_bytes()
            ),
            "environment_sha256": digest(
                (HELPER / "devco/remote-own-grant-env.nix").read_bytes()
            ),
            "root_source_review_sha256": SPEC["root_source_review_sha256"],
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": 1,
        },
    )
    pin = git(SOURCE, "show", SPEC["source_base_sha"] + ":" + SPEC["toolchain_path"])
    assert digest(pin) == SPEC["toolchain_sha256"]
    channel = tomllib.loads(pin.decode())["toolchain"]["channel"]
    toolchain = Path(os.environ["REMOTE_GRANT_TOOLCHAIN"])
    assert str(toolchain).startswith("/nix/store/") and re.fullmatch(
        r"[A-Za-z0-9.-]+", channel
    )
    env = os.environ.copy()
    cleared = [
        key
        for key in env
        if key.startswith("CARGO_")
        or (key.startswith("DUCKDB_") and key.endswith("_DIR"))
        or key
        in (
            "RUSTFLAGS",
            "RUSTC",
            "RUSTDOC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTUP_TOOLCHAIN",
            "BASH_ENV",
        )
    ]
    for key in cleared:
        del env[key]
    with tempfile.TemporaryDirectory(
        prefix="remote-own-grant-", dir=os.environ["RUNNER_TEMP"]
    ) as directory:
        SCRATCH = Path(directory).resolve()
        EXECUTION = SCRATCH / "execution"
        env.update(
            CARGO_HOME=str(SCRATCH / "cargo-home"),
            CARGO_TARGET_DIR=str(SCRATCH / "target"),
            CARGO_TERM_COLOR="never",
            NO_COLOR="1",
            INSTA_UPDATE="no",
            RUSTC=str(toolchain / "bin/rustc"),
            RUSTDOC=str(toolchain / "bin/rustdoc"),
            PATH=str(toolchain / "bin") + os.pathsep + env["PATH"],
        )
        Path(env["CARGO_HOME"]).mkdir()
        write(
            "environment.json",
            {
                "toolchain_channel": channel,
                "toolchain_store_path": str(toolchain),
                "pin_sha256": digest(pin),
                "cleared_variable_names": cleared,
                "cargo_home": env["CARGO_HOME"],
                "target": env["CARGO_TARGET_DIR"],
                "fresh_leaf_artifacts_each_row": True,
                "all_features": True,
                "locked": True,
            },
        )
        for tool in ("cargo", "rustc"):
            status, _ = execute(
                tool + "-version", [str(toolchain / "bin" / tool), "-vV"], HELPER, env
            )
            assert status == 0
        try:
            results = proof(env)
        finally:
            write(
                "checkout-final.json",
                {name: checkout_state(root) for name, root in CHECKOUT_ROOTS.items()},
            )
            guard_sources()
    accepted = len(results) == 12 and all(result["accepted"] for result in results)
    write(
        "verdict.json",
        {
            "status": "ALL_12_ACCEPTED" if accepted else "INCOMPLETE_OR_NONVERDICT",
            "accepted_12_rows": accepted,
            "rows": results,
            "numeric_promotion": False,
            "limits": SPEC["limits"],
            "runtime_receipt_limit": SPEC["runtime_receipt_limit"],
        },
    )
    assert accepted, "all twelve terminal qualified rows are required"

"""Sixteen source-bound connector executions; full gates and native data remain separate."""

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


def verify(label: str, root: Path, expected: dict) -> None:
    observed = state(root)
    write(
        label + "-source.json",
        {
            "expected_execution_tree": expected["execution_tree"],
            "frozen_origin_tree": expected["origin_tree"],
            "manifest": observed,
            "expected_manifest_equal": observed == expected["files"],
            "lock_sha256": observed.get("Cargo.lock", {}).get("sha256"),
        },
    )
    assert observed == expected["files"], (
        "source bytes, permissions, entries or lock differ from frozen execution tree"
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
    for name, root in SOURCES.items():
        assert checkout_state(root) == CHECKOUTS[name], (
            "source/helper checkout or index changed: " + name
        )


def remove_owned(root: Path) -> None:
    assert (
        root.parent == SCRATCH
        and root.name in ("upstream", "vendor")
        and not root.is_symlink()
    )
    if root.exists():
        shutil.rmtree(root)
    root.mkdir()


def materialize(
    root: Path, source: Path, revision: str, package: dict, expected: dict
) -> None:
    remove_owned(root)
    tree = revision + (":" + package["prefix"] if package["prefix"] else "^{tree}")
    assert git(source, "rev-parse", tree).decode().strip() == expected["package_tree"]
    archive = git(source, "archive", "--format=tar", tree)
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        stream.extractall(root, filter="data")
    if package["name"] == "vendor":
        lock = git(
            source, "show", package["source_sha"] + ":" + SPEC["vendor_lock_path"]
        )
        assert digest(lock) == SPEC["vendor_lock_sha256"]
        (root / "Cargo.lock").write_bytes(lock)
    for name, entry in expected["files"].items():
        if entry["mode"] != "120000":
            (root / name).chmod(entry["permissions"])
    verify(package["name"] + "-materialized-" + revision, root, expected)


def snapshot(root: Path) -> dict:
    return {
        name: (
            entry,
            os.fsencode(os.readlink(root / name))
            if entry["mode"] == "120000"
            else (root / name).read_bytes(),
        )
        for name, entry in state(root).items()
    }


def restore(root: Path, original: dict, package: dict, label: str) -> None:
    remove_owned(root)
    for name, (entry, data) in original.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if entry["mode"] == "120000":
            path.symlink_to(os.fsdecode(data))
        else:
            path.write_bytes(data)
            path.chmod(entry["permissions"])
    verify(label, root, package["states"]["head"])
    guard_sources()


def apply(label: str, root: Path, patch: dict, env: dict) -> None:
    data = patch["text"].encode()
    assert digest(data) == patch["sha256"]
    path = OUT / (label + ".patch")
    path.write_bytes(data)
    command = ["git", "apply", "--no-index", "-p" + str(patch["strip"])]
    for suffix, arguments in (("check", ["--check"]), ("apply", [])):
        status, _ = execute(
            label + "-" + suffix, [*command, *arguments, str(path)], root, env
        )
        assert status == 0, "patch application is not behavior evidence"


def own_result(status: int, text: str, row: dict, cell: str) -> dict:
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
        assert len(re.findall(r"^thread [^\n]+ panicked at ", text, re.MULTILINE)) == 1
        panic = re.search(
            rf"^thread '{re.escape(cell)}'(?: \([^\n]*\))? panicked at src/lib\.rs:{row['line']}:\d+:\n([^\n]+)",
            text,
            re.MULTILINE,
        )
        assert panic and row["message"] in panic[1], (
            "not the selected test's exact first own assertion"
        )
        return {"accepted": True, "own_panic": panic[0], "selected": 1}
    return {"accepted": True, "selected": 1}


def run_row(package: dict, row: dict, root: Path, env: dict) -> dict:
    label = package["name"] + "-" + row["name"]
    expected = package["states"][row["state"]]
    guard_sources()
    verify(label + "-before", root, expected)
    command = [
        "timeout",
        "--kill-after=30s",
        "30m",
        str(Path(env["CONNECTOR_TOOLCHAIN"], "bin/cargo")),
        "test",
        "--locked",
        "--all-features",
        "--lib",
        package["cell"],
        "--",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]
    status, text = execute(label, command, root, env)
    result = {
        "package": package["name"],
        "row": row,
        "exit": status,
        "accepted": False,
        "actual_source_sha": package["source_sha"],
        "actual_source_tree": package["source_tree"],
        "execution_tree": expected["execution_tree"],
    }
    write(label + "-result.json", result)
    try:
        result.update(own_result(status, text, row, package["cell"]))
    except AssertionError as error:
        result["reason"] = redact(str(error))
    verify(label + "-after", root, expected)
    guard_sources()
    write(label + "-result.json", result)
    return result


def proof(package: dict, env: dict) -> list[dict]:
    name = package["name"]
    source = SOURCES[name]
    assert git(source, "rev-parse", "HEAD").decode().strip() == package["source_sha"]
    assert (
        git(source, "rev-parse", "HEAD^{tree}").decode().strip()
        == package["source_tree"]
    )
    assert (
        git(source, "merge-base", package["base_sha"], package["source_sha"])
        .decode()
        .strip()
        == package["base_sha"]
    )
    pin = git(source, "show", package["source_sha"] + ":" + package["toolchain_path"])
    assert digest(pin) == package["toolchain_sha256"]
    channel = tomllib.loads(pin.decode())["toolchain"]["channel"]
    assert re.fullmatch(r"[A-Za-z0-9.-]+", channel)
    env = env | {
        "CARGO_HOME": str(SCRATCH / (name + "-cargo-home")),
        "CARGO_TARGET_DIR": str(SCRATCH / (name + "-target")),
    }
    toolchain = Path(env["CONNECTOR_" + name.upper() + "_TOOLCHAIN"])
    assert str(toolchain).startswith("/nix/store/")
    env["CONNECTOR_TOOLCHAIN"] = str(toolchain)
    env["RUSTC"] = str(toolchain / "bin/rustc")
    Path(env["CARGO_HOME"]).mkdir()
    write(
        name + "-environment.json",
        {
            "toolchain_channel": channel,
            "toolchain_store_path": str(toolchain),
            "toolchain_file_sha256": digest(pin),
            "cleared_variable_names": CLEARED,
            "cargo_home": env["CARGO_HOME"],
            "target": env["CARGO_TARGET_DIR"],
            "all_features": True,
            "locked": True,
        },
    )
    for tool in ("cargo", "rustc"):
        status, _ = execute(
            name + "-" + tool + "-version",
            [str(toolchain / "bin" / tool), "-vV"],
            HELPER,
            env,
        )
        assert status == 0
    root = SCRATCH / name
    materialize(root, source, package["source_sha"], package, package["states"]["head"])
    original = snapshot(root)
    results = []
    try:
        for row in package["rows"]:
            if row["state"] == "base-overlay":
                materialize(
                    root,
                    source,
                    package["base_sha"],
                    package,
                    package["states"]["actual-base"],
                )
                apply(
                    name + "-base-overlay",
                    root,
                    package["patches"]["base-test-overlay"],
                    env,
                )
            elif row["state"] != "head":
                verify(
                    name + "-independent-" + row["name"],
                    root,
                    package["states"]["head"],
                )
                apply(
                    name + "-" + row["name"],
                    root,
                    package["patches"][row["state"]],
                    env,
                )
            try:
                result = run_row(package, row, root, env)
                results.append(result)
                write(name + "-rows.json", results)
                if row["exit"] == 0:
                    assert result["accepted"], (
                        "pristine/restored head failed; stop the batch"
                    )
            finally:
                if row["state"] != "head":
                    restore(
                        root,
                        original,
                        package,
                        name + "-restoration-after-" + row["name"],
                    )
        assert len(results) == 8
    finally:
        restore(root, original, package, name + "-final-restoration")
    return results


if __name__ == "__main__":
    assert sys.version_info >= (3, 12), "source extraction requires the data filter"
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    HELPER = Path(__file__).resolve().parent.parent
    ASSETS = HELPER / "devco/connector-proof"
    SPEC = json.loads((ASSETS / "spec.json").read_text(encoding="utf-8"))
    assert SPEC["status"] == "BOUND", (
        "vendor source publication and final rebind are required"
    )
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
    SOURCES = {
        "helper": HELPER,
        "upstream": HELPER / "upstream-source",
        "vendor": HELPER / "vendor-source",
    }
    for package in SPEC["packages"]:
        assert package["source_sha"] and package["source_tree"], "unbound source"
        for entry in package["states"].values():
            entry["files"] = package["head_files"] | entry["changed_files"]
            for name in entry["removed_files"]:
                del entry["files"][name]
        key = package["name"].upper()
        assert (os.environ[key + "_SHA"], os.environ[key + "_TREE"]) == (
            package["source_sha"],
            package["source_tree"],
        )
        assert len(package["rows"]) == 8
    CHECKOUTS = {name: checkout_state(root) for name, root in SOURCES.items()}
    assert all(not entry["tracked_status"] for entry in CHECKOUTS.values())
    OUT = HELPER / "target/clickhouse-connector-proof"
    OUT.mkdir(parents=True, exist_ok=False)
    request = {
        "ref": SPEC["ref"],
        "inputs": {
            "upstream_repository": os.environ["UPSTREAM_REPOSITORY"],
            "upstream_sha": os.environ["UPSTREAM_SHA"],
            "upstream_tree": os.environ["UPSTREAM_TREE"],
            "vendor_sha": os.environ["VENDOR_SHA"],
            "vendor_tree": os.environ["VENDOR_TREE"],
            "helper_tree": os.environ["HELPER_TREE"],
            "request_id": SPEC["request_id"],
        },
    }
    write("request.json", request)
    write("checkout-initial.json", CHECKOUTS)
    write(
        "provenance.json",
        {
            "request_id": SPEC["request_id"],
            "request_sha256": digest(json.dumps(request, indent=2).encode() + b"\n"),
            "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
            "helper_tree": os.environ["HELPER_TREE"],
            "payload_sha256": digest(Path(__file__).read_bytes()),
            "spec_sha256": digest((ASSETS / "spec.json").read_bytes()),
            "workflow_sha256": digest(
                (HELPER / ".github/workflows/format.yml").read_bytes()
            ),
            "environment_sha256": digest(
                (HELPER / "devco/connector-proof-env.nix").read_bytes()
            ),
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": 1,
        },
    )
    env = os.environ.copy()
    CLEARED = [
        key
        for key in env
        if key.startswith("CARGO_")
        or (key.startswith("DUCKDB_") and key.endswith("_DIR"))
        or key
        in (
            "RUSTFLAGS",
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTUP_TOOLCHAIN",
            "BASH_ENV",
        )
    ]
    for key in CLEARED:
        del env[key]
    env.update(CARGO_TERM_COLOR="never", NO_COLOR="1", INSTA_UPDATE="no")
    all_results = []
    write("verdict.json", {"status": "INCOMPLETE", "accepted_16_rows": False})
    with tempfile.TemporaryDirectory(
        prefix="connector-proof-", dir=os.environ["RUNNER_TEMP"]
    ) as directory:
        SCRATCH = Path(directory).resolve()
        try:
            for package in SPEC["packages"]:
                all_results.extend(proof(package, env))
        finally:
            write(
                "checkout-final.json",
                {name: checkout_state(root) for name, root in SOURCES.items()},
            )
            guard_sources()
    accepted = len(all_results) == 16 and all(
        result["accepted"] for result in all_results
    )
    write(
        "verdict.json",
        {
            "status": "ALL_16_ACCEPTED" if accepted else "INCOMPLETE_OR_NONVERDICT",
            "accepted_16_rows": accepted,
            "rows": all_results,
            "limits": "Custom TLS HTTP response and bounded idle retirement only. No real ClickHouse result, mutual client authentication, exact two-second timing, TCP keepalive runtime, native client eviction, fleet, capacity, full gates or delivery proof.",
        },
    )
    assert accepted, "all sixteen terminal qualified rows are required"

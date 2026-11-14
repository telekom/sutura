"""Hosted-only raw-index diagnostics over the frozen Oracle bootstrap."""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SOURCE = "d994a05519aa8f20d39c842861c46e2eb23890d1"
TREE = "0d39cb5ec946a2b142a413f8b98a9678ec830959"
REQUEST = "0c1c6832-dc8a-4b2f-ab17-071fcf60fb94"
REF = "ci/oracle-index-diagnostic-0c1c6832"
NIX_FILE = "nix/multi-node-oracle.nix"
TEST_FILE = "nix/multi-node-oracle.py"
NIX_SHA = "2c9be32739d39eb1d19834f04725756d6c37b4547f3331a8ff1cc9db8817abb7"
MUTANT_SHA = "95f87a1c413cdcb2f697ee6e0db8cee250af5df80e17e87f2d74a25dc8548f4e"
TEST_SHA = "d3227c241bbdb3d8da93b98ef31e432b5e016e8c04e32aa99ee70053d0c048b2"
PATCH_SHA = "5058e343504218db73ba540eb61ea0e66de3d5059555199469a4eada03caf222"
PREREQUISITE_SHA = "671d1c5d984bedab6ecfdf6fc114688d255b4d9291611e2006efb063b303dedb"
ATTRIBUTE = ".#checks.x86_64-linux.multi-node-oracle"
INBOUND = b"""    inbound = {
      mode = "direct";
      resource = "https://query";
      authorization_server = "https://idp";
      key_set_file = "${publicKeys}/jwks.json";
      algorithms = [ "ES256" ];
    };
"""
DIFF = (
    "diff",
    "--binary",
    "--full-index",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--diff-algorithm=myers",
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(
        ["git", "--no-optional-locks", "-C", str(root), *args]
    )


def write(name: str, value: object) -> None:
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def state() -> dict:
    return {
        name: [
            (ROOT / name).lstat().st_mode,
            digest(
                os.fsencode(os.readlink(ROOT / name))
                if (ROOT / name).is_symlink()
                else (ROOT / name).read_bytes()
            ),
        ]
        for name in git(ROOT, "ls-files", "-z").decode().rstrip("\0").split("\0")
    }


def locks(manifest: dict) -> dict:
    return {
        name: value
        for name, value in manifest.items()
        if name.endswith((".lock", "-lock.json", "-lock.yaml", "-lock.yml"))
    }


def snapshot(label: str, result: dict | None = None) -> str:
    global CAPTURE_NUMBER
    CAPTURE_NUMBER += 1
    name = f"index-{CAPTURE_NUMBER:02d}-{label}"
    record = {
        "phase": PHASE,
        "label": label,
        "sequence": CAPTURE_NUMBER,
        "time_ns": time.time_ns(),
        "monotonic_ns": time.monotonic_ns(),
        "pid": os.getpid(),
        "source": SOURCE,
        "tree": TREE,
        "request_id": REQUEST,
        "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
        "run_id": os.environ["GITHUB_RUN_ID"],
        "run_attempt": 1,
        "job": os.environ["GITHUB_JOB"],
        "index": str(INDEX),
        "command": COMMAND,
        "result": result,
        "outer_initial_sha256": OUTER_SHA,
        "git_optional_locks": os.environ.get("GIT_OPTIONAL_LOCKS"),
    }
    try:
        with INDEX.open("rb") as stream:
            before = os.fstat(stream.fileno())
            raw = stream.read()
            after = os.fstat(stream.fileno())
        fields = (
            "st_dev",
            "st_ino",
            "st_size",
            "st_mode",
            "st_mtime_ns",
            "st_ctime_ns",
        )
        observed = [
            {field: getattr(stat, field) for field in fields}
            for stat in (before, after, INDEX.stat())
        ]
        raw_sha = digest(raw)
        stable = (
            observed[0] == observed[1] == observed[2] and len(raw) == before.st_size
        )
        record.update(
            sha256=raw_sha,
            bytes=len(raw),
            stat=observed,
            stable=stable,
            equals_outer_initial=raw_sha == OUTER_SHA,
        )
        (OUT / (name + ".index")).write_bytes(raw)
    except OSError as error:
        record["capture_error"] = type(error).__name__
        write(name + ".json", record)
        raise
    write(name + ".json", record)
    assert stable, "raw-index capture was not stable"
    assert raw_sha == OUTER_SHA, "actual source index changed from outer baseline"
    return raw_sha


def exact(expected: dict) -> None:
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert state() == expected, (
        "tracked bytes or modes changed beyond the frozen mutation"
    )
    assert locks(state()) == locks(BASELINE), "lock bytes or modes changed"
    assert git(ROOT, "ls-files", "--stage", "-z") == INDEX_ENTRIES
    raw = INDEX.read_bytes()
    if digest(raw) != INDEX_SHA:
        name = f"index-strict-failure-{time.monotonic_ns()}"
        (OUT / (name + ".index")).write_bytes(raw)
        write(
            name + ".json",
            {
                "phase": PHASE,
                "sha256": digest(raw),
                "bytes": len(raw),
                "expected_sha256": INDEX_SHA,
                "time_ns": time.time_ns(),
                "limit": "Exact bytes used by the failed check; read stability unmeasured.",
            },
        )
    assert digest(raw) == INDEX_SHA, "actual source index changed"
    assert not git(ROOT, "ls-files", "--others", "--exclude-standard")
    assert not git(ROOT, "diff", "--cached", "--name-only")


def run(label: str, command: list[str], cwd: Path) -> tuple[int, str]:
    global COMMAND, NIX_COMMANDS
    NIX_COMMANDS += 1
    assert NIX_COMMANDS <= 6 and command[0] == "nix"
    COMMAND = {"sequence": NIX_COMMANDS, "argv": command, "cwd": str(cwd)}
    print(label, flush=True)
    snapshot(label + "-before")
    status = None
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            status = subprocess.run(
                command,
                cwd=cwd,
                stdout=stdout,
                stderr=stderr,
                timeout=4500,
                check=False,
            ).returncode
        except subprocess.TimeoutExpired:
            status = 124
        finally:
            error = sys.exc_info()[0]
            try:
                snapshot(
                    label + "-after",
                    {
                        "exit": status,
                        "exception": error.__name__ if error else None,
                    },
                )
            finally:
                logs = {}
                for name, stream in (("stdout", stdout), ("stderr", stderr)):
                    stream.seek(0)
                    text = stream.read().decode(errors="replace")
                    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
                    secrets.extend(
                        value
                        for key, value in os.environ.items()
                        if key.endswith("PASSWORD")
                    )
                    for secret in sorted(filter(None, secrets), key=len, reverse=True):
                        text = text.replace(secret, "***")
                    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
                    (OUT / f"{label}.{name}").write_text(text, encoding="utf-8")
                    logs[name] = text
                write(
                    label + ".json",
                    {
                        "command": command,
                        "cwd": str(cwd),
                        "exit": status,
                        "exception": error.__name__ if error else None,
                        "stdout_sha256": digest(logs["stdout"].encode()),
                        "stderr_sha256": digest(logs["stderr"].encode()),
                        "source": SOURCE,
                        "tree": TREE,
                        "overlay_sha256": digest(git(ROOT, *DIFF, "HEAD")),
                    },
                )
    assert status is not None
    return status, logs["stdout"]


def bootstrap(label: str, expected: dict) -> dict:
    global PHASE, COMMAND
    PHASE, COMMAND = label, None
    exact(expected)
    status, check = run(
        label + "-check-drv",
        [
            "nix",
            "eval",
            "--no-write-lock-file",
            "--raw",
            ATTRIBUTE + ".drvPath",
        ],
        ROOT,
    )
    assert status == 0
    status, text = run(
        label + "-driver-build",
        [
            "nix",
            "build",
            "--no-write-lock-file",
            "--no-link",
            "--json",
            "-L",
            ATTRIBUTE + ".driver",
        ],
        ROOT,
    )
    assert status == 0, "driver setup/build is not behavioral evidence"
    built = json.loads(text)
    assert len(built) == 1
    driver = Path(built[0]["outputs"]["out"])
    assert re.fullmatch(
        r"/nix/store/[a-z0-9]{32}-nixos-test-driver-multi-node-oracle", str(driver)
    )
    script = (driver / "test-script").read_bytes()
    harness = re.findall(
        rb'harness = runpy.run_path\("(/nix/store/[^"\n]+-multi-node-oracle.py)"\)',
        script,
    )
    assert len(harness) == 1
    harness_path = Path(os.fsdecode(harness[0]))
    assert digest(harness_path.read_bytes()) == TEST_SHA
    (OUT / (label + "-test-script.py")).write_bytes(script)
    (OUT / (label + "-harness.py")).write_bytes(harness_path.read_bytes())
    wrapper = (driver / "bin/nixos-test-driver").read_bytes()
    (OUT / (label + "-driver-wrapper.sh")).write_bytes(wrapper)
    binding = {
        "check_drv_path": check.strip(),
        "driver_path": str(driver),
        "driver_build": built,
        "wrapper_sha256": digest(wrapper),
        "test_script_sha256": digest(script),
        "harness_sha256": TEST_SHA,
    }
    write(label + "-bootstrap.json", binding)
    exact(expected)
    return binding


def prerequisite() -> dict:
    assert os.environ["PREREQUISITE_SHA256"] == PREREQUISITE_SHA
    raw = os.environ["PREREQUISITE_JSON"]
    assert re.fullmatch(r"[0-9a-f]{64}", os.environ["PREREQUISITE_SHA256"])
    assert digest(raw.encode()) == os.environ["PREREQUISITE_SHA256"]
    (OUT / "prerequisite.json").write_text(raw, encoding="utf-8")
    receipt = json.loads(raw)
    assert receipt["schema"] == "oracle-fleet-native-prerequisite/v2"
    assert receipt["source_sha"] == SOURCE and receipt["source_tree"] == TREE
    assert receipt["checkout_tree"] == TREE
    assert re.fullmatch(r"[0-9a-f]{40}", receipt["checkout_sha"])
    for name in ("run_id", "run_attempt", "job_id"):
        assert type(receipt[name]) is int and receipt[name] > 0
    assert receipt["job_name"] == "multi-node-oracle"
    assert receipt["conclusion"] == "success"
    assert receipt["full_venue"] is True and receipt["native_executed"] is True
    assert receipt["review_status"] == "accepted"
    for name in (
        "job_log_sha256",
        "native_log_sha256",
        "evidence_sha256",
        "root_review_sha256",
    ):
        assert re.fullmatch(r"[0-9a-f]{64}", receipt[name])
    assert re.fullmatch(
        r"/nix/store/[a-z0-9]{32}-nixos-test-driver-multi-node-oracle\.drv",
        receipt["driver_drv_path"],
    )
    # The caller's independently reviewed receipt is a prerequisite, not an API observation here.
    return receipt


def diagnose() -> None:
    global PHASE, COMMAND
    receipt = prerequisite()
    source = bootstrap("source", BASELINE)
    assert source["check_drv_path"] == receipt["check_drv_path"]
    assert source["driver_build"][0]["drvPath"] == receipt["driver_drv_path"]
    original = (ROOT / NIX_FILE).read_bytes()
    assert digest(original) == NIX_SHA and original.count(INBOUND) == 1
    changed = original.replace(INBOUND, b"", 1)
    assert digest(changed) == MUTANT_SHA
    mutant = dict(BASELINE)
    mutant[NIX_FILE] = [BASELINE[NIX_FILE][0], MUTANT_SHA]
    PHASE, COMMAND = "apply-mutation", None
    try:
        (ROOT / NIX_FILE).write_bytes(changed)
        exact(mutant)
        overlay = git(ROOT, *DIFF, "HEAD")
        assert digest(overlay) == PATCH_SHA
        (OUT / "mutation.patch").write_bytes(overlay)
        write("mutant-source.json", state())
        altered = bootstrap("mutant", mutant)
        assert altered["driver_path"] != source["driver_path"]
        assert altered["check_drv_path"] != source["check_drv_path"]
    finally:
        PHASE, COMMAND = "restore-source", None
        (ROOT / NIX_FILE).write_bytes(original)
        (ROOT / NIX_FILE).chmod(BASELINE[NIX_FILE][0] & 0o7777)
        exact(BASELINE)
        restored = bootstrap("restored", BASELINE)
        assert restored == source, (
            "restored driver is not the exact source-pinned driver"
        )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "fresh request; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    assert (
        os.environ["SOURCE_SHA"],
        os.environ["SOURCE_TREE"],
        os.environ["REQUEST_ID"],
        os.environ["GITHUB_REF_NAME"],
    ) == (SOURCE, TREE, REQUEST, REF)
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    ROOT = Path.cwd().resolve()
    OUT = ROOT / "target/oracle-index-diagnostic"
    OUT.mkdir(parents=True, exist_ok=False)
    HELPER = Path(__file__).resolve().parent.parent
    INDEX = Path(
        git(ROOT, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    SHELL_OUT = Path(os.environ["INDEX_DIAGNOSTIC_DIR"])
    OUTER_SHA = digest((SHELL_OUT / "outer-before.index").read_bytes())
    INDEX_SHA = OUTER_SHA
    CAPTURE_NUMBER, NIX_COMMANDS = 0, 0
    PHASE, COMMAND = "initial", None
    BASELINE = None
    try:
        INDEX_SHA = snapshot("initial")
        INDEX_ENTRIES = git(ROOT, "ls-files", "--stage", "-z")
        BASELINE = state()
        (OUT / "source-index-initial.z").write_bytes(INDEX_ENTRIES)
        write("source-initial.json", BASELINE)
        write("source-locks-initial.json", locks(BASELINE))
        (OUT / "source-tree.z").write_bytes(git(ROOT, "ls-tree", "-r", "-z", SOURCE))
        request = {
            "ref": REF,
            "inputs": {
                "source_sha": SOURCE,
                "source_tree": TREE,
                "request_id": REQUEST,
                "prerequisite_json": os.environ["PREREQUISITE_JSON"],
                "prerequisite_sha256": os.environ["PREREQUISITE_SHA256"],
            },
        }
        write("request.json", request)
        nix = shutil.which("nix")
        assert nix
        nix = Path(nix).resolve(strict=True)
        write(
            "provenance.json",
            {
                "schema": "oracle-bootstrap-index-diagnostic/v1",
                "request_id": REQUEST,
                "request_sha256": digest(
                    json.dumps(request, indent=2).encode() + b"\n"
                ),
                "helper_sha": git(HELPER, "rev-parse", "HEAD").decode().strip(),
                "helper_tree": git(HELPER, "rev-parse", "HEAD^{tree}").decode().strip(),
                "payload_sha256": digest(Path(__file__).read_bytes()),
                "workflow_sha256": digest(
                    (HELPER / ".github/workflows/format.yml").read_bytes()
                ),
                "source": SOURCE,
                "source_tree": TREE,
                "source_index_sha256": INDEX_SHA,
                "outer_initial_sha256": OUTER_SHA,
                "nix_path": str(nix),
                "nix_sha256": digest(nix.read_bytes()),
                "run_id": os.environ["GITHUB_RUN_ID"],
                "run_attempt": 1,
                "job": os.environ["GITHUB_JOB"],
            },
        )
        assert (
            git(HELPER, "rev-parse", "HEAD").decode().strip()
            == os.environ["GITHUB_WORKFLOW_SHA"]
        )
        assert not git(HELPER, "status", "--porcelain", "--untracked-files=no")
        assert not git(ROOT, "status", "--porcelain")
        assert digest((ROOT / TEST_FILE).read_bytes()) == TEST_SHA
        assert (
            (ROOT / TEST_FILE).read_text(encoding="utf-8").splitlines()[54].strip()
            == 'assert status == "200" and reply.get("outcome") == "answer", (status, reply)'
        )
        exact(BASELINE)
        diagnose()
        assert NIX_COMMANDS == 6
    finally:
        primary = sys.exc_info()[0]
        try:
            (OUT / "source-index-final.z").write_bytes(
                git(ROOT, "ls-files", "--stage", "-z")
            )
            write(
                "source-final.json",
                {
                    "manifest": state(),
                    "locks": locks(state()),
                    "status": git(ROOT, "status", "--porcelain").decode(),
                    "index_sha256": digest(INDEX.read_bytes()),
                },
            )
            if BASELINE is not None:
                exact(BASELINE)
        finally:
            try:
                snapshot("final")
            finally:
                terminal = sys.exc_info()[0]
                write(
                    "verdict.json",
                    {
                        "status": "failed"
                        if primary or terminal
                        else "bootstrap/index diagnostic complete",
                        "primary_exception": primary.__name__ if primary else None,
                        "terminal_exception": terminal.__name__ if terminal else None,
                        "phase": PHASE,
                        "source": SOURCE,
                        "tree": TREE,
                        "nix_child_commands": NIX_COMMANDS,
                        "native_executed": False,
                        "prerequisite_sha256": os.environ["PREREQUISITE_SHA256"],
                        "limit": "Intervals do not identify a writer. No admission, kill, native restoration, runtime, finishing or queue credit; guest storage is separate.",
                    },
                )
                write(
                    "evidence-manifest.json",
                    {
                        str(path.relative_to(OUT)): [
                            path.lstat().st_mode,
                            digest(path.read_bytes()),
                        ]
                        for path in sorted(OUT.rglob("*"))
                        if path.is_file()
                    },
                )

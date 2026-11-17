"""Hosted-only Oracle reader admission mutation and full native restoration proof."""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SOURCE = "15d4485e65c09556d874bb5cd62cd5e73fe87e49"
TREE = "7c38820904463f171a5bca753d576ea53568a407"
REQUEST = "9f383726-2e94-4c9d-90f6-964351aecde6"
REF = "ci/oracle-topology-proof-9f383726"
NIX_FILE = "nix/multi-node-oracle.nix"
TEST_FILE = "nix/multi-node-oracle.py"
NIX_SHA = "c17021de8f6e81d5a479960ec657fe2819e85f09ecf9025907c1bb818571a9e5"
MUTANT_SHA = "36827d0b301cbc0abd36448676f4680f421a6c821222615f5ce4cf330c7fae6f"
TEST_SHA = "5e707e842635c3f6e773310cbb4767e729cc5a09a4be7a38d88e2b7b23ed2047"
PATCH_SHA = "d45c3fa3a180340ff98d5a09fc30db90a62edf4aff1bbacea5a9777e2168c7ed"
ATTRIBUTE = ".#checks.x86_64-linux.multi-node-oracle"
INBOUND = b"""    inbound = {
      mode = "direct";
      resource = "https://query";
      authorization_server = "https://idp";
      key_set_file = "${publicKeys}/jwks.json";
      algorithms = [ "ES256" ];
    };
"""
STAGES = (
    "the whole Oracle lookup returns the exact ordered reference",
    "all declared IPv4 role paths connect and every other role pair is refused",
    "two source readers answer exactly what the whole Oracle lookup answers",
    "the two bucket statements equal the whole leg at their exact SCN",
    "both reader addresses fetched bucket rows and the serving address fetched none",
    "lost readers refuse without a local bucket fallback, then restart restores the answer",
    "unresolved readers refuse without a local bucket fallback",
    "a changed catalog on both readers refuses re-certification",
    "anonymous callers and untrusted or missing node certificates are refused",
)
COMPLETE = "oracle-fleet: exact whole/split ordered answer; two source reader addresses and one SCN; no serving bucket row fetch; lost/unresolved/uncertified readers refused; IPv4 role matrix; shared service user and fixture issuer"
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


def capture_index(label: str) -> None:
    if "INDEX" not in globals():
        return
    data = INDEX.read_bytes()
    snapshots = OUT / "index-snapshots"
    snapshots.mkdir(exist_ok=True)
    key = digest(data)
    path = snapshots / (key + ".bin")
    if not path.exists():
        path.write_bytes(data)
    INDEX_CAPTURES.append(
        {"order": len(INDEX_CAPTURES) + 1, "label": label, "sha256": key}
    )
    write("index-capture-order.json", INDEX_CAPTURES)


def git(root: Path, *args: str) -> bytes:
    label = "git " + " ".join(args)
    capture_index("before " + label)
    try:
        return subprocess.check_output(
            ["git", "--no-optional-locks", "-C", str(root), *args]
        )
    finally:
        capture_index("after " + label)


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


def exact(expected: dict) -> None:
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert state() == expected, (
        "tracked bytes or modes changed beyond the frozen mutation"
    )
    assert locks(state()) == locks(BASELINE), "lock bytes or modes changed"
    assert git(ROOT, "ls-files", "--stage", "-z") == INDEX_ENTRIES
    assert digest(INDEX.read_bytes()) == INDEX_SHA, "actual source index changed"
    assert not git(ROOT, "ls-files", "--others", "--exclude-standard")
    assert not git(ROOT, "diff", "--cached", "--name-only")


def run(
    label: str,
    command: list[str],
    cwd: Path,
    combined: bool = False,
    environment: dict | None = None,
) -> tuple[int, str]:
    print(label, flush=True)
    capture_index("before command " + label)
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            status = subprocess.run(
                command,
                cwd=cwd,
                stdout=stdout,
                stderr=subprocess.STDOUT if combined else stderr,
                timeout=4500,
                check=False,
                env=environment,
            ).returncode
        except subprocess.TimeoutExpired:
            status = 124
        logs = {}
        for name, stream in (("stdout", stdout), ("stderr", stderr)):
            stream.seek(0)
            text = stream.read().decode(errors="replace")
            secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
            secrets.extend(
                value for key, value in os.environ.items() if key.endswith("PASSWORD")
            )
            for secret in sorted(filter(None, secrets), key=len, reverse=True):
                text = text.replace(secret, "***")
            text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
            (OUT / f"{label}.{name}").write_text(text, encoding="utf-8")
            logs[name] = text
    capture_index("after command " + label)
    write(
        label + ".json",
        {
            "command": command,
            "cwd": str(cwd),
            "exit": status,
            "stdout_sha256": digest(logs["stdout"].encode()),
            "stderr_sha256": digest(logs["stderr"].encode()),
            "source": SOURCE,
            "tree": TREE,
            "overlay_sha256": digest(git(ROOT, *DIFF, "HEAD")),
        },
    )
    return status, logs["stdout"]


def bootstrap(label: str, expected: dict) -> dict:
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


def native(label: str, binding: dict, expected: dict, red: bool = False) -> None:
    exact(expected)
    work = Path(
        tempfile.mkdtemp(
            prefix="oracle-admission-" + label + "-", dir=os.environ["RUNNER_TEMP"]
        )
    )
    work.chmod(0o700)
    environment = dict(os.environ, XDG_RUNTIME_DIR=str(work))
    storage = {}
    for name, raw_path in {
        "inherited_runtime": os.environ.get("XDG_RUNTIME_DIR"),
        "runner_temp": os.environ["RUNNER_TEMP"],
        "chosen_runtime": str(work),
    }.items():
        if raw_path and Path(raw_path).exists():
            path = Path(raw_path)
            stat = path.stat()
            space = os.statvfs(path)
            storage[name] = {
                "path": str(path.resolve()),
                "mode": stat.st_mode,
                "device": stat.st_dev,
                "block_size": space.f_frsize,
                "blocks": space.f_blocks,
                "available_blocks": space.f_bavail,
                "files": space.f_files,
                "available_files": space.f_favail,
            }
        else:
            storage[name] = {"path": raw_path, "exists": False}
    write(label + "-storage-before.json", storage)
    (OUT / (label + "-mountinfo.txt")).write_bytes(
        Path("/proc/self/mountinfo").read_bytes()
    )
    output = OUT / (label + "-output")
    output.mkdir()
    status, raw = run(
        label,
        [
            TIMEOUT,
            "--kill-after=30s",
            "70m",
            binding["driver_path"] + "/bin/nixos-test-driver",
            "-o",
            str(output),
        ],
        work,
        combined=True,
        environment=environment,
    )
    space = os.statvfs(work)
    write(
        label + "-storage-after.json",
        {
            "runtime": str(work),
            "mode": work.stat().st_mode,
            "block_size": space.f_frsize,
            "blocks": space.f_blocks,
            "available_blocks": space.f_bavail,
            "files": space.f_files,
            "available_files": space.f_favail,
        },
    )
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", raw)
    admissions = {"reader1": [], "reader2": []}
    for line in text.splitlines():
        event_json = re.search(r'\{"v":.*\}$', line)
        if event_json:
            event = json.loads(event_json[0])
            name, message = event.get("hostname"), event.get("resolved", "")
            if (
                name in admissions
                and "admitted_by: " in message
                and "inbound: " in message
            ):
                declared = re.search(r"admitted_by: (None|Some\(\{[^}]*\}\))", message)
                assert declared, "no complete source admission value"
                membership = declared[1]
                if red:
                    assert "inbound: []" in message and membership == "None"
                else:
                    assert (
                        membership.startswith("Some(")
                        and "primary" in membership
                        and "," not in membership
                    )
                admissions[name].append(membership)
    assert all(admissions.values()), (
        "both readers' actual source admission must be logged"
    )
    write(label + "-reader-admission.json", admissions)
    exact(expected)
    if red:
        assert status != 0 and status != 124, "timeout/setup is not an assertion kill"
        finished = [
            text.find("(finished: subtest: " + stage + ",") for stage in STAGES[:2]
        ]
        started = text.find("subtest: " + STAGES[2])
        failed = text.find('!!! Test "' + STAGES[2] + '" failed with error:')
        assert 0 <= finished[0] < finished[1] < started < failed
        failures = re.findall(r'!!! Test "([^"\n]+)" failed with error:', text)
        assert failures == [STAGES[2]], "not the first split positive's own failure"
        assert "(finished: subtest: " + STAGES[2] + "," not in text
        assert "subtest: " + STAGES[3] not in text
        trace = re.search(
            r'File "(/nix/store/[^"\n]+-multi-node-oracle.py)", line 256, in main\n'
            r"\s+split = answer\(\)\n"
            r'\s+File "\1", line 55, in answer\n'
            r'\s+assert status == "200" and reply.get\("outcome"\) == "answer", \(status, reply\)',
            text,
        )
        assert trace, "no exact frozen first-split/answer assertion traceback"
        assert digest(Path(trace[1]).read_bytes()) == TEST_SHA
        refusal = re.search(
            r"^AssertionError: \('403', \{'outcome': 'refusal', 'reason': \{'code': 'remote_leg_not_certified', 'status': 403, 'detail': '[^\n]*through `geo`[^\n]*'\}\}\)$",
            text,
            re.MULTILINE,
        )
        assert refusal, "not the expected-200 assertion observing the source's 403"
        write(
            label + "-own-assertion.json",
            {
                "stage": STAGES[2],
                "test_sha256": TEST_SHA,
                "site": TEST_FILE + ":55",
                "caller": TEST_FILE + ":256",
                "traceback": trace[0],
                "failure": refusal[0],
            },
        )
    else:
        assert status == 0 and "!!! Test " not in text
        for stage in STAGES:
            assert "(finished: subtest: " + stage + "," in text, stage
        assert COMPLETE in text, "no full unchanged venue completion"
        assert (output / "oracle-traces.log").is_file()
        write(
            label + "-full-venue.json",
            {
                "completed_stages": STAGES,
                "marker": COMPLETE,
                "source_trace_sha256": digest(
                    (output / "oracle-traces.log").read_bytes()
                ),
            },
        )


def prerequisite() -> dict:
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


def proof() -> None:
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
        native("mutant", altered, mutant, red=True)
    finally:
        (ROOT / NIX_FILE).write_bytes(original)
        (ROOT / NIX_FILE).chmod(BASELINE[NIX_FILE][0] & 0o7777)
        exact(BASELINE)
        restored = bootstrap("restored", BASELINE)
        assert restored == source, (
            "restored driver is not the exact source-pinned driver"
        )
        native("restored", restored, BASELINE)


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
    TIMEOUT = shutil.which("timeout")
    assert TIMEOUT and TIMEOUT.startswith("/nix/store/")
    ROOT = Path.cwd().resolve()
    OUT = ROOT / "target/oracle-reader-admission-proof"
    OUT.mkdir(parents=True, exist_ok=False)
    HELPER = Path(__file__).resolve().parent.parent
    INDEX = Path(
        git(ROOT, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    INDEX_CAPTURES = []
    capture_index("initial before staged entry read")
    INDEX_SHA = digest(INDEX.read_bytes())
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
    write(
        "provenance.json",
        {
            "schema": "oracle-reader-admission-proof/v1",
            "request_id": REQUEST,
            "request_sha256": digest(json.dumps(request, indent=2).encode() + b"\n"),
            "helper_sha": git(HELPER, "rev-parse", "HEAD").decode().strip(),
            "helper_tree": git(HELPER, "rev-parse", "HEAD^{tree}").decode().strip(),
            "payload_sha256": digest(Path(__file__).read_bytes()),
            "workflow_sha256": digest(
                (HELPER / ".github/workflows/format.yml").read_bytes()
            ),
            "source": SOURCE,
            "source_tree": TREE,
            "source_index_sha256": INDEX_SHA,
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": 1,
        },
    )
    try:
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
        proof()
    finally:
        capture_index("final before staged entry read")
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
        exact(BASELINE)
    write(
        "verdict.json",
        {
            "status": "independent full native prerequisite, own first-split 403 kill, full native restoration pass",
            "source": SOURCE,
            "tree": TREE,
            "prerequisite_sha256": os.environ["PREREQUISITE_SHA256"],
            "limit": "Same source-locked full native driver outside the Nix build sandbox. No sandbox-equivalence, served-human, caller-source-impersonation, capacity, finishing or full-issue claim.",
        },
    )
    write(
        "evidence-manifest.json",
        {
            str(path.relative_to(OUT)): [
                path.lstat().st_mode,
                digest(
                    os.fsencode(os.readlink(path))
                    if path.is_symlink()
                    else path.read_bytes()
                ),
            ]
            for path in sorted(OUT.rglob("*"))
            if path.is_file() or path.is_symlink()
        },
    )

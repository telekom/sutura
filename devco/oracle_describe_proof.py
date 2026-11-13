"""Hosted-only native SELECT describe proof; no topology or finishing verdict."""

import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

SOURCE = "40254b4743a209d61f7590aa1e0a334cf4d570b8"
SOURCE_TREE = "5e1b5b85a6cdb0593e9f00435d3f0ae2b9407caa"
BASE = "3ee8874e6c5b3054a91a63c28aaca90302bf2dbe"
BASE_TREE = "6903bc16ed77cb039a7bcd1de0638a848c372432"
CELL = "acceptance::a_split_of_an_integer_key_and_a_text_column_is_described_and_every_bucket_answers_it"
TEST_FILE = "crates/sutura-exec-oracle/tests/flashback.rs"
OVERLAY_HASH = "5d342610caabcaa3e083070fdf12b16afb70ce19f3128947c8e17c0c6ca8d45f"
DIFF = (
    "diff",
    "--binary",
    "--full-index",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--diff-algorithm=myers",
)
MESSAGE = (
    "describing the partition must parse its SELECT without executing or fetching it: "
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(root), *args])


def write(name: str, value: object) -> None:
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def state(root: Path) -> dict:
    return {
        name: [
            (root / name).lstat().st_mode,
            digest(
                os.fsencode(os.readlink(root / name))
                if (root / name).is_symlink()
                else (root / name).read_bytes()
            ),
        ]
        for name in git(root, "ls-files", "-z").decode().rstrip("\0").split("\0")
    }


def exact(root: Path, revision: str, tree: str, expected: dict) -> None:
    assert git(root, "rev-parse", "HEAD").decode().strip() == revision
    assert git(root, "rev-parse", "HEAD^{tree}").decode().strip() == tree
    assert state(root) == expected, "tracked bytes or modes changed"
    assert not git(root, "diff", "--cached", "--name-only")
    assert not git(root, "ls-files", "--others", "--exclude-standard")


def execute(label: str, command: list[str], root: Path) -> tuple[int, str]:
    print(label, flush=True)
    with tempfile.TemporaryFile() as log:
        result = subprocess.run(
            command, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=False
        )
        log.seek(0)
        text = log.read().decode(errors="replace")
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets.extend(
        value for key, value in os.environ.items() if key.endswith("PASSWORD")
    )
    for secret in sorted(filter(None, secrets), key=len, reverse=True):
        text = text.replace(secret, "***")
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    (OUT / (label + ".log")).write_text(text, encoding="utf-8")
    write(
        label + ".json",
        {
            "command": command,
            "exit": result.returncode,
            "log_sha256": digest(text.encode()),
            "source": git(root, "rev-parse", "HEAD").decode().strip(),
            "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
            "overlay_sha256": digest(git(root, *DIFF, "HEAD")),
        },
    )
    return result.returncode, re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)


def run_cell(label: str, root: Path, red: bool = False) -> None:
    status, text = execute(
        label,
        [
            "cargo",
            "test",
            "--locked",
            "--profile",
            "ci",
            "-p",
            "sutura-exec-oracle",
            "--all-features",
            "--test",
            "flashback",
            CELL,
            "--",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        root,
    )
    assert re.search(r"^running 1 test$", text, re.MULTILINE), "no single execution"
    assert re.search(rf"^test {re.escape(CELL)} \.\.\. ", text, re.MULTILINE)
    verdict = "FAILED" if red else "ok"
    # Nocapture can put output between libtest's prefix and verdict.
    assert re.search(
        rf"^{verdict}$|^test {re.escape(CELL)} \.\.\. {verdict}$", text, re.MULTILINE
    )
    summary = (
        "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
        if red
        else "test result: ok. 1 passed; 0 failed; 0 ignored;"
    )
    assert summary in text, "no completed one-cell verdict"
    assert (status != 0) if red else (status == 0)
    if red:
        panic = re.search(
            rf"^thread '{re.escape(CELL)}'(?: \([^\n]*\))? panicked at "
            rf"{re.escape(TEST_FILE)}:635:\d+:\n([^\n]*(?:\n[^\n]+)*)",
            text,
            re.MULTILINE,
        )
        assert panic, "not the named test's own no-execute/fetch assertion"
        observed = re.search(re.escape(MESSAGE) + r"\((\d+), (\d+), (\d+)\)", panic[1])
        assert observed, "no measured cursor tuple at the intended assertion"
        cursors, executions, fetches = map(int, observed.groups())
        assert cursors > 0 and (executions > 0 or fetches > 0), (
            "absent cursor is not behavior credit"
        )
        write(
            label + "-own-assertion.json",
            {
                "cell": CELL,
                "site": TEST_FILE + ":635",
                "panic": panic[0],
                "cursor_count": cursors,
                "executions": executions,
                "fetches": fetches,
            },
        )


def bootstrap() -> None:
    installable = ".#apps.x86_64-linux.oracle-acceptance.program"
    app = Path(subprocess.check_output(["nix", "eval", "--raw", installable]).decode())
    context = json.loads(
        subprocess.check_output(
            ["nix", "eval", "--json", "--apply", "builtins.getContext", installable]
        )
    )
    assert len(context) == 1 and all(path.endswith(".drv") for path in context)
    status, _ = execute("realise", ["nix-store", "--realise", *context], ROOT)
    assert status == 0
    original = app.read_text(encoding="utf-8")
    marker = 'if [ "${SUTURA_ORACLE_SPLIT_CLAIM_PROOF:-0}" = 1 ]; then'
    assert original.count(marker) == 1
    # ponytail: retain the source-pinned provisioner and warm environment, replacing only tests.
    script = (
        original.split(marker)[0]
        + "export SUTURA_ORACLE_DESCRIBE_PROOF_BOUND=1\nexec "
        + shlex.join(["python3", "-I", str(Path(__file__).resolve())])
        + "\n"
    )
    (OUT / "environment-original.sh").write_text(original, encoding="utf-8")
    (OUT / "environment-proof.sh").write_text(script, encoding="utf-8")
    write(
        "bootstrap.json",
        {
            "app": str(app),
            "context": context,
            "original_sha256": digest(original.encode()),
            "proof_sha256": digest(script.encode()),
        },
    )
    status, _ = execute("bootstrap", ["bash", str(OUT / "environment-proof.sh")], ROOT)
    assert status == 0, "hosted task failed; original failed and restored logs retained"


def base_proof(baseline: dict) -> None:
    overlay = git(ROOT, *DIFF, BASE, SOURCE, "--", TEST_FILE)
    assert digest(overlay) == OVERLAY_HASH
    (OUT / "test-only-base-overlay.patch").write_bytes(overlay)
    base = (
        Path(
            tempfile.mkdtemp(
                prefix="oracle-describe-base-", dir=os.environ["RUNNER_TEMP"]
            )
        )
        / "source"
    )
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(base), BASE], cwd=ROOT, check=True
    )
    before = state(base)
    exact(base, BASE, BASE_TREE, before)
    subprocess.run(
        ["git", "apply", str(OUT / "test-only-base-overlay.patch")],
        cwd=base,
        check=True,
    )
    expected = dict(before)
    expected[TEST_FILE] = baseline[TEST_FILE]
    exact(base, BASE, BASE_TREE, expected)
    assert git(base, *DIFF, "HEAD") == overlay, (
        "only the exact frozen test delta is allowed"
    )
    discovery = ROOT / ".sutura-dev/endpoints.json"
    (base / ".sutura-dev").mkdir()
    shutil.copyfile(discovery, base / ".sutura-dev/endpoints.json")
    write(
        "base-overlay.json",
        {
            "base": BASE,
            "base_tree": BASE_TREE,
            "source": SOURCE,
            "source_tree": SOURCE_TREE,
            "overlay_sha256": digest(overlay),
            "allowed_changes": [TEST_FILE],
            "base_manifest": before,
            "injected_manifest": expected,
            "locks": {
                name: {"source": baseline[name][1], "base": before[name][1]}
                for name in ("Cargo.lock", "flake.lock", "fuzz/Cargo.lock", "pixi.lock")
            },
            "discovery_sha256": digest(discovery.read_bytes()),
            "tier_limit": "one source-pinned live tier, copied discovery and same CA; shared fixture",
        },
    )
    initial_index = git(base, "ls-files", "--stage", "-z")
    (OUT / "base-index-initial.z").write_bytes(initial_index)
    try:
        run_cell("base", base, red=True)
    finally:
        exact(base, BASE, BASE_TREE, expected)
        final_index = git(base, "ls-files", "--stage", "-z")
        (OUT / "base-index-final.z").write_bytes(final_index)
        write(
            "base-final.json",
            {
                "manifest": state(base),
                "overlay_sha256": digest(git(base, *DIFF, "HEAD")),
                "staged_entries_unchanged": final_index == initial_index,
            },
        )
        assert final_index == initial_index
        assert git(base, *DIFF, "HEAD") == overlay


def proof() -> None:
    assert os.environ["SUTURA_DEV_REQUIRE_TIER"] == "1"
    baseline = json.loads((OUT / "source-initial.json").read_text(encoding="utf-8"))
    exact(ROOT, SOURCE, SOURCE_TREE, baseline)
    environment = {
        name: os.environ[name]
        for name in (
            "CARGO_HOME",
            "CARGO_TARGET_DIR",
            "SUTURA_DUCKDB_ADBC_DRIVER",
            "SUTURA_ORACLE_TIER_CA",
        )
    }
    for tool in ("cargo", "cargo-nextest", "clang"):
        path = shutil.which(tool)
        assert path and path.startswith("/nix/store/"), (tool, path)
        environment[tool] = path
    environment["warm_stamp"] = Path(
        os.environ["CARGO_TARGET_DIR"], ".sutura-warm-start"
    ).read_text(encoding="utf-8")
    environment["inherited_config_sha256"] = digest(
        Path(os.environ["CARGO_HOME"], "config.toml").read_bytes()
    )
    os.environ["CARGO_HOME"] = tempfile.mkdtemp(
        prefix="oracle-describe-cargo-", dir=os.environ["RUNNER_TEMP"]
    )
    os.environ.pop("CARGO_NET_OFFLINE", None)
    os.environ.pop("SUTURA_SATELLITE_CARGO_VENDOR_DIR", None)
    environment["native_home"] = os.environ["CARGO_HOME"]
    assert not list(Path(os.environ["CARGO_HOME"]).iterdir())
    write("environment.json", environment)
    try:
        run_cell("head", ROOT)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        try:
            base_proof(baseline)
        finally:
            exact(ROOT, SOURCE, SOURCE_TREE, baseline)
            run_cell("restored", ROOT)
    finally:
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        final_index = git(ROOT, "ls-files", "--stage", "-z")
        (OUT / "source-index-final.z").write_bytes(final_index)
        unchanged = final_index == (OUT / "source-index-initial.z").read_bytes()
        write(
            "source-final.json",
            {
                "manifest": state(ROOT),
                "status": git(ROOT, "status", "--porcelain").decode(),
                "staged_entries_unchanged": unchanged,
            },
        )
        assert unchanged
    write(
        "verdict.json",
        {
            "status": "exact named head pass, own-site measured base red, restored head pass",
            "limit": "Native generated SELECT parse versus execute/fetch only; shared-user one-host tier and socket-idle timeout. No topology, cap, identity, prefetch/memory/time or finishing proof. Head passes include later positive counters/schema/unprobed controls; their numeric tuples are not printed by the frozen test.",
        },
    )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required, no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    assert (
        os.environ["SOURCE_SHA"],
        os.environ["SOURCE_TREE"],
        os.environ["COMPARISON_SHA"],
    ) == (SOURCE, SOURCE_TREE, BASE)
    assert str(uuid.UUID(os.environ["REQUEST_ID"])) == os.environ["REQUEST_ID"]
    ROOT = Path.cwd().resolve()
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == SOURCE_TREE
    assert git(ROOT, "merge-base", BASE, SOURCE).decode().strip() == BASE
    OUT = ROOT / "target/oracle-describe-proof"
    if os.environ.get("SUTURA_ORACLE_DESCRIBE_PROOF_BOUND") == "1":
        proof()
    else:
        OUT.mkdir(parents=True, exist_ok=False)
        helper = Path(__file__).resolve().parent.parent
        assert (
            git(helper, "rev-parse", "HEAD").decode().strip()
            == os.environ["GITHUB_WORKFLOW_SHA"]
        )
        assert not git(helper, "status", "--porcelain", "--untracked-files=no")
        assert not git(ROOT, "status", "--porcelain")
        write("source-initial.json", state(ROOT))
        (OUT / "source-index-initial.z").write_bytes(
            git(ROOT, "ls-files", "--stage", "-z")
        )
        request = {
            "ref": os.environ["GITHUB_REF_NAME"],
            "inputs": {
                "source_sha": SOURCE,
                "source_tree": SOURCE_TREE,
                "comparison_sha": BASE,
                "request_id": os.environ["REQUEST_ID"],
            },
        }
        write("request.json", request)
        write(
            "provenance.json",
            {
                "schema": "oracle-describe-proof/v1",
                "request_sha256": digest(
                    json.dumps(request, indent=2).encode() + b"\n"
                ),
                "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
                "helper_tree": git(helper, "rev-parse", "HEAD^{tree}").decode().strip(),
                "payload_sha256": digest(Path(__file__).read_bytes()),
                "workflow_sha256": digest(
                    (helper / ".github/workflows/format.yml").read_bytes()
                ),
                "source": SOURCE,
                "source_tree": SOURCE_TREE,
                "base": BASE,
                "base_tree": BASE_TREE,
                "run_id": os.environ["GITHUB_RUN_ID"],
                "run_attempt": 1,
            },
        )
        bootstrap()

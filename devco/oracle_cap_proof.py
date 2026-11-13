"""Hosted-only Oracle cap evidence; ordinary CI and finishing gates remain separate."""

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

SOURCE = "9013b346f93fa9fb90d98803ec8e82b72b5225cf"
SOURCE_TREE = "bf82f2e7269282188aff99338918e1db41a159bd"
BASE = "3ee8874e6c5b3054a91a63c28aaca90302bf2dbe"
BASE_TREE = "6903bc16ed77cb039a7bcd1de0638a848c372432"
DIRECT = (
    "acceptance::a_partition_at_one_million_distinct_keys_reads_and_one_more_is_refused"
)
CAP = "oracle_split::an_oracle_granted_million_row_bucket_answers_and_one_more_is_refused_without_a_prefix"
OLD = "oracle_split::an_oracle_lookup_split_across_readers_equals_the_whole_and_refuses_uncertified_readers"
FLASHBACK = "crates/sutura-exec-oracle/tests/flashback.rs"
SERVED = "crates/sutura-cli/tests/served/oracle_split.rs"
IMPLEMENTATION = "crates/sutura-cli/src/serve/kind/split/oracle.rs"
DIFF = (
    "diff",
    "--binary",
    "--full-index",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--diff-algorithm=myers",
)
OVERLAY_HASH = "6798b1668f03d924b0dba87037cfd31227dcab60f0c59a352ad4ced256ae6b04"
ADAPTER_MARKERS = (
    "oracle-partition-row-cap: fixture rows=1000000 distinct=1000000 bucket=0",
    "oracle-partition-row-cap: fixture rows=1000001 distinct=1000001 bucket=0",
    "oracle-partition-row-cap: boundary returned rows=1000000",
)
SERVED_MARKERS = (
    "oracle-granted-row-cap: boundary bucket rows=1000000 distinct=1000000",
    "oracle-granted-row-cap: exact boundary answered",
    "oracle-granted-row-cap: overflow bucket rows=1000001 distinct=1000001",
)
CAP_PASS = "oracle-granted-row-cap: overflow refused as result_too_large without rows"
OLD_PASS = "oracle-split: 40 actual rows, 1200000 declared rows, two nonempty buckets; exact whole/split columns and ordered rows; unresolved and uncertified readers refused"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(root), *args])


def write(name: str, value: object) -> None:
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


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


def state(root: Path) -> dict:
    return {
        path: [
            (root / path).lstat().st_mode,
            digest(
                os.fsencode(os.readlink(root / path))
                if (root / path).is_symlink()
                else (root / path).read_bytes()
            ),
        ]
        for path in git(root, "ls-files", "-z").decode().rstrip("\0").split("\0")
    }


def exact(root: Path, revision: str, tree: str, expected: dict) -> None:
    assert git(root, "rev-parse", "HEAD").decode().strip() == revision
    assert git(root, "rev-parse", "HEAD^{tree}").decode().strip() == tree
    assert state(root) == expected, "tracked bytes or modes changed"
    assert not git(root, "diff", "--cached", "--name-only")
    assert not git(root, "ls-files", "--others", "--exclude-standard"), (
        "untracked source"
    )


def bootstrap() -> None:
    installable = ".#apps.x86_64-linux.oracle-acceptance.program"
    app = Path(subprocess.check_output(["nix", "eval", "--raw", installable]).decode())
    context = json.loads(
        subprocess.check_output(
            ["nix", "eval", "--json", "--apply", "builtins.getContext", installable]
        )
    )
    status, _ = execute("realise", ["nix-store", "--realise", *context], ROOT)
    assert status == 0
    assert len(context) == 1 and all(path.endswith(".drv") for path in context)
    original = app.read_text(encoding="utf-8")
    marker = 'if [ "${SUTURA_ORACLE_SPLIT_CLAIM_PROOF:-0}" = 1 ]; then'
    assert original.count(marker) == 1
    # ponytail: reuse the pinned provisioner and warm environment; replace only its test tail.
    command = shlex.join(["python3", "-I", str(Path(__file__).resolve())])
    script = original.split(marker)[0] + (
        "export SUTURA_ORACLE_CAP_PROOF_BOUND=1\nexec " + command + "\n"
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
    assert status == 0, "hosted proof failed; original failure logs retained"


def test(label: str, root: Path, cell: str, failure: tuple | None = None) -> str:
    if cell == DIRECT:
        selection = [
            "-p",
            "sutura-exec-oracle",
            "--all-features",
            "--test",
            "flashback",
        ]
    else:
        selection = ["-p", "sutura-cli", "--features", "oracle,distributed"]
        status, _ = execute(
            label + "-binary",
            [
                "cargo",
                "build",
                "--locked",
                "--profile",
                "ci",
                *selection,
                "--bin",
                "sutura",
            ],
            root,
        )
        assert status == 0, "CLI build is setup, never behavioral evidence"
        selection += ["--test", "served"]
    status, text = execute(
        label,
        [
            "cargo",
            "test",
            "--locked",
            "--profile",
            "ci",
            *selection,
            cell,
            "--",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        root,
    )
    # libtest interleaves nocapture output with its test prefix, so pin both separately.
    assert re.search(r"^running 1 test$", text, re.MULTILINE), (
        "no single named execution"
    )
    assert re.search(rf"^test {re.escape(cell)} \.\.\. ", text, re.MULTILINE)
    verdict = "FAILED" if failure else "ok"
    assert re.search(
        rf"^{verdict}$|^test {re.escape(cell)} \.\.\. {verdict}$", text, re.MULTILINE
    )
    summary = (
        "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
        if failure
        else "test result: ok. 1 passed; 0 failed; 0 ignored;"
    )
    assert summary in text, "no completed one-cell verdict"
    assert (status != 0) if failure else (status == 0)
    if failure:
        path, line, message, left, right = failure
        panic = re.search(
            rf"^thread '{re.escape(cell)}'(?: \([^\n]*\))? panicked at "
            rf"{re.escape(path)}:{line}:\d+:\n([^\n]*(?:\n[^\n]+)*)",
            text,
            re.MULTILINE,
        )
        assert panic and message in panic[1], "not the intended test's own assertion"
        if left is not None:
            assert re.search(rf"^\s*left: {left}$", panic[1], re.MULTILINE)
            assert re.search(rf"^\s*right: {right}$", panic[1], re.MULTILINE)
        write(
            label + "-own-assertion.json",
            {"cell": cell, "site": f"{path}:{line}", "panic": panic[0]},
        )
    if cell != DIRECT:
        binary = Path(os.environ["CARGO_TARGET_DIR"], "ci/sutura")
        write(
            label + "-binary-used.json",
            {"path": str(binary), "sha256": digest(binary.read_bytes())},
        )
    return text


def markers(text: str, expected: tuple[str, ...]) -> None:
    assert all(marker in text for marker in expected), (
        "missing numeric/content evidence"
    )


def base_proof(baseline: dict) -> None:
    overlay = git(ROOT, *DIFF, BASE, SOURCE, "--", FLASHBACK)
    assert digest(overlay) == OVERLAY_HASH
    (OUT / "base-test-only.patch").write_bytes(overlay)
    base = (
        Path(tempfile.mkdtemp(prefix="oracle-cap-base-", dir=os.environ["RUNNER_TEMP"]))
        / "source"
    )
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(base), BASE], cwd=ROOT, check=True
    )
    before = state(base)
    exact(base, BASE, BASE_TREE, before)
    subprocess.run(
        ["git", "apply", str(OUT / "base-test-only.patch")], cwd=base, check=True
    )
    expected = dict(before)
    expected[FLASHBACK] = baseline[FLASHBACK]
    exact(base, BASE, BASE_TREE, expected)
    assert git(base, "diff", "--name-only").decode().splitlines() == [FLASHBACK]
    # The base intentionally uses the one source-pinned live tier, not a second provisioner.
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
            "allowed_changes": [FLASHBACK],
            "source_manifest": baseline,
            "base_manifest": before,
            "locks": {
                name: {"source": baseline[name][1], "base": before[name][1]}
                for name in ("Cargo.lock", "flake.lock", "fuzz/Cargo.lock", "pixi.lock")
            },
            "injected_manifest": expected,
            "discovery_sha256": digest(discovery.read_bytes()),
            "tier": "same source-pinned Oracle tier, copied discovery; no guessed endpoint",
        },
    )
    text = test(
        "adapter-base",
        base,
        DIRECT,
        (
            FLASHBACK,
            318,
            "a partition with 1000001 distinct keys must be refused",
            None,
            None,
        ),
    )
    markers(
        text,
        ADAPTER_MARKERS + ("oracle-partition-row-cap: overflow returned rows=1000001",),
    )
    exact(base, BASE, BASE_TREE, expected)
    assert git(base, *DIFF, "HEAD") == overlay
    write(
        "base-final.json",
        {"manifest": state(base), "overlay_sha256": digest(git(base, *DIFF, "HEAD"))},
    )


def mutation(label: str, patch: str, cell: str, failure: tuple, baseline: dict) -> None:
    exact(ROOT, SOURCE, SOURCE_TREE, baseline)
    saved = (ROOT / IMPLEMENTATION).read_bytes()
    patch_data = (ROOT / patch).read_bytes()
    (OUT / (label + ".patch")).write_bytes(patch_data)
    subprocess.run(["git", "apply", "--check", patch], check=True)
    subprocess.run(["git", "apply", patch], check=True)
    try:
        expected = dict(baseline)
        expected[IMPLEMENTATION] = [
            (ROOT / IMPLEMENTATION).lstat().st_mode,
            digest((ROOT / IMPLEMENTATION).read_bytes()),
        ]
        exact(ROOT, SOURCE, SOURCE_TREE, expected)
        delta = git(ROOT, *DIFF, "HEAD")
        (OUT / (label + "-applied.patch")).write_bytes(delta)
        write(
            label + "-mutation.json",
            {
                "patch": patch,
                "committed_sha256": digest(patch_data),
                "applied_sha256": digest(delta),
                "manifest": expected,
            },
        )
        text = test(label + "-kill", ROOT, cell, failure)
        if cell == CAP:
            markers(text, SERVED_MARKERS)
        exact(ROOT, SOURCE, SOURCE_TREE, expected)
    finally:
        (ROOT / IMPLEMENTATION).write_bytes(saved)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        text = test(label + "-restored", ROOT, cell)
        markers(text, SERVED_MARKERS + (CAP_PASS,) if cell == CAP else (OLD_PASS,))
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)


def proof() -> None:
    assert os.environ["SUTURA_DEV_REQUIRE_TIER"] == "1"
    assert not git(ROOT, "status", "--porcelain"), "source must start clean"
    baseline = state(ROOT)
    exact(ROOT, SOURCE, SOURCE_TREE, baseline)
    write("source-manifest.json", baseline)
    body = git(ROOT, "show", "-s", "--format=%B", SOURCE).decode()
    for cell in (CAP, OLD):
        assert f"Claim-Cell: {cell.split('::')[-1]}" in body.splitlines()
    (OUT / "source-commit-message.txt").write_text(body, encoding="utf-8")
    environment = {
        name: os.environ.get(name)
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
    assert Path(os.environ["CARGO_HOME"], "config.toml").is_file()
    assert Path(os.environ["CARGO_TARGET_DIR"], ".sutura-warm-start").is_file()
    environment["warm_stamp"] = Path(
        os.environ["CARGO_TARGET_DIR"], ".sutura-warm-start"
    ).read_text(encoding="utf-8")
    environment["inherited_config_sha256"] = digest(
        Path(os.environ["CARGO_HOME"], "config.toml").read_bytes()
    )
    # Same repair as the finishing helper: keep warm targets, resolve in an empty native home.
    os.environ["CARGO_HOME"] = tempfile.mkdtemp(
        prefix="oracle-cap-cargo-", dir=os.environ["RUNNER_TEMP"]
    )
    os.environ.pop("CARGO_NET_OFFLINE", None)
    os.environ.pop("SUTURA_SATELLITE_CARGO_VENDOR_DIR", None)
    environment["native_home"] = os.environ["CARGO_HOME"]
    assert not list(Path(os.environ["CARGO_HOME"]).iterdir())
    write("environment.json", environment)
    initial_index = git(ROOT, "ls-files", "--stage", "-z")
    (OUT / "index-initial.z").write_bytes(initial_index)
    try:
        text = test("adapter-head", ROOT, DIRECT)
        markers(
            text,
            ADAPTER_MARKERS
            + ("oracle-partition-row-cap: overflow refused as result_too_large",),
        )
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        base_proof(baseline)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        markers(test("served-cap-head", ROOT, CAP), SERVED_MARKERS + (CAP_PASS,))
        mutation(
            "mapping",
            f"devco/claim-mutations/{CAP.split('::')[-1]}.patch",
            CAP,
            (
                SERVED,
                543,
                "the granted oversized Oracle bucket must be a refusal",
                503,
                413,
            ),
            baseline,
        )
        markers(test("served-differential-head", ROOT, OLD), (OLD_PASS,))
        mutation(
            "whole",
            f"devco/claim-mutations/{OLD.split('::')[-1]}.patch",
            OLD,
            (
                SERVED,
                339,
                "a granted Oracle lookup with no resolved reader must refuse",
                200,
                503,
            ),
            baseline,
        )
        mutation(
            "reader",
            "nix/oracle-split-reader.patch",
            OLD,
            (
                SERVED,
                353,
                "the granted Oracle lookup must answer through its reading workers",
                413,
                200,
            ),
            baseline,
        )
    finally:
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        final_index = git(ROOT, "ls-files", "--stage", "-z")
        (OUT / "index-final.z").write_bytes(final_index)
        write(
            "final-state.json",
            {
                "manifest": state(ROOT),
                "status": git(ROOT, "status", "--porcelain").decode(),
                "staged_entries_unchanged": final_index == initial_index,
            },
        )
        assert final_index == initial_index
    write(
        "verdict.json",
        {
            "status": "exact named head/base, three own-assertion kills and restored passes",
            "limit": "Live single-host tier; shared service user; row check after fetch/decode. No topology, ordinary CI or finishing verdict.",
        },
    )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
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
    OUT = ROOT / "target/oracle-cap-proof"
    if os.environ.get("SUTURA_ORACLE_CAP_PROOF_BOUND") == "1":
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
                "schema": "oracle-cap-proof/v1",
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

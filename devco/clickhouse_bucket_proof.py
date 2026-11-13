"""Hosted bucket/own-user witnesses only; full gates and fleet evidence remain separate."""

import difflib
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

SOURCE = "b3cd23eac05a90212c494fd450a2c6462db7ffd0"
SOURCE_TREE = "991850e66f23c4a87c4df414d7a026525b9680be"
BASE = "3ee8874e6c5b3054a91a63c28aaca90302bf2dbe"
BASE_TREE = "6903bc16ed77cb039a7bcd1de0638a848c372432"
TIER = "a_clickhouse_lookup_split_reads_every_group_once_under_the_callers_grants_and_settings"
GUARD = "a_switched_bucket_refuses_the_reading_workers_own_service_user_before_loading_the_driver"
TEST = "crates/sutura-exec-clickhouse/tests/adbc_execute_as_legs.rs"
OVERLAY_FILES = (TEST,)
IMPLEMENTATION = "crates/sutura-exec-clickhouse/src/adbc/buckets.rs"
OVERLAY_HASH = "626bc8c7d751c468a0de306b8ee74ce7e22e2ca94b9249decb3e84080d5b9ce0"
MUTATION_HASH = "5fa70b7c3846e1d6fe45a90bb7c4115f3f52ae533a36b424fb5ca7559d83e34c"
DIFF = (
    "diff",
    "--binary",
    "--full-index",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--diff-algorithm=myers",
)
LOCKS = ("Cargo.lock", "flake.lock", "fuzz/Cargo.lock", "pixi.lock")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(root), *args])


def write(name: str, value: object) -> None:
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def execute(
    label: str, command: list[str], root: Path, *, separate_stdout: bool = False
) -> tuple[int, str]:
    print(label, flush=True)
    with tempfile.TemporaryFile() as log:
        result = subprocess.run(
            command,
            cwd=root,
            stdout=subprocess.PIPE if separate_stdout else log,
            stderr=log,
            check=False,
        )
        log.seek(0)
        text = log.read().decode(errors="replace")
    stdout = result.stdout.decode(errors="replace") if separate_stdout else ""
    text += stdout
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets.extend(
        value
        for key, value in os.environ.items()
        if re.search(r"PASSWORD|TOKEN|SECRET|CREDENTIAL", key)
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
    return result.returncode, stdout if separate_stdout else re.sub(
        r"\x1b\[[0-?]*[ -/]*[@-~]", "", text
    )


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
    assert not git(root, "ls-files", "--others", "--exclude-standard"), (
        "untracked source"
    )


def bootstrap() -> None:
    installable = ".#apps.x86_64-linux.causality.program"
    commands = [
        ["nix", "eval", "--raw", installable],
        ["nix", "eval", "--json", "--apply", "builtins.getContext", installable],
    ]
    status, output = execute(
        "bootstrap-program-eval", commands[0], ROOT, separate_stdout=True
    )
    assert status == 0
    app = Path(output)
    status, output = execute(
        "bootstrap-context-eval", commands[1], ROOT, separate_stdout=True
    )
    assert status == 0
    context = json.loads(output)
    assert len(context) == 1 and all(path.endswith(".drv") for path in context)
    write(
        "bootstrap-eval.json",
        {"commands": commands, "exits": [0, 0], "app": str(app), "context": context},
    )
    status, _ = execute("bootstrap-realise", ["nix-store", "--realise", *context], ROOT)
    assert status == 0
    original = app.read_text(encoding="utf-8")
    marker = 'exec cargo run -q --profile ci -p xtask -- test-causality "$@"'
    assert original.count(marker) == 1
    # ponytail: keep the source's warm target, provisioner and outer teardown trap; replace its test tail only.
    command = shlex.join(["python3", "-I", str(Path(__file__).resolve())])
    replacement = (
        "export SUTURA_DEV_REQUIRE_TIER=1\n  export SUTURA_CLICKHOUSE_BUCKET_PROOF_BOUND=1\n  exec "
        + command
    )
    script = original.replace(marker, replacement)
    (OUT / "environment-original.sh").write_text(original, encoding="utf-8")
    (OUT / "environment-proof.sh").write_text(script, encoding="utf-8")
    write(
        "bootstrap.json",
        {
            "app": str(app),
            "context": context,
            "original_sha256": digest(original.encode()),
            "proof_sha256": digest(script.encode()),
            "marker": marker,
            "outer_teardown_retained": True,
        },
    )
    status, _ = execute(
        "bootstrap-run", ["bash", str(OUT / "environment-proof.sh")], ROOT
    )
    assert status == 0 and (OUT / "verdict.json").is_file(), (
        "proof failed; original failure logs retained"
    )


def test_command(unit: bool) -> list[str]:
    command = [
        "cargo",
        "test",
        "--locked",
        "--profile",
        "ci",
        "-p",
        "sutura-exec-clickhouse",
        "--all-features",
    ]
    if unit:
        command += ["--lib"]
        # The own-user cell uses a deliberately absent driver and needs no tier credentials.
        command = [
            "env",
            "-u",
            "SUTURA_DEV_REQUIRE_TIER",
            "-u",
            "SUTURA_CLICKHOUSE_TIER_USER",
            "-u",
            "SUTURA_CLICKHOUSE_TIER_PASSWORD",
            "-u",
            "SUTURA_CLICKHOUSE_TIER_TOKEN",
            *command,
        ]
    else:
        command += ["--test", "adbc_execute_as_legs"]
    return command


def discover(label: str, root: Path, unit: bool) -> str:
    status, text = execute(label, [*test_command(unit), "--", "--list"], root)
    assert status == 0, "listing/build failure is not behavior evidence"
    suffix = GUARD if unit else TIER
    names = re.findall(r"^([^\n ]+): test$", text, re.MULTILINE)
    selected = [name for name in names if name.split("::")[-1] == suffix]
    assert len(selected) == 1
    assert selected[0] in (
        ("adbc::buckets::tests::" + GUARD,)
        if unit
        else (
            "adbc_execute_as_legs::buckets::" + TIER,
            "buckets::" + TIER,
        )
    ), "unexpected actual libtest namespace"
    write(
        label + "-selection.json",
        {"actual_name": selected[0], "all_names_sha256": digest(text.encode())},
    )
    return selected[0]


def test(
    label: str,
    root: Path,
    cell: str,
    unit: bool,
    failure: tuple[str, int, str] | None = None,
) -> None:
    status, text = execute(
        label,
        [*test_command(unit), cell, "--", "--exact", "--nocapture", "--test-threads=1"],
        root,
    )
    assert re.findall(r"^running (\d+) test[s]?$", text, re.MULTILINE) == ["1"], (
        "not one executed cell"
    )
    assert len(re.findall(rf"^test {re.escape(cell)} \.\.\. ", text, re.MULTILINE)) == 1
    verdict = "FAILED" if failure else "ok"
    assert re.search(
        rf"^{verdict}$|^test {re.escape(cell)} \.\.\. {verdict}$", text, re.MULTILINE
    )
    summary = (
        "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
        if failure
        else "test result: ok. 1 passed; 0 failed; 0 ignored;"
    )
    assert (
        len(re.findall(r"^test result: ", text, re.MULTILINE)) == 1 and summary in text
    )
    assert status == (101 if failure else 0), (
        "compile/setup/other exit is not a witness"
    )
    assert (
        "NOT RUN" not in text
        and "This run requires a provisioned service tier" not in text
    )
    if failure:
        path, line, message = failure
        panic = re.search(
            rf"^thread '{re.escape(cell)}'(?: \([^\n]*\))? panicked at {re.escape(path)}:{line}:\d+:\n([^\n]+)",
            text,
            re.MULTILINE,
        )
        assert panic and message in panic[1], "not the intended cell's own assertion"
        write(
            label + "-own-assertion.json",
            {"cell": cell, "site": f"{path}:{line}", "panic": panic[0]},
        )


def base_proof(baseline: dict, tier_cell: str) -> None:
    overlay = git(ROOT, *DIFF, BASE, SOURCE, "--", *OVERLAY_FILES)
    assert digest(overlay) == OVERLAY_HASH
    overlay_path = OUT / "base-test-only.patch"
    overlay_path.write_bytes(overlay)
    base = (
        Path(
            tempfile.mkdtemp(
                prefix="clickhouse-proof-base-", dir=os.environ["RUNNER_TEMP"]
            )
        )
        / "source"
    )
    status, _ = execute(
        "base-worktree", ["git", "worktree", "add", "--detach", str(base), BASE], ROOT
    )
    assert status == 0
    before = state(base)
    initial_index = git(base, "ls-files", "--stage", "-z")
    (OUT / "base-index-initial.z").write_bytes(initial_index)
    exact(base, BASE, BASE_TREE, before)
    expected = before | {name: baseline[name] for name in OVERLAY_FILES}
    discovery = ROOT / ".sutura-dev/endpoints.json"
    applied = False
    try:
        status, _ = execute("base-apply", ["git", "apply", str(overlay_path)], base)
        assert status == 0
        applied = True
        exact(base, BASE, BASE_TREE, expected)
        assert git(base, "diff", "HEAD", "--name-only").decode().splitlines() == list(
            OVERLAY_FILES
        )
        assert digest(git(base, *DIFF, "HEAD")) == OVERLAY_HASH
        (base / ".sutura-dev").mkdir()
        shutil.copyfile(discovery, base / ".sutura-dev/endpoints.json")
        write(
            "base-overlay.json",
            {
                "base": BASE,
                "base_tree": BASE_TREE,
                "source": SOURCE,
                "overlay_sha256": digest(overlay),
                "allowed_changes": OVERLAY_FILES,
                "base_manifest": before,
                "injected_manifest": expected,
                "shared_discovery_sha256": digest(
                    (base / ".sutura-dev/endpoints.json").read_bytes()
                ),
                "locks": {
                    name: {"source": baseline[name][1], "base": before[name][1]}
                    for name in LOCKS
                },
            },
        )
        assert discover("base-list", base, False) == tier_cell
        test(
            "tier-base",
            base,
            tier_cell,
            False,
            (TEST, 794, "the 34-row lookup must split across two 17-row targets"),
        )
        exact(base, BASE, BASE_TREE, expected)
    finally:
        if applied:
            status, _ = execute(
                "base-restore", ["git", "apply", "--reverse", str(overlay_path)], base
            )
            assert status == 0
        final_index = git(base, "ls-files", "--stage", "-z")
        (OUT / "base-index-final.z").write_bytes(final_index)
        write(
            "base-final-state.json",
            {
                "manifest": state(base),
                "staged_entries_unchanged": final_index == initial_index,
                "status": git(base, "status", "--porcelain").decode(),
            },
        )
        exact(base, BASE, BASE_TREE, before)
        assert final_index == initial_index
        subprocess.run(["git", "worktree", "remove", str(base)], cwd=ROOT, check=True)


def guard_proof(baseline: dict, guard_cell: str) -> None:
    path = ROOT / IMPLEMENTATION
    original = path.read_bytes()
    guard = b" if name.as_str() != adapter.username"
    assert original.count(guard) == 1
    mutant = original.replace(guard, b"", 1)
    patch = "".join(
        difflib.unified_diff(
            original.decode().splitlines(keepends=True),
            mutant.decode().splitlines(keepends=True),
            fromfile="a/" + IMPLEMENTATION,
            tofile="b/" + IMPLEMENTATION,
        )
    ).encode()
    assert digest(patch) == MUTATION_HASH
    patch_path = OUT / "own-user-guard-removed.patch"
    patch_path.write_bytes(patch)
    test("guard-head", ROOT, guard_cell, True)
    try:
        status, _ = execute("guard-apply", ["git", "apply", str(patch_path)], ROOT)
        assert status == 0
        expected = baseline | {
            IMPLEMENTATION: [baseline[IMPLEMENTATION][0], digest(mutant)]
        }
        exact(ROOT, SOURCE, SOURCE_TREE, expected)
        assert git(ROOT, "diff", "--name-only").decode().splitlines() == [
            IMPLEMENTATION
        ]
        test(
            "guard-removed",
            ROOT,
            guard_cell,
            True,
            (IMPLEMENTATION, 280, "could not load the ClickHouse ADBC driver:"),
        )
    finally:
        path.write_bytes(original)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
    test("guard-restored", ROOT, guard_cell, True)


def proof() -> None:
    assert os.environ.get("SUTURA_DEV_REQUIRE_TIER") == "1"
    baseline = state(ROOT)
    exact(ROOT, SOURCE, SOURCE_TREE, baseline)
    initial_index = git(ROOT, "ls-files", "--stage", "-z")
    (OUT / "index-initial.z").write_bytes(initial_index)
    write("source-manifest.json", baseline)
    write("source-locks.json", {name: baseline[name] for name in LOCKS})
    discovery = ROOT / ".sutura-dev/endpoints.json"
    document = json.loads(discovery.read_text(encoding="utf-8"))
    endpoint = document["services"]["clickhouse"]
    assert endpoint["host"] == "127.0.0.1" and 0 < endpoint["port"] < 65536
    assert os.environ.get("SUTURA_CLICKHOUSE_TIER_USER") and os.environ.get(
        "SUTURA_CLICKHOUSE_TIER_PASSWORD"
    )
    driver = Path(os.environ["SUTURA_CLICKHOUSE_ADBC_DRIVER"])
    assert driver.is_file() and str(driver).startswith("/nix/store/")
    (OUT / "endpoints.json").write_bytes(discovery.read_bytes())
    environment = {
        "require_tier": os.environ["SUTURA_DEV_REQUIRE_TIER"],
        "endpoint": endpoint,
        "discovery_sha256": digest(discovery.read_bytes()),
        "driver": str(driver),
        "driver_sha256": digest(driver.read_bytes()),
        "inherited_cargo_home": os.environ["CARGO_HOME"],
        "target": os.environ["CARGO_TARGET_DIR"],
        "warm_stamp": Path(
            os.environ["CARGO_TARGET_DIR"], ".sutura-warm-start"
        ).read_text(encoding="utf-8"),
        "inherited_config_sha256": digest(
            Path(os.environ["CARGO_HOME"], "config.toml").read_bytes()
        ),
        "credential_values_retained": False,
        "shared_live_fixture": True,
    }
    for name in ("cargo", "rustc", "clang"):
        tool = shutil.which(name)
        assert tool and tool.startswith("/nix/store/"), (name, tool)
        environment[name] = tool
    # ponytail: retain the pinned warm target; use the accepted empty native Cargo-home resolution route.
    os.environ["CARGO_HOME"] = tempfile.mkdtemp(
        prefix="clickhouse-proof-cargo-", dir=os.environ["RUNNER_TEMP"]
    )
    os.environ.pop("CARGO_NET_OFFLINE", None)
    os.environ.pop("SUTURA_SATELLITE_CARGO_VENDOR_DIR", None)
    assert not list(Path(os.environ["CARGO_HOME"]).iterdir())
    environment["native_cargo_home"] = os.environ["CARGO_HOME"]
    write("environment.json", environment)
    try:
        tier_cell = discover("head-tier-list", ROOT, False)
        guard_cell = discover("head-guard-list", ROOT, True)
        test("tier-head", ROOT, tier_cell, False)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        base_proof(baseline, tier_cell)
        guard_proof(baseline, guard_cell)
        test("tier-restored", ROOT, tier_cell, False)
    finally:
        final_index = git(ROOT, "ls-files", "--stage", "-z")
        final_manifest = state(ROOT)
        (OUT / "index-final.z").write_bytes(final_index)
        write(
            "final-state.json",
            {
                "manifest": final_manifest,
                "locks": {name: final_manifest[name] for name in LOCKS},
                "status": git(ROOT, "status", "--porcelain").decode(),
                "staged_entries_unchanged": initial_index == final_index,
            },
        )
        exact(ROOT, SOURCE, SOURCE_TREE, baseline)
        assert final_index == initial_index
    write(
        "verdict.json",
        {
            "status": "named tier head/base split witness; own-user guard head/mutation/restored; final tier pass",
            "guard_base_verdict": "NONE_SAME_FILE",
            "later_controls_independently_killed": False,
            "limit": "One source-pinned live tier shared by head/base. No fleet, two distinct successful callers, placement, served grant negatives, full gates or delivery verdict.",
        },
    )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    assert (
        os.environ["SOURCE_SHA"],
        os.environ["SOURCE_TREE"],
        os.environ["COMPARISON_SHA"],
    ) == (SOURCE, SOURCE_TREE, BASE)
    assert str(uuid.UUID(os.environ["REQUEST_ID"])) == os.environ["REQUEST_ID"]
    assert os.environ["GITHUB_REF_NAME"] == "ci/clickhouse-bucket-proof"
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    ROOT = Path.cwd().resolve()
    HELPER = Path(__file__).resolve().parent.parent
    assert ROOT != HELPER
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == SOURCE_TREE
    assert git(ROOT, "rev-parse", BASE + "^{tree}").decode().strip() == BASE_TREE
    assert git(ROOT, "merge-base", BASE, SOURCE).decode().strip() == BASE
    assert (
        git(HELPER, "rev-parse", "HEAD").decode().strip()
        == os.environ["GITHUB_WORKFLOW_SHA"]
    )
    assert not git(HELPER, "status", "--porcelain", "--untracked-files=no")
    assert not git(ROOT, "status", "--porcelain")
    OUT = ROOT / "target/clickhouse-bucket-proof"
    if os.environ.get("SUTURA_CLICKHOUSE_BUCKET_PROOF_BOUND") == "1":
        assert (OUT / "provenance.json").is_file()
        proof()
    else:
        OUT.mkdir(parents=True, exist_ok=False)
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
                "schema": "clickhouse-bucket-proof/v1",
                "request_sha256": digest(
                    json.dumps(request, indent=2).encode() + b"\n"
                ),
                "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
                "helper_tree": git(HELPER, "rev-parse", "HEAD^{tree}").decode().strip(),
                "payload_sha256": digest(Path(__file__).read_bytes()),
                "workflow_sha256": digest(
                    (HELPER / ".github/workflows/format.yml").read_bytes()
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

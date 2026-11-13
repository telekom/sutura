"""Hosted isolated native bucket controls; full gates and fleet evidence remain separate."""

import difflib
import hashlib
import http.client
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

SOURCE = "dc78fb01027cdea69dce3338b7b85614da9dc317"
SOURCE_TREE = "fda87660821d59cdcb673e783eb36dfd4662e4a0"
REF = "ci/clickhouse-later-controls-proof"
REQUEST_ID = "284ae664-975d-4e88-babe-b948a5e8839e"
TIER = "a_clickhouse_lookup_split_reads_every_group_once_under_the_callers_grants_and_settings"
TEST = "crates/sutura-exec-clickhouse/tests/adbc_execute_as_legs.rs"
LOCKS = (
    "Cargo.lock",
    "flake.lock",
    "fuzz/Cargo.lock",
    "pixi.lock",
    "nix/clickhouse-adbc.Cargo.lock",
)
DIFF = (
    "diff",
    "--binary",
    "--full-index",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--diff-algorithm=myers",
)
CONTROLS = (
    {
        "name": "ungranted",
        "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",
        "edits": [
            (
                "            RunsAs::Switched(crate::execute_as::ClickHouseUser::parse(name.as_str()).map_err(AdbcError::NotAClickHouseUser)?)",
                "            RunsAs::Deployment",
            )
        ],
        "line": 824,
        "message": "no IMPERSONATE means no bucket",
        "scope": "bucket switching and refusal without service fallback; first bucket only",
        "patch_sha256": "42033ea5e447ca130adeeb0bc33df2daff55147a6947b09fe519a2b67c72f5b1",
        "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",
        "mutant_sha256": "84712abc90cdb7ed11586f5d8a3224f60bb426a541b03b11ede7248a692258d3",
    },
    {
        "name": "missing-principal",
        "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",
        "edits": [
            (
                "    let runs_as = match (&adapter.posture, identity) {\n",
                "    let runs_as = match (&adapter.posture, identity) {\n        (SourcePosture::ImpersonationAtSource, PartitionIdentity::ServiceUser) => RunsAs::Deployment,\n",
            )
        ],
        "line": 836,
        "message": "a missing principal never falls back to the deployment",
        "scope": "receiving bucket admission of a missing principal; first bucket only",
        "patch_sha256": "608404eb4a9c8e277b3701c8d5b2e5b9015fb18502e166dacecbb8af43d263fa",
        "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",
        "mutant_sha256": "0a9012283c9e7b2582bd34452597c2ccad2e9002bafd3a6f3a719980c66d5ae4",
    },
    {
        "name": "shared-mismatch",
        "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",
        "edits": [
            (
                "    let runs_as = match (&adapter.posture, identity) {\n",
                "    let runs_as = match (&adapter.posture, identity) {\n        (SourcePosture::SharedServiceUser { .. }, PartitionIdentity::Switched(_)) => RunsAs::Deployment,\n",
            )
        ],
        "line": 849,
        "message": "a shared source does not silently drop a switched principal",
        "scope": "receiving shared-source/switched-identity mismatch; first bucket only",
        "patch_sha256": "a9456c61fb94563ca2c55905ee700781072d239b224bf1c2be2be7a1bd5c1088",
        "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",
        "mutant_sha256": "e9e84afa8ae9cc698d27952ebc5dbf2d5eb90e5f6aaac611a08cd2b29be644ec",
    },
    {
        "name": "undeclared",
        "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",
        "edits": [
            (
                "    if !adapter.hash_buckets {\n        return Err(AdbcError::HashBucketsNotDeclared);\n    }\n",
                "",
            )
        ],
        "line": 861,
        "message": "an undeclared source reads no bucket",
        "scope": "receiving bucket opt-in only; planner guard unchanged; first bucket only",
        "patch_sha256": "153fbb4890fe48271560b39581bdaea77fce20694382486b7920d6a5fd119bbf",
        "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",
        "mutant_sha256": "35850ba4fb6ffb7dcfa856c4549325c0459ac583119afef28ff7c4b594b715b1",
    },
    {
        "name": "combined-schema",
        "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",
        "edits": [
            (
                "    if described != *granted {\n        return Err(AdbcError::PartitionSchemaChanged);\n    }\n",
                "    let _ = granted;\n",
            ),
            (
                "    if rows.schema() != granted {\n        return Err(AdbcError::PartitionSchemaChanged);\n    }\n",
                "",
            ),
        ],
        "line": 874,
        "message": "a mismatched grant schema reads no bucket",
        "scope": "combined schema agreement barriers only; neither barrier independently; first bucket only",
        "patch_sha256": "b6fb27669499698c11ec84680f1a3835da2293f083eb9e693240f68ccfd0cf95",
        "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",
        "mutant_sha256": "196eb6f5d60f1cbdce81b1425f3808beb2ac5a6934e027800bc6ba75129ed506",
    },
    {
        "name": "readonly-mapper",
        "path": "crates/sutura-exec-clickhouse/src/adbc.rs",
        "edits": [
            (
                "    const READONLY: Self = Self(164);",
                "    const READONLY: Self = Self(165);",
            )
        ],
        "line": 835,
        "message": "ClickHouse ADBC call failed:",
        "scope": "native code 164 to typed SettingRefused only; no code 452 or own-clause semantic credit",
        "patch_sha256": "84d8317f99eb79e1526cac2bbdc7312637607e39b0afc5c1008f804aac6cdd3c",
        "original_sha256": "5c426b7f0ee735e46010ba8e55bf9bc41865ad878f169389c7229f42a913da56",
        "mutant_sha256": "21d484678d57a8d7b9da88466bf5dbc120aded2d9f96929d4d20cde18f5383e1",
    },
    {
        "name": "readonly-settings",
        "path": "crates/sutura-exec-clickhouse/src/adbc.rs",
        "edits": [
            (
                "            .map_err(|settings::Lifted(keyword)| AdbcError::SettingsNotOwn { keyword })?;\n        let sql = match *runs_as {",
                '            .map_err(|settings::Lifted(keyword)| AdbcError::SettingsNotOwn { keyword })?;\n        let sql = match sql.split_once(" SETTINGS ") {\n            Some((select, _)) => select.to_owned(),\n            None => sql,\n        };\n        let sql = match *runs_as {',
            )
        ],
        "line": 834,
        "message": "readonly must not drop required settings",
        "scope": "SQL own-clause settings only; driver options retained; a survivor earns no credit",
        "patch_sha256": "97572934e6e695bb699cb5385ea3bdcb8aed738a5690787907eb56c5cd211ee6",
        "original_sha256": "5c426b7f0ee735e46010ba8e55bf9bc41865ad878f169389c7229f42a913da56",
        "mutant_sha256": "d60f2805f758fcea4da20c765a9b72f6b236d76d5232cd509a99efedbc58d4ec",
    },
)


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
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    return text


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
    text = redact(text)
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


def index_bytes(root: Path) -> bytes:
    path = (
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    return Path(path).read_bytes()


def exact(
    root: Path, revision: str, tree: str, expected: dict, original_index: bytes
) -> None:
    assert git(root, "rev-parse", "HEAD").decode().strip() == revision
    assert git(root, "rev-parse", "HEAD^{tree}").decode().strip() == tree
    assert state(root) == expected, "tracked bytes or modes changed"
    assert index_bytes(root) == original_index, "raw source index changed"
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
        "export SUTURA_DEV_REQUIRE_TIER=1\n  export SUTURA_CLICKHOUSE_LATER_CONTROLS_PROOF_BOUND=1\n  exec "
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


def test_command() -> list[str]:
    return [
        "cargo",
        "test",
        "--locked",
        "--profile",
        "ci",
        "-p",
        "sutura-exec-clickhouse",
        "--all-features",
        "--test",
        "adbc_execute_as_legs",
    ]


def discover(label: str, root: Path) -> str:
    status, text = execute(label, [*test_command(), "--", "--list"], root)
    assert status == 0, "listing/build failure is not behavior evidence"
    names = re.findall(r"^([^\n ]+): test$", text, re.MULTILINE)
    selected = [name for name in names if name.split("::")[-1] == TIER]
    assert len(selected) == 1
    assert selected[0] in (
        "adbc_execute_as_legs::buckets::" + TIER,
        "buckets::" + TIER,
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
    failure: tuple[str, int, str] | None = None,
) -> str:
    status, text = execute(
        label,
        [*test_command(), cell, "--", "--exact", "--nocapture", "--test-threads=1"],
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
        assert (
            len(re.findall(r"^thread [^\n]+ panicked at ", text, re.MULTILINE)) == 1
        ), "not exactly one first panic"
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

    return text


def native_query(sql: str) -> str:
    endpoint = json.loads(
        (ROOT / ".sutura-dev/endpoints.json").read_text(encoding="utf-8")
    )["services"]["clickhouse"]
    assert endpoint["host"] == "127.0.0.1" and 0 < endpoint["port"] < 65536
    connection = http.client.HTTPConnection(
        endpoint["host"], endpoint["port"], timeout=30
    )
    try:
        connection.request(
            "POST",
            "/?log_queries=0",
            body=sql.encode(),
            headers={
                "X-ClickHouse-User": os.environ["SUTURA_CLICKHOUSE_TIER_USER"],
                "X-ClickHouse-Key": os.environ["SUTURA_CLICKHOUSE_TIER_PASSWORD"],
            },
        )
        response = connection.getresponse()
        text = response.read().decode()
        assert response.status == 200, redact(text)
        return text
    finally:
        connection.close()


def native_test(
    label: str, cell: str, failure: tuple[str, int, str] | None = None
) -> tuple[str, list[dict]]:
    since = native_query("SELECT toUnixTimestamp64Micro(now64(6)) FORMAT TSV").strip()
    assert re.fullmatch(r"[0-9]+", since)
    write(
        label + "-checkpoint.json",
        {
            "since_microseconds": since,
            "scope": "entire combined cell phase, not a per-call absence witness",
        },
    )
    try:
        text = test(label, ROOT, cell, failure)
    finally:
        native_query("SYSTEM FLUSH LOGS")
        raw = native_query(
            "SELECT event_time_microseconds, toString(type) AS type, query_id, initial_query_id, "
            "user, initial_user, toString(address) AS address, toString(initial_address) AS initial_address, "
            "toString(query_kind) AS query_kind, current_database, query, Settings, "
            "result_rows, result_bytes, read_rows, read_bytes, exception_code, exception "
            "FROM system.query_log WHERE toUnixTimestamp64Micro(event_time_microseconds) >= "
            + since
            + " ORDER BY event_time_microseconds, query_id, type FORMAT JSONEachRow"
        )
        raw = redact(raw)
        (OUT / (label + "-native-query-log.jsonl")).write_text(raw, encoding="utf-8")
    events = [json.loads(line) for line in raw.splitlines() if line]
    return text, events


def native_causes(events: list[dict]) -> dict:
    return {
        "ungranted_497_impersonate": any(
            event["exception_code"] == 497
            and 'EXECUTE AS "sutura_switched_ungranted"' in event["query"]
            and "IMPERSONATE" in event["exception"].upper()
            and "sutura_switched_ungranted" in event["exception"]
            for event in events
        ),
        "readonly_164": any(
            event["exception_code"] == 164
            and (
                'EXECUTE AS "sutura_switched_readonly"' in event["query"]
                or event["user"] == "sutura_switched_readonly"
            )
            for event in events
        ),
    }


def control_proof(
    control: dict, baseline: dict, original_index: bytes, cell: str, pristine_label: str
) -> dict:
    exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
    fixture_config = (ROOT / ".sutura-dev/clickhouse/config.yaml").read_bytes()
    discovery_bytes = (ROOT / ".sutura-dev/endpoints.json").read_bytes()
    name, relative = control["name"], control["path"]
    path = ROOT / relative
    original, mode = path.read_bytes(), path.stat().st_mode
    assert digest(original) == control["original_sha256"]
    mutant = original
    for before, after in control["edits"]:
        assert mutant.count(before.encode()) == 1, "mutation anchor not unique"
        mutant = mutant.replace(before.encode(), after.encode(), 1)
    assert digest(mutant) == control["mutant_sha256"]
    patch = "".join(
        difflib.unified_diff(
            original.decode().splitlines(keepends=True),
            mutant.decode().splitlines(keepends=True),
            fromfile="a/" + relative,
            tofile="b/" + relative,
        )
    ).encode()
    assert digest(patch) == control["patch_sha256"]
    patch_path = OUT / (name + ".patch")
    patch_path.write_bytes(patch)
    result = {
        "name": name,
        "scope": control["scope"],
        "patch_sha256": digest(patch),
        "intended_first_assertion": f"{TEST}:{control['line']}",
        "pristine_pass_reference": pristine_label,
        "own_assertion_killed": False,
        "accepted_kill": False,
        "restored_pass": False,
        "status": "UNRUN",
    }
    write(name + "-verdict.json", result)
    try:
        status, _ = execute(
            name + "-check", ["git", "apply", "--check", str(patch_path)], ROOT
        )
        assert status == 0
        status, _ = execute(name + "-apply", ["git", "apply", str(patch_path)], ROOT)
        assert status == 0
        expected = baseline | {relative: [baseline[relative][0], digest(mutant)]}
        exact(ROOT, SOURCE, SOURCE_TREE, expected, original_index)
        assert git(ROOT, "diff", "--name-only").decode().splitlines() == [relative]
        try:
            text, events = native_test(
                name + "-mutant", cell, (TEST, control["line"], control["message"])
            )
            if name == "readonly-mapper":
                assert (
                    re.search(r"Code: 164\b", text)
                    and native_causes(events)["readonly_164"]
                ), "mapper failed without the native 164 cause"
            result.update(
                own_assertion_killed=True,
                status="OWN_ASSERTION_KILLED_RESTORATION_PENDING",
            )
        except AssertionError as error:
            result.update(status="NO_ACCEPTED_KILL", reason=redact(str(error)))
        write(name + "-verdict.json", result)
    finally:
        path.write_bytes(original)
        path.chmod(mode)
        exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
        _, restored_events = native_test(name + "-restored", cell)
        assert all(native_causes(restored_events).values()), (
            "restored fixture native causes changed"
        )
        exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
        assert (
            ROOT / ".sutura-dev/clickhouse/config.yaml"
        ).read_bytes() == fixture_config, "tier configuration changed"
        assert (ROOT / ".sutura-dev/endpoints.json").read_bytes() == discovery_bytes, (
            "tier endpoint changed"
        )
        result["restored_pass"] = True
        result["accepted_kill"] = result["own_assertion_killed"]
        if result["accepted_kill"]:
            result["status"] = "OWN_ASSERTION_KILLED_AND_RESTORED"
        write(name + "-verdict.json", result)
    return result


def proof() -> None:
    assert os.environ.get("SUTURA_DEV_REQUIRE_TIER") == "1"
    baseline = state(ROOT)
    original_index = index_bytes(ROOT)
    (OUT / "index-initial.bin").write_bytes(original_index)
    exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
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
        "server_version": native_query("SELECT version() FORMAT TSV").strip(),
        "fixture_config_sha256": digest(
            (ROOT / ".sutura-dev/clickhouse/config.yaml").read_bytes()
        ),
    }
    assert environment["server_version"] == "26.9.15.5"
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
        tier_cell = discover("head-tier-list", ROOT)
        _, events = native_test("tier-head", tier_cell)
        causes = native_causes(events)
        write("native-baseline-causes.json", causes)
        assert all(causes.values()), (
            "native IMPERSONATE/readonly causes not established"
        )
        exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
        results = []
        pristine_label = "tier-head"
        for control in CONTROLS:
            assert (
                digest((ROOT / ".sutura-dev/clickhouse/config.yaml").read_bytes())
                == environment["fixture_config_sha256"]
            )
            assert discovery.read_bytes() == (OUT / "endpoints.json").read_bytes()
            results.append(
                control_proof(
                    control, baseline, original_index, tier_cell, pristine_label
                )
            )
            pristine_label = control["name"] + "-restored"
        native_test("tier-final", tier_cell)
    finally:
        final_index = git(ROOT, "ls-files", "--stage", "-z")
        final_manifest = state(ROOT)
        final_raw_index = index_bytes(ROOT)
        (OUT / "index-final.bin").write_bytes(final_raw_index)
        (OUT / "index-final.z").write_bytes(final_index)
        write(
            "final-state.json",
            {
                "manifest": final_manifest,
                "locks": {name: final_manifest[name] for name in LOCKS},
                "status": git(ROOT, "status", "--porcelain").decode(),
                "staged_entries_unchanged": initial_index == final_index,
                "raw_index_unchanged": original_index == final_raw_index,
            },
        )
        exact(ROOT, SOURCE, SOURCE_TREE, baseline, original_index)
        assert final_index == initial_index
    killed = {result["name"]: result["accepted_kill"] for result in results}
    write(
        "verdict.json",
        {
            "status": "all isolated controls killed"
            if all(killed.values())
            else "incomplete; ineffective attempts receive no credit",
            "controls": results,
            "later_controls_independently_killed": len(killed) == 7
            and all(killed.values()),
            "readonly_semantic_killed": killed["readonly-settings"],
            "readonly_mapper_killed": killed["readonly-mapper"],
            "schema_combined_barrier_killed": killed["combined-schema"],
            "schema_pre_read_independently_killed": False,
            "schema_post_read_independently_killed": False,
            "native_code_452_proven": False,
            "prior_r4_credit_transferred": False,
            "limit": "One native tier and unchanged combined cell; first bucket only. Phase query logs do not isolate a negative call or prove data absence. No fleet, placement, two successful differently granted callers, served grant, full gates, delivery or issue-completion claim.",
        },
    )
    assert len(killed) == 7 and all(killed.values()), (
        "not all required controls have accepted own-assertion witnesses"
    )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    assert (os.environ["SOURCE_SHA"], os.environ["SOURCE_TREE"]) == (
        SOURCE,
        SOURCE_TREE,
    )
    assert str(uuid.UUID(os.environ["REQUEST_ID"])) == os.environ["REQUEST_ID"]
    assert os.environ["REQUEST_ID"] == REQUEST_ID
    assert os.environ["GITHUB_REF_NAME"] == REF
    assert re.fullmatch(r"[0-9a-f]{40}", os.environ["HELPER_TREE"])
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    ROOT = Path.cwd().resolve()
    HELPER = Path(__file__).resolve().parent.parent
    assert ROOT != HELPER
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == SOURCE_TREE
    assert (
        git(HELPER, "rev-parse", "HEAD").decode().strip()
        == os.environ["GITHUB_WORKFLOW_SHA"]
    )
    assert (
        git(HELPER, "rev-parse", "HEAD^{tree}").decode().strip()
        == os.environ["HELPER_TREE"]
    )
    assert not git(HELPER, "status", "--porcelain", "--untracked-files=no")
    assert not git(ROOT, "status", "--porcelain")
    OUT = ROOT / "target/clickhouse-later-controls-proof"
    if os.environ.get("SUTURA_CLICKHOUSE_LATER_CONTROLS_PROOF_BOUND") == "1":
        assert (OUT / "provenance.json").is_file()
        proof()
    else:
        OUT.mkdir(parents=True, exist_ok=False)
        request = {
            "ref": os.environ["GITHUB_REF_NAME"],
            "inputs": {
                "source_sha": SOURCE,
                "source_tree": SOURCE_TREE,
                "helper_tree": os.environ["HELPER_TREE"],
                "request_id": os.environ["REQUEST_ID"],
            },
        }
        write("request.json", request)
        write(
            "provenance.json",
            {
                "schema": "clickhouse-later-controls-proof/v1",
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
                "controls_sha256": digest(
                    json.dumps(CONTROLS, sort_keys=True).encode()
                ),
                "run_id": os.environ["GITHUB_RUN_ID"],
                "run_attempt": 1,
            },
        )
        bootstrap()

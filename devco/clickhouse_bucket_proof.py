"""Six controlled-source own-site executions; historical base and full gates stay separate."""

import hashlib
import json
import os
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

SOURCE = "9969e37e2996406bda09a4beb72c882d427c6539"
SOURCE_TREE = "730494bb300390a539dc5290a2feb4c2a6a850a1"
REF = "ci/clickhouse-cli-own-site-proof"
REQUEST_ID = "6a14b8ca-62b1-4f9a-a204-fb46e1339ac3"
CONTROLS = json.loads(
    '[\n  {\n    "name": "switched-admission",\n    "path": "crates/sutura-cli/src/serve/worker/partition.rs",\n    "package": "sutura-cli",\n    "target": "--bin sutura",\n    "binary": "sutura-cli::bin/sutura",\n    "cell": "serve::worker::partition::tests::the_concrete_reader_checks_its_sources_verifier_membership_before_decoding_or_dispatch",\n    "patch_text": "--- a/crates/sutura-cli/src/serve/worker/partition.rs\\n+++ b/crates/sutura-cli/src/serve/worker/partition.rs\\n@@ -96,7 +96,7 @@\\n         switched: SwitchedPrincipal,\\n         capture: Option<SubmissionCapture>,\\n     ) -> Result<SendableRecordBatchStream, Unread> {\\n-        if !self.0.admissions.admits_verifier(source, verifier) {\\n+        if self.0.admissions.admits_verifier(source, verifier) {\\n             return Err(Unread::LegNotCertified);\\n         }\\n         let opened = Arc::clone(&self.0);\\n",\n    "patch_sha256": "e6dee0bb137ae6ec3e5d188e6b712da7e57908c54afa5f8305f0159cbca54148",\n    "original_sha256": "96b53a9012881961b9797fd49278e80413081261c59d9a158b7585162f45b381",\n    "mutant_sha256": "50f0eb5f7766c78f9c80c8300af4e7e53f2b8b58b01cbb75186c99d96c6225f3",\n    "mutant_tree": "a982ccec7c395e6d5432d747152dedce33317a12",\n    "fixture": "crates/sutura-cli/src/serve/worker/partition/tests.rs",\n    "fixture_sha256": "e84ce9f185a5a7c12474c92cfe3bc4c9adf5caf4fdfdb7a4b05cf10992216581",\n    "cell_lines": [\n      57,\n      101\n    ],\n    "cell_sha256": "5a5139959bf877f43aceb54216f2fc9218513faa899523052b48f8c0a8eeda94",\n    "own_line": 84,\n    "message_fragments": [\n      "assertion failed: matches!",\n      "reader.read_switched",\n      "Err(Unread::LegNotCertified)"\n    ],\n    "own_assertion_source": "assert!(matches!("\n  },\n  {\n    "name": "own-service-user",\n    "path": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",\n    "package": "sutura-exec-clickhouse",\n    "target": "--lib",\n    "binary": "sutura-exec-clickhouse",\n    "cell": "adbc::buckets::tests::a_switched_bucket_refuses_the_reading_workers_own_service_user_before_loading_the_driver",\n    "patch_text": "--- a/crates/sutura-exec-clickhouse/src/adbc/buckets.rs\\n+++ b/crates/sutura-exec-clickhouse/src/adbc/buckets.rs\\n@@ -220,7 +220,7 @@\\n     }\\n     let runs_as = match (&adapter.posture, identity) {\\n         (SourcePosture::SharedServiceUser { .. }, PartitionIdentity::ServiceUser) => RunsAs::Deployment,\\n-        (SourcePosture::ImpersonationAtSource, PartitionIdentity::Switched(name)) if name.as_str() != adapter.username => {\\n+        (SourcePosture::ImpersonationAtSource, PartitionIdentity::Switched(name)) => {\\n             RunsAs::Switched(crate::execute_as::ClickHouseUser::parse(name.as_str()).map_err(AdbcError::NotAClickHouseUser)?)\\n         }\\n         _ => return Err(AdbcError::PartitionIdentityDisagrees),\\n",\n    "patch_sha256": "ae3a1462886651b85ec1d7e8715093b14a7995aa659b38210046848b585c48c0",\n    "original_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",\n    "mutant_sha256": "4c9c1b70906d91717f53f83f23f08488961569b2ddc13eb42cf776dbd8bcc15c",\n    "mutant_tree": "c2239c0d561a78f6dde4e076bbc5879c32b7b8e0",\n    "fixture": "crates/sutura-exec-clickhouse/src/adbc/buckets.rs",\n    "fixture_sha256": "788d4b6b08a538b08e2da41a2ea968510bcb5f3bb47b01c367ffb4791c90b7af",\n    "cell_lines": [\n      258,\n      285\n    ],\n    "cell_sha256": "60f9144e37f11bcf141af01308cecb2aa0074eceaba0e1d8d156f1ec35c128a5",\n    "own_line": 284,\n    "message_fragments": [\n      "could not load the ClickHouse ADBC driver:"\n    ],\n    "own_assertion_source": "assert!(matches!(error, AdbcError::PartitionIdentityDisagrees), \\"{error}\\");"\n  }\n]\n'
)
LOCKS = (
    "Cargo.lock",
    "flake.lock",
    "fuzz/Cargo.lock",
    "pixi.lock",
    "nix/clickhouse-adbc.Cargo.lock",
)
TASK = """proof-clickhouse-reader package target cell:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean --locked --profile ci -p {{package}}
    exec cargo nextest run --locked --cargo-profile ci --profile default --all-features -p {{package}} {{target}} -E 'test(={{cell}})' --retries 0 --no-tests=fail --test-threads 1 --no-capture --status-level pass --final-status-level none
"""


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


def execute(
    label: str, command: list[str], root: Path, *, separate_stdout: bool = False
) -> tuple[int, str]:
    print(label, flush=True)
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        result = subprocess.run(
            command, cwd=root, stdout=stdout, stderr=stderr, check=False
        )
        stdout.seek(0)
        stderr.seek(0)
        original_stdout, original_stderr = stdout.read(), stderr.read()
    out = redact(original_stdout.decode(errors="replace"))
    err = redact(original_stderr.decode(errors="replace"))
    (OUT / (label + ".stdout.log")).write_text(out, encoding="utf-8")
    (OUT / (label + ".stderr.log")).write_text(err, encoding="utf-8")
    write(
        label + "-process.json",
        {
            "command": command,
            "exit": result.returncode,
            "original_stdout_sha256": digest(original_stdout),
            "original_stderr_sha256": digest(original_stderr),
            "retained_stdout_sha256": digest(out.encode()),
            "retained_stderr_sha256": digest(err.encode()),
            "output_limit": "Complete separate streams, credential redaction only; invalid UTF-8 replaced during text decoding.",
        },
    )
    return result.returncode, out if separate_stdout else re.sub(
        r"\x1b\[[0-?]*[ -/]*[@-~]", "", err + "\n" + out
    )


def entry(path: Path) -> dict:
    mode = path.lstat().st_mode
    assert stat.S_ISLNK(mode) or stat.S_ISREG(mode)
    data = os.fsencode(os.readlink(path)) if stat.S_ISLNK(mode) else path.read_bytes()
    return {
        "mode": "120000"
        if stat.S_ISLNK(mode)
        else ("100755" if mode & 0o111 else "100644"),
        "permissions": stat.S_IMODE(mode),
        "blob": hashlib.sha1(
            b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False
        ).hexdigest(),
        "sha256": digest(data),
    }


def state(root: Path) -> dict:
    return {
        name: entry(root / name)
        for name in git(root, "ls-files", "-z").decode().rstrip("\0").split("\0")
    }


def index_bytes(root: Path) -> bytes:
    return Path(
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    ).read_bytes()


def initial_checkout(root: Path) -> dict:
    files = state(root)
    expected = {}
    for line in git(root, "ls-tree", "-rz", "HEAD").split(b"\0"):
        if line:
            metadata, path = line.split(b"\t", 1)
            mode, kind, blob = metadata.decode().split()
            assert kind == "blob"
            expected[path.decode()] = [mode, blob]
    assert {
        name: [value["mode"], value["blob"]] for name, value in files.items()
    } == expected
    return {
        "head": git(root, "rev-parse", "HEAD").decode().strip(),
        "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
        "manifest": files,
        "staged_entries_sha256": digest(git(root, "ls-files", "--stage", "-z")),
        "raw_index_sha256": digest(index_bytes(root)),
    }


def phase(label: str, expected: dict, executed_tree: str) -> None:
    actual, raw, staged = (
        state(ROOT),
        index_bytes(ROOT),
        git(ROOT, "ls-files", "--stage", "-z"),
    )
    (OUT / (label + "-index.bin")).write_bytes(raw)
    (OUT / (label + "-index.z")).write_bytes(staged)
    write(
        label + "-source.json",
        {
            "source": SOURCE,
            "source_tree": SOURCE_TREE,
            "executed_tree": executed_tree,
            "manifest": actual,
            "locks": {name: actual[name] for name in LOCKS},
            "status": git(ROOT, "status", "--porcelain").decode(),
            "manifest_equal": actual == expected,
            "staged_entries_equal": digest(staged)
            == INITIAL["source"]["staged_entries_sha256"],
            "raw_index_equal": digest(raw) == INITIAL["source"]["raw_index_sha256"],
            "raw_index_contract": "retained diagnostic; semantic stage entries are enforced",
        },
    )
    assert actual == expected, "tracked bytes, Git blobs or modes differ"
    assert all(actual[name] == INITIAL["source"]["manifest"][name] for name in LOCKS)
    assert digest(staged) == INITIAL["source"]["staged_entries_sha256"]
    assert git(ROOT, "rev-parse", "HEAD").decode().strip() == SOURCE
    assert git(ROOT, "rev-parse", "HEAD^{tree}").decode().strip() == SOURCE_TREE
    assert not git(ROOT, "diff", "--cached", "--name-only")
    assert not git(ROOT, "ls-files", "--others", "--exclude-standard")
    (OUT / (label + "-helper-index.bin")).write_bytes(index_bytes(HELPER))
    (OUT / (label + "-helper-index.z")).write_bytes(
        git(HELPER, "ls-files", "--stage", "-z")
    )
    helper = initial_checkout(HELPER)
    write(label + "-helper.json", helper)
    assert {
        key: value for key, value in helper.items() if key != "raw_index_sha256"
    } == {
        key: value
        for key, value in INITIAL["helper"].items()
        if key != "raw_index_sha256"
    }
    for name, data in FIXTURE.items():
        assert (ROOT / name).read_bytes() == data, "shared fixture changed"


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
        "export SUTURA_DEV_REQUIRE_TIER=1\n  export SUTURA_CLICKHOUSE_CLI_OWN_SITE_PROOF_BOUND=1\n  exec "
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


def own_result(status: int, text: str, control: dict, negative: bool) -> dict:
    assert re.findall(
        r"^\s*Starting (\d+) tests? across (\d+) binar(?:y|ies)", text, re.MULTILINE
    ) == [("1", "1")]
    outcomes = re.findall(
        r"^\s*(PASS|FAIL|TIMEOUT|LEAK|ABORT|XFAIL|XPASS)\s+\[[^\]\n]+\]\s+(?:\(1/1\)\s+)?([^\n]+)$",
        text,
        re.MULTILINE,
    )
    assert outcomes == [
        ("FAIL" if negative else "PASS", control["binary"] + " " + control["cell"])
    ], "not one exact selected terminal outcome"
    summaries = re.findall(
        r"^\s*Summary\s+\[[^\]\n]+\]\s+1 test run: ([^\n]+)$", text, re.MULTILINE
    )
    counts = "0 passed, 1 failed" if negative else "1 passed"
    assert len(summaries) == 1 and re.fullmatch(
        re.escape(counts) + r"(?:, \d+ skipped)?", summaries[0]
    ), "not exactly one executed test"
    assert status == (100 if negative else 0), (
        "compile/setup/timeout/other exit earns no credit"
    )
    assert not any(
        word in text
        for word in ("NOT RUN", "SKIPPED", "requires a provisioned service tier")
    )
    if negative:
        assert len(re.findall(r"^thread [^\n]+ panicked at ", text, re.MULTILINE)) == 1
        panic = re.search(
            rf"^thread '{re.escape(control['cell'])}'(?: \([^\n]*\))? panicked at {re.escape(control['fixture'])}:{control['own_line']}:\d+:\n([^\n]+(?:\n(?!\s*(?:note:|test |failures:|FAIL|Summary|error:))[^\n]+)*)",
            text,
            re.MULTILINE,
        )
        assert panic and all(
            fragment in panic[1] for fragment in control["message_fragments"]
        ), "wrong own panic site or message"
        return {"accepted": True, "own_panic": panic[0]}
    assert not re.search(r"^thread [^\n]+ panicked at ", text, re.MULTILINE)
    return {"accepted": True}


def run_row(control: dict, suffix: str, expected: dict) -> dict:
    label = control["name"] + "-" + suffix
    negative = suffix == "mutant"
    tree = control["mutant_tree"] if negative else SOURCE_TREE
    phase(label + "-before", expected, tree)
    command = [
        "timeout",
        "--kill-after=30s",
        "35m",
        "just",
        "--justfile",
        str(OUT / "proof.just"),
        "--working-directory",
        str(ROOT),
        "proof-clickhouse-reader",
        control["package"],
        control["target"],
        control["cell"],
    ]
    if control["name"] == "own-service-user":
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
    status, text = execute(label, command, ROOT)
    result = {
        "name": label,
        "cell": control["cell"],
        "expected_tree": tree,
        "exit": status,
        "accepted": False,
    }
    write(label + "-result.json", result)
    try:
        result.update(own_result(status, text, control, negative))
    except AssertionError as error:
        result["reason"] = str(error)
    phase(label + "-after", expected, tree)
    write(label + "-result.json", result)
    return result


def proof() -> None:
    global INITIAL, FIXTURE
    assert os.environ.get("SUTURA_DEV_REQUIRE_TIER") == "1"
    INITIAL = json.loads((OUT / "checkout-initial.json").read_text())
    baseline = INITIAL["source"]["manifest"]
    FIXTURE = {
        name: (ROOT / name).read_bytes()
        for name in (".sutura-dev/endpoints.json", ".sutura-dev/clickhouse/config.yaml")
    }
    write("fixture.json", {name: digest(data) for name, data in FIXTURE.items()})
    document = json.loads(FIXTURE[".sutura-dev/endpoints.json"])
    endpoint = document["services"]["clickhouse"]
    assert endpoint["host"] == "127.0.0.1" and 0 < endpoint["port"] < 65536
    driver = Path(os.environ["SUTURA_CLICKHOUSE_ADBC_DRIVER"])
    assert driver.is_file() and str(driver).startswith("/nix/store/")
    environment = {
        "endpoint": endpoint,
        "driver": str(driver),
        "driver_sha256": digest(driver.read_bytes()),
        "inherited_cargo_home": os.environ["CARGO_HOME"],
        "target": os.environ["CARGO_TARGET_DIR"],
        "warm_stamp": Path(
            os.environ["CARGO_TARGET_DIR"], ".sutura-warm-start"
        ).read_text(),
        "inherited_config_sha256": digest(
            Path(os.environ["CARGO_HOME"], "config.toml").read_bytes()
        ),
        "credential_values_retained": False,
    }
    for name in ("cargo", "cargo-nextest", "rustc", "clang", "just"):
        executable = shutil.which(name)
        assert executable and executable.startswith("/nix/store/"), name
        environment[name] = executable
    # ponytail: retain the accepted warm target and empty Cargo-home route, without replaying the native tier.
    with tempfile.TemporaryDirectory(
        prefix="clickhouse-own-site-cargo-", dir=os.environ["RUNNER_TEMP"]
    ) as cargo_home:
        os.environ["CARGO_HOME"] = cargo_home
        os.environ.pop("CARGO_NET_OFFLINE", None)
        os.environ.pop("SUTURA_SATELLITE_CARGO_VENDOR_DIR", None)
        for key in list(os.environ):
            if key.startswith("NEXTEST_"):
                del os.environ[key]
        environment["native_cargo_home"] = cargo_home
        write("environment.json", environment)
        (OUT / "proof.just").write_text(TASK, encoding="utf-8")
        results = []
        try:
            for control in CONTROLS:
                phase(control["name"] + "-pristine", baseline, SOURCE_TREE)
                path = ROOT / control["path"]
                original, mode = path.read_bytes(), path.stat().st_mode
                assert digest(original) == control["original_sha256"]
                fixture = (ROOT / control["fixture"]).read_bytes()
                assert digest(fixture) == control["fixture_sha256"]
                head = run_row(control, "head", baseline)
                results.append(head)
                write("rows.json", results)
                assert head["accepted"], "head failed; stop the batch"
                try:
                    patch = control["patch_text"].encode()
                    assert digest(patch) == control["patch_sha256"]
                    patch_path = OUT / (control["name"] + ".patch")
                    patch_path.write_bytes(patch)
                    for suffix, flags in (("check", ["--check"]), ("apply", [])):
                        status, _ = execute(
                            control["name"] + "-" + suffix,
                            [
                                "git",
                                "apply",
                                "--whitespace=error",
                                *flags,
                                str(patch_path),
                            ],
                            ROOT,
                        )
                        assert status == 0, "mutation setup is not a witness"
                    assert digest(path.read_bytes()) == control["mutant_sha256"]
                    span = (
                        (ROOT / control["fixture"])
                        .read_bytes()
                        .splitlines(keepends=True)
                    )
                    start, end = control["cell_lines"]
                    assert (
                        digest(b"".join(span[start - 1 : end]))
                        == control["cell_sha256"]
                    )
                    expected = baseline | {control["path"]: entry(path)}
                    assert git(ROOT, "diff", "--name-only").decode().splitlines() == [
                        control["path"]
                    ]
                    results.append(run_row(control, "mutant", expected))
                    write("rows.json", results)
                finally:
                    path.write_bytes(original)
                    path.chmod(stat.S_IMODE(mode))
                    phase(control["name"] + "-restoration", baseline, SOURCE_TREE)
                restored = run_row(control, "restored", baseline)
                results.append(restored)
                write("rows.json", results)
                assert restored["accepted"], "restored head failed; stop the batch"
        finally:
            phase("final", baseline, SOURCE_TREE)
    accepted = len(results) == 6 and all(row["accepted"] for row in results)
    write(
        "verdict.json",
        {
            "status": "SIX_OWN_SITE_ROWS_ACCEPTED"
            if accepted
            else "INCOMPLETE_OR_NONVERDICT",
            "accepted_six_rows": accepted,
            "rows": results,
            "historical_base_credit": False,
            "native_tier_replayed": False,
            "limit": "Two controlled-source cells only. No historical-base expansion, native-seven replay, service/fleet/identity/general-admission/full-gates/queue/landing or issue-completion credit.",
        },
    )
    assert accepted, "six terminal qualified rows required"


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    assert os.environ["GITHUB_RUN_ATTEMPT"] == "1", "new request required; no rerun"
    assert not sys.flags.optimize and not os.environ.get("SKIP")
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    assert (os.environ["SOURCE_SHA"], os.environ["SOURCE_TREE"]) == (
        SOURCE,
        SOURCE_TREE,
    )
    assert (
        os.environ["REQUEST_ID"]
        == REQUEST_ID
        == str(uuid.UUID(os.environ["REQUEST_ID"]))
    )
    assert os.environ["GITHUB_REF_NAME"] == REF
    ROOT, HELPER = Path.cwd().resolve(), Path(__file__).resolve().parent.parent
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
    OUT = ROOT / "target/clickhouse-cli-own-site-proof"
    if os.environ.get("SUTURA_CLICKHOUSE_CLI_OWN_SITE_PROOF_BOUND") == "1":
        assert (OUT / "provenance.json").is_file()
        proof()
    else:
        OUT.mkdir(parents=True, exist_ok=False)
        request = {
            "ref": REF,
            "inputs": {
                "source_sha": SOURCE,
                "source_tree": SOURCE_TREE,
                "helper_tree": os.environ["HELPER_TREE"],
                "request_id": REQUEST_ID,
            },
        }
        write("request.json", request)
        write(
            "checkout-initial.json",
            {"source": initial_checkout(ROOT), "helper": initial_checkout(HELPER)},
        )
        (OUT / "source-index-initial.bin").write_bytes(index_bytes(ROOT))
        (OUT / "source-index-initial.z").write_bytes(
            git(ROOT, "ls-files", "--stage", "-z")
        )
        (OUT / "helper-index-initial.bin").write_bytes(index_bytes(HELPER))
        (OUT / "helper-index-initial.z").write_bytes(
            git(HELPER, "ls-files", "--stage", "-z")
        )
        write("verdict.json", {"status": "INCOMPLETE", "accepted_six_rows": False})
        write(
            "provenance.json",
            {
                "schema": "clickhouse-cli-own-site-proof/v1",
                "request_sha256": digest(
                    json.dumps(request, indent=2).encode() + b"\n"
                ),
                "source": SOURCE,
                "source_tree": SOURCE_TREE,
                "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
                "helper_tree": os.environ["HELPER_TREE"],
                "payload_sha256": digest(Path(__file__).read_bytes()),
                "workflow_sha256": digest(
                    (HELPER / ".github/workflows/format.yml").read_bytes()
                ),
                "controls_sha256": digest(
                    json.dumps(CONTROLS, sort_keys=True).encode()
                ),
                "task_sha256": digest(TASK.encode()),
                "run_id": os.environ["GITHUB_RUN_ID"],
                "run_attempt": 1,
            },
        )
        bootstrap()

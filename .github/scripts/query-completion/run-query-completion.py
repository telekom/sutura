"""Hosted-only named positive, assertion-kill and restoration receipts."""

import hashlib
import json
import os
import re
import secrets
import subprocess
import sys
import tempfile
from pathlib import Path

PAYLOAD = Path(__file__).resolve().parent
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def record(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def snapshot(root):
    index = Path(
        git(root, "rev-parse", "--path-format=absolute", "--git-path", "index")
        .decode()
        .strip()
    )
    raw_before = sha(index.read_bytes())
    stages = sha(git(root, "ls-files", "--stage", "-z"))
    files = {}
    for raw in git(root, "ls-files", "-z").split(b"\0"):
        if raw:
            path = root / os.fsdecode(raw)
            link = path.is_symlink()
            files[os.fsdecode(raw)] = [
                sha(os.fsencode(os.readlink(path)) if link else path.read_bytes()),
                link,
                path.lstat().st_mode,
            ]
    result = {
        "head": git(root, "rev-parse", "HEAD").decode().strip(),
        "files": files,
        "stages_sha256": stages,
        "status": git(root, "status", "--porcelain").decode(),
        "diff_sha256": sha(git(root, "diff", "--binary")),
    }
    assert stages == sha(git(root, "ls-files", "--stage", "-z")), (
        "staged entries changed during capture"
    )
    result["index_raw_diagnostic"] = {
        "before_git_reads": raw_before,
        "after_git_reads": sha(index.read_bytes()),
        "capture_order": "raw, stages, tracked contents/kinds/full modes, HEAD/status/diff, stages, raw",
    }
    return result


def same_source(left, right):
    return {k: v for k, v in left.items() if k != "index_raw_diagnostic"} == {
        k: v for k, v in right.items() if k != "index_raw_diagnostic"
    }


def restore(root, out, label, paths, expected):
    record(out / f"{label}-restore-before.json", snapshot(root))
    subprocess.run(
        ["git", "restore", "--source=HEAD", "--worktree", "--", *paths],
        cwd=root,
        check=True,
    )
    after = snapshot(root)
    record(out / f"{label}-restore-after.json", after)
    assert same_source(after, expected), (
        "tracked contents/kinds/full modes or staged entries differ after restore"
    )


def invoke(root, out, label, argv):
    before = snapshot(root)
    record(out / f"{label}-before.json", before)
    record(out / f"{label}-argv.json", argv)
    env = dict(os.environ, INSTA_UPDATE="no")
    record(
        out / f"{label}-environment.json",
        {
            "INSTA_UPDATE": env["INSTA_UPDATE"],
            "SUTURA_DEV_REQUIRE_TIER": env.get("SUTURA_DEV_REQUIRE_TIER"),
            "SUTURA_ORACLE_TIER_CA": env.get("SUTURA_ORACLE_TIER_CA"),
        },
    )
    result = subprocess.run(
        ["timeout", "--kill-after=30s", "100m", *argv],
        cwd=root,
        env=env,
        capture_output=True,
        check=False,
    )
    stdout = result.stdout.decode(errors="replace")
    text = ANSI.sub("", stdout + result.stderr.decode(errors="replace"))
    # Credentialed logs stay outside the exported output, following the source venue's digest.
    if GROUP == "bigquery-cli" and label == "diagnostic":
        diagnostic = json.loads((PAYLOAD / "bigquery-cli.json").read_text())[
            "diagnostic"
        ]
        markers = [
            line
            for line in text.splitlines()
            if re.fullmatch(diagnostic["marker_pattern"], line.strip())
        ]
        statuses = [
            status
            for status in ["PASS", "FAIL", "TIMEOUT", "SIGABRT", "SIGSEGV"]
            if outcome(text, status, diagnostic["binary"], diagnostic["test"])
        ]
        verdicts = [
            "diagnostic test=" + diagnostic["test"] + " status=" + status
            for status in statuses
        ]
        (out / f"{label}.log").write_text(
            "\n".join(line.strip() for line in markers + verdicts) + "\n"
        )
    elif GROUP.startswith("bigquery"):
        raw = Path(os.environ["RUNNER_TEMP"]) / f"completion-{label}.raw"
        raw.write_text(text)
        try:
            subprocess.run(
                [
                    sys.executable,
                    "-I",
                    str(SOURCE / "nix/bigquery-leg-digest.py"),
                    str(raw),
                    str(out / f"{label}.log"),
                ],
                check=True,
            )
        finally:
            raw.unlink()
    else:
        (out / f"{label}.log").write_text(text)
    (out / f"{label}.exit").write_text(str(result.returncode) + "\n")
    after = snapshot(root)
    record(out / f"{label}-after.json", after)
    assert same_source(after, before), (
        f"{label}: command changed tracked source or staged entries"
    )
    return result.returncode, stdout if label.endswith("census") else text


def own_text(root, cell):
    lines = (root / cell["path"]).read_text().splitlines(keepends=True)
    first, last = cell["first"], cell["last"]
    text = "".join(lines[first - 1 : last])
    assert re.search(r"\bfn\s+" + re.escape(cell["name"]) + r"\s*\(", lines[first - 1])
    assert lines[last - 1].strip() == "}"
    assert sha(text.encode()) == cell["function_sha256"]
    return text


def outcome(text, status, binary, name):
    return bool(
        re.search(
            r"^\s*"
            + status
            + r"\s+\[[^\n]*\]\s+(?:\([^\n]*\)\s+)?"
            + re.escape(binary)
            + r"\s+"
            + re.escape(name)
            + r"\s*$",
            text,
            re.MULTILINE,
        )
    )


def own_kill(text, root, cell, body, binary, name, marker, panic_lines=None):
    if not outcome(text, "FAIL", binary, name) or "Summary" not in text:
        return False
    current = (root / cell["path"]).read_text()
    assert current.count(body) == 1, (
        "mutation rewrote or duplicated the named test function"
    )
    start = current[: current.index(body)].count("\n") + 1
    end = start + body.count("\n")
    first, last = (start, end - 1) if panic_lines is None else panic_lines
    assert start <= first <= last < end, "panic site is outside the named function"
    # Associate the panic with the named Rust test thread, not any other failing cell.
    for hit in re.finditer(
        r"thread '([^']+)'(?: \([^\n]*\))? panicked at ([^\n]+):(\d+):(\d+):\n([^\n]*)",
        text,
    ):
        thread, path, line, _, message = hit.groups()
        if (
            thread == name
            and path == cell["path"]
            and first <= int(line) <= last
            and (not marker or marker in message)
        ):
            return True
    return False


def selfcheck():
    one = {
        "files": {"test": ["hash", False, 0o100644]},
        "stages_sha256": "stage",
        "index_raw_diagnostic": "before",
    }
    assert same_source(one, one | {"index_raw_diagnostic": "after"})
    assert not same_source(one, one | {"stages_sha256": "changed"})
    assert not same_source(one, one | {"files": {}})
    line = "FAIL [0.1s] (1/1) example tests::cell\nSummary [0.1s] 1 test run\n"
    assert outcome(line, "FAIL", "example", "tests::cell")
    assert not outcome(line, "FAIL", "other", "tests::cell")
    assert not outcome("error: could not compile", "FAIL", "example", "tests::cell")
    body = "fn cell() {\n    assert!(false);\n}\n"
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "tests.rs").write_text(body + "fn helper() { panic!(); }\n")
        panic = "thread 'tests::cell' panicked at tests.rs:2:5:\nmarker\n"
        cell = {"path": "tests.rs"}
        assert own_kill(
            line + panic, root, cell, body, "example", "tests::cell", "marker"
        )
        assert not own_kill(
            line + panic.replace(":2:5:", ":4:5:"),
            root,
            cell,
            body,
            "example",
            "tests::cell",
            "marker",
        )
        assert not own_kill(
            line + panic.replace("'tests::cell'", "'tests::other'"),
            root,
            cell,
            body,
            "example",
            "tests::cell",
            "marker",
        )
        assert not own_kill(
            line + panic, root, cell, body, "example", "tests::cell", "absent"
        )
        assert own_kill(
            line + panic, root, cell, body, "example", "tests::cell", None, [2, 2]
        )
        assert not own_kill(
            line + panic, root, cell, body, "example", "tests::cell", None, [3, 3]
        )
        assert not own_kill(
            line + panic.replace(":2:5:", ":3:5:"),
            root,
            cell,
            body,
            "example",
            "tests::cell",
            None,
            [2, 2],
        )


def task(root, name, *args):
    return [
        JUST,
        "--justfile",
        str(PAYLOAD / "completion-proof.just"),
        "--working-directory",
        str(root),
        name,
        *args,
    ]


def binary_for(census, cell):
    matches = []
    for suite in census["rust-suites"].values():
        binary = suite["binary-id"]
        for name in suite["testcases"]:
            if (
                name.split("::")[-1] == cell["name"]
                and suite["package-name"] == cell["package"]
            ):
                matches.append((binary, name))
    assert len(matches) == 1, (cell["name"], matches)
    binary, name = matches[0]
    assert re.fullmatch(r"[A-Za-z0-9_:/.-]+", binary)
    return binary, name


def run_base(config, out):
    base = config["base_catalog"]
    root = SOURCE / "target/completion-catalog-base"
    assert (
        not root.exists()
        and base["source"] == "e5ec141fae2ca4f63792cd83ce6e4658b10819b3"
    )
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(root), base["source"]], check=True
    )
    before = snapshot(root)
    record(out / "base-overlay-before.json", before)
    path = base["cell"]["path"]
    assert path == "crates/sutura-exec-datafusion/src/combine/across/receipts/tests.rs"
    data = (SOURCE / path).read_bytes()
    assert sha(data) == base["overlay_sha256"]
    try:
        (root / path).write_bytes(data)
        overlaid = snapshot(root)
        record(out / "base-overlay-after.json", overlaid)
        assert overlaid["stages_sha256"] == before["stages_sha256"]
        assert {
            p for p in before["files"] if before["files"][p] != overlaid["files"][p]
        } == {path}
        code, raw = invoke(
            root, out, "base-census", task(root, "proof-census", "offline")
        )
        assert code == 0, "base overlay did not compile; no red-before-green credit"
        binary, name = binary_for(json.loads(raw), base["cell"])
        body = own_text(root, base["cell"])
        code, log = invoke(
            root, out, "base-catalog", [JUST, "test-only", f"binary_id(={binary})"]
        )
        killed = code != 0 and own_kill(
            log, root, base["cell"], body, binary, name, base["marker"]
        )
        record(
            out / "base-catalog-result.json",
            {
                "source": base["source"],
                "test_only_overlay": path,
                "whole_binary": binary,
                "own_assertion_red_on_published_base": killed,
            },
        )
        assert killed, "published catalog base did not fail by its named own assertion"
    finally:
        restore(root, out, "base", [path], before)
        record(out / "base-final.json", snapshot(root))
        assert same_source(snapshot(root), before)


def run_postgres(config, out):
    root = SOURCE / "target/completion-native-postgres"
    assert not root.exists() and Path("/dev/kvm").exists()
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(root), config["source"]], check=True
    )
    baseline = snapshot(root)
    patch = PAYLOAD / config["patch"]
    assert sha(patch.read_bytes()) == config["patch_sha256"]
    paths = [
        line.split("\t", 2)[2]
        for line in git(root, "apply", "--numstat", str(patch)).decode().splitlines()
    ]
    assert paths == ["crates/sutura-cli/src/serve/worker/partition.rs"]

    def run(label):
        code, build_log = invoke(root, out, label, task(root, "proof-postgres"))
        log_code, _venue_log = invoke(
            root, out, label + "-venue-log", task(root, "proof-postgres-log")
        )
        assert log_code == 0, "could not retain this exact VM derivation's log"
        # The derivation log may predate this rebuild; verdicts use the direct recipe output.
        return code, build_log

    code, log = run("postgres-pristine")
    assert code == 0 and config["positive_subtest"] in log
    try:
        record(out / "postgres-apply-before.json", snapshot(root))
        subprocess.run(["git", "apply", "--check", str(patch)], cwd=root, check=True)
        subprocess.run(["git", "apply", str(patch)], cwd=root, check=True)
        changed = snapshot(root)
        record(out / "postgres-apply-after.json", changed)
        assert changed["stages_sha256"] == baseline["stages_sha256"]
        assert {
            p for p in baseline["files"] if baseline["files"][p] != changed["files"][p]
        } == set(paths)
        code, log = run("postgres-mutant")
        killed = (
            code != 0
            and config["mutant_failed_subtest"] in log
            and re.search(r"\bin answered\b", log) is not None
            and re.search(
                r"AssertionError:.*['\"]outcome['\"]:\s*['\"]refused['\"]", log
            )
            is not None
        )
    finally:
        restore(root, out, "postgres", paths, baseline)
    code, log = run("postgres-restored")
    restored = code == 0 and config["positive_subtest"] in log
    record(
        out / "postgres-result.json",
        {
            "native_vm_pristine_pass": True,
            "answer_refusal_mutation": bool(killed),
            "restored_pass": restored,
            "exact_source_stage_restored": same_source(snapshot(root), baseline),
            "limit": "shared Python answered() refusal, not Rust own-function panic; mutant earns no later receipt-comparison credit",
        },
    )
    assert killed and restored


def run_cli_diagnostic(config, out, root, baseline):
    diagnostic = config["diagnostic"]
    assert GROUP == "bigquery-cli" and not config["cases"]
    overlays = diagnostic["overlays"]
    paths = [item["path"] for item in overlays]
    assert paths == [
        "crates/sutura-exec-datafusion/src/combine/across/partition.rs",
        "crates/sutura-exec-datafusion/src/combine/across.rs",
        "vendor/datafusion-distributed/src/protocol/grpc/errors/mod.rs",
        "crates/sutura-exec-datafusion/src/combine/across/receipts.rs",
        "vendor/datafusion-distributed/src/protocol/grpc/worker_client.rs",
        "vendor/datafusion-distributed/src/protocol/grpc/worker_service.rs",
    ]
    for item in overlays:
        overlay = PAYLOAD / item["overlay"]
        assert sha(overlay.read_bytes()) == item["overlay_sha256"]
        assert sha((root / item["path"]).read_bytes()) == item["before_sha256"]
        assert (
            git(root, "apply", "--numstat", str(overlay))
            .decode()
            .strip()
            .split("\t")[2]
            == item["path"]
        )
    try:
        for item in overlays:
            overlay = PAYLOAD / item["overlay"]
            subprocess.run(
                ["git", "apply", "--check", str(overlay)], cwd=root, check=True
            )
            subprocess.run(["git", "apply", str(overlay)], cwd=root, check=True)
            assert sha((root / item["path"]).read_bytes()) == item["after_sha256"]
        changed = snapshot(root)
        assert changed["stages_sha256"] == baseline["stages_sha256"]
        assert {
            p for p in baseline["files"] if baseline["files"][p] != changed["files"][p]
        } == set(paths)
        record(out / "diagnostic-overlay.json", diagnostic)
        code, raw = invoke(root, out, "census", task(root, "proof-census", GROUP))
        assert code == 0, "diagnostic census did not compile"
        census = json.loads(raw)
        record(out / "census.json", census)
        suite = census["rust-suites"][diagnostic["binary"]]
        assert suite["binary-id"] == diagnostic["binary"]
        ignored = [n for n, row in suite["testcases"].items() if row["ignored"]]
        assert ignored == [diagnostic["test"]]
        code, text = invoke(
            root,
            out,
            "diagnostic",
            task(root, "proof-bigquery-cli", diagnostic["binary"]),
        )
        record(
            out / "diagnostic-result.json",
            {
                "source": config["source"],
                "tree": config["tree"],
                "overlay_sha256": [item["overlay_sha256"] for item in overlays],
                "exit": code,
                "named_test_pass": outcome(
                    text, "PASS", diagnostic["binary"], diagnostic["test"]
                ),
                "diagnostic_only": True,
                "runtime_credit": False,
                "mutations_run": 0,
                "markers": (out / "diagnostic.log").read_text().splitlines(),
                "limit": "fixed late consumer stages and at most eight source-chain nodes; direct/Arc known-type checks; unknown_type and chain_limit remain unclassified; Collection branches not exhaustively explored; absent marker is not_observed",
            },
        )
    finally:
        restore(root, out, "diagnostic-overlay", paths, baseline)
    assert code == 0, (
        "diagnostic fixture failed; retained categories confer no proof credit"
    )


def run_cases(config, out):
    root = SOURCE / "target" / f"completion-{GROUP}"
    assert not root.exists()
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(root), config["source"]], check=True
    )
    baseline = snapshot(root)
    record(out / "worktree-initial.json", baseline)
    try:
        if config.get("diagnostic"):
            run_cli_diagnostic(config, out, root, baseline)
            return
        if GROUP == "vendor":
            for name in ["Cargo.toml", "Cargo.lock"]:
                data = (PAYLOAD / "vendor" / name).read_bytes()
                assert sha(data) == config["vendor_overlays"][name]
                (root / name).write_bytes(data)
        assert snapshot(root)["stages_sha256"] == baseline["stages_sha256"]
        expected = snapshot(root)
        record(out / "approved-overlays.json", expected)
        if GROUP == "native-oracle":
            os.environ.update(
                SUTURA_DEV_USER="sutura",
                SUTURA_DEV_PASSWORD=secrets.token_hex(12),
                SUTURA_DEV_REQUIRE_TIER="1",
                SUTURA_ORACLE_TIER_CA=str(root / "target/oracle-tier-tls/ca.pem"),
            )
            code, _ = invoke(
                root, out, "oracle-setup", task(root, "proof-oracle-setup", JUST)
            )
            assert code == 0, "proof-worktree Oracle setup failed"
            assert Path(os.environ["SUTURA_ORACLE_TIER_CA"]).is_file()
        code, raw = invoke(
            root,
            out,
            "census",
            task(root, "proof-census", "offline" if GROUP == "claims" else GROUP),
        )
        assert code == 0, "containing-binary census did not compile"
        census = json.loads(raw)
        record(out / "census.json", census)
        if GROUP == "native-oracle":
            assert {row["binary-name"] for row in census["rust-suites"].values()} == {
                "acceptance",
                "flashback",
            }
            code, raw = invoke(
                root, out, "served-census", task(root, "proof-census", "oracle-served")
            )
            assert code == 0, "served containing-binary census did not compile"
            served_census = json.loads(raw)
            record(out / "served-census.json", served_census)
            assert {
                row["binary-name"] for row in served_census["rust-suites"].values()
            } == {"served"}
            assert set(census["rust-suites"]).isdisjoint(served_census["rust-suites"])
            census["rust-suites"].update(served_census["rust-suites"])
        for case in config["cases"]:
            cell = case["cell"]
            binary, name = binary_for(census, cell)
            if GROUP == "bigquery-cli":
                suite = next(
                    row
                    for row in census["rust-suites"].values()
                    if row["binary-id"] == binary
                )
                ignored = [n for n, row in suite["testcases"].items() if row["ignored"]]
                assert ignored == [name], (
                    "the CLI ignored-only binary no longer selects exactly this live cell"
                )
            body = own_text(root, cell)
            label = case["id"]
            suite = next(
                row
                for row in census["rust-suites"].values()
                if row["binary-id"] == binary
            )
            ignored = sorted(
                n for n, row in suite["testcases"].items() if row["ignored"]
            )
            ordinary = sorted(set(suite["testcases"]) - set(ignored))
            served = GROUP == "native-oracle" and cell["package"] == "sutura-cli"
            live_argv = None
            if GROUP == "native-oracle" or label == "source-identity":
                assert case.get("panic_lines") is not None, (
                    "first assertion site is unbound"
                )
            if served:
                assert suite["binary-name"] == "served" and name in ignored and ordinary
                assert (
                    name
                    == "oracle_split::an_oracle_lookup_split_across_readers_equals_the_whole_and_refuses_uncertified_readers"
                )
                code, raw = invoke(
                    root,
                    out,
                    label + "-default-census",
                    task(root, "proof-oracle-served-default-census", binary),
                )
                assert code == 0, "served default selection census failed"
                default_census = json.loads(raw)
                record(out / f"{label}-default-census.json", default_census)
                default_suites = list(default_census["rust-suites"].values())
                assert (
                    len(default_suites) == 1
                    and default_suites[0]["binary-id"] == binary
                )
                assert default_suites[0]["testcases"] == suite["testcases"]
                code, raw = invoke(
                    root,
                    out,
                    label + "-live-census",
                    task(root, "proof-oracle-served-live-census", binary, name),
                )
                assert code == 0, "served live selection census failed"
                live_census = json.loads(raw)
                record(out / f"{label}-live-census.json", live_census)
                live_rows = [
                    (row["binary-id"], n)
                    for row in live_census["rust-suites"].values()
                    for n, test in row["testcases"].items()
                    if test["filter-match"]["status"] == "matches"
                ]
                assert live_rows == [(binary, name)], (
                    "served live census did not select exactly its declared row"
                )
                scope = {
                    "kind": "complete-nonignored-plus-one-declared-ignored-Oracle-cell",
                    "default_selected": ordinary,
                    "live_selected": [name],
                    "excluded_ignored": sorted(set(ignored) - {name}),
                    "default_census": f"{label}-default-census.json",
                    "live_census": f"{label}-live-census.json",
                }
                argv = task(root, "proof-oracle-served-default", binary)
                live_argv = task(root, "proof-oracle-served-live", binary, name)
            elif GROUP == "native-oracle":
                assert cell["package"] == "sutura-exec-oracle" and suite[
                    "binary-name"
                ] in {"acceptance", "flashback"}
                assert suite["testcases"]
                scope = {
                    "kind": "complete-containing-binary-including-ignored",
                    "selected": sorted(suite["testcases"]),
                    "excluded": [],
                    "census": "census.json",
                }
                argv = task(root, "proof-oracle-adapter", binary)
            else:
                only_ignored = GROUP.startswith("bigquery")
                scope = {
                    "kind": "ignored-only"
                    if only_ignored
                    else "complete-nonignored-containing-binary",
                    "selected": ignored if only_ignored else ordinary,
                    "excluded": ordinary if only_ignored else ignored,
                    "census": "census.json",
                }
                argv = (
                    [JUST, "test-only", f"binary_id(={binary})"]
                    if GROUP in ["offline", "claims"]
                    else task(root, "proof-" + GROUP, binary)
                )
            record(out / f"{label}-scope.json", scope)

            def run(
                phase,
                *,
                label=label,
                served=served,
                argv=argv,
                live_argv=live_argv,
                binary=binary,
                ordinary=ordinary,
                name=name,
                scope=scope,
            ):
                code, log = invoke(
                    root,
                    out,
                    label + "-" + phase + ("-default" if served else ""),
                    argv,
                )
                if served:
                    # A nonzero default exit must not suppress the separate live invocation.
                    live_code, live_log = invoke(
                        root, out, label + "-" + phase + "-live", live_argv
                    )
                    passed = (
                        code == 0
                        and all(outcome(log, "PASS", binary, n) for n in ordinary)
                        and live_code == 0
                        and outcome(live_log, "PASS", binary, name)
                    )
                    record(
                        out / f"{label}-{phase}-phases.json",
                        {
                            "default_exit": code,
                            "live_exit": live_code,
                            "all_selected_named_pass": passed,
                            "scope": f"{label}-scope.json",
                        },
                    )
                    return live_code, live_log, passed
                selected = scope["selected"] if GROUP == "native-oracle" else [name]
                return (
                    code,
                    log,
                    code == 0
                    and all(outcome(log, "PASS", binary, n) for n in selected),
                )

            code, log, pristine = run("pristine")
            assert pristine, label + ": pristine selected rows did not all pass"
            patch = PAYLOAD / case["patch"]
            assert sha(patch.read_bytes()) == case["patch_sha256"]
            paths = git(root, "apply", "--numstat", str(patch)).decode().splitlines()
            paths = [line.split("\t", 2)[2] for line in paths]
            assert paths and set(paths).isdisjoint({"Cargo.toml", "Cargo.lock"})
            try:
                record(out / f"{label}-apply-before.json", snapshot(root))
                subprocess.run(
                    ["git", "apply", "--check", str(patch)], cwd=root, check=True
                )
                subprocess.run(["git", "apply", str(patch)], cwd=root, check=True)
                mutated = snapshot(root)
                record(out / f"{label}-apply-after.json", mutated)
                assert mutated["stages_sha256"] == expected["stages_sha256"]
                assert {
                    p
                    for p in mutated["files"]
                    if mutated["files"][p] != expected["files"][p]
                } == set(paths)
                code, log, _ = run("mutant")
                killed = code != 0 and own_kill(
                    log,
                    root,
                    cell,
                    body,
                    binary,
                    name,
                    case.get("marker"),
                    case.get("panic_lines"),
                )
            finally:
                restore(root, out, label, paths, expected)
            code, log, restored = run("restored")
            record(
                out / f"{label}-result.json",
                {
                    "binary": binary,
                    "test": name,
                    "scope": scope,
                    "own_function_assertion_kill": killed,
                    "restored_pass": restored,
                    "exact_source_stage_restoration": True,
                },
            )
            assert killed and restored, label + ": no runtime kill/restored pass"
    finally:
        if GROUP == "vendor":
            restore(root, out, "vendor-overlay", ["Cargo.toml", "Cargo.lock"], baseline)
        record(out / "worktree-final.json", snapshot(root))
        assert same_source(snapshot(root), baseline), (
            "source/staged entries differ after restoring the approved overlays"
        )


def main():
    global GROUP, SOURCE, JUST
    assert os.environ["GITHUB_ACTIONS"] == "true", "CI only"
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    GROUP = sys.argv[1]
    assert GROUP in [
        "offline",
        "claims",
        "vendor",
        "bigquery-cli",
        "bigquery-adapter",
        "native-postgres",
        "native-oracle",
    ]
    SOURCE = Path.cwd()
    config = json.loads((PAYLOAD / f"{GROUP}.json").read_text())
    assert re.fullmatch(r"[a-f0-9]{40}", config["source"] or ""), (
        "unbound proposal cannot execute"
    )
    assert git(SOURCE, "rev-parse", "HEAD").decode().strip() == config["source"]
    assert git(SOURCE, "rev-parse", "HEAD^{tree}").decode().strip() == config["tree"]
    assert not git(SOURCE, "status", "--porcelain")
    out = Path(os.environ["COMPLETION_PROOF_OUT"])
    before = snapshot(SOURCE)
    if "--inside" not in sys.argv and GROUP != "native-postgres":
        out.mkdir(parents=True, exist_ok=False)
        record(out / "source-before.json", before)
        app_expr = (
            ".#apps.x86_64-linux."
            + (
                "bigquery-declared-principal"
                if GROUP.startswith("bigquery")
                else "causality"
            )
            + ".program"
        )
        app = Path(subprocess.check_output(["nix", "eval", "--raw", app_expr]).decode())
        context = json.loads(
            subprocess.check_output(
                ["nix", "eval", "--json", "--apply", "builtins.getContext", app_expr]
            )
        )
        assert context
        for path, value in context.items():
            if path.endswith(".drv"):
                subprocess.run(["nix-store", "--realise", path], check=True)
            else:
                assert value == {"path": True}
                subprocess.run(["nix-store", "--realise", path], check=True)
        original = app.read_text()
        needle = config["app_final_command"]
        assert original.count(needle) == 1
        replacement = (
            'exec python3 "$COMPLETION_PROOF_SCRIPT" "$COMPLETION_PROOF_GROUP" --inside'
        )
        oracle_environment = None
        if GROUP == "native-oracle":
            oracle_expr = ".#apps.x86_64-linux.oracle-acceptance.program"
            oracle_app = Path(
                subprocess.check_output(["nix", "eval", "--raw", oracle_expr]).decode()
            )
            oracle_context = json.loads(
                subprocess.check_output(
                    [
                        "nix",
                        "eval",
                        "--json",
                        "--apply",
                        "builtins.getContext",
                        oracle_expr,
                    ]
                )
            )
            assert oracle_context
            for path, value in oracle_context.items():
                assert path.endswith(".drv") or value == {"path": True}
                subprocess.run(["nix-store", "--realise", path], check=True)
            oracle_original = oracle_app.read_text()
            path_lines = re.findall(
                r'^export PATH="([^"\n]+):\$PATH"$', oracle_original, re.MULTILINE
            )
            assert len(path_lines) == 1, "Oracle app PATH shape changed"
            openssl_bins = [
                part
                for part in path_lines[0].split(":")
                if (Path(part) / "openssl").is_file()
            ]
            assert len(openssl_bins) == 1 and re.fullmatch(
                r"/nix/store/[A-Za-z0-9+._-]+/bin", openssl_bins[0]
            )
            replacement = 'export PATH="' + openssl_bins[0] + ':$PATH"\n' + replacement
            (out / "oracle-environment-original.sh").write_text(oracle_original)
            oracle_environment = {
                "app": str(oracle_app),
                "context": oracle_context,
                "original_sha256": sha(oracle_original.encode()),
                "openssl_bin": openssl_bins[0],
            }
        script = original.replace(needle, replacement)
        (out / "environment-original.sh").write_text(original)
        (out / "environment-proof.sh").write_text(script)
        record(
            out / "environment.json",
            {
                "app": str(app),
                "context": context,
                "original_sha256": sha(original.encode()),
                "proof_sha256": sha(script.encode()),
                "script_sha256": sha(Path(__file__).read_bytes()),
                "oracle_openssl": oracle_environment,
            },
        )
        env = dict(
            os.environ,
            COMPLETION_PROOF_SCRIPT=str(Path(__file__).resolve()),
            COMPLETION_PROOF_GROUP=GROUP,
            NEXTEST_PROFILE="nofailfast",
            CARGO_TERM_COLOR="never",
            INSTA_UPDATE="no",
        )
        result = subprocess.run(
            ["bash", str(out / "environment-proof.sh")], env=env, check=False
        )
        record(out / "source-after.json", snapshot(SOURCE))
        assert same_source(snapshot(SOURCE), before)
        assert result.returncode == 0
    else:
        if GROUP == "native-postgres":
            out.mkdir(parents=True, exist_ok=False)
            record(out / "source-before.json", before)
        selfcheck()
        expression = ".#apps.x86_64-linux.just.program"
        JUST = subprocess.check_output(["nix", "eval", "--raw", expression]).decode()
        context = json.loads(
            subprocess.check_output(
                ["nix", "eval", "--json", "--apply", "builtins.getContext", expression]
            )
        )
        assert context
        for path, value in context.items():
            assert path.endswith(".drv") or value == {"path": True}
            subprocess.run(["nix-store", "--realise", path], check=True)
        if GROUP == "native-postgres":
            run_postgres(config, out)
        else:
            run_cases(config, out)
        if GROUP == "offline":
            run_base(config, out)
        assert same_source(snapshot(SOURCE), before)
        record(out / "source-after.json", snapshot(SOURCE))
        if config.get("diagnostic"):
            record(
                out / "diagnostic-complete.json",
                {"diagnostic_only": True, "runtime_credit": False},
            )
            return
        record(
            out / "complete.json",
            {
                "source": config["source"],
                "tree": config["tree"],
                "group": GROUP,
                "cases": [case["id"] for case in config.get("cases", [])],
                "tracked_source_and_stages_unchanged": True,
                "raw_index_bytes": "diagnostic snapshots only; never rewritten or an equality verdict",
                "limit": "only these recorded scopes/native venue controls; no final validation, served-human, native-duplicate or detached-work-stop claim",
            },
        )


if __name__ == "__main__":
    main()

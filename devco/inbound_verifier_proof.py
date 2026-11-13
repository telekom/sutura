"""Hosted-only supplemental proof; ordinary causality remains a separate verdict."""

import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args])


def write(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def state():
    return {
        name: [
            os.lstat(name).st_mode,
            digest(
                os.fsencode(os.readlink(name))
                if Path(name).is_symlink()
                else Path(name).read_bytes()
            ),
        ]
        for name in TRACKED
    }


def capture_index(label):
    data = INDEX.read_bytes()
    (OUT / (label + ".bin")).write_bytes(data)
    INDEX_CAPTURES.append(
        {
            "order": len(INDEX_CAPTURES) + 1,
            "label": label,
            "bytes": len(data),
            "sha256": digest(data),
        }
    )
    return data


def exact(expected):
    assert state() == expected, "tracked bytes changed outside the declared overlay"
    assert INDEX.read_bytes() == SAVED_INDEX, "index changed"
    assert git("rev-parse", "HEAD").decode().strip() == PLAN["source_sha"]
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == PLAN["source_tree"]


def execute(label, command):
    path = OUT / (label + ".log")
    print(label, command, flush=True)
    with tempfile.TemporaryFile() as log:
        result = subprocess.run(
            command, stdout=log, stderr=subprocess.STDOUT, check=False
        )
        log.seek(0)
        text = log.read().decode(errors="replace")
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets += [
        value
        for key, value in os.environ.items()
        if key.startswith("SUTURA_") and key.endswith("PASSWORD")
    ]
    for secret in sorted(filter(None, secrets), key=len, reverse=True):
        text = text.replace(secret, "***")
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    path.write_text(text)
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    write(
        label + ".json",
        {
            "command": command,
            "exit": result.returncode,
            "log_sha256": digest(path.read_bytes()),
        },
    )
    print("exit", result.returncode, flush=True)
    return result.returncode, text


RESULT = re.compile(
    r"^\s*(PASS|FAIL)\s+\[[^\]]+\]\s+\(\s*\d+/\d+\)\s+(\S+)\s+(\S+)\s*$", re.MULTILINE
)


def results(text):
    return {(binary, name): status for status, binary, name in RESULT.findall(text)}


def whole(label, binary, cells, green):
    status, text = execute(label, ["just", "test-only", f"binary_id(={binary})"])
    rows = results(text)
    assert rows and {key[0] for key in rows} == {binary}, (
        "missing or competing test binaries"
    )
    assert set(rows) == BINARY_CELLS[binary], (
        "whole binary did not finish every discovered cell"
    )
    assert "Summary" in text, "no completed test run"
    assert all(
        rows.get((binary, cell)) == ("PASS" if green else "FAIL") for cell in cells
    )
    assert (status == 0) if green else (status != 0)
    return text


def region(path, cell):
    lines = Path(path).read_text().splitlines()
    starts = [
        (i, m.group(1))
        for i, line in enumerate(lines)
        if (m := re.match(rf"^([ \t]*)(?:async )?fn {re.escape(cell)}\(", line))
    ]
    assert len(starts) == 1, (path, cell, starts)
    start, indent = starts[0]
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == indent + "}")
    return start + 1, end + 1, "\n".join(lines[start : end + 1])


def own_sites(text, cell):
    path = PLAN["test_files"][cell]
    start, end, body = region(path, cell)
    sites = []
    for match in re.finditer(r" panicked at ([^\n]+):(\d+):(\d+):", text):
        found_path, line = match.group(1), int(match.group(2))
        if found_path == path and start <= line <= end:
            lines = text[match.start() :].splitlines()
            sites.append({"path": path, "line": line, "panic": "\n".join(lines[:8])})
    assert sites, ("no panic site inside the named test function", cell)
    return {"cell": cell, "function_sha256": digest(body.encode()), "sites": sites}


def mutations():
    global BINARY_CELLS
    os.environ["NEXTEST_PROFILE"] = "nofailfast"
    packages = sorted({mutant["package"] for mutant in PLAN["mutants"]})
    status, text = execute(
        "discover",
        ["just", "test-only", " | ".join(f"package(={p})" for p in packages)],
    )
    assert status == 0
    rows = results(text)
    BINARY_CELLS = {
        binary: {key for key in rows if key[0] == binary} for binary, _ in rows
    }
    cells = sorted({cell for mutant in PLAN["mutants"] for cell in mutant["tests"]})
    locations = {}
    for cell in cells:
        found = [
            (binary, full)
            for (binary, full), verdict in rows.items()
            if verdict == "PASS" and full.split("::")[-1] == cell
        ]
        assert len(found) == 1, (cell, found)
        locations[cell] = found[0]
    write("binaries.json", locations)
    exact(BASELINE)
    saved_tests = {
        cell: digest(region(PLAN["test_files"][cell], cell)[2].encode())
        for cell in cells
    }
    for mutant in PLAN["mutants"]:
        label = mutant["label"]
        names = mutant["tests"]
        binaries = {locations[cell][0] for cell in names}
        assert len(binaries) == 1
        binary = binaries.pop()
        full_names = [locations[cell][1] for cell in names]
        exact(BASELINE)
        assert not git("status", "--porcelain")
        whole(label + "-head", binary, full_names, True)
        exact(BASELINE)
        changed = {}
        evidence = None
        try:
            if "patch" in mutant:
                patch = Path(mutant["patch"])
                assert digest(patch.read_bytes()) == mutant["patch_sha256"]
                paths = re.findall(r"^\+\+\+ b/(.+)$", patch.read_text(), re.MULTILINE)
                assert paths == ["crates/sutura-http/src/inbound/gate.rs"]
                changed = {path: Path(path).read_bytes() for path in paths}
                subprocess.run(["git", "apply", "--check", str(patch)], check=True)
                subprocess.run(["git", "apply", str(patch)], check=True)
            else:
                for edit in mutant["edits"]:
                    path = edit["path"]
                    assert "/tests/" not in path and not path.endswith("/tests.rs")
                    file = Path(path)
                    changed.setdefault(path, file.read_bytes())
                    source = file.read_text()
                    assert source.count(edit["before"]) == edit["count"] == 1
                    file.write_text(source.replace(edit["before"], edit["after"], 1))
            expected = dict(BASELINE)
            for path, old in changed.items():
                new = Path(path).read_bytes()
                old_tail = re.split(
                    rb"(?m)^#\[cfg\(test\)\]\nmod tests", old, maxsplit=1
                )[1:]
                new_tail = re.split(
                    rb"(?m)^#\[cfg\(test\)\]\nmod tests", new, maxsplit=1
                )[1:]
                assert old_tail == new_tail, "inline test module changed"
                expected[path] = [os.lstat(path).st_mode, digest(new)]
            assert all(
                digest(region(PLAN["test_files"][cell], cell)[2].encode())
                == saved_tests[cell]
                for cell in cells
            )
            exact(expected)
            write(label + "-mutant-state.json", expected)
            text = whole(label + "-mutant", binary, full_names, False)
            exact(expected)
            evidence = [own_sites(text, cell) for cell in names]
        finally:
            for path, content in changed.items():
                Path(path).write_bytes(content)
            exact(BASELINE)
            whole(label + "-restored", binary, full_names, True)
            exact(BASELINE)
            assert not git("status", "--porcelain")
        write(
            label + "-proof.json",
            {
                "source": PLAN["source_sha"],
                "tree": PLAN["source_tree"],
                "binary_id": binary,
                "own_assertions": evidence,
                "restored": True,
            },
        )
    write(
        "mutation-summary.json",
        {
            "mutants": len(PLAN["mutants"]),
            "named_cells": len(cells),
            "status": "own-assertion kills and whole-binary head/restored passes",
        },
    )


def bootstrap(mode, plan_path):
    out = Path("target/inbound-verifier-proof") / ("bootstrap-" + mode)
    out.mkdir(parents=True, exist_ok=False)
    installable = ".#apps.x86_64-linux.causality.program"
    app = Path(subprocess.check_output(["nix", "eval", "--raw", installable]).decode())
    context = json.loads(
        subprocess.check_output(
            ["nix", "eval", "--json", "--apply", "builtins.getContext", installable]
        )
    )
    assert len(context) == 1 and all(path.endswith(".drv") for path in context)
    subprocess.run(["nix-store", "--realise", *context], check=True)
    original = app.read_text()
    needle = 'exec cargo run -q --profile ci -p xtask -- test-causality "$@"'
    assert original.count(needle) == 1
    command = shlex.join(
        [
            "python3",
            "-I",
            str(Path(__file__).resolve()),
            mode,
            str(Path(plan_path).resolve()),
        ]
    )
    replacement = (
        'export SUTURA_PROOF_BOUND=1\nexport SUTURA_PROOF_CARGO="$(command -v cargo)"\nexec '
        + command
    )
    script = original.replace(needle, replacement, 1)
    (out / "environment-original.sh").write_text(original)
    (out / "environment-proof.sh").write_text(script)
    (out / "context.json").write_text(
        json.dumps(
            {
                "app": str(app),
                "context": context,
                "original_sha256": digest(original.encode()),
                "proof_sha256": digest(script.encode()),
            },
            indent=2,
        )
        + "\n"
    )
    env = dict(os.environ)
    for name in (
        "BASH_ENV",
        "CARGO_NET_OFFLINE",
        "CARGO_PROFILE_DEV_CODEGEN_BACKEND",
        "CARGO_UNSTABLE_CODEGEN_BACKEND",
        "NEXTEST_PROFILE",
    ):
        env.pop(name, None)
    result = subprocess.run(
        ["bash", str(out / "environment-proof.sh")], env=env, check=False
    )
    (out / "exit.status").write_text(str(result.returncode) + "\n")
    assert result.returncode == 0, (
        "hosted task failed; its original exit and logs are retained"
    )


def environment():
    assert os.environ["SUTURA_DEV_REQUIRE_TIER"] == "1"
    facts = {"required_tier": "1", "tools": {}, "drivers": {}}
    for tool in (
        "cargo",
        "cargo-nextest",
        "just",
        "sutura-postgres-tier",
        "sutura-clickhouse-tier",
    ):
        path = shutil.which(tool)
        assert path and path.startswith("/nix/store/"), (tool, path)
        facts["tools"][tool] = path
    assert facts["tools"]["cargo"] == os.environ["SUTURA_PROOF_CARGO"]
    for tool in ("cargo", "cargo-nextest"):
        facts[tool + "_version"] = subprocess.check_output(
            [facts["tools"][tool], "--version"]
        ).decode()
    for system in ("POSTGRES", "CLICKHOUSE", "DUCKDB"):
        name = "SUTURA_" + system + "_ADBC_DRIVER"
        path = Path(os.environ[name])
        assert str(path).startswith("/nix/store/") and path.is_file()
        facts["drivers"][name] = str(path)
    for tool in ("sutura-postgres-tier", "sutura-clickhouse-tier"):
        subprocess.run([facts["tools"][tool], "status"], check=True)
    for name in ("CARGO_HOME", "CARGO_TARGET_DIR"):
        facts[name] = os.environ[name]
    config = Path(os.environ["CARGO_HOME"]) / "config.toml"
    stamp = Path(os.environ["CARGO_TARGET_DIR"]) / ".sutura-warm-start"
    assert config.is_file() and stamp.is_file()
    facts["vendor_config_sha256"] = digest(config.read_bytes())
    facts["warm_artifacts"] = stamp.read_text()
    write("environment.json", facts)


def fuzz_metadata_preflight(inherited_home):
    command = [
        "cargo",
        "metadata",
        "--manifest-path",
        "fuzz/Cargo.toml",
        "--format-version",
        "1",
        "--locked",
    ]
    expected = [
        p["version"]
        for p in tomllib.loads(Path("fuzz/Cargo.lock").read_text())["package"]
        if p["name"] == "libfuzzer-sys"
    ]
    assert len(expected) == 1
    outcomes = []
    for label, home in [
        ("inherited", inherited_home),
        ("fresh", os.environ["CARGO_HOME"]),
    ]:
        result = subprocess.run(
            command,
            env=dict(os.environ, CARGO_HOME=home),
            capture_output=True,
            check=False,
        )
        for stream, data in [("stdout", result.stdout), ("stderr", result.stderr)]:
            (OUT / ("fuzz-metadata-" + label + "." + stream)).write_bytes(data)
        outcomes.append(
            {
                "environment": label,
                "command": command,
                "exit": result.returncode,
                "stdout_sha256": digest(result.stdout),
                "stderr_sha256": digest(result.stderr),
            }
        )
        write("fuzz-metadata-results.json", outcomes)
        if label == "inherited":
            assert result.returncode != 0
            assert b"no matching package named `libfuzzer-sys` found" in result.stderr
            assert b"which is replacing registry `crates-io`" in result.stderr
        else:
            assert result.returncode == 0, result.stderr.decode(errors="replace")
            metadata = json.loads(result.stdout)
            found = [
                p["version"]
                for p in metadata["packages"]
                if p["name"] == "libfuzzer-sys"
            ]
            assert found == expected
    write(
        "preflight.json",
        {
            "source": PLAN["source_sha"],
            "tree": PLAN["source_tree"],
            "libfuzzer_versions": expected,
            "outcomes": outcomes,
            "limit": "Cargo metadata only; no validate, ship-check or causality execution.",
        },
    )


if __name__ == "__main__":
    assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
    os.environ["GIT_OPTIONAL_LOCKS"] = "0"
    os.environ["INSTA_UPDATE"] = "no"
    mode, plan_path = sys.argv[1:]
    assert mode in ("preflight", "validate", "ship-check")
    assert not os.environ.get("SKIP"), "no skipped hooks in the hosted finishing gate"
    PLAN = json.loads(Path(plan_path).read_text())
    assert git("rev-parse", "HEAD").decode().strip() == PLAN["source_sha"]
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == PLAN["source_tree"]
    assert (
        git("merge-base", PLAN["comparison_sha"], PLAN["source_sha"]).decode().strip()
        == PLAN["comparison_sha"]
    )
    assert not git("status", "--porcelain")
    if not os.environ.get("SUTURA_PROOF_BOUND"):
        bootstrap(mode, plan_path)
        sys.exit(0)
    OUT = Path("target/inbound-verifier-proof") / mode
    OUT.mkdir(parents=True, exist_ok=False)
    INDEX = Path(git("rev-parse", "--git-path", "index").decode().strip())
    INDEX_CAPTURES = []
    SAVED_INDEX = capture_index("index-initial")
    initial_entries = git("ls-files", "--stage", "-z")
    (OUT / "index-initial-entries.z").write_bytes(initial_entries)
    capture_index("index-after-initial-entries")
    TRACKED = git("ls-files", "-z").decode().rstrip("\0").split("\0")
    BASELINE = state()
    exact(BASELINE)
    assert not git("status", "--porcelain")
    write(
        "inputs.json",
        {
            "source": PLAN["source_sha"],
            "tree": PLAN["source_tree"],
            "mode": mode,
            "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"],
            "run_id": os.environ["GITHUB_RUN_ID"],
            "plan_sha256": digest(Path(plan_path).read_bytes()),
            "payload_sha256": digest(Path(__file__).read_bytes()),
            "index_sha256": digest(SAVED_INDEX),
        },
    )
    write("source-manifest.json", BASELINE)
    try:
        environment()
        os.environ["SHIP_CHECK_BASE_REF"] = PLAN["comparison_sha"]
        inherited_home = os.environ["CARGO_HOME"]
        native_home = Path(tempfile.mkdtemp(prefix=mode + "-cargo-home-")).resolve()
        os.environ["CARGO_HOME"] = str(native_home)
        os.environ.pop("CARGO_NET_OFFLINE", None)
        os.environ.pop("SUTURA_SATELLITE_CARGO_VENDOR_DIR", None)
        assert not list(native_home.iterdir())
        write(
            "native-environment.json",
            {
                "CARGO_HOME": str(native_home),
                "initial_home_entries": [],
                "CARGO_TARGET_DIR": os.environ.get("CARGO_TARGET_DIR"),
                "CARGO_NET_OFFLINE": os.environ.get("CARGO_NET_OFFLINE"),
                "SUTURA_SATELLITE_CARGO_VENDOR_DIR": os.environ.get(
                    "SUTURA_SATELLITE_CARGO_VENDOR_DIR"
                ),
                "limit": "Fresh Cargo home after source-pinned warm-environment checks; warm target retained.",
            },
        )
        if mode == "preflight":
            fuzz_metadata_preflight(inherited_home)
            status = 0
        else:
            status, _ = execute(mode, ["just", mode])
        exact(BASELINE)
        assert not git("status", "--porcelain")
        assert status == 0
    finally:
        capture_index("index-before-export")
        export_entries = git("ls-files", "--stage", "-z")
        (OUT / "index-before-export-entries.z").write_bytes(export_entries)
        capture_index("index-after-export-entries")
        (OUT / "final.patch").write_bytes(git("diff", "--binary", "HEAD"))
        capture_index("index-after-diff")
        untracked = (
            git("ls-files", "--others", "--exclude-standard", "-z").decode().split("\0")
        )
        capture_index("index-after-untracked")
        with tarfile.open(OUT / "generated-untracked.tar.gz", "w:gz") as archive:
            for path in filter(None, untracked):
                archive.add(path, arcname=path, recursive=False)
        final_status = git("status", "--porcelain").decode()
        capture_index("index-after-status")
        final_entries = git("ls-files", "--stage", "-z")
        (OUT / "index-final-entries.z").write_bytes(final_entries)
        final_index = capture_index("index-final")
        write(
            "index-capture-order.json",
            {
                "captures": INDEX_CAPTURES,
                "limit": "Each raw snapshot precedes the next Git command; final is after the last one. No index bytes are rewritten.",
                "entry_manifests": {
                    label: {"sha256": digest(data), "entries": data.count(b"\0")}
                    for label, data in (
                        ("initial", initial_entries),
                        ("before-export", export_entries),
                        ("final", final_entries),
                    )
                },
            },
        )
        write(
            "final-state.json",
            {
                "tracked": state(),
                "status": final_status,
                "index_sha256": digest(final_index),
                "index_unchanged": final_index == SAVED_INDEX,
                "staged_entries_unchanged": final_entries == initial_entries,
            },
        )
        assert final_entries == initial_entries, (
            "staged entries changed; raw manifests retained"
        )
        assert state() == BASELINE, (
            "tracked source or test bytes/modes changed during evidence export"
        )

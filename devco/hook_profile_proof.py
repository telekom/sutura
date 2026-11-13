"""Hosted proof of the configured Rust hook profile and its driver boundary."""

import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
assert sys.platform == "linux" and os.uname().machine == "x86_64"
assert not os.environ.get("SKIP") and not os.environ.get("PREK_SKIP")
os.environ["GIT_OPTIONAL_LOCKS"] = "0"
os.environ["INSTA_UPDATE"] = "no"
PLAN = json.loads(Path(sys.argv[1]).read_text())
OUT = Path("target/hook-profile-proof").resolve()
OUT.mkdir(parents=True, exist_ok=False)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def git(*args):
    return subprocess.check_output(["git", "--no-optional-locks", *args])


def run(label, argv, env=None, allow_failure=False):
    write(label + ".command.json", {"argv": argv})
    result = subprocess.run(argv, env=env, capture_output=True, check=False)
    (OUT / (label + ".stdout")).write_bytes(result.stdout)
    (OUT / (label + ".stderr")).write_bytes(result.stderr)
    write(label + ".exit.json", {"exit": result.returncode})
    if not allow_failure:
        assert result.returncode == 0, (label, result.returncode)
    return result


assert git("rev-parse", "HEAD").decode().strip() == PLAN["source_sha"]
assert git("rev-parse", "HEAD^{tree}").decode().strip() == PLAN["source_tree"]
assert (
    git("merge-base", PLAN["comparison_sha"], "HEAD").decode().strip()
    == PLAN["comparison_sha"]
)
assert not git("status", "--porcelain")
INDEX = Path(git("rev-parse", "--git-path", "index").decode().strip())
SAVED_INDEX = INDEX.read_bytes()
(OUT / "index-initial").write_bytes(SAVED_INDEX)
ENTRIES = git("ls-files", "--stage", "-z")
(OUT / "index-initial-entries.z").write_bytes(ENTRIES)
TRACKED = git("ls-files", "-z").decode().rstrip("\0").split("\0")


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


BASELINE = state()
write("source-initial.json", BASELINE)
write(
    "inputs.json",
    {
        **PLAN,
        "helper": os.environ["GITHUB_WORKFLOW_SHA"],
        "run": os.environ["GITHUB_RUN_ID"],
        "payload_sha256": digest(Path(__file__).read_bytes()),
    },
)
ORIGINAL = Path("devenv.nix").read_bytes()
DRIVERS = {"POSTGRES": "postgresql", "CLICKHOUSE": "clickhouse"}
NATIVE = (
    "stdenv",
    "nativeBuildInputs",
    "buildInputs",
    "propagatedBuildInputs",
    "propagatedNativeBuildInputs",
    "SUTURA_DUCKDB_ADBC_DRIVER",
    "RUST_SRC_PATH",
    "hardeningDisable",
)


def drivers(label):
    result = {}
    for name, package in DRIVERS.items():
        result[name] = json.loads(
            run(
                label + "-" + name,
                [
                    "nix",
                    "eval",
                    "--option",
                    "max-jobs",
                    "0",
                    "--json",
                    "--apply",
                    "p: { drv = p.drvPath; out = p.outPath; }",
                    ".#packages.x86_64-linux.adbc-driver-" + package + "-host",
                ],
            ).stdout
        )
    return result


def validity(label, paths):
    return {
        name: run(
            label + "-" + name,
            ["nix-store", "--check-validity", value["out"]],
            allow_failure=True,
        ).returncode
        == 0
        for name, value in paths.items()
    }


def shell(label, profile):
    argv = ["devenv", "--no-eval-cache", "--nix-option", "max-jobs", "0"]
    if profile:
        argv += ["--profile", "hooks"]
    value = json.loads(run(label + "-eval", argv + ["eval", "shell.drvPath"]).stdout)
    drv = value["shell.drvPath"]
    assert drv.startswith("/nix/store/") and drv.endswith(".drv")
    closure = set(
        run(label + "-requisites", ["nix-store", "--query", "--requisites", drv])
        .stdout.decode()
        .splitlines()
    )
    definition = json.loads(
        run(label + "-derivation", ["nix", "derivation", "show", drv]).stdout
    )
    assert definition["version"] == 4
    return drv, closure, definition["derivations"][Path(drv).name]["env"]


def clear_closure(label):
    paths = drivers(label + "-drivers")
    before = validity(label + "-before", paths)
    drv, closure, environment = shell(label, True)
    after = validity(label + "-after", paths)
    assert before == after, "closure observation realized a driver output"
    found = sorted(name for name, value in paths.items() if value["drv"] in closure)
    write(
        label + ".json",
        {
            "shell_drv": drv,
            "drivers": paths,
            "driver_outputs_valid_before": before,
            "driver_outputs_valid_after": after,
            "present": found,
            "native": {key: environment.get(key) for key in NATIVE},
        },
    )
    assert not found, ("mutable-driver-closure", found)
    return environment


PROBE = r"""
import json, os, shutil, subprocess, sys
from pathlib import Path
names = ("SUTURA_POSTGRES_ADBC_DRIVER", "SUTURA_CLICKHOUSE_ADBC_DRIVER", "SUTURA_DUCKDB_ADBC_DRIVER", "RUST_SRC_PATH", "NIX_CC", "NIX_CFLAGS_COMPILE", "NIX_LDFLAGS", "LIBCLANG_PATH", "DUCKDB_LIB_DIR", "DUCKDB_INCLUDE_DIR", "PKG_CONFIG_PATH")
facts = {"env": {name: os.environ.get(name) for name in names}, "tools": {name: shutil.which(name) for name in ("cargo", "rustc", "rustfmt", "cargo-clippy", "cc", "c++", "pkg-config")}}
for name in ("SUTURA_POSTGRES_ADBC_DRIVER", "SUTURA_CLICKHOUSE_ADBC_DRIVER"):
    assert facts["env"][name] in (None, ""), (name, facts["env"][name])
for name, value in facts["tools"].items():
    assert value and value.startswith("/nix/store/"), (name, value)
assert facts["env"]["SUTURA_DUCKDB_ADBC_DRIVER"] and Path(facts["env"]["SUTURA_DUCKDB_ADBC_DRIVER"]).is_file()
assert facts["env"]["RUST_SRC_PATH"] and Path(facts["env"]["RUST_SRC_PATH"]).is_dir()
facts["versions"] = {name: subprocess.check_output([name, "--version"], text=True) for name in ("cargo", "rustc", "rustfmt", "cargo-clippy", "cc")}
Path(sys.argv[1]).write_text(json.dumps(facts, indent=2))
"""

try:
    run("devenv-version", ["devenv", "--version"])
    run("nix-version", ["nix", "--version"])
    default_paths = drivers("default-drivers")
    before = validity("default-before", default_paths)
    _, default_closure, default_environment = shell("default", False)
    assert before == validity("default-after", default_paths)
    assert all(value["drv"] in default_closure for value in default_paths.values())
    environment = clear_closure("head")
    for key in NATIVE:
        assert environment.get(key) == default_environment.get(key), (
            "native environment changed",
            key,
        )
    for key in (
        "stdenv",
        "nativeBuildInputs",
        "SUTURA_DUCKDB_ADBC_DRIVER",
        "RUST_SRC_PATH",
    ):
        assert environment.get(key), ("missing native environment", key)

    for name in DRIVERS:
        needle = ("    SUTURA_" + name + '_ADBC_DRIVER = lib.mkForce "";\n').encode()
        assert ORIGINAL.count(needle) == 1
        try:
            Path("devenv.nix").write_bytes(ORIGINAL.replace(needle, b"", 1))
            (OUT / ("mutation-" + name + ".patch")).write_bytes(
                git("diff", "--binary", "HEAD")
            )
            try:
                clear_closure("mutation-" + name)
            except AssertionError as error:
                assert error.args == (("mutable-driver-closure", [name]),), error.args
                write(
                    "mutation-" + name + "-kill.json", {"caught_assertion": error.args}
                )
            else:
                raise AssertionError(
                    "restoring driver context did not kill the closure check"
                )
        finally:
            Path("devenv.nix").write_bytes(ORIGINAL)
        clear_closure("restored-" + name)
        assert state() == BASELINE and INDEX.read_bytes() == SAVED_INDEX

    config = Path(".pre-commit-config.yaml").read_text()
    expected = {
        "rust-fmt": ["cargo", "run", "-q", "-p", "xtask", "--", "fmt", "--check"],
        "rust-clippy": [
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    }
    stale = dict(
        os.environ,
        SUTURA_POSTGRES_ADBC_DRIVER="stale-postgres-driver",
        SUTURA_CLICKHOUSE_ADBC_DRIVER="stale-clickhouse-driver",
    )
    for name, tail in expected.items():
        block = config.split("      - id: " + name + "\n", 1)[1].split(
            "      - id:", 1
        )[0]
        entries = re.findall(r"^        entry: (.+)$", block, re.MULTILINE)
        assert len(entries) == 1
        argv = shlex.split(entries[0])
        assert argv == ["devenv", "--profile", "hooks", "shell", *tail]
        # The probe uses the configured shell prefix; the literal hook follows unchanged.
        run(
            name + "-probe",
            argv[:4]
            + ["python3", "-I", "-c", PROBE, str(OUT / (name + "-environment.json"))],
            env=stale,
        )
        run(name + "-literal-hook", argv, env=stale)
        active = Path(".devenv/gc/shell").resolve(strict=True)
        assert str(active).startswith("/nix/store/")
        active_closure = set(
            run(
                name + "-active-requisites",
                ["nix-store", "--query", "--requisites", str(active)],
            )
            .stdout.decode()
            .splitlines()
        )
        paths = drivers(name + "-active-drivers")
        assert not any(value["out"] in active_closure for value in paths.values())
        write(name + "-active-shell.json", {"gc_root": str(active), "drivers": paths})
        assert state() == BASELINE and INDEX.read_bytes() == SAVED_INDEX
    write(
        "verdict.json",
        {
            "status": "head and restored closures pass; each restored driver context kills; both literal configured hooks pass under stale inherited driver variables",
            "limit": "Linux venue only. Environment probe uses each configured shell prefix immediately before its unchanged literal hook. No Darwin linking or general cold-bootstrap claim.",
        },
    )
finally:
    (OUT / "final.patch").write_bytes(git("diff", "--binary", "HEAD"))
    final = state()
    status = git("status", "--porcelain").decode()
    final_entries = git("ls-files", "--stage", "-z")
    final_index = INDEX.read_bytes()
    (OUT / "index-final").write_bytes(final_index)
    (OUT / "index-final-entries.z").write_bytes(final_entries)
    write("source-final.json", final)
    write(
        "final-state.json",
        {
            "status": status,
            "index_unchanged": final_index == SAVED_INDEX,
            "entries_unchanged": final_entries == ENTRIES,
            "tracked_unchanged": final == BASELINE,
            "limit": "Tracked lstat modes and hashes of file bytes/link targets; no other filesystem inventory.",
        },
    )
    assert not status and final == BASELINE and final_index == SAVED_INDEX
    assert final_entries == ENTRIES

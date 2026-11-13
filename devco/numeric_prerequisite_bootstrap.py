"""Generate the root lock and compile the frozen prerequisite in hosted scratch only."""

import base64
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path

BASE = "a70aa4fbc16a1a7bd7735675f1a2e7419adb771e"
BASE_TREE = "308146aa489ff12038b037161a9cff276760aa87"
PREPARED_TREE = "10f0b31ed8bd8d68bf305265b6aca65838908c01"
PATCH_SHA256 = "a999e89595667a638732bc5bb0b4958287f993f7d989f16ff26cc772b5a51a2c"
MANIFEST_SHA256 = "9a788b3260e9d1970a892d5c55670fdc84d86e4ba712a057730b97d0cf7a3551"
REQUEST_ID = "3cb4e097-8c85-480c-aafe-51d55a1476a8"
REF = "refs/heads/ci/numeric-prerequisite-bootstrap"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write(out: Path, name: str, value: object) -> None:
    (out / name).write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, stdout=subprocess.PIPE
    ).stdout


def manifest(root: Path) -> dict[str, dict[str, object]]:
    result = {}
    for name in git(root, "ls-files", "-z").decode().split("\0"):
        if not name:
            continue
        path = root / name
        data = os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
        result[name] = {
            "sha256": digest(data),
            "size": len(data),
            "mode": oct(stat.S_IMODE(path.lstat().st_mode)),
            "symlink": path.is_symlink(),
        }
    return result


def execute(root: Path, out: Path, label: str, argv: list[str]) -> int:
    print(f"{label}: starting", flush=True)
    with (out / f"{label}.log").open("wb") as log:
        status = subprocess.run(
            argv, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=False
        ).returncode
    write(out, f"{label}.json", {"argv": argv, "exit": status})
    print(f"{label}: exit {status}", flush=True)
    return status


def checks(root: Path, out: Path) -> None:
    generated = json.loads(
        (out / "generated-manifest.json").read_text(encoding="utf-8")
    )
    require(manifest(root) == generated, "source differs before check/lint")
    tools = {}
    for tool in ("cargo", "rustc", "clang", "ld.lld", "just", "jscpd"):
        path = shutil.which(tool)
        require(
            path is not None and path.startswith("/nix/store/"),
            f"unpinned tool: {tool}",
        )
        tools[tool] = path
        require(execute(root, out, f"version-{tool}", [tool, "--version"]) == 0, tool)
    write(
        out,
        "environment.json",
        {
            "tools": tools,
            "cargo_home": os.environ["CARGO_HOME"],
            "target_dir": os.environ["CARGO_TARGET_DIR"],
        },
    )
    status = execute(
        root,
        out,
        "cargo-check",
        [
            "cargo",
            "check",
            "--locked",
            "-p",
            "sutura-app",
            "-p",
            "sutura-exec-node",
            "--all-targets",
            "--all-features",
        ],
    )
    require(manifest(root) == generated, "cargo check changed source or locks")
    require(status == 0, "cargo check failed; raw failure retained")
    status = execute(root, out, "just-lint", ["just", "lint"])
    require(manifest(root) == generated, "just lint changed source or locks")
    require(status == 0, "just lint failed; raw failure retained")


def main() -> None:
    root = Path.cwd().resolve()
    helper = Path(__file__).resolve().parents[1]
    out = Path(os.environ["RUNNER_TEMP"]) / "numeric-prerequisite-bootstrap"
    require(
        os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch",
        "hosted dispatch only",
    )
    require(os.environ.get("GITHUB_REPOSITORY") == "telekom/sutura", "wrong repository")
    require(os.environ.get("GITHUB_REF") == REF, "wrong helper ref")
    require(
        os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "new request required; no reruns"
    )
    require(os.environ.get("REQUEST_ID") == REQUEST_ID, "wrong request")
    require(
        re.fullmatch("[0-9a-f]{40}", os.environ.get("HELPER_TREE", "")) is not None,
        "bad helper tree",
    )
    require(
        git(helper, "rev-parse", "HEAD").decode().strip() == os.environ["GITHUB_SHA"],
        "wrong helper commit",
    )
    require(
        git(helper, "rev-parse", "HEAD^{tree}").decode().strip()
        == os.environ["HELPER_TREE"],
        "wrong helper tree",
    )
    require(
        not git(helper, "status", "--porcelain", "--untracked-files=all"),
        "dirty helper",
    )
    require(
        git(root, "rev-parse", "HEAD").decode().strip() == BASE, "wrong scratch base"
    )
    require(
        git(root, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "wrong scratch base tree",
    )
    if sys.argv[1:] == ["checks"]:
        checks(root, out)
        return
    require(not sys.argv[1:], "no configurable operation")
    out.mkdir(exist_ok=False)
    write(
        out,
        "verdict.json",
        {"status": "INCOMPLETE", "tests": "UNRUN", "causality": "UNRUN"},
    )
    write(
        out,
        "request.json",
        {
            "ref": REF.removeprefix("refs/heads/"),
            "inputs": {
                "helper_tree": os.environ["HELPER_TREE"],
                "request_id": REQUEST_ID,
            },
        },
    )
    write(
        out,
        "provenance.json",
        {
            "base": BASE,
            "base_tree": BASE_TREE,
            "prepared_tree": PREPARED_TREE,
            "helper_commit": os.environ["GITHUB_SHA"],
            "helper_tree": os.environ["HELPER_TREE"],
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        },
    )
    require(
        not git(root, "status", "--porcelain", "--untracked-files=all"), "dirty scratch"
    )
    write(out, "base-manifest.json", manifest(root))
    gitdir = Path(git(root, "rev-parse", "--absolute-git-dir").decode().strip())
    (out / "base-index.bin").write_bytes((gitdir / "index").read_bytes())
    payload = helper / "devco/numeric-prerequisite-bootstrap"
    patch = out / "accepted-source.patch"
    patch_data = base64.b64decode(
        b"".join((payload / "source.patch.b64").read_bytes().splitlines()),
        validate=True,
    )
    binding = payload / "source-manifest.json"
    require(digest(patch_data) == PATCH_SHA256, "wrong source patch")
    require(digest(binding.read_bytes()) == MANIFEST_SHA256, "wrong source manifest")
    accepted = json.loads(binding.read_text(encoding="utf-8"))
    require(
        accepted["base"] == BASE and accepted["prepared_tree"] == PREPARED_TREE,
        "wrong P binding",
    )
    patch.write_bytes(patch_data)
    shutil.copyfile(binding, out / "accepted-source-manifest.json")
    generated = None
    success = False
    try:
        git(root, "apply", "--check", "--index", "--binary", str(patch))
        git(root, "apply", "--index", "--binary", str(patch))
        require(
            git(root, "write-tree").decode().strip() == PREPARED_TREE,
            "patch does not reconstruct exact P",
        )
        git(root, "diff", "--exit-code")
        prepared = manifest(root)
        write(out, "prepared-manifest.json", prepared)
        for name, expected in accepted["files"].items():
            actual = prepared[name]
            require(
                all(actual[key] == expected[key] for key in ("sha256", "size", "mode")),
                name,
            )
        for key in tuple(os.environ):
            if key.startswith(("CARGO_", "RUSTUP_")) or key in (
                "RUSTC",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "RUSTFLAGS",
                "RUSTDOCFLAGS",
                "DUCKDB_LIB_DIR",
                "DUCKDB_INCLUDE_DIR",
                "BASH_ENV",
            ):
                os.environ.pop(key)
        os.environ["CARGO_HOME"] = str(out.parent / "numeric-prerequisite-cargo")
        os.environ["CARGO_TARGET_DIR"] = str(out.parent / "numeric-prerequisite-target")
        os.environ["CARGO_TERM_COLOR"] = "never"
        os.environ["NO_COLOR"] = "1"
        Path(os.environ["CARGO_HOME"]).mkdir(exist_ok=False)
        require(
            execute(
                root,
                out,
                "generator-version",
                [
                    "nix",
                    "run",
                    "--no-write-lock-file",
                    ".#cargo",
                    "--",
                    "--version",
                ],
            )
            == 0,
            "pinned Cargo app unavailable",
        )
        status = execute(
            root,
            out,
            "lock-generator",
            [
                "nix",
                "run",
                "--no-write-lock-file",
                ".#cargo",
                "--",
                "update",
                "--workspace",
            ],
        )
        shutil.copyfile(root / "Cargo.lock", out / "generated-Cargo.lock")
        after_generator = manifest(root)
        write(out, "after-generator-manifest.json", after_generator)
        require(status == 0, "lock generation failed; no compilation attempted")
        require(
            {name: row for name, row in after_generator.items() if name != "Cargo.lock"}
            == {name: row for name, row in prepared.items() if name != "Cargo.lock"},
            "generation changed source or another lock",
        )
        require(
            git(root, "write-tree").decode().strip() == PREPARED_TREE,
            "generation changed scratch index",
        )
        require(
            git(root, "diff", "--name-only").decode().splitlines() == ["Cargo.lock"],
            "generator changed more than root lock, or changed no lock",
        )
        require(
            not git(root, "ls-files", "--others", "--exclude-standard"),
            "generator added source files",
        )
        git(root, "add", "--", "Cargo.lock")
        generated_tree = git(root, "write-tree").decode().strip()
        generated = manifest(root)
        write(out, "generated-manifest.json", generated)
        (out / "generated-source.patch").write_bytes(
            git(root, "diff", "--binary", "--full-index", BASE, generated_tree)
        )
        write(
            out,
            "generated-source.json",
            {
                "base": BASE,
                "prepared_tree": PREPARED_TREE,
                "generated_tree": generated_tree,
                "lock_sha256": digest((out / "generated-Cargo.lock").read_bytes()),
                "patch_sha256": digest((out / "generated-source.patch").read_bytes()),
                "manifest_sha256": digest(
                    (out / "generated-manifest.json").read_bytes()
                ),
            },
        )
        status = execute(
            root,
            out,
            "check-environment",
            [
                "nix",
                "develop",
                "--impure",
                "--file",
                str(helper / "devco/numeric_prerequisite_bootstrap.nix"),
                "--argstr",
                "source",
                str(root),
                "--command",
                "python3",
                "-I",
                str(Path(__file__).resolve()),
                "checks",
            ],
        )
        require(status == 0, "hosted check/lint failed; generated source retained")
        require(
            git(root, "write-tree").decode().strip() == generated_tree,
            "check/lint changed scratch index",
        )
        success = True
    finally:
        final = manifest(root)
        write(out, "final-manifest.json", final)
        shutil.copyfile(root / "Cargo.lock", out / "final-Cargo.lock")
        (out / "final-index.bin").write_bytes((gitdir / "index").read_bytes())
        write(
            out,
            "final-status.json",
            {
                "status": git(
                    root, "status", "--porcelain=v2", "--untracked-files=all"
                ).decode()
            },
        )
        if generated is not None:
            require(final == generated, "source or locks changed after generation")
        require(
            not git(root, "ls-files", "--others", "--exclude-standard"),
            "unexpected added source",
        )
        write(
            out,
            "verdict.json",
            {
                "status": "BOOTSTRAP_CHECK_LINT_PASS" if success else "FAILED",
                "tests": "UNRUN",
                "claim_kills": "UNRUN",
                "causality": "UNRUN",
                "validate": "UNRUN",
                "ship_check": "UNRUN",
                "actual_main_union": "UNREVIEWED",
                "limit": "Frozen-base lock/check/lint only; not final P or product verification.",
            },
        )


if __name__ == "__main__":
    main()

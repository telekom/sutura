"""Generate both diagnostic locks and compile the frozen source in hosted scratch only."""

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

import tomllib

BASE = "1a30b5783bbc0e987927fdbbed9503cf51477f13"
BASE_TREE = "2d7e0f6c06ff5b696fccc376f592ef82905d4eda"
PREPARED_TREE = "957cad621e1c8711e684ea1db6e81010f28490bc"
PATCH_SHA256 = "fb21dfd69efebf72366c5013c79f66c207599dadc232d6ca7cda73e6c878299f"
MANIFEST_SHA256 = "e5432d394b00920dd9e5b96855ea87a1a4b0ac246e696ccda44c50a0fc0e688d"
REQUEST_ID = "be072cff-add1-4cde-9838-16cf3bb59748"
REF = "refs/heads/ci/distributed-diagnostic-bootstrap"
LOCKS = ("Cargo.lock", "fuzz/Cargo.lock")


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
        ["git", "--no-optional-locks", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
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


def check_lock_delta(root: Path, name: str) -> None:
    expected = tomllib.loads(git(root, "show", f"{BASE}:{name}").decode())
    vendors = [
        package
        for package in expected["package"]
        if package["name"] == "datafusion-distributed" and package["version"] == "4.0.0"
    ]
    require(len(vendors) == 1, "one base distributed package required")
    dependencies = vendors[0]["dependencies"]
    require("tracing" not in dependencies, "base already has the diagnostic edge")
    require(
        any(
            package["name"] == "tracing" and package["version"] == "0.1.44"
            for package in expected["package"]
        ),
        "base tracing package missing",
    )
    vendors[0]["dependencies"] = sorted([*dependencies, "tracing"])
    require(
        tomllib.loads((root / name).read_text(encoding="utf-8")) == expected,
        f"unexpected semantic lock delta: {name}",
    )


def index_bytes(root: Path) -> bytes:
    gitdir = Path(git(root, "rev-parse", "--absolute-git-dir").decode().strip())
    return (gitdir / "index").read_bytes()


def unchanged_base(base: Path, out: Path) -> None:
    require(
        git(base, "rev-parse", "HEAD").decode().strip() == BASE,
        "immutable base ref changed",
    )
    require(
        git(base, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "immutable base tree changed",
    )
    require(
        manifest(base)
        == json.loads(
            (out / "immutable-base-manifest.json").read_text(encoding="utf-8")
        ),
        "immutable base source changed",
    )
    require(
        index_bytes(base) == (out / "immutable-base-index.bin").read_bytes(),
        "immutable base index changed",
    )
    require(
        not git(base, "status", "--porcelain", "--untracked-files=all"),
        "dirty immutable base",
    )


def unchanged_generated(root: Path, out: Path) -> None:
    require(
        git(root, "rev-parse", "HEAD").decode().strip() == BASE, "scratch ref changed"
    )
    require(
        manifest(root)
        == json.loads((out / "generated-manifest.json").read_text(encoding="utf-8")),
        "source or locks differ from generated manifest",
    )
    require(
        index_bytes(root) == (out / "generated-index.bin").read_bytes(),
        "generated index changed",
    )
    require(
        not git(root, "ls-files", "--others", "--exclude-standard"),
        "unexpected added source",
    )
    unchanged_base(root.parent / "base", out)


def checks(root: Path, out: Path) -> None:
    unchanged_generated(root, out)
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
    for label, argv in (
        (
            "cargo-check",
            [
                "cargo",
                "check",
                "--locked",
                "--workspace",
                "--all-targets",
                "--all-features",
            ],
        ),
        (
            "fuzz-lock-check",
            [
                "cargo",
                "metadata",
                "--locked",
                "--all-features",
                "--format-version",
                "1",
                "--no-deps",
                "--manifest-path",
                "fuzz/Cargo.toml",
            ],
        ),
        ("just-lint", ["just", "lint"]),
    ):
        unchanged_generated(root, out)
        status = execute(root, out, label, argv)
        unchanged_generated(root, out)
        require(status == 0, f"{label} failed; raw failure retained")


def main() -> None:
    require(sys.version_info >= (3, 11), "Python 3.11+ required")
    require(sys.executable.startswith("/nix/store/"), "pinned Python required")
    root = Path.cwd().resolve()
    helper = Path(__file__).resolve().parents[1]
    base = helper.parent / "base"
    workspace = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    require(
        helper == workspace / "helper"
        and root == workspace / "source"
        and base.resolve() == workspace / "base",
        "separate helper/base/source checkouts required",
    )
    out = Path(os.environ["RUNNER_TEMP"]) / "distributed-diagnostic-bootstrap"
    require(
        os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch",
        "hosted dispatch only",
    )
    require(os.environ.get("GITHUB_REPOSITORY") == "telekom/sutura", "wrong repository")
    require(os.environ.get("GITHUB_REF") == REF, "wrong helper ref")
    require(
        os.environ.get("GITHUB_WORKFLOW_REF")
        == f"telekom/sutura/.github/workflows/format.yml@{REF}",
        "wrong workflow",
    )
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
    require(
        git(base, "rev-parse", "HEAD").decode().strip() == BASE, "wrong immutable base"
    )
    require(
        git(base, "rev-parse", "HEAD^{tree}").decode().strip() == BASE_TREE,
        "wrong immutable base tree",
    )
    require(
        not git(base, "status", "--porcelain", "--untracked-files=all"),
        "dirty immutable base",
    )
    if sys.argv[1:] == ["compile-only"]:
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
            "python_executable": sys.executable,
            "python_version": sys.version,
        },
    )
    require(
        not git(root, "status", "--porcelain", "--untracked-files=all"), "dirty scratch"
    )
    write(out, "immutable-base-manifest.json", manifest(base))
    (out / "immutable-base-index.bin").write_bytes(index_bytes(base))
    write(out, "base-manifest.json", manifest(root))
    gitdir = Path(git(root, "rev-parse", "--absolute-git-dir").decode().strip())
    (out / "base-index.bin").write_bytes((gitdir / "index").read_bytes())
    payload = helper / "devco/distributed-diagnostic-bootstrap"
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
        accepted["base"] == BASE
        and accepted["base_tree"] == BASE_TREE
        and accepted["prepared_tree"] == PREPARED_TREE
        and accepted["patch_sha256"] == PATCH_SHA256,
        "wrong diagnostic source binding",
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
            "patch does not reconstruct exact diagnostic source",
        )
        git(root, "diff", "--exit-code")
        prepared = manifest(root)
        write(out, "prepared-manifest.json", prepared)
        (out / "prepared-index.bin").write_bytes(index_bytes(root))
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
        os.environ["CARGO_HOME"] = str(out.parent / "distributed-diagnostic-cargo")
        os.environ["CARGO_TARGET_DIR"] = str(
            out.parent / "distributed-diagnostic-target"
        )
        os.environ["CARGO_TERM_COLOR"] = "never"
        os.environ["NO_COLOR"] = "1"
        Path(os.environ["CARGO_HOME"]).mkdir(exist_ok=False)
        Path(os.environ["CARGO_TARGET_DIR"]).mkdir(exist_ok=False)
        cargo_app = f"git+file://{base}?rev={BASE}#cargo"
        require(
            execute(
                root,
                out,
                "generator-version",
                [
                    "nix",
                    "run",
                    "--no-write-lock-file",
                    cargo_app,
                    "--",
                    "--version",
                ],
            )
            == 0,
            "pinned Cargo app unavailable",
        )
        for i in range(len(LOCKS)):
            label = "root-lock-generator" if i == 0 else "fuzz-lock-generator"
            argv = [
                "nix",
                "run",
                "--no-write-lock-file",
                cargo_app,
                "--",
                "update",
                "--workspace",
            ]
            if i != 0:
                argv.extend(["--manifest-path", "fuzz/Cargo.toml"])
            status = execute(root, out, label, argv)
            for lock in LOCKS:
                shutil.copyfile(
                    root / lock, out / f"after-{label}-{lock.replace('/', '-')}"
                )
            after_generator = manifest(root)
            write(out, f"after-{label}-manifest.json", after_generator)
            (out / f"after-{label}.patch").write_bytes(
                git(root, "diff", "--binary", "--full-index")
            )
            require(status == 0, "lock generation failed; no compilation attempted")
            require(
                {
                    path: row
                    for path, row in after_generator.items()
                    if path not in LOCKS
                }
                == {path: row for path, row in prepared.items() if path not in LOCKS},
                "generation changed source or another lock",
            )
            require(
                git(root, "write-tree").decode().strip() == PREPARED_TREE,
                "generation changed scratch index",
            )
            require(
                index_bytes(root) == (out / "prepared-index.bin").read_bytes(),
                "generation changed index bytes",
            )
            require(
                git(root, "diff", "--name-only").decode().splitlines()
                == sorted(LOCKS[: i + 1]),
                "wrong set of changed locks",
            )
            require(
                not git(root, "ls-files", "--others", "--exclude-standard"),
                "generator added source files",
            )
            for lock in LOCKS[: i + 1]:
                check_lock_delta(root, lock)
            unchanged_base(base, out)
        write(
            out,
            "semantic-lock-delta.json",
            {
                "status": "EXACT_EDGE_ONLY",
                "locks": list(LOCKS),
                "package": "datafusion-distributed 4.0.0",
                "added_edge": "tracing",
            },
        )
        git(root, "add", "--", *LOCKS)
        generated_tree = git(root, "write-tree").decode().strip()
        generated = manifest(root)
        write(out, "generated-manifest.json", generated)
        (out / "generated-index.bin").write_bytes(index_bytes(root))
        for lock in LOCKS:
            shutil.copyfile(root / lock, out / f"generated-{lock.replace('/', '-')}")
        (out / "generated-source.tar").write_bytes(
            git(root, "archive", "--format=tar", generated_tree)
        )
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
                "lock_sha256": {
                    lock: digest((root / lock).read_bytes()) for lock in LOCKS
                },
                "archive_sha256": digest((out / "generated-source.tar").read_bytes()),
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
                str(helper / "devco/distributed_diagnostic_bootstrap.nix"),
                "--argstr",
                "base",
                str(base),
                "--command",
                "python3",
                "-I",
                str(Path(__file__).resolve()),
                "compile-only",
            ],
        )
        require(status == 0, "hosted check/lint failed; generated source retained")
        require(
            git(root, "write-tree").decode().strip() == generated_tree,
            "check/lint changed scratch index",
        )
        unchanged_generated(root, out)
        success = True
    finally:
        final = manifest(root)
        write(out, "final-manifest.json", final)
        for lock in LOCKS:
            shutil.copyfile(root / lock, out / f"final-{lock.replace('/', '-')}")
        (out / "final-source.patch").write_bytes(
            git(root, "diff", "--binary", "--full-index", BASE)
        )
        write(
            out, "verdict.json", {"status": "FAILED_OR_INCOMPLETE", "native": "UNRUN"}
        )
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
            unchanged_generated(root, out)
        unchanged_base(base, out)
        require(
            git(helper, "rev-parse", "HEAD").decode().strip()
            == os.environ["GITHUB_SHA"],
            "helper ref changed",
        )
        require(
            git(helper, "rev-parse", "HEAD^{tree}").decode().strip()
            == os.environ["HELPER_TREE"],
            "helper tree changed",
        )
        require(
            not git(helper, "status", "--porcelain", "--untracked-files=all"),
            "helper source changed",
        )
        require(
            git(root, "rev-parse", "HEAD").decode().strip() == BASE,
            "scratch ref changed",
        )
        require(
            not git(root, "ls-files", "--others", "--exclude-standard"),
            "unexpected added source",
        )
        write(
            out,
            "verdict.json",
            {
                "status": "TWO_LOCK_CHECK_LINT_PASS" if success else "FAILED",
                "native": "UNRUN",
                "root_cause": "UNRESOLVED",
                "tests": "UNRUN",
                "claim_kills": "UNRUN",
                "causality": "UNRUN",
                "validate": "UNRUN",
                "ship_check": "UNRUN",
                "actual_main_union": "UNREVIEWED",
                "limit": "Frozen-base two-lock/check/lint only; no product verification, native pass or root-cause credit.",
            },
        )


if __name__ == "__main__":
    main()

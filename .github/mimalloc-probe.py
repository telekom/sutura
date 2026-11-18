"""Bound native allocator observations; four shipped cross targets have a separate venue."""

import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

import tomllib

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "source"
SOURCE_SHA = "e243e87faf80b3a03f5e4564a800a562123a15f9"
SOURCE_TREE = "104185c0544bc4a96d17d63912e56f1dfd151dac"
HELPER_REF = "refs/heads/ci/mimalloc-runtime-probe-concierge0811-r1"
REQUEST_ID = "06787ddf-861a-4e96-ad88-427d648169f7"
PROBE = r"""
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mimalloc.h>

static int overflow_armed;
static void on_error(int error, void *argument) {
  (void)argument;
  fprintf(stderr, "probe_padding_error=%d armed=%d\n", error, overflow_armed);
  fflush(stderr);
  _Exit(overflow_armed && error == EFAULT ? 42 : 43);
}

int main(int argc, char **argv) {
  if (argc == 2 && strcmp(argv[1], "overflow") == 0) {
    mi_register_error(on_error, NULL);
    mi_option_enable(mi_option_show_errors);
    volatile unsigned char *bytes = mi_malloc(17);
    if (bytes == NULL) return 2;
    overflow_armed = 1;
    bytes[17] ^= 1;
    mi_free((void *)bytes);
    return 13;
  }
  if (argc != 1) return 3;
  void *bytes = mi_zalloc(17);
  if (bytes == NULL) return 2;
  memset(bytes, 0x5A, 17);
  mi_free(bytes);
  mi_options_print();
  printf("{\"version\":%d,\"verbose\":%ld,\"show_stats\":%ld,"
         "\"allow_thp\":%ld,\"guarded_sample_rate\":%ld,"
         "\"destroy_on_exit\":%ld,\"purge_decommits\":%ld}\n",
         mi_version(), mi_option_get(mi_option_verbose),
         mi_option_get(mi_option_show_stats), mi_option_get(mi_option_allow_thp),
         mi_option_get(mi_option_guarded_sample_rate),
         mi_option_get(mi_option_destroy_on_exit),
         mi_option_get(mi_option_purge_decommits));
  return 0;
}
"""


def require(ok, message):
    if not ok:
        raise SystemExit(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(*arguments, cwd=SOURCE):
    return (
        subprocess.check_output(["git", "--no-optional-locks", *arguments], cwd=cwd)
        .decode()
        .strip()
    )


def dump(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")


def guard():
    require(os.environ["GITHUB_REPOSITORY"] == "telekom/sutura", "Wrong repository")
    require(os.environ["GITHUB_RUN_ATTEMPT"] == "1", "Reruns refused")
    require(os.environ["CONSUMER"] == "allocator-probe", "Wrong consumer")
    require(os.environ["GITHUB_REF"] == HELPER_REF, "Wrong helper ref")
    expected = os.environ["EXPECTED_HELPER_SHA"]
    require(re.fullmatch(r"[0-9a-f]{40}", expected), "Unbound helper")
    require(
        expected
        == os.environ["GITHUB_SHA"]
        == os.environ["WORKFLOW_SHA"]
        == git("rev-parse", "HEAD", cwd=ROOT),
        "Helper identity mismatch",
    )
    require(not git("status", "--porcelain", cwd=ROOT), "Dirty helper")
    for key in (
        "BASH_ENV",
        "SKIP",
        "PREK_SKIP",
        "PRE_COMMIT_ALLOW_NO_CONFIG",
        "PREK_ALLOW_NO_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "SUTURA_MIMALLOC_LIB_DIR",
    ):
        require(not os.environ.get(key), "Unapproved environment: " + key)
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        stream.write("source_sha=" + SOURCE_SHA + "\n")


def observed(stdout, stderr, diagnostic):
    value = json.loads(stdout)
    require(
        value
        == {
            "version": 30700,
            "verbose": int(diagnostic),
            "show_stats": int(diagnostic),
            "allow_thp": 0 if diagnostic else 2,
            "guarded_sample_rate": 0,
            "destroy_on_exit": 0,
            "purge_decommits": 1,
        },
        "Unexpected effective allocator options",
    )
    for text in (
        "v3.7.0",
        "secure level: 4",
        "guarded mode: enabled, rate=0",
        "padding     : enabled",
    ):
        require(text in stderr, "Missing allocator banner: " + text)
    return value


def probe(out):
    require(
        platform.system() == "Linux" and platform.machine() == "x86_64",
        "Native x86_64 Linux required",
    )
    require(
        git("rev-parse", "HEAD") == SOURCE_SHA
        and git("rev-parse", "HEAD^{tree}") == SOURCE_TREE
        and not git("status", "--porcelain", "--untracked-files=all"),
        "Source identity mismatch",
    )
    out.mkdir(parents=True, exist_ok=False)
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("MIMALLOC_")
    }
    for key in list(env):
        if (key.startswith("CARGO_") and "CODEGEN_BACKEND" in key) or (
            key.startswith("DUCKDB_") and key.endswith("_DIR")
        ):
            del env[key]
    env.pop("SUTURA_MIMALLOC_LIB_DIR", None)
    cargo = Path(os.environ["RUNNER_TEMP"]) / "mimalloc-probe-cargo"
    target = Path(os.environ["RUNNER_TEMP"]) / "mimalloc-probe-target"
    cargo.mkdir(exist_ok=False)
    target.mkdir(exist_ok=False)
    env.update(CARGO_TARGET_DIR=str(target), CC_ENABLE_DEBUG_OUTPUT="1")

    def run(arguments, name, cwd=SOURCE, extra=None, expected=0):
        with (
            (out / (name + ".stdout")).open("xb") as stdout,
            (out / (name + ".stderr")).open("xb") as stderr,
        ):
            result = subprocess.run(
                arguments,
                cwd=cwd,
                env=env | (extra or {}),
                stdout=stdout,
                stderr=stderr,
                check=False,
            )
        dump(
            out / (name + ".result.json"),
            {"command": arguments, "exit": result.returncode, "expected": expected},
        )
        require(result.returncode == expected, "Command failed: " + name)
        return (out / (name + ".stdout")).read_bytes(), (
            out / (name + ".stderr")
        ).read_text(errors="replace")

    dependency = SOURCE / "vendor/mimalloc_rust/libmimalloc-sys"
    manifest = (
        '[package]\nname="allocator-probe"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nlibmimalloc-sys={path='
        + json.dumps(str(dependency))
        + ', features=["secure"]}\n[profile.release]\nopt-level=3\ndebug=false\ndebug-assertions=false\n'
    )
    (cargo / "Cargo.toml").write_text(manifest)
    (out / "Cargo.toml").write_text(manifest)
    (cargo / "lib.rs").write_text("pub use libmimalloc_sys as allocator;\n")
    shutil.copyfile(SOURCE / "Cargo.lock", cargo / "Cargo.lock")
    run(
        [
            "cargo",
            "build",
            "--manifest-path",
            str(cargo / "Cargo.toml"),
            "--release",
            "--all-features",
            "-vv",
        ],
        "bundled-build",
        cwd=cargo,
    )
    shutil.copyfile(cargo / "Cargo.lock", out / "Cargo.lock")
    pinned = tomllib.loads((SOURCE / "Cargo.lock").read_text())["package"]
    actual = tomllib.loads((cargo / "Cargo.lock").read_text())["package"]

    def identity(package):
        return tuple(
            package.get(key) for key in ("name", "version", "source", "checksum")
        )

    require(
        all(
            identity(package) in {identity(item) for item in pinned}
            for package in actual
            if package["name"] != "allocator-probe"
        ),
        "Isolated Cargo dependency drift",
    )
    build_log = (out / "bundled-build.stdout").read_text() + (
        out / "bundled-build.stderr"
    ).read_text()
    for flag in (
        "MI_SECURE=4",
        "MI_PADDING_CHECK_BYTES=1",
        "MI_GUARDED=1",
        "MI_DEFAULT_GUARDED_SAMPLE_RATE=0",
    ):
        require(flag in build_log, "Missing actual cc build flag: " + flag)
    bundled = list(target.glob("release/build/libmimalloc-sys/*/out/libmimalloc.a"))
    require(len(bundled) == 1, "Expected exactly one fallback archive")
    expression = (
        "let flake = builtins.getFlake "
        + json.dumps(str(SOURCE))
        + '; pkgs = import flake.inputs.nixpkgs { system = "x86_64-linux"; }; in (import '
        + json.dumps(str(SOURCE / "nix/mimalloc.nix"))
        + ' { inherit pkgs; }).mimallocFor { targetPkgs = pkgs; optLevel = "3"; isMusl = false; }'
    )
    (out / "prebuilt-expression.nix").write_text(expression + "\n")
    result, _ = run(
        ["nix", "build", "--impure", "--no-link", "--json", "-L", "--expr", expression],
        "prebuilt-build",
    )
    built = json.loads(result)
    require(len(built) == 1, "Expected one prebuilt derivation")
    prebuilt = Path(built[0]["outputs"]["out"]) / "lib/libmimalloc.a"
    run(["nix", "derivation", "show", built[0]["drvPath"]], "prebuilt-derivation")
    (out / "probe.c").write_text(PROBE)
    answers = {}
    allowed = {
        "MIMALLOC_VERBOSE": "1",
        "MIMALLOC_SHOW_STATS": "1",
        "MIMALLOC_ALLOW_THP": "0",
    }
    phases = {
        "default": {},
        "allowed": allowed,
        "blocked": allowed
        | {
            "MIMALLOC_GUARDED_SAMPLE_RATE": "1",
            "MIMALLOC_DESTROY_ON_EXIT": "1",
            "MIMALLOC_PURGE_DECOMMITS": "0",
        },
    }
    for name, archive in (("bundled", bundled[0]), ("prebuilt", prebuilt)):
        directory = out / name
        directory.mkdir()
        shutil.copyfile(archive, directory / "libmimalloc.a")
        executable = directory / "probe"
        run(
            shlex.split(env.get("CC", "cc"))
            + [
                "-I",
                str(dependency / "c_src/mimalloc/v3/include"),
                str(out / "probe.c"),
                str(archive),
                "-lpthread",
                "-ldl",
                "-lm",
                "-o",
                str(executable),
            ],
            name + "-link",
        )
        answers[name] = {}
        for phase, settings in phases.items():
            stdout, stderr = run([str(executable)], name + "-" + phase, extra=settings)
            answers[name][phase] = observed(stdout, stderr, phase != "default")
        _, stderr = run([str(executable), "overflow"], name + "-overflow", expected=42)
        require(
            "probe_padding_error=14 armed=1" in stderr,
            "No owned one-byte corruption diagnostic",
        )
    require(answers["bundled"] == answers["prebuilt"], "Build paths disagree")
    require(
        not git("status", "--porcelain", "--untracked-files=all"),
        "Probe changed source",
    )
    dump(
        out / "receipt.json",
        {
            "source_sha": SOURCE_SHA,
            "source_tree": SOURCE_TREE,
            "helper_sha": os.environ["EXPECTED_HELPER_SHA"],
            "request_id": REQUEST_ID,
            "run_id": os.environ["GITHUB_RUN_ID"],
            "attempt": os.environ["GITHUB_RUN_ATTEMPT"],
            "observed": answers,
            "files": {
                str(path.relative_to(out)): sha(path.read_bytes())
                for path in out.rglob("*")
                if path.is_file()
            },
            "limits": "Native x86_64 GNU isolated allocator probe at O3, actual build.rs fallback and Nix prebuilt archive. Four shipped build/link targets are a separate venue. No ARM/musl allocator runtime, elevated-process, MTE/PAC, performance or served-product credit.",
        },
    )


def self_check():
    value = {
        "version": 30700,
        "verbose": 0,
        "show_stats": 0,
        "allow_thp": 2,
        "guarded_sample_rate": 0,
        "destroy_on_exit": 0,
        "purge_decommits": 1,
    }
    banner = (
        "v3.7.0 secure level: 4 guarded mode: enabled, rate=0 padding     : enabled"
    )
    assert observed(json.dumps(value), banner, False) == value
    for key, change in (
        ("version", 30601),
        ("guarded_sample_rate", 1),
        ("destroy_on_exit", 1),
        ("purge_decommits", 0),
    ):
        try:
            observed(json.dumps(value | {key: change}), banner, False)
        except SystemExit:
            pass
        else:
            raise AssertionError("Incorrect option accepted: " + key)
    print("allocator receipt parser self-check passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["guard"]:
        guard()
    elif sys.argv[1:] == ["self-check"]:
        self_check()
    elif len(sys.argv) == 3 and sys.argv[1] == "probe":
        probe(Path(sys.argv[2]).resolve())
    else:
        raise SystemExit("Unknown operation")

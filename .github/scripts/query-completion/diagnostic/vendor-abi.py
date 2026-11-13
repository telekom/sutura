"""Retain only static ELF and native library discovery metadata after one census."""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def record(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


out = Path(sys.argv[1])
code = int(sys.argv[2])
log = (out / "census.log").read_text()
if code == 0:
    suites = list(json.loads((out / "census.json").read_text())["rust-suites"].values())
    assert len(suites) == 1
    binary = Path(suites[0]["binary-path"])
else:
    matches = re.findall(r"command `([^`\n]+) --list --format terse` exited", log)
    assert len(matches) == 1, "no unique failed binary in census output"
    binary = Path(matches[0])
target = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
assert binary.resolve().is_relative_to(target)
assert re.fullmatch(r"datafusion_distributed-[0-9a-f]+", binary.name)
identities = {}
for name in [
    "clang",
    "ld.lld",
    "rustc",
    "cargo",
    "cargo-nextest",
    "pkg-config",
    "readelf",
]:
    found = shutil.which(name)
    identities[name] = (
        None
        if found is None
        else {
            "path": found,
            "resolved": str(Path(found).resolve()),
            "sha256": sha(Path(found).read_bytes()),
        }
    )
record(out / "abi-tools.json", identities)
assert identities["readelf"] is not None and identities["clang"] is not None
record(
    out / "abi-binary.json",
    {
        "path": str(binary),
        "sha256": sha(binary.read_bytes()),
        "size": binary.stat().st_size,
        "mode": binary.stat().st_mode,
        "census_exit": code,
        "tests_or_mutations_run": False,
    },
)
readelf = identities["readelf"]["path"]
for label, args in [
    ("layout", ["--program-headers", "--dynamic", "--version-info"]),
    ("symbols", ["--dyn-syms"]),
]:
    text = subprocess.check_output(
        [readelf, "--wide", *args, str(binary)], text=True, timeout=30
    )
    if label == "symbols":
        text = (
            "\n".join(line for line in text.splitlines() if "GLIBC_2.43" in line) + "\n"
        )
    (out / f"abi-{label}.txt").write_text(text)
support = Path(identities["clang"]["path"]).parent.parent / "nix-support"
record(
    out / "abi-wrapper.json",
    {
        name: (support / name).read_text() if (support / name).is_file() else None
        for name in [
            "cc-cflags",
            "cc-ldflags",
            "libc-cflags",
            "libc-ldflags",
            "orig-cc",
            "orig-libc",
            "dynamic-linker",
        ]
    },
)
env_names = [
    "LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "NIX_LDFLAGS",
    "NIX_CFLAGS_COMPILE",
    "NIX_ENFORCE_PURITY",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
    "LZMA_API_STATIC",
    "PKG_CONFIG_PATH_FOR_TARGET",
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS",
]
for name in [
    "PKG_CONFIG",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
]:
    env_names.extend(
        [
            name + "_x86_64-unknown-linux-gnu",
            name + "_x86_64_unknown_linux_gnu",
            "HOST_" + name,
            name,
        ]
    )
record(
    out / "abi-discovery-environment.json",
    {name: os.environ.get(name) for name in env_names},
)
links = {}
for path in sorted((target / "ci/build").rglob("output")):
    lines = [
        line
        for line in path.read_text().splitlines()
        if re.match(r"cargo::?rustc-link-(search|lib|arg)=", line)
    ]
    if lines:
        links[str(path.relative_to(target))] = lines
record(out / "abi-build-script-links.json", links)
pkg_env = os.environ.copy()
for name in [
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
]:
    value = next(
        (
            os.environ[key]
            for key in [
                name + "_x86_64-unknown-linux-gnu",
                name + "_x86_64_unknown_linux_gnu",
                "HOST_" + name,
                name,
            ]
            if key in os.environ
        ),
        None,
    )
    if value is not None:
        pkg_env[name] = value
pkg_env.update(PKG_CONFIG_ALLOW_SYSTEM_LIBS="1", PKG_CONFIG_ALLOW_SYSTEM_CFLAGS="1")
pkg = next(
    (
        os.environ[key]
        for key in [
            "PKG_CONFIG_x86_64-unknown-linux-gnu",
            "PKG_CONFIG_x86_64_unknown_linux_gnu",
            "HOST_PKG_CONFIG",
            "PKG_CONFIG",
        ]
        if key in os.environ
    ),
    "pkg-config",
)
discovery = []
for args in [
    ["--variable=pcfiledir", "liblzma"],
    ["--libs", "--cflags", "liblzma", "liblzma >= 5.8.0"],
]:
    argv = [pkg, *args]
    result = subprocess.run(
        argv,
        check=False,
        env=pkg_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=30,
    )
    discovery.append({"argv": argv, "exit": result.returncode, "output": result.stdout})
record(out / "abi-pkg-config.json", discovery)
libraries = set(re.findall(r"(/nix/store/[^ :\n]+/lib/libm\.so\.6)", log))
libraries.update(
    [
        "/usr/lib/x86_64-linux-gnu/libm.so.6",
        "/lib/x86_64-linux-gnu/libm.so.6",
    ]
)
runtime = {}
for name in sorted(libraries):
    path = Path(name)
    if path.is_file():
        runtime[name] = {
            "resolved": str(path.resolve()),
            "sha256": sha(path.read_bytes()),
            "versions": subprocess.check_output(
                [readelf, "--wide", "--version-info", str(path)],
                text=True,
                timeout=30,
            ),
        }
record(out / "abi-libm.json", runtime)

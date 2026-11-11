"""Hosted-only proof of the narrowly backported Oracle TLS provider selection."""

import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

source = Path.cwd()
output = Path(os.environ["RUNNER_TEMP"]) / "oracle-provider-proof"
output.mkdir()
scratch = Path(os.environ["RUNNER_TEMP"]) / "oracle-provider-scratch"
relative = "vendor/rust-oracledb/src/transport.rs"
fallback = "transport::tests::tls_uses_aws_lc_fallback_without_installing_a_provider"
installed = "transport::tests::tls_preserves_an_installed_provider_and_its_resolver_snapshot"


def run(command, name, cwd=source):
    result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    log = result.stdout.decode(errors="replace")
    (output / f"{name}.log").write_text(log)
    print(log, flush=True)
    return result.returncode, log


status, _ = run(
    ["nix", "build", ".#checks.x86_64-linux.vendored-oracledb", "-L", "--out-link", str(output / "nix-head")],
    "nix-head",
)
if status:
    raise SystemExit("declared vendor check failed; no behavioral verdict")
head_suite = (output / "nix-head/test.log").read_text()
(output / "nix-unit-head.log").write_text(head_suite)
for cell in (fallback, installed):
    assert f"test {cell} ... ok" in head_suite, cell

shutil.copytree(source / "vendor/rust-oracledb", scratch)
shutil.copyfile(source / "Cargo.lock", scratch / "Cargo.lock")
manifest = scratch / "Cargo.toml"
manifest.write_text(re.sub(r"(?ms)^\[dev-dependencies\]\n.*?(?=^\[|\Z)", "", manifest.read_text()).replace(
    'rustls = "0.23.42"', 'rustls = { version = "0.23.42", features = ["ring"] }',
))
# The scratch venue deliberately enables both providers; the declared Nix check above
# measures the unmodified manifest, while this venue witnesses the original panic setup.
assert 'features = ["ring"]' in manifest.read_text()
for key in ("CARGO_UNSTABLE_CODEGEN_BACKEND", "CARGO_PROFILE_DEV_CODEGEN_BACKEND", "DUCKDB_LIB_DIR", "DUCKDB_INCLUDE_DIR", "ORACLE_PROVIDER_TEST_CHILD"):
    os.environ.pop(key, None)
command = ["nix", "run", f"{source}#cargo", "--"]
status, _ = run(command + ["metadata", "--format-version", "1", "--features", "arrow"], "scratch-metadata", scratch)
assert status == 0, "scratch resolution failed"


def identities(path):
    return {(p["name"], p["version"], p.get("source"), p.get("checksum")) for p in tomllib.loads(path.read_text())["package"]}


authoritative = identities(source / "Cargo.lock")
derived = identities(scratch / "Cargo.lock")
proof = {"root_packages": len(authoritative), "scratch_packages": len(derived), "unexpected": sorted(derived - authoritative)}
(output / "scratch-lock-proof.json").write_text(json.dumps(proof, indent=2) + "\n")
assert not proof["unexpected"], "scratch resolution introduced a package identity"
shutil.copyfile(scratch / "Cargo.lock", output / "scratch-Cargo.lock")
command += ["test", "--locked", "--lib", "--features", "arrow"]


def measure(name, cell, expect_green):
    status, log = run(command + [cell, "--", "--exact", "--nocapture"], name, scratch)
    if expect_green:
        assert status == 0 and f"test {cell} ... ok" in log and "test result: ok" in log, name
    else:
        assert status != 0 and f"test {cell} ... FAILED" in log and "test result: FAILED" in log, name


path = scratch / "src/transport.rs"
head = path.read_text()
for name, cell in (("fallback-head", fallback), ("installed-head", installed)):
    measure(name, cell, True)

# Retain the exact head test tail and restore only the parent's production bytes.
# No source-text assertion substitutes for the actual named runtime failures below.
base_sha = "cef39d54a2c321ca6a5572331eb9c2692a78eb03"
subprocess.run(["git", "merge-base", "--is-ancestor", base_sha, "HEAD"], check=True)
base = subprocess.check_output(["git", "show", f"{base_sha}:{relative}"], text=True)
marker = "#[cfg(test)]\nmod tests {"
assert head.count(marker) == base.count(marker) == 1
base_production = base.split(marker)[0]
head_tests = marker + head.split(marker)[1]
(output / "base-proof.json").write_text(json.dumps({
    "base_sha": base_sha,
    "head_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
    "base_transport_sha256": hashlib.sha256(base.encode()).hexdigest(),
    "head_test_tail_sha256": hashlib.sha256(head_tests.encode()).hexdigest(),
}, indent=2) + "\n")
try:
    path.write_text(base_production + head_tests)
    measure("fallback-base", fallback, False)
    measure("installed-base", installed, False)
finally:
    path.write_text(head)

selection = """provider: CryptoProvider::get_default().cloned().unwrap_or_else(
                || Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            ),"""
cases = [
    ("fallback-ring", "|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()),", "|| Arc::new(rustls::crypto::ring::default_provider()),", fallback),
    ("ignore-installed", selection, "provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),", installed),
    ("late-provider-selection", "TlsClientConfig::builder_with_provider(self.provider.clone())", "TlsClientConfig::builder_with_provider(CryptoProvider::get_default().cloned().unwrap_or_else(|| self.provider.clone()))", installed),
]
for name, before, after, cell in cases:
    assert head.count(before) == 1, name
    changed = head.replace(before, after, 1)
    (output / f"{name}.patch").write_text("".join(difflib.unified_diff(head.splitlines(keepends=True), changed.splitlines(keepends=True), fromfile=f"a/{relative}", tofile=f"b/{relative}")))
    try:
        path.write_text(changed)
        measure(name, cell, False)
    finally:
        path.write_text(head)
for name, cell in (("fallback-restored", fallback), ("installed-restored", installed)):
    measure(name, cell, True)
# Do not upload a dependency tree or compiled artifacts with the evidence.
shutil.rmtree(scratch)
print("two separate head/base/restored cells; three named behavioral mutation kills", flush=True)

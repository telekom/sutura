"""Hosted-only vendor proof. Mutation runs share Cargo artifacts outside the Nix sandbox."""

import difflib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


source = Path.cwd()
output = Path("/tmp/oracle-export")
scratch = Path("/tmp/oracle-driver-mutants")
summary = []


def run(command, log, cwd=source):
    with log.open("w") as stream:
        result = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, check=False)
    print(log.read_text(), flush=True)
    return result.returncode, log.read_text()


# This is the real declared check, including its selected modules and floor.
status, _ = run(
    ["nix", "build", ".#checks.x86_64-linux.vendored-oracledb", "-L", "--out-link", "/tmp/oracle-driver-head"],
    output / "driver-nix-head.log",
)
if status:
    raise SystemExit("declared vendor Nix check failed; no mutation verdict")
head = Path("/tmp/oracle-driver-head/test.log").read_text()
(output / "driver-unit-head.log").write_text(head)
print(head, flush=True)

# Match the declared check's source/lock/manifest/features, retaining a shared target
# to avoid recompiling every third-party dependency for every one-line mutation.
shutil.copytree(source / "vendor/rust-oracledb", scratch)
shutil.copyfile(source / "Cargo.lock", scratch / "Cargo.lock")
manifest = scratch / "Cargo.toml"
manifest.write_text(re.sub(r"(?ms)^\[dev-dependencies\]\n.*?(?=^\[|\Z)", "", manifest.read_text()))
command = ["nix", "run", f"{source}#cargo", "--", "test", "--locked", "--lib", "--features", "arrow"]
# A developer host's optional backend and driver paths are not test inputs.
for key in ["CARGO_UNSTABLE_CODEGEN_BACKEND", "CARGO_PROFILE_DEV_CODEGEN_BACKEND", "DUCKDB_LIB_DIR", "DUCKDB_INCLUDE_DIR"]:
    os.environ.pop(key, None)

parser = "nne::tests::required_services_accept_only_compiled_algorithms_and_modern_versions"
dh = "nne::dh::tests::approved_group_checks_peer_range_and_subgroup_before_key_exchange"
vectors = "nne::record::tests::protected_records_match_independent_go_ora_v3_vectors"
tamper = "nne::record::tests::every_single_byte_record_corruption_and_truncation_is_refused_uniformly"
terminal = "transport::native_tests::corrupt_record_poisoning_prevents_any_later_plaintext_write"
deadline = "transport::native_tests::partial_packet_trickle_cannot_renew_the_absolute_native_deadline"
legacy = "messages::auth::legacy_transport_tests::legacy_opt_in_still_requires_an_actually_encrypted_transport"
control = "transport::native_tests::unprotected_control_and_marker_packets_close_a_native_transport"

# Each target must have passed in the actual head check, not merely compiled.
cases = [
    ("encryption-allowlist", "src/nne/mod.rs", "if !matches!(encryption, 15..=17)", "if false && !matches!(encryption, 15..=17)", parser),
    ("integrity-allowlist", "src/nne/mod.rs", "if !matches!(integrity, 4..=6)", "if false && !matches!(integrity, 4..=6)", parser),
    ("modern-version", "src/nne/mod.rs", "if version >> 24 >= 23", "if true || version >> 24 >= 23", parser),
    ("dh-range", "src/nne/dh.rs", "if peer <= U2048::ONE || peer >= GROUP14.wrapping_sub(&U2048::ONE)", "if false && (peer <= U2048::ONE || peer >= GROUP14.wrapping_sub(&U2048::ONE))", dh),
    ("dh-subgroup", "src/nne/dh.rs", "if peer_form.pow(&order).retrieve() != U2048::ONE", "if false && peer_form.pow(&order).retrieve() != U2048::ONE", dh),
    ("directional-mask", "src/nne/record.rs", "seed[15] = 0x5a;", "seed[15] = 0x5b;", vectors),
    ("fold-iv", "src/nne/record.rs", "self.iv.iter_mut().zip(authentication_key)", "self.iv.iter_mut().zip(authentication_key.iter().map(|_| 0u8))", vectors),
    ("digest-verification", "src/nne/record.rs", "let valid = self.hash.verify(&plaintext[..data_end], &plaintext[data_end..end]);", "let valid = true;", tamper),
    ("terminal-read-failure", "src/transport.rs", "let result = self.receive_packet_inner();\n        if result.is_err() && self.native_requested", "let result = self.receive_packet_inner();\n        if false && result.is_err() && self.native_requested", terminal),
    ("absolute-read-deadline", "src/transport.rs", "fn read_packet(&mut self) -> Result<usize, Error> {\n        if let Some(deadline) = self.native_deadline {\n            self.negotiation_deadline(deadline)?;\n        }", "fn read_packet(&mut self) -> Result<usize, Error> {", deadline),
    ("active-encryption-legacy", "src/messages/auth.rs", "if verifier != Verifier::Pbkdf2 && !client.encrypted_transport()", "if false && verifier != Verifier::Pbkdf2 && !client.encrypted_transport()", legacy),
    ("unprotected-control", "src/transport.rs", "if self.protection.is_some() && packet.packet_type != constants::PACKET_TYPE_DATA", "if false && self.protection.is_some() && packet.packet_type != constants::PACKET_TYPE_DATA", control),
]

for _, _, _, _, test in cases:
    if f"test {test} ... ok" not in head:
        raise SystemExit(f"head did not execute {test}")

for name, relative, before, after, test in cases:
    path = scratch / relative
    original = path.read_text()
    if original.count(before) != 1:
        raise SystemExit(f"{name}: mutation does not match exactly once")
    changed = original.replace(before, after, 1)
    (output / f"driver-{name}.patch").write_text("".join(difflib.unified_diff(
        original.splitlines(keepends=True), changed.splitlines(keepends=True),
        fromfile=f"a/vendor/rust-oracledb/{relative}", tofile=f"b/vendor/rust-oracledb/{relative}",
    )))
    try:
        path.write_text(changed)
        status, log = run(command + [test, "--", "--exact"], output / f"driver-{name}.log", cwd=scratch)
    finally:
        path.write_text(original)
    killed = status != 0 and f"test {test} ... FAILED" in log and "test result: FAILED" in log
    summary.append({"mutation": name, "test": test, "exit": status, "killed": killed})
    (output / "driver-mutation-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    if not killed:
        raise SystemExit(f"{name}: no named behavioral kill; compile/setup errors do not count")

# Prove the same scratch/toolchain route green again after restoring the final mutation.
status, restored = run(command + ["nne::", "transport::native_tests::", "messages::auth::legacy_transport_tests::"], output / "driver-restored.log", cwd=scratch)
if status or "test result: ok" not in restored:
    raise SystemExit("restored standalone driver tests failed")
print(f"{len(summary)} named behavioral kills; head and restored tests green", flush=True)

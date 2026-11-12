"""Hosted parser regressions; importing the helper does not invoke its hosted main."""

import importlib.util
import sys
from pathlib import Path

runner = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("inbound_verifier_proof.py")
spec = importlib.util.spec_from_file_location("proof", runner)
proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proof)
proof.PLAN = {"test_files": {"target": "cell.rs"}}
proof.region = lambda path, cell: (10, 20, "fn target() {}")


def block(binary, name, line):
    return (f"        FAIL [   0.001s] ( 1/2) {binary} {name}\n"
            f"  stderr ───\n    thread 'worker' panicked at cell.rs:{line}:5:\n"
            "    assertion failed\n")


def own_sites(text, expected_line=15):
    if hasattr(proof, "failed_output"):
        return proof.own_sites(text, "target", "pkg::bin", "tests::target", expected_line, proof.digest(b"fn target() {}"))
    return proof.own_sites(text, "target")


def refuses(text, expected_line=15):
    try:
        own_sites(text, expected_line)
    except AssertionError:
        return
    raise AssertionError("accepted unassociated panic evidence")


correct = block("pkg::bin", "tests::target", 15)
summary = "────────────\n     Summary [   0.001s] 2 tests run: 2 failed\n"
summary += "        FAIL [   0.001s] ( 1/2) pkg::bin tests::target\n"
assert own_sites(correct + summary)["sites"][0]["line"] == 15
refuses(block("pkg::bin", "tests::target", 30) + block("pkg::bin", "tests::other", 15) + summary)
refuses(block("other::bin", "tests::target", 15) + summary)
refuses(correct + correct + summary)
refuses(block("pkg::bin", "tests::target", 30) + summary + "    thread 'worker' panicked at cell.rs:15:5:\n")
refuses(correct + summary, 16)

proof.PLAN["helper_site_bindings"] = {"target": {
    "binary": "pkg::bin", "full_cell": "tests::target",
    "test": {"path": "cell.rs", "function_sha256": proof.digest(b"fn target() {}")},
    "assertion_owner": {"path": "cell.rs", "non_test_helper": "holds_until",
                        "function_sha256": proof.digest(b"fn holds_until() {}"),
                        "line": 30, "literal_message": "reviewed behavior assertion"}}}
proof.region = lambda path, cell: (10, 20, "fn target() {}") if cell == "target" else (25, 35, "fn holds_until() {}")
helper = block("pkg::bin", "tests::target", 30).replace("assertion failed", "reviewed behavior assertion")
assert own_sites(helper + summary)["sites"][0]["line"] == 30
refuses(correct + summary)
refuses(helper.replace("reviewed behavior assertion", "setup failed") + summary)
refuses(helper.replace("cell.rs:30:5", "cell.rs:31:5") + summary)
proof.PLAN["helper_site_bindings"]["target"]["assertion_owner"]["function_sha256"] = "wrong"
refuses(helper + summary)
print("named FAIL panic association and exact helper binding: PASS")

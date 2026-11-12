"""Hosted production-only old-None control for UTC timestamp metadata."""

import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path


def main() -> None:
    root = Path.cwd()
    out = Path(os.environ["ORACLE_PROOF_OUTPUT"])
    path = Path("crates/sutura-exec-oracle/src/batches.rs")
    cell = "batches::tests::utc_timestamp_metadata_keeps_each_units_instant_and_nulls"
    source, tree, helper, base = (os.environ[key] for key in ("SOURCE_SHA", "SOURCE_TREE", "GITHUB_WORKFLOW_SHA", "BASE_SHA"))
    for revision in (source, tree, helper, base):
        assert re.fullmatch(r"[0-9a-f]{40}", revision), revision

    def git(*args: str) -> bytes:
        return subprocess.check_output(["git", *args])

    assert git("rev-parse", "HEAD").decode().strip() == source
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == tree
    assert not git("status", "--porcelain")
    subprocess.run(["git", "merge-base", "--is-ancestor", base, source], check=True)
    script = git("show", helper + ":.github/scripts/oracle-beta6-proof.py")
    assert script == Path(__file__).read_bytes()
    index = git("ls-files", "--stage", "-z")
    original = path.read_bytes()
    old = b'fn is_timestamp(data_type: &DataType) -> bool {\n    matches!(data_type, DataType::Timestamp(_, zone) if zone.as_deref().is_none_or(|zone| zone == "UTC"))\n}'
    new = b'const fn is_timestamp(data_type: &DataType) -> bool {\n    matches!(data_type, DataType::Timestamp(_, None))\n}'
    assert original.count(old) == 1
    assertion = b'            assert!(\n                matches!(&answer, Ok(rows) if rows.rows() == [text(expected), vec![Value::Null]]),\n                "{answer:?}"\n            );'
    assert original.count(assertion) == 1
    line = original[:original.index(assertion)].count(b"\n") + 1
    package = "package(=sutura-exec-oracle)"
    selected = package + " & test(=" + cell + ")"
    env = os.environ | {"CARGO_TARGET_DIR": str(root / "target/oracle-beta6-proof"), "CARGO_TERM_COLOR": "never", "INSTA_UPDATE": "no", "NEXTEST_PROFILE": "nofailfast"}
    receipts = []

    def measured(label: str, filterset: str, passed: bool, directory: Path) -> None:
        contents = (directory / path).read_bytes()
        assert contents.count(assertion) == 1
        assertion_line = contents[:contents.index(assertion)].count(b"\n") + 1
        command = ["nix", "run", str(root) + "#just", "--", "test-only", filterset]
        log = out / f"utc-{label}.log"
        with log.open("w", encoding="utf-8") as output:
            result = subprocess.run(command, cwd=directory, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=3600, check=False)
        text = log.read_text(encoding="utf-8")
        print(text, flush=True)
        (out / f"utc-{label}.exit").write_text(str(result.returncode) + "\n", encoding="utf-8")
        assert result.returncode == (0 if passed else 100), (label, result.returncode)
        verdict = "PASS" if passed else "FAIL"
        assert re.search(r"(?m)^\s*" + verdict + r"\s+\[[^\n]+\]\s+(?:\(\d+/\d+\)\s+)?sutura-exec-oracle\s+" + re.escape(cell) + r"$", text), label
        if not passed:
            own_site = r"(?m)^\s*thread '" + re.escape(cell) + r"'(?: \(\d+\))? panicked at " + re.escape(str(path)) + ":" + str(assertion_line) + r":\d+:\n[ \t]*Err\(UnsupportedType \{ column: \"D\","
            assert re.search(own_site, text), (label, assertion_line)
        receipts.append({"label": label, "command": command, "assertion_line": assertion_line, "exit": result.returncode, "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()})

    measured("head-package", package, True, root)
    assert not git("status", "--porcelain")
    start = original.index(b"    #[test]\n    fn " + cell.rsplit("::", 1)[1].encode() + b"()")
    cell_bytes = original[start:original.index(b"    #[test]", start + 1)]
    with tempfile.TemporaryDirectory() as temporary:
        base_root = Path(temporary) / "base"
        subprocess.run(["git", "worktree", "add", "--detach", str(base_root), base], check=True)
        try:
            base_path = base_root / path
            before = base_path.read_bytes()
            marker = b"    #[test]\n    fn a_time_of_day_before_1970_lands_on_its_day()"
            assert before.count(marker) == 1 and cell.rsplit("::", 1)[1].encode() not in before
            base_path.write_bytes(before.replace(marker, cell_bytes + marker))
            overlay = git("-C", str(base_root), "diff", "--binary")
            assert git("-C", str(base_root), "diff", "--name-only").decode().splitlines() == [str(path)]
            (out / "utc-base-test-only.patch").write_bytes(overlay)
            measured("base-test-only", selected, False, base_root)
        finally:
            subprocess.run(["git", "worktree", "remove", "--force", str(base_root)], check=True)
    try:
        path.write_bytes(original.replace(old, new))
        (out / "utc-old-none-mutation.patch").write_bytes(git("diff", "--binary"))
        assert git("diff", "--name-only").decode().splitlines() == [str(path)]
        assert path.read_bytes() == original.replace(old, new)
        measured("old-none-mutation", selected, False, root)
    finally:
        path.write_bytes(original)
    measured("restored-package", package, True, root)
    assert path.read_bytes() == original
    assert not git("status", "--porcelain")
    assert git("ls-files", "--stage", "-z") == index
    assert git("write-tree").decode().strip() == tree
    report = {"source": source, "source_tree": tree, "helper": helper, "base": base, "base_test_overlay_sha256": hashlib.sha256(overlay).hexdigest(), "script_sha256": hashlib.sha256(script).hexdigest(), "cell": cell, "assertion_line": line, "production_sha256": hashlib.sha256(original).hexdigest(), "restored_tree": tree, "runs": receipts, "limit": "The failing loop stops at its first Second row; no independent kill is claimed for later units or nulls. Package tests are not live Oracle or full validation."}
    (out / "utc-proof.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

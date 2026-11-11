"""Hosted assertion proof for the query answer execution disclosures."""

import hashlib
import json
import os
import re
import subprocess
from pathlib import Path

ROOT = Path.cwd()
OUT = Path("/tmp/oracle-export")
SOURCE = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
BASE = os.environ["QUERY_BASE_SHA"]
assert re.fullmatch(r"[a-f0-9]{40}", BASE), BASE
subprocess.run(["git", "merge-base", "--is-ancestor", BASE, SOURCE], check=True)
(OUT / "query-base-sha").write_text(BASE + "\n")
CELLS = [
    "a_served_duckdb_answer_reports_its_statement_and_only_its_own_bindings_on_both_transports",
    "a_files_answer_reports_a_logical_plan_but_a_combined_answer_claims_no_single_execution",
    "reported_execution_text_counts_toward_the_served_response_budget",
]
SELECTED = "binary(served) & (" + " | ".join(f"test({cell})" for cell in CELLS) + ")"
SAFETY = f"binary(served) & test({CELLS[2]})"


def measured(directory, label, selected, cells, passed):
    log = OUT / f"query-{label}.log"
    env = os.environ | {"INSTA_UPDATE": "no", "CARGO_TERM_COLOR": "never", "NEXTEST_PROFILE": "nofailfast", "CARGO_TARGET_DIR": str(ROOT / "target/query-proof")}
    with log.open("w") as output:
        result = subprocess.run(
            ["nix", "run", str(ROOT) + "#just", "--", "test", selected],
            cwd=directory, env=env, stdout=output, stderr=subprocess.STDOUT, check=False,
        )
    text = log.read_text()
    print(text, flush=True)
    (OUT / f"query-{label}.exit").write_text(str(result.returncode) + "\n")
    assert result.returncode == (0 if passed else 100), f"{label}: unexpected exit"
    for cell in cells:
        assert re.search(r"(?m)^\s*" + ("PASS" if passed else "FAIL") + r"\s+\[[^\n]+\] [^\n]*\b" + cell + r"$", text), (label, cell)
    if not passed:
        assert "panicked at" in text, f"{label}: no assertion failure"
    return text


measured(ROOT, "head", SELECTED, CELLS, True)

# Re-prove the existing claim patches this source reanchors through the native gate.
patches = sorted(Path("devco/claim-mutations").glob("*.patch"))
changed = subprocess.check_output(
    ["git", "diff", "--name-only", BASE, SOURCE, "--", "devco/claim-mutations"], text=True,
).splitlines()
assert len(changed) == 13 and all(Path(path) in patches for path in changed), changed
census = [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
          for path in patches]
(OUT / "query-claim-census.json").write_text(json.dumps(census, indent=2) + "\n")
targets = [{"cell": Path(path).stem, "path": path, "sha256": census[patches.index(Path(path))]["sha256"],
            "shard": f"{patches.index(Path(path)) + 1}/{len(patches)}"} for path in changed]
(OUT / "query-claim-targets.json").write_text(json.dumps(targets, indent=2) + "\n")
claim_cells = [target["cell"] for target in targets]
measured(ROOT, "claims-head", " | ".join(f"test(/^(?:.*::)?{cell}$/)" for cell in claim_cells), claim_cells, True)
for target in targets:
    cell, shard = target["cell"], target["shard"]
    command = ["nix", "run", ".#claim-mutation-kills", "--", "--shard", shard]
    log = OUT / f"query-claim-{cell}.log"
    with log.open("w") as output:
        output.write(json.dumps({"source": SOURCE, **target, "command": command}) + "\n")
        output.flush()
        result = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, check=False)
    text = log.read_text()
    print(text, flush=True)
    (OUT / f"query-claim-{cell}.exit").write_text(str(result.returncode) + "\n")
    assert result.returncode == 0, (cell, result.returncode)
    assert f"xtask check-claim-mutation-kills: shard {shard}: 1 of {len(patches)} patches" in text, cell
    assert "ok - claim cells: 1 declared, 1 killed" in text, cell
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT), cell
    assert not (ROOT / "target/causality-claim-worktree").exists(), cell
    assert [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            for path in sorted(Path("devco/claim-mutations").glob("*.patch"))] == census, cell
    (OUT / f"query-claim-{cell}.restored").write_text("Source clean; native claim worktree removed; census unchanged.\n")

base = Path("/tmp/query-execution-base")
subprocess.run(["git", "worktree", "add", "--detach", str(base), BASE], check=True)
cell_file = Path("crates/sutura-cli/tests/served/duckdb.rs")
(base / cell_file).write_bytes((ROOT / cell_file).read_bytes())
subprocess.run(["git", "diff", "--binary"], cwd=base, check=True, stdout=(OUT / "query-base-test-overlay.patch").open("w"))
for index, (cell, assertions) in enumerate(zip(CELLS, [
    ["a DuckDB answer carries its executed statement"],
    ["left: None", "right: Some(Null)"],
    ["rows alone fit; execution text must cross the budget", "left: 200", "right: 413"],
], strict=True)):
    base_text = measured(base, f"base-{index}", f"binary(served) & test({cell})", [cell], False)
    for assertion in assertions:
        assert assertion in base_text, (cell, assertion)

mutant = Path("/tmp/query-execution-mutant")
subprocess.run(["git", "worktree", "add", "--detach", str(mutant), SOURCE], check=True)
production = mutant / "crates/sutura-app/src/lib.rs"
original = production.read_bytes()
needle = b"exceeds_response_bound(&rows, execution.as_ref())"
assert original.count(needle) == 1
production.write_bytes(original.replace(needle, b"exceeds_response_bound(&rows, None)"))
try:
    subprocess.run(["git", "diff", "--binary"], cwd=mutant, check=True, stdout=(OUT / "query-safety-mutation.patch").open("w"))
    mutant_text = measured(mutant, "mutant", SAFETY, [CELLS[2]], False)
    assert "rows alone fit; execution text must cross the budget" in mutant_text
    assert "left: 200" in mutant_text and "right: 413" in mutant_text
finally:
    production.write_bytes(original)
subprocess.run(["git", "diff", "--exit-code"], cwd=mutant, check=True)
measured(mutant, "restored", SAFETY, [CELLS[2]], True)
assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
(OUT / "query-proof-summary").write_text("Three named head passes and base assertion failures; production-only safety mutation killed; restored safety cell passed. Thirteen existing named head passes and native single-item claim-shard kills; source restored. Native gate attests each cell's own assertion site but does not export its internal captured panic text.\n")

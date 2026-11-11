"""Diagnostic only: capture fresh compiler output for one native claim shard."""

import hashlib
import json
import os
import re
import subprocess
import time
from pathlib import Path

ROOT = Path.cwd()
OUT = Path("/tmp/oracle-export")
SOURCE = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
BASE = os.environ["QUERY_BASE_SHA"]
assert re.fullmatch(r"[a-f0-9]{40}", BASE), BASE
subprocess.run(["git", "merge-base", "--is-ancestor", BASE, SOURCE], check=True)
(OUT / "query-base-sha").write_text(BASE + "\n")
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
target = targets[0]
cell, shard = target["cell"], target["shard"]
assert cell == "a_cell_cannot_forge_a_row" and shard == "26/820", target
command = ["nix", "run", ".#claim-mutation-kills", "--", "--shard", shard]
log = OUT / f"query-claim-{cell}.log"
print("DIAGNOSTIC ONLY: one native shard; no head, base, budget or remaining claim runs.", flush=True)
started_ns = time.time_ns()
with log.open("w") as output:
    output.write(json.dumps({"source": SOURCE, **target, "command": command}) + "\n")
    output.flush()
    result = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, check=False)
text = log.read_text()
print(text, flush=True)
(OUT / f"query-claim-{cell}.exit").write_text(str(result.returncode) + "\n")
diagnostics = []
for path in sorted((ROOT / "target/causality-target/ci/.fingerprint").glob("*/output-*")):
    if path.is_symlink() or not path.is_file() or path.stat().st_mtime_ns < started_ns:
        continue
    data = path.read_bytes()
    exported = OUT / "query-compiler-diagnostics" / path.parent.name / path.name
    exported.parent.mkdir(parents=True, exist_ok=True)
    exported.write_bytes(data)
    diagnostics.append({"path": str(path.relative_to(ROOT)), "mtime_ns": path.stat().st_mtime_ns,
                        "sha256": hashlib.sha256(data).hexdigest()})
(OUT / "query-compiler-diagnostics.json").write_text(json.dumps(
    {"source": SOURCE, "cell": cell, "shard": shard, "started_ns": started_ns, "files": diagnostics}, indent=2,
) + "\n")
(OUT / "query-diagnostic-summary").write_text("Diagnostic only: first native shard and fresh compiler files. No full-proof verdict.\n")
assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT), cell
assert not (ROOT / "target/causality-claim-worktree").exists(), cell
assert [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        for path in sorted(Path("devco/claim-mutations").glob("*.patch"))] == census, cell
(OUT / f"query-claim-{cell}.restored").write_text("Source clean; native claim worktree removed; census unchanged.\n")
assert result.returncode == 0, (cell, result.returncode)
assert f"xtask check-claim-mutation-kills: shard {shard}: 1 of {len(patches)} patches" in text, cell
assert "ok - claim cells: 1 declared, 1 killed" in text, cell

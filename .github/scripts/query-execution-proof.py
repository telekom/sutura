"""Diagnostic only: capture one patched cell's build through the existing test task."""

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
cell = target["cell"]
assert cell == "a_cell_cannot_forge_a_row" and target["shard"] == "26/820", target
worktree = Path("/tmp/query-cell-build-diagnostic")
subprocess.run(["git", "worktree", "add", "--detach", str(worktree), SOURCE], check=True)
production = worktree / "crates/sutura-mcp/src/wire.rs"
original = production.read_bytes()
try:
    subprocess.run(["git", "apply", str(ROOT / target["path"])], cwd=worktree, check=True)
    assert subprocess.check_output(["git", "diff", "--name-only"], cwd=worktree, text=True).splitlines() == ["crates/sutura-mcp/src/wire.rs"]
    diff = subprocess.check_output(["git", "diff", "--binary", "--full-index"], cwd=worktree)
    (OUT / "query-cell-diagnostic.patch").write_bytes(diff)
    (OUT / "query-cell-diagnostic.patch.sha256").write_text(hashlib.sha256(diff).hexdigest() + "\n")
    selected = f"binary_id(=sutura-mcp) & test(/^(?:.*::)?{cell}$/)"
    command = ["nix", "run", str(ROOT) + "#just", "--", "test", selected]
    env = os.environ | {"INSTA_UPDATE": "no", "CARGO_TERM_COLOR": "never", "NEXTEST_PROFILE": "nofailfast", "CARGO_TARGET_DIR": str(ROOT / "target/query-diagnostic")}
    with (OUT / "query-cell-diagnostic.log").open("w") as output:
        result = subprocess.run(command, cwd=worktree, env=env, stdout=output, stderr=subprocess.STDOUT, check=False)
    (OUT / "query-cell-diagnostic.exit").write_text(str(result.returncode) + "\n")
    print((OUT / "query-cell-diagnostic.log").read_text(), flush=True)
    (OUT / "query-cell-diagnostic.json").write_text(json.dumps({
        "source": SOURCE, **target, "command": command, "exit": result.returncode,
        "original_sha256": hashlib.sha256(original).hexdigest(),
        "mutated_sha256": hashlib.sha256(production.read_bytes()).hexdigest(),
        "limit": "Diagnostic only: just test uses Cargo dev; the native gate uses ci. No native kill or full-proof verdict.",
    }, indent=2) + "\n")
finally:
    production.write_bytes(original)
    assert production.read_bytes() == original
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=worktree)
    subprocess.run(["git", "worktree", "remove", str(worktree)], check=True)
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
    assert [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            for path in sorted(Path("devco/claim-mutations").glob("*.patch"))] == census
    (OUT / "query-cell-diagnostic.restored").write_text("Exact original bytes restored; diagnostic worktree removed; source clean; census unchanged.\n")
(OUT / "query-diagnostic-summary").write_text("Diagnostic only: one patched cell, raw build output and exit captured. No native replay, head, base, budget or full-proof verdict.\n")

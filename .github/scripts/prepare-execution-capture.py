"""Return exact generation and formatting artifacts; never publish or activate them."""

import hashlib
import json
import os
import subprocess
import sys
import tarfile
from pathlib import Path

EXPECTED_TREE = "a185e017b7f749fd898905e7bd6572d42ec20d69"
source, output, helper = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
output.mkdir(parents=True, exist_ok=False)


def git(*args):
    return subprocess.check_output(["git", *args])


def snapshot(name):
    index = git("ls-files", "--stage", "-z")
    (output / f"{name}-index.zlist").write_bytes(index)
    records = {}
    for entry in index.split(b"\0"):
        if not entry:
            continue
        attributes, raw_path = entry.split(b"\t", 1)
        mode, blob, stage = attributes.decode().split()
        assert stage == "0"
        path = Path(os.fsdecode(raw_path))
        data = os.fsencode(os.readlink(path)) if path.is_symlink() else path.read_bytes()
        records[str(path)] = {"mode": mode, "blob": blob, "sha256": hashlib.sha256(data).hexdigest()}
    (output / f"{name}-source.json").write_text(json.dumps(records, indent=2) + "\n")
    return records


assert git("rev-parse", "HEAD").decode().strip() == source
assert git("rev-parse", "HEAD^{tree}").decode().strip() == EXPECTED_TREE
assert not git("status", "--porcelain")
before = snapshot("original")
results = {}
for name, command in [
    ("generation", ["bash", str(helper / "generate-worker-protocol.sh"), str(Path.cwd()), str(output / "generator")]),
    ("format", ["just", "fmt"]),
]:
    with (output / f"{name}.log").open("wb") as log:
        results[name] = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=False).returncode

# Generated protocol files remain separate from the candidate; review them before importing.
after = snapshot("formatted")
for lock in ["Cargo.lock", "fuzz/Cargo.lock", "flake.lock", "devenv.lock"]:
    assert before[lock] == after[lock], lock
assert before.keys() == after.keys(), "formatting must not add or remove source paths"
changed = [path for path in before if before[path]["sha256"] != after[path]["sha256"]]
(output / "formatted.patch").write_bytes(git("diff", "--binary", "HEAD"))
(output / "status.txt").write_bytes(git("status", "--porcelain", "--untracked-files=all"))
with tarfile.open(output / "formatted-files.tar.gz", "w:gz") as archive:
    for path in changed:
        archive.add(path, arcname=path, recursive=False)
(output / "result.json").write_text(json.dumps({"source": source, "source_tree": EXPECTED_TREE, "results": results, "formatted_paths": changed, "generated_files_installed": False, "compilation_or_runtime_proof": False}, indent=2) + "\n")
manifest = {str(path.relative_to(output)): hashlib.sha256(path.read_bytes()).hexdigest() for path in output.rglob("*") if path.is_file()}
(output / "artifacts.sha256.json").write_text(json.dumps(manifest, indent=2) + "\n")
raise SystemExit(0 if all(code == 0 for code in results.values()) else 1)

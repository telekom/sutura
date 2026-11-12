"""Hosted formatting/export only; source binding is filled after the coordinator freezes it."""

import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(*args, **kwargs):
    return subprocess.check_output(["git", *args], **kwargs)


def write(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def manifest():
    result = {}
    for name in TRACKED:
        path = Path(name)
        if not path.exists() and not path.is_symlink():
            result[name] = {"missing": True}
            continue
        mode = path.lstat().st_mode
        data = os.fsencode(os.readlink(path)) if stat.S_ISLNK(mode) else path.read_bytes()
        result[name] = {"mode": mode, "sha256": sha(data)}
    return result


def run(label, command):
    print(label, command, flush=True)
    with (OUT / (label + ".log")).open("wb") as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=False)
    write(label + ".json", {"command": command, "exit": result.returncode,
                            "log_sha256": sha((OUT / (label + ".log")).read_bytes())})
    if result.returncode:
        raise RuntimeError(f"{label} failed: {result.returncode}; see retained log")


assert os.environ.get("GITHUB_ACTIONS") == "true", "hosted execution only"
os.environ["GIT_OPTIONAL_LOCKS"] = "0"
os.environ["INSTA_UPDATE"] = "no"
plan_path = Path(sys.argv[1])
PLAN = json.loads(plan_path.read_text())
for key in ("source_sha", "source_tree"):
    assert isinstance(PLAN[key], str) and re.fullmatch(r"[a-f0-9]{40}", PLAN[key]), "unfrozen source binding"
assert os.environ["SOURCE_SHA"] == PLAN["source_sha"]
assert git("rev-parse", "HEAD").decode().strip() == PLAN["source_sha"]
assert git("rev-parse", "HEAD^{tree}").decode().strip() == PLAN["source_tree"]
assert not git("status", "--porcelain"), "source checkout must start clean"
OUT = Path("target/inbound-generation")
OUT.mkdir(parents=True, exist_ok=False)
TRACKED = [os.fsdecode(p) for p in git("ls-files", "-z").split(b"\0") if p]
allowed = PLAN["allowed_paths"]
assert allowed == sorted(set(allowed)) and set(allowed) <= set(TRACKED)
assert all(not Path(p).is_absolute() and ".." not in Path(p).parts for p in allowed)
assert all(Path(p).suffix in {".rs", ".md", ".toml", ".yml", ".yaml", ".py", ".nix", ".sh"} for p in allowed)
assert set(PLAN["nix_paths"]) <= set(allowed)
assert all(p.endswith(".nix") for p in PLAN["nix_paths"])
index = Path(git("rev-parse", "--git-path", "index").decode().strip())
index_before = index.read_bytes()
entries_before = git("ls-files", "--stage", "-z")
before = manifest()
write("source-files.json", before)
write("allowlist.json", allowed)
(OUT / "source-index.z").write_bytes(entries_before)
(OUT / "index-before.bin").write_bytes(index_before)
write("binding.json", {
    "source_sha": PLAN["source_sha"], "source_tree": PLAN["source_tree"],
    "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"], "run_id": os.environ["GITHUB_RUN_ID"],
    "plan_sha256": sha(plan_path.read_bytes()), "runner_sha256": sha(Path(__file__).read_bytes()),
    "toolchain_sha256": sha(Path("devco/rust-toolchain-nightly.toml").read_bytes()),
    "flake_lock_sha256": sha(Path("flake.lock").read_bytes()),
})
success = False
try:
    run("format", [
        "nix", "develop", ".#checks.x86_64-linux.nextest", "--command", "bash", "-euo", "pipefail", "-c",
        "nix run .#just -- fmt",
    ])
    if PLAN["nix_paths"]:
        run("format-nix", ["nix", "fmt", "--", *PLAN["nix_paths"]])
    success = True
finally:
    after = manifest()
    changed = sorted(p for p in TRACKED if after[p] != before[p])
    unexpected = sorted(set(changed) - set(allowed))
    invalid_shape = sorted(p for p in changed if "missing" in after[p] or after[p].get("mode") != before[p]["mode"])
    untracked = [os.fsdecode(p) for p in git("ls-files", "--others", "--exclude-standard", "-z").split(b"\0") if p]
    write("output-files.json", after)
    (OUT / "generated.patch").write_bytes(git("diff", "--binary", "--full-index", "HEAD"))
    (OUT / "generated-files.zlist").write_bytes(b"".join(os.fsencode(p) + b"\0" for p in changed))
    generated = {p: after[p] for p in changed}
    write("generated-files.json", generated)
    with tarfile.open(OUT / "generated-files.tar.gz", "w:gz") as archive:
        for p in changed:
            if not after[p].get("missing") and not Path(p).is_symlink():
                archive.add(p, arcname=p, recursive=False)
    output_tree = None
    if not unexpected and not invalid_shape and not untracked:
        with tempfile.TemporaryDirectory() as directory:
            env = os.environ | {"GIT_INDEX_FILE": str(Path(directory) / "index")}
            subprocess.run(["git", "read-tree", "HEAD"], env=env, check=True)
            subprocess.run(["git", "add", "-u"], env=env, check=True)
            output_tree = git("write-tree", env=env).decode().strip()
    entries_after = git("ls-files", "--stage", "-z")
    (OUT / "output-index.z").write_bytes(entries_after)
    bound = git("rev-parse", "HEAD").decode().strip() == PLAN["source_sha"] and git("rev-parse", "HEAD^{tree}").decode().strip() == PLAN["source_tree"]
    index_after = index.read_bytes()
    (OUT / "index-after.bin").write_bytes(index_after)
    accepted = success and bound and not unexpected and not invalid_shape and not untracked and entries_after == entries_before
    write("result.json", {
        "source_sha": PLAN["source_sha"], "source_tree": PLAN["source_tree"], "output_tree": output_tree,
        "accepted_generation": accepted, "commands_succeeded": success, "source_binding_unchanged": bound,
        "changed_paths": changed, "unexpected_paths": unexpected, "invalid_shapes": invalid_shape, "untracked": untracked,
        "staged_entries_unchanged": entries_after == entries_before,
        "index_raw_bytes_equal": index_after == index_before,
        "index_before_sha256": sha(index_before), "index_after_sha256": sha(index_after),
        "patch_sha256": sha((OUT / "generated.patch").read_bytes()),
        "archive_sha256": sha((OUT / "generated-files.tar.gz").read_bytes()),
        "scope": "formatting only; no API, snapshot, runtime, mutation, default-feature or validation verdict",
    })
    assert accepted, "generation failed or exceeded reviewed scope; inspect retained result and patch"

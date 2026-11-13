"""Hosted-only lock for an isolated workspace exposing the vendored test binary."""

import hashlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

OUT = Path("/tmp/oracle-export")
COPY = Path("/tmp/query-vendor-lock")


def git(*args):
    return subprocess.check_output(["git", *args])


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(root, label):
    records = []
    for name in names:
        path = root / name
        symlink = path.is_symlink()
        data = os.fsencode(os.readlink(path)) if symlink else path.read_bytes()
        mode = (
            "120000"
            if symlink
            else "100755"
            if path.stat().st_mode & 0o111
            else "100644"
        )
        records.append(
            {
                "path": name,
                "mode": mode,
                "symlink": symlink,
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        )
    (OUT / f"query-vendor-{label}-files.json").write_text(
        json.dumps(records, indent=2) + "\n"
    )
    return records


source, tree = os.environ["SOURCE_SHA"], os.environ["SOURCE_TREE"]
assert re.fullmatch(r"[a-f0-9]{40}", source) and re.fullmatch(r"[a-f0-9]{40}", tree)
assert git("rev-parse", "HEAD").decode().strip() == source
assert git("rev-parse", "HEAD^{tree}").decode().strip() == tree
assert not git("status", "--porcelain")
assert not COPY.exists()
index = Path(git("rev-parse", "--git-path", "index").decode().strip())
index_before = digest(index)
names = [os.fsdecode(p) for p in git("ls-files", "-z").split(b"\0") if p]
source_before = manifest(Path.cwd(), "source-before")
for name in names:
    original, copied = Path(name), COPY / name
    copied.parent.mkdir(parents=True, exist_ok=True)
    if original.is_symlink():
        copied.symlink_to(original.readlink())
    else:
        shutil.copy2(original, copied)
copy_before = manifest(COPY, "copy-before")
assert source_before == copy_before
cargo = COPY / "Cargo.toml"
text = cargo.read_text()
assert text.count("members = [") == 1 and text.count('exclude = ["vendor"]') == 1
excluded = sorted(
    str(p.relative_to(COPY))
    for p in (COPY / "vendor").iterdir()
    if p.is_dir() and p.name != "datafusion-distributed"
)
text = text.replace("members = [", 'members = [\n  "vendor/datafusion-distributed",', 1)
text = text.replace('exclude = ["vendor"]', "exclude = " + json.dumps(excluded), 1)
cargo.write_text(text)
(OUT / "query-vendor-workspace-Cargo.toml").write_bytes(cargo.read_bytes())
try:
    with (OUT / "query-vendor-lock.log").open("wb") as output:
        result = subprocess.run(
            [
                "nix",
                "run",
                ".#cargo",
                "--",
                "metadata",
                "--format-version",
                "1",
                "--manifest-path",
                str(cargo),
            ],
            stdout=output,
            stderr=subprocess.STDOUT,
            check=False,
        )
    (OUT / "query-vendor-lock.exit").write_text(f"{result.returncode}\n")
finally:
    source_after = manifest(Path.cwd(), "source-after")
    copy_after = manifest(COPY, "copy-after")
    assert source_before == source_after and digest(index) == index_before
    assert not git("status", "--porcelain")
    assert {
        str(p.relative_to(COPY))
        for p in COPY.rglob("*")
        if p.is_file() or p.is_symlink()
    } == set(names)
    (OUT / "query-vendor-source-index.sha256").write_text(index_before + "\n")
assert result.returncode == 0, result.returncode
changed = [a["path"] for a, b in zip(copy_before, copy_after, strict=True) if a != b]
assert set(changed) <= {"Cargo.toml", "Cargo.lock"}, changed
(OUT / "query-vendor-Cargo.lock").write_bytes((COPY / "Cargo.lock").read_bytes())
(OUT / "query-vendor-result.json").write_text(
    json.dumps(
        {
            "source": source,
            "source_tree": tree,
            "source_entries": len(names),
            "copy_changed_paths": changed,
            "source_unchanged": True,
            "source_index_unchanged": True,
            "workspace_sha256": digest(cargo),
            "lock_sha256": digest(COPY / "Cargo.lock"),
            "scope": "isolated vendor-test workspace metadata and lock only; no test or generation credit",
        },
        indent=2,
    )
    + "\n"
)

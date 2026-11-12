"""Pinned hosted generation for the execution-record candidate; no mutation proof."""

import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

OUT = Path("/tmp/oracle-export")
ALLOWED = {
    "Cargo.lock",
    "crates/sutura-app/tests/golden/service.rs",
    "crates/sutura-app/tests/support/support.rs",
    "crates/sutura-cli/Cargo.toml",
    "crates/sutura-cli/src/serve/worker/partition/tests.rs",
    "crates/sutura-cli/src/serve/worker/partition/tests/native_bigquery.rs",
    "crates/sutura-cli/tests/snapshots/example_prompt_glossary.snap",
    "crates/sutura-cli/tests/snapshots/example_prompt_without_prose.snap",
    "crates/sutura-domain/src/pinned/tests.rs",
    "crates/sutura-exec-bigquery/Cargo.toml",
    "crates/sutura-exec-bigquery/tests/declared_principal/split.rs",
    "crates/sutura-exec-datafusion/src/combine/across/receipts.rs",
    "crates/sutura-exec-datafusion/src/combine/across/receipts/tests.rs",
    "devco/ignored-tests",
    "nix/multi-node-postgres.nix",
    "vendor/datafusion-distributed/src/completion_chain_tests.rs",
    "vendor/datafusion-distributed/src/lib.rs",
}


def git(*args):
    return subprocess.check_output(["git", *args])


def manifest(label):
    records = []
    for raw in git("ls-files", "--stage", "-z").split(b"\0"):
        if not raw:
            continue
        metadata, name = raw.split(b"\t", 1)
        index_mode, index_blob, stage = metadata.split()
        assert stage == b"0"
        path = Path(os.fsdecode(name))
        symlink = path.is_symlink()
        data = os.fsencode(os.readlink(path)) if symlink else path.read_bytes()
        mode = "120000" if symlink else "100755" if path.stat().st_mode & 0o111 else "100644"
        records.append({
            "path": str(path), "mode": mode, "symlink": symlink,
            "sha256": hashlib.sha256(data).hexdigest(),
            "index_mode": index_mode.decode(), "index_blob": index_blob.decode(),
        })
    (OUT / f"query-preparation-{label}-files.json").write_text(json.dumps(records, indent=2) + "\n")
    return records


def run(label, command):
    with (OUT / f"query-preparation-{label}.log").open("wb") as output:
        result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT, check=False)
    (OUT / f"query-preparation-{label}.exit").write_text(f"{result.returncode}\n")
    if label != "lock":
        print((OUT / f"query-preparation-{label}.log").read_text(), flush=True)
    assert result.returncode == 0, (label, result.returncode)


source = os.environ["SOURCE_SHA"]
tree = os.environ["SOURCE_TREE"]
assert re.fullmatch(r"[a-f0-9]{40}", source)
assert re.fullmatch(r"[a-f0-9]{40}", tree)
assert git("rev-parse", "HEAD").decode().strip() == source
assert git("rev-parse", "HEAD^{tree}").decode().strip() == tree
assert not git("status", "--porcelain")
index = Path(git("rev-parse", "--git-path", "index").decode().strip())
index_before = hashlib.sha256(index.read_bytes()).hexdigest()
before = manifest("source")
(OUT / "query-preparation-allowlist.json").write_text(json.dumps(sorted(ALLOWED), indent=2) + "\n")
(OUT / "query-preparation-toolchain.sha256").write_text(hashlib.sha256(Path("devco/rust-toolchain-nightly.toml").read_bytes()).hexdigest() + "\n")
try:
    run("lock", ["nix", "run", ".#cargo", "--", "metadata", "--format-version", "1"])
    run("format-snapshots", [
        "nix", "develop", ".#checks.x86_64-linux.nextest", "--command", "bash", "-euo", "pipefail", "-c",
        'nix run .#just -- fmt\n'
        'rustfmt --edition 2024 vendor/datafusion-distributed/src/completion_chain_tests.rs\n'
        'export INSTA_UPDATE=always\n'
        'export CARGO_TARGET_DIR="$PWD/target/query-completion-prepare"\n'
        'nix run .#just -- test "binary(example) & test(the_prompt_for_the_example_catalog_is_pinned)"',
    ])
finally:
    after = manifest("output")
    assert hashlib.sha256(index.read_bytes()).hexdigest() == index_before
    (OUT / "query-preparation-index.sha256").write_text(index_before + "\n")
changed = [os.fsdecode(path) for path in git("diff", "--name-only", "-z").split(b"\0") if path]
assert set(changed) <= ALLOWED, changed
assert not git("ls-files", "--others", "--exclude-standard", "-z")
assert len(before) == len(after)
with tempfile.TemporaryDirectory() as directory:
    environment = os.environ | {"GIT_INDEX_FILE": str(Path(directory) / "index")}
    subprocess.run(["git", "read-tree", "HEAD"], env=environment, check=True)
    subprocess.run(["git", "add", "-u"], env=environment, check=True)
    output_tree = subprocess.check_output(["git", "write-tree"], env=environment, text=True).strip()
assert hashlib.sha256(index.read_bytes()).hexdigest() == index_before
(OUT / "query-preparation-result.json").write_text(json.dumps({
    "source": source, "source_tree": tree, "output_tree": output_tree,
    "changed_paths": changed, "real_index_unchanged": True,
    "scope": "pinned locks, formatting and both prompt snapshots; no mutation or validation credit",
}, indent=2) + "\n")

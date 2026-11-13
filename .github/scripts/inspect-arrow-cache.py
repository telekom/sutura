import hashlib
import json
import os
import re
import sqlite3
import subprocess
import tempfile
from pathlib import Path
from urllib.parse import quote

out = Path("/tmp/oracle-export")
locked = json.loads(Path("flake.lock").read_text())["nodes"]["arrow-adbc-src"]["locked"]
uri = f"github:apache/arrow-adbc/{locked['rev']}?narHash={quote(locked['narHash'], safe='')}"
(out / "arrow-cache-locked.json").write_text(json.dumps(locked, indent=2) + "\n")
(out / "nix-version.txt").write_text(
    subprocess.check_output(["nix", "--version"], text=True)
)
foreign_hash = "sha256-npnBOQekeyRWjNunspB2l3C9I5WtzR+DmoExDVOtTqs="


def snapshot(phase, env):
    cache = Path(env.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "nix"
    records = []
    for path in sorted(cache.glob("*.sqlite")):
        record = {
            "file": path.name,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "arrow_rows": [],
        }
        connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
        try:
            for (table,) in connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ):
                identifier = '"' + table.replace('"', '""') + '"'
                for row in connection.execute(f"SELECT * FROM {identifier}"):
                    text = " ".join(str(value) for value in row)
                    if locked["rev"] in text or "arrow-adbc" in text:
                        record["arrow_rows"].append(
                            {
                                "table": table,
                                "row_sha256": hashlib.sha256(text.encode()).hexdigest(),
                                "hashes": sorted(
                                    set(re.findall(r"sha256-[A-Za-z0-9+/]+=*", text))
                                ),
                                "revisions": sorted(
                                    set(re.findall(r"\b[0-9a-f]{40}\b", text))
                                ),
                            }
                        )
        finally:
            connection.close()
        records.append(record)
    (out / f"arrow-cache-{phase}.json").write_text(json.dumps(records, indent=2) + "\n")


def probe(name, env, command):
    try:
        result = subprocess.run(
            command, env=env, text=True, capture_output=True, timeout=300, check=False
        )
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or "", error.stderr or ""
        result = subprocess.CompletedProcess(
            command,
            124,
            stdout.decode() if isinstance(stdout, bytes) else stdout,
            stderr.decode() if isinstance(stderr, bytes) else stderr,
        )
    (out / f"arrow-cache-{name}.stdout").write_text(result.stdout)
    (out / f"arrow-cache-{name}.stderr").write_text(result.stderr)
    (out / f"arrow-cache-{name}.status").write_text(str(result.returncode) + "\n")
    print(name, result.returncode, result.stdout, result.stderr, flush=True)


# Actual locked evaluation runs before prefetch can seed metadata.
evaluation = [
    "nix",
    "eval",
    "--raw",
    ".#checks.x86_64-linux.adbc-driver-bigquery.drvPath",
]
prefetch = ["nix", "flake", "prefetch", "--json", uri]
for label, env in [
    ("original", os.environ.copy()),
    (
        "clean",
        os.environ | {"XDG_CACHE_HOME": tempfile.mkdtemp(prefix="arrow-final-cache-")},
    ),
]:
    snapshot(label + "-before", env)
    probe(label + "-eval-first", env, evaluation)
    snapshot(label + "-after-eval", env)
    probe(label + "-prefetch", env, prefetch)
    snapshot(label + "-after-prefetch", env)
    probe(label + "-eval-after-prefetch", env, evaluation)
    snapshot(label + "-final", env)

# export-subst applies to archives, not checkout-index.
repo = Path(tempfile.mkdtemp(prefix="arrow-raw-git-"))
raw_tree = Path(tempfile.mkdtemp(prefix="arrow-raw-tree-"))
subprocess.run(["git", "init", str(repo)], check=True, timeout=30)
subprocess.run(
    [
        "git",
        "-C",
        str(repo),
        "fetch",
        "--depth=1",
        "https://github.com/apache/arrow-adbc.git",
        locked["rev"],
    ],
    check=True,
    timeout=180,
)
subprocess.run(
    ["git", "-C", str(repo), "read-tree", "FETCH_HEAD"], check=True, timeout=30
)
subprocess.run(
    [
        "git",
        "-C",
        str(repo),
        "-c",
        "core.autocrlf=false",
        "checkout-index",
        "--all",
        "--prefix=" + str(raw_tree) + "/",
    ],
    check=True,
    timeout=30,
)
assert (
    subprocess.check_output(
        ["git", "-C", str(repo), "rev-parse", "FETCH_HEAD"], text=True
    ).strip()
    == locked["rev"]
)
raw_hash = subprocess.check_output(
    ["nix", "hash", "path", str(raw_tree)], text=True, timeout=60
).strip()
summary = {
    "commit": locked["rev"],
    "tree": subprocess.check_output(
        ["git", "-C", str(repo), "rev-parse", "FETCH_HEAD^{tree}"], text=True
    ).strip(),
    "locked_archive_hash": locked["narHash"],
    "observed_computed_hash": foreign_hash,
    "raw_git_hash": raw_hash,
    "raw_matches_computed_hash": raw_hash == foreign_hash,
}
(out / "arrow-raw-tree.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2), flush=True)

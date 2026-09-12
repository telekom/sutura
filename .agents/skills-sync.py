#!/usr/bin/env python3
"""Refresh every imported skill, and record what was imported.

One entry point (`pixi run skills-refresh`) so "are our skills current?" has an answer, and
`skills.lock.json` so "has anyone edited a mirror?" has one too. Without the lock, a local
edit to a mirrored skill is invisible: it still says `mirror` in its provenance while no
longer being one.

Sources are declared below. Each is fetched at a ref, copied to its local path, and hashed.
`cargo xtask check-skills` compares those hashes against what is on disk.

Offline or behind a proxy: `--offline` re-hashes what is already checked out and rewrites the
lock without fetching. Useful to adopt the lock for skills already imported by hand.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOCK = HERE / "skills.lock.json"

# Every imported skill, and where it came from. `catalog` is the tier: `active` skills are
# routed and discoverable, `library` skills are not.
SOURCES: list[dict[str, str]] = [
    # Upstream, ref, the path inside it, and where it lands under .agents/.
    *[
        {
            "name": name,
            "repo": "https://github.com/addyosmani/agent-skills",
            "ref": "5a5ea45e806f82273549fd85e60adb95d55f510d",
            "upstream_path": f"skills/{name}",
            "local_path": f"skill-library/{group}/{name}",
            "catalog": "library",
            "status": "mirror",
        }
        for group, name in [
            ("engineering", "api-and-interface-design"),
            ("engineering", "code-simplification"),
            ("engineering", "debugging-and-error-recovery"),
            ("engineering", "deprecation-and-migration"),
            ("engineering", "incremental-implementation"),
            ("engineering", "observability-and-instrumentation"),
            ("engineering", "performance-optimization"),
            ("engineering", "security-and-hardening"),
            ("engineering", "source-driven-development"),
            ("planning", "doubt-driven-development"),
            ("planning", "idea-refine"),
            ("planning", "planning-and-task-breakdown"),
            ("planning", "spec-driven-development"),
            ("project-management", "code-review-and-quality"),
            ("project-management", "documentation-and-adrs"),
            ("project-management", "shipping-and-launch"),
            ("git-ops", "ci-cd-and-automation"),
            ("git-ops", "git-workflow-and-versioning"),
            ("agent-system", "context-engineering"),
            ("testing", "test-driven-development"),
        ]
    ],
    # Adapted skills: fetched for comparison, never overwritten. Their local content is ours,
    # so the lock records the hash of OUR file and the ref we last compared against.
    {
        "name": "ponytail",
        "repo": "https://github.com/DietrichGebert/ponytail",
        "ref": "2ed6c52c9d7e5e56942508591085fd45dea277d3",
        "upstream_path": ".openclaw/skills/ponytail",
        "local_path": "skills/engineering/ponytail",
        "catalog": "active",
        "status": "adapted",
    },
    {
        "name": "oauth",
        "repo": "https://github.com/curityio/oauth-developer-skills",
        "ref": "d411f195ab0d03bc68de6b16504036b6f7533244",
        "upstream_path": "skills/oauth-api-developer",
        "local_path": "skills/engineering/oauth",
        "catalog": "active",
        "status": "adapted",
    },
    # Generated from a published document rather than copied from a repo.
    {
        "name": "ms-rust",
        "repo": "https://microsoft.github.io/rust-guidelines",
        "ref": "agents/all.txt",
        "upstream_path": "-",
        "local_path": "skills/engineering/ms-rust",
        "catalog": "active",
        "status": "generated",
        "generator": "skills/engineering/ms-rust/generate.py",
    },
]

PROVENANCE = "## Provenance"


def sha256_of(path: Path) -> str:
    """Hash of a skill's SKILL.md, normalised to LF so the platform cannot change it."""
    raw = path.read_bytes().replace(b"\r\n", b"\n")
    return hashlib.sha256(raw).hexdigest()


def fetch(repo: str, ref: str, into: Path) -> bool:
    """Shallow-clone `repo` at `ref`. False when the network says no."""
    try:
        subprocess.run(
            [
                "git",
                "clone",
                "--quiet",
                "--filter=blob:none",
                "--no-checkout",
                repo,
                str(into),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["git", "-C", str(into), "fetch", "--quiet", "--depth", "1", "origin", ref],
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["git", "-C", str(into), "checkout", "--quiet", ref],
            check=True,
            capture_output=True,
            text=True,
        )
        return True
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        print(
            f"    fetch failed: {detail.strip().splitlines()[-1] if detail.strip() else exc}"
        )
        return False


def refresh_mirror(entry: dict[str, str], clone: Path) -> str:
    """Copy upstream over the local mirror, preserving the provenance block."""
    src = clone / entry["upstream_path"]
    dst = HERE / entry["local_path"]
    if not (src / "SKILL.md").is_file():
        return "upstream path missing"

    keep = ""
    local_skill = dst / "SKILL.md"
    if local_skill.is_file():
        text = local_skill.read_text(encoding="utf-8").replace("\r\n", "\n")
        if PROVENANCE in text:
            after = text.split(PROVENANCE, 1)[1]
            # The block runs until the next heading.
            end = after.find("\n## ")
            keep = PROVENANCE + (after[:end] if end != -1 else after).rstrip() + "\n\n"

    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)

    text = (dst / "SKILL.md").read_text(encoding="utf-8").replace("\r\n", "\n")
    if keep:
        lines = text.split("\n")
        if lines and lines[0].strip() == "---":
            close = next(
                i for i, l in enumerate(lines[1:], start=1) if l.strip() == "---"
            )
            text = (
                "\n".join(lines[: close + 1])
                + "\n\n"
                + keep
                + "\n".join(lines[close + 1 :]).lstrip("\n")
            )
        else:
            text = keep + text
    with open(dst / "SKILL.md", "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text.rstrip("\n") + "\n")

    for f in dst.rglob("*"):
        if f.is_file() and f.suffix in (".md", ".json", ".txt", ".yaml", ".yml"):
            t = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
            with open(f, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(t)
    return "refreshed"


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--offline",
        action="store_true",
        help="re-hash what is checked out; do not fetch anything",
    )
    ap.add_argument("--only", help="limit to skills whose name contains this")
    args = ap.parse_args(argv)

    entries = [e for e in SOURCES if not args.only or args.only in e["name"]]
    locked: list[dict[str, object]] = []
    changed = 0

    for entry in entries:
        local = HERE / entry["local_path"] / "SKILL.md"
        note = "unchanged"

        if not args.offline and entry["status"] == "generated":
            gen = HERE / entry["generator"]
            out = subprocess.run(
                [sys.executable, str(gen)], capture_output=True, text=True, check=False
            )
            note = "generator ok" if out.returncode == 0 else "generator failed"
            print(
                f"  {entry['name']}: {out.stdout.strip().splitlines()[-1] if out.stdout.strip() else note}"
            )
        elif not args.offline and entry["status"] == "mirror":
            before = sha256_of(local) if local.is_file() else ""
            with tempfile.TemporaryDirectory() as tmp:
                clone = Path(tmp) / "src"
                if fetch(entry["repo"], entry["ref"], clone):
                    note = refresh_mirror(entry, clone)
                else:
                    note = "offline, kept local"
            after = sha256_of(local) if local.is_file() else ""
            if before != after:
                changed += 1
                note += " (content changed)"
            print(f"  {entry['name']}: {note}")
        else:
            # `adapted` skills are never overwritten: the local content is ours. The lock
            # records the ref we last compared against, so a future diff has a starting point.
            print(f"  {entry['name']}: {entry['status']}, not overwritten")

        if not local.is_file():
            print(f"    WARNING: {entry['local_path']}/SKILL.md is missing")
            continue

        locked.append(
            {
                "name": entry["name"],
                "catalog": entry["catalog"],
                "status": entry["status"],
                "local_path": entry["local_path"],
                "repo": entry["repo"],
                "ref": entry["ref"],
                "upstream_path": entry["upstream_path"],
                "sha256": sha256_of(local),
            }
        )

    if args.only:
        print("--only given: lock not rewritten (it would drop every other entry)")
        return 0

    payload = {
        "version": 1,
        "skills": sorted(locked, key=lambda s: (s["catalog"], s["name"])),
    }
    with open(LOCK, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(payload, fh, indent=2)
        fh.write("\n")
    print(f"{LOCK.name}: {len(locked)} skill(s), {changed} changed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

"""Trusted-base npm maintenance. The native fetcher trusts registry redirects and declared SRI;
ordinary CI still decides whether the pinned bytes install, build and pass the advisory gate.
"""

import base64
import copy
import json
import os
import re
import subprocess
import sys
from pathlib import Path

DECK = "slides/2026-introducing-sutura/"
MANIFEST = DECK + "package.json"
LOCK = DECK + "package-lock.json"
GENERATED = "nix/slides.nix"
PATHS = {MANIFEST, LOCK, GENERATED}
PACKAGE = r"(?:@[A-Za-z0-9_~-][A-Za-z0-9._~-]*/)?[A-Za-z0-9_~-][A-Za-z0-9._~-]*"
TARBALL = re.compile(
    r"https://registry\.npmjs\.org/"
    + PACKAGE
    + r"/-/[A-Za-z0-9_~-][A-Za-z0-9._~+-]*\.tgz"
)
SLOT = re.compile(rb'(?m)^  npmDepsHash = "(sha256-[A-Za-z0-9+/]{43}=)";$')


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def unique(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def json_object(data: bytes, limit: int) -> dict:
    require(len(data) <= limit, "JSON exceeds maintenance limit")
    result = json.loads(
        data.decode("utf-8"),
        object_pairs_hook=unique,
        parse_constant=lambda _: require(False, "non-finite JSON constant"),
    )
    require(isinstance(result, dict), "expected JSON object")
    return result


def sri(value: str, algorithm: str, size: int) -> bytes:
    require(
        isinstance(value, str) and value.startswith(algorithm + "-"), "unexpected SRI"
    )
    encoded = value[len(algorithm) + 1 :]
    decoded = base64.b64decode(encoded, validate=True)
    require(
        len(decoded) == size and base64.b64encode(decoded).decode("ascii") == encoded,
        "noncanonical SRI",
    )
    return value.encode("ascii")


def validate_graph(manifest_bytes: bytes, lock_bytes: bytes) -> bytes:
    # ponytail: this deck's v3 registry graph only; other lock forms need review.
    manifest = json_object(manifest_bytes, 64 * 1024)
    lock = json_object(lock_bytes, 2 * 1024 * 1024)
    require(
        type(lock.get("lockfileVersion")) is int and lock["lockfileVersion"] == 3,
        "only lockfile v3 is supported",
    )
    require("dependencies" not in lock, "legacy lock graph is unsupported")
    packages = lock.get("packages")
    require(
        isinstance(packages, dict) and 1 < len(packages) <= 4097, "invalid package map"
    )
    root = packages.get("")
    require(isinstance(root, dict), "missing lock root")
    require(
        "workspaces" not in manifest and "workspaces" not in root,
        "workspaces unsupported",
    )
    for field in (
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ):
        dependencies = manifest.get(field, {})
        require(isinstance(dependencies, dict), "invalid manifest dependencies")
        require(dependencies == root.get(field, {}), "manifest and lock root disagree")
        require(
            all(
                isinstance(spec, str)
                and re.fullmatch(
                    r"[A-Za-z0-9~^*<>=|+_ -][A-Za-z0-9~^*<>=|.+_ -]*", spec
                )
                for spec in dependencies.values()
            ),
            "unsupported direct dependency spec",
        )
    integrities = {}
    for key, entry in packages.items():
        if not key:
            continue
        require(
            re.fullmatch(
                r"node_modules/" + PACKAGE + r"(?:/node_modules/" + PACKAGE + r")*", key
            )
            is not None,
            "unsupported package path",
        )
        require(
            isinstance(entry, dict) and entry.get("link", False) is False,
            "linked package",
        )
        url = entry.get("resolved")
        require(
            isinstance(url, str) and TARBALL.fullmatch(url) is not None,
            "nonregistry tarball",
        )
        integrity = entry.get("integrity")
        sri(integrity, "sha512", 64)
        require(
            integrities.get(url, integrity) == integrity,
            "conflicting tarball integrities",
        )
        integrities[url] = integrity
    return lock_bytes


def template_slot(base: bytes, head: bytes) -> re.Match[bytes]:
    require(
        max(len(base), len(head)) <= 64 * 1024, "Nix template exceeds maintenance limit"
    )
    base_slots, head_slots = list(SLOT.finditer(base)), list(SLOT.finditer(head))
    require(len(base_slots) == len(head_slots) == 1, "expected one generated hash slot")
    require(
        SLOT.sub(b"<hash>", base) == SLOT.sub(b"<hash>", head), "non-hash Nix change"
    )
    slot = head_slots[0]
    sri(base_slots[0].group(1).decode("ascii"), "sha256", 32)
    sri(slot.group(1).decode("ascii"), "sha256", 32)
    return slot


def rewrite(base: bytes, head: bytes, new_hash: str) -> bytes:
    slot = template_slot(base, head)
    return head[: slot.start(1)] + sri(new_hash, "sha256", 32) + head[slot.end(1) :]


def ref(value: str) -> str:
    require(re.fullmatch(r"[0-9a-f]{40}", value) is not None, "invalid Git object id")
    return value


def git(*args: str) -> bytes:
    return subprocess.run(
        ["git", "--no-optional-locks", *args],
        env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"},
        check=True,
        stdout=subprocess.PIPE,
        timeout=60,
    ).stdout


def changes(raw: bytes, api: bytes) -> None:
    fields = raw.split(b"\0")
    require(fields.pop() == b"" and len(fields) % 2 == 0, "invalid Git diff")
    paths = set()
    for header, path in zip(fields[::2], fields[1::2], strict=True):
        words = header.split()
        require(
            len(words) == 5
            and words[:2] == [b":100644", b"100644"]
            and words[4] == b"M",
            "only modified regular files are supported",
        )
        name = path.decode("utf-8")
        require(name in PATHS and name not in paths, "unexpected changed path")
        paths.add(name)
    require(LOCK in paths, "lockfile must change")
    expected = {"modified\t" + path for path in paths}
    lines = api.decode("utf-8").splitlines()
    require(
        len(lines) == len(expected) and set(lines) == expected,
        "API and exact-head diff disagree",
    )


def blob(object_id: str, path: str, limit: int) -> bytes:
    tree = git("ls-tree", "-z", ref(object_id), "--", path)
    require(tree.endswith(b"\0") and tree.count(b"\0") == 1, "missing regular input")
    metadata, name = tree[:-1].split(b"\t", 1)
    require(metadata.split()[:2] == [b"100644", b"blob"], "nonregular input")
    require(name.decode("utf-8") == path, "unexpected tree path")
    spec = object_id + ":" + path
    require(
        int(git("cat-file", "-s", spec)) <= limit, "Git input exceeds maintenance limit"
    )
    return git("cat-file", "blob", spec)


def prepare(base: str, head: str, api: Path, out: Path) -> None:
    ancestor = ref(git("merge-base", ref(base), ref(head)).decode("ascii").strip())
    changes(
        git(
            "diff", "--no-ext-diff", "--no-renames", "--raw", "-z", ancestor, head, "--"
        ),
        api.read_bytes(),
    )
    merged = ref(git("merge-tree", "--write-tree", base, head).decode("ascii").strip())
    manifest = blob(merged, MANIFEST, 64 * 1024)
    lock = blob(merged, LOCK, 2 * 1024 * 1024)
    lock = validate_graph(manifest, lock)
    base_nix = blob(base, GENERATED, 64 * 1024)
    head_nix = blob(head, GENERATED, 64 * 1024)
    # Refuse a branch needing non-hash Nix changes before any network fetch.
    template_slot(base_nix, head_nix)
    out.mkdir()
    for name, data in (
        ("package-lock.json", lock),
        ("base.nix", base_nix),
        ("head.nix", head_nix),
    ):
        (out / name).write_bytes(data)


def finish(out: Path) -> None:
    raw = (out / "hash").read_bytes()
    require(len(raw) <= 128, "unexpected native hash output")
    new_hash = raw.removesuffix(b"\n").decode("ascii")
    head = (out / "head.nix").read_bytes()
    generated = rewrite((out / "base.nix").read_bytes(), head, new_hash)
    (out / "slides.nix").write_bytes(generated)
    print("changed=" + str(generated != head).lower())


def self_check() -> None:
    """One offline behavioral check; no downloader, credentials or subprocesses."""
    integrity = "sha512-" + base64.b64encode(bytes(64)).decode("ascii")
    url = "https://registry.npmjs.org/example/-/example-1.0.0.tgz"
    manifest = {"dependencies": {"example": "^1.0.0"}}
    lock = {
        "lockfileVersion": 3,
        "packages": {
            "": manifest,
            "node_modules/example": {
                "resolved": url,
                "integrity": integrity,
                "workspaces": ["lib"],
            },
        },
    }
    raw = json.dumps(lock, indent=4).encode() + b"\n"
    assert validate_graph(json.dumps(manifest).encode(), raw) == raw
    for field, value in (
        ("resolved", None),
        ("resolved", "file:example"),
        ("resolved", "git+https://github.com/example/example"),
        ("resolved", "https://registry.npmjs.org.evil.example/example.tgz"),
        ("resolved", "https://user@example.com/example.tgz"),
        ("resolved", url + "?query"),
        ("resolved", url.replace("/-/", "/../")),
        ("resolved", url.replace("https:", "http:")),
        ("integrity", None),
        ("integrity", "sha512-AA=="),
        ("link", True),
    ):
        bad = copy.deepcopy(lock)
        bad["packages"]["node_modules/example"][field] = value
        refuses(
            lambda bad=bad: validate_graph(
                json.dumps(manifest).encode(), json.dumps(bad).encode()
            )
        )
    bad = copy.deepcopy(lock)
    bad["packages"]["node_modules/../example"] = bad["packages"].pop(
        "node_modules/example"
    )
    refuses(
        lambda: validate_graph(json.dumps(manifest).encode(), json.dumps(bad).encode())
    )
    refuses(lambda: validate_graph(b"{}", raw))
    for bad_manifest in (
        {"dependencies": {"example": "file:example"}},
        {**manifest, "workspaces": ["lib"]},
    ):
        bad = copy.deepcopy(lock)
        bad["packages"][""] = bad_manifest
        refuses(
            lambda bad_manifest=bad_manifest, bad=bad: validate_graph(
                json.dumps(bad_manifest).encode(), json.dumps(bad).encode()
            )
        )
    refuses(lambda: json_object(b'{"key":1,"key":2}', 1024))
    refuses(lambda: json_object(b'{"key":NaN}', 1024))
    refuses(lambda: json_object(b"{}", 1))
    refuses(lambda: ref("--help"))
    old_hash = "sha256-" + base64.b64encode(bytes(32)).decode("ascii")
    new_hash = "sha256-" + base64.b64encode(b"1" * 32).decode("ascii")
    template = ('before\n  npmDepsHash = "' + old_hash + '";\nafter\n').encode()
    updated = rewrite(template, template, new_hash)
    assert updated == template.replace(old_hash.encode(), new_hash.encode())
    assert rewrite(template, updated, new_hash) == updated
    refuses(lambda: rewrite(template, updated + b"code", new_hash))
    refuses(lambda: rewrite(template, template * 2, new_hash))
    refuses(lambda: rewrite(template, template, new_hash + "\nextra"))
    diff = (
        b":100644 100644 "
        + b"0" * 40
        + b" "
        + b"1" * 40
        + b" M\0"
        + LOCK.encode()
        + b"\0"
    )
    api = ("modified\t" + LOCK + "\n").encode()
    changes(diff, api)
    refuses(lambda: changes(diff.replace(b":100644", b":120000"), api))
    refuses(lambda: changes(diff.replace(b" M\0", b" A\0"), api))
    refuses(lambda: changes(diff.replace(LOCK.encode(), b"flake.nix"), api))
    refuses(lambda: changes(diff, b"modified\tflake.nix\n"))
    print("dependabot-npm: graph, exact-head scope and hash-only rewrite checks passed")


def refuses(call) -> None:
    try:
        call()
    except ValueError:
        return
    raise AssertionError("unsafe maintenance input was accepted")


if __name__ == "__main__":
    if sys.argv[1:] == ["self-check"]:
        self_check()
    elif len(sys.argv) == 6 and sys.argv[1] == "prepare":
        prepare(sys.argv[2], sys.argv[3], Path(sys.argv[4]), Path(sys.argv[5]))
    elif len(sys.argv) == 3 and sys.argv[1] == "finish":
        finish(Path(sys.argv[2]))
    else:
        raise SystemExit(
            "usage: dependabot_npm.py self-check | prepare BASE HEAD API OUT | finish OUT"
        )

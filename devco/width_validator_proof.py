"""Hosted validator proof only; no database or capacity claim."""

import contextlib
import hashlib
import json
import runpy
import shutil
import subprocess
import tempfile
from pathlib import Path
from types import SimpleNamespace

BASE = "39fe9886db7480381153e8d0cd2c6245547b43ad"
HEAD = "cc413f608f15a23d204dd5721e99085bff68f7d8"
WITNESS = "revenue-by-segment-yearly"
FAILURE = WITNESS + ": ONE must refuse the stream-width witness as result_too_large"


def replay(namespace, corpus, one_answers):
    answer = {"outcome": "answer", "rows": 35, "ms": 0, "file": "answer"}
    refusal = {"outcome": "refusal", "code": "result_too_large", "ms": 0}
    statements = [
        ("query", "SELECT pg_export_snapshot()"),
        ("reader1", "SELECT * FROM customer WHERE customer.ctid < '(5536,0)'"),
        ("reader2", "SELECT * FROM customer WHERE customer.ctid >= '(5536,0)'"),
    ]
    venue = SimpleNamespace(
        query=SimpleNamespace(succeed=lambda command: None),
        ask=lambda topology, *args: (
            refusal if topology == "one" and not one_answers else answer
        ),
        say=lambda message: None,
        log_lines=lambda: 0,
        log_since=lambda mark: statements,
        statements=lambda lines: lines,
        agree=lambda *args: "",
    )
    run = namespace["run"]
    run.__globals__.update(
        start_service=lambda *args: None, stop_service=lambda *args: None
    )
    corpus.names = lambda: [WITNESS]
    result = run(venue, corpus, contextlib.nullcontext)
    assert result["questions"][WITNESS]["ranges"] == {
        "query": [],
        "reader1": [("customer", 0, 5536)],
        "reader2": [("customer", 5536, -1)],
    }
    return result["failures"]


with tempfile.TemporaryDirectory(prefix="width-validator-") as temporary:
    root = Path(temporary)
    assert (
        subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"]).decode().strip()
        == HEAD
    )
    assert subprocess.check_output(
        ["git", "rev-parse", "HEAD:nix/tpch"]
    ) == subprocess.check_output(["git", "rev-parse", BASE + ":nix/tpch"])
    shutil.copytree("nix/tpch", root / "corpus")
    (root / "data").mkdir()
    (root / "data/counts.json").write_text(
        json.dumps({"scale": 10, "counts": {"customer": 1500000, "part": 2000000}})
    )
    plan = {"corpus": str(root / "corpus"), "data": str(root / "data"), "scale": 10}
    namespaces = {}
    hashes = {}
    for label, revision in [("base", BASE), ("head", HEAD)]:
        source = subprocess.check_output(
            ["git", "show", revision + ":nix/tpch-across-nodes.py"]
        )
        path = root / (label + ".py")
        path.write_bytes(source)
        namespace = runpy.run_path(str(path))
        namespaces[label] = namespace
        hashes[label] = hashlib.sha256(source).hexdigest()
        assert replay(namespace, namespace["Corpus"](plan), False) == []
        expected = [] if label == "base" else [FAILURE]
        assert replay(namespace, namespace["Corpus"](plan), True) == expected
    acceptance = root / "corpus/acceptance.json"
    original = acceptance.read_bytes()
    try:
        changed = json.loads(original)
        changed["questions"].remove(WITNESS)
        changed["observed"].append(WITNESS)
        acceptance.write_text(json.dumps(changed))
        namespaces["base"]["Corpus"](plan)
        try:
            namespaces["head"]["Corpus"](plan)
        except AssertionError as cause:
            assert (
                str(cause)
                == "tpch-across-nodes: the required stream-width witness must stay listed"
            )
        else:
            raise AssertionError("head admitted a witness moved out of listed")
    finally:
        acceptance.write_bytes(original)
    assert replay(namespaces["head"], namespaces["head"]["Corpus"](plan), False) == []
    print(
        json.dumps(
            {
                "base": BASE,
                "head_tree": HEAD,
                "source_hashes": hashes,
                "verdict": "base permits missing differential; head rejects it; restored passes",
                "limit": "stdlib validator replay with fake answers and real range judging; no native capacity proof",
            },
            indent=2,
        )
    )

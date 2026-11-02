"""Measure how often a TPC-H-shaped corpus is refused, split over DuckDB and PostgreSQL.

`nix run .#tpch-refusal-rate -- --scale N --out DIR` generates TPC-H at scale factor N with DuckDB's
`dbgen`, keeps `lineitem` and `orders` in a DuckDB file (source `lake`) and moves the dimension
tables into the running PostgreSQL tier (source `crm`). It then serves `nix/tpch/catalog` once per
point in POINTS and asks every question in `nix/tpch/questions.json`, one at a time. `results.json`
and `summary.md` land in DIR, and the summary is appended to `$GITHUB_STEP_SUMMARY` when set.

Dispatch only (`.github/workflows/tpch-refusal-rate.yml`), never a gate. Exits 1 after writing its
outputs when any question failed for a reason that means the corpus or the deployment is wrong
rather than measured: a non-refusal HTTP failure, or a refusal code in CORPUS_ERRORS. A server
that never reports `listening` ends the run at once, with its log in DIR and no summary.

Limits: one client asks sequentially, so the latencies are single-flight and say nothing about
contention. `partsupp` is not loaded and no question needs it.
"""

import argparse
import json
import os
import queue
import secrets
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path

import duckdb

DIMENSION_TABLES = {
    "customer": ["c_custkey", "c_nationkey", "c_mktsegment"],
    "part": ["p_partkey", "p_mfgr", "p_brand", "p_type", "p_container"],
    "supplier": ["s_suppkey", "s_nationkey"],
    "nation": ["n_nationkey", "n_name", "n_regionkey"],
    "region": ["r_regionkey", "r_name"],
}

# The fact-side keys a federated join compares with a PostgreSQL key, which is BIGINT. Widened in
# DuckDB too, so the link's two types agree whatever integer width `dbgen` chose.
CROSS_KEYS = [
    ("lineitem", "l_partkey"),
    ("lineitem", "l_suppkey"),
    ("orders", "o_custkey"),
]

# Point (a) is what ships. Point (b) raises the two settings a federated answer is bounded by; the
# single-source row cap and `governance.top_row_ceiling` stay at their defaults in both.
POINTS = {
    "shipped-defaults": "",
    "large": "governance:\n  federated_row_ceiling: 1000000\nruntime:\n  working_set_max_bytes: 8589934592\n",
}

# A refusal with one of these codes says the corpus or the deployment is wrong, not that a bound held.
CORPUS_ERRORS = {
    "metric_unknown",
    "too_many_metrics",
    "duplicate_metric_name",
    "grain_not_supported",
    "dimension_not_permitted",
    "dimension_not_conformed",
    "dimension_not_filterable",
    "dimension_value_not_allowed",
    "duplicate_dimension",
    "too_many_dimensions",
    "too_many_filters",
    "time_range_too_long",
    "federation_link_type_mismatch",
    "source_unavailable",
    "source_refused",
    "credential_unavailable",
    "address_outside_domains",
    "bill_to_not_allowed",
    "no_billing_project",
    "remote_leg_not_certified",
    "workers_not_ready",
    "node_lost",
    "cross_model_ratio_without_shared_calendar",
    "cross_model_ratio_without_shared_dimension",
    "cross_model_ratio_link_ambiguous",
}

# Kept through the scrub: which driver file a host mounts is a property of the host.
KEPT = {"SUTURA_POSTGRES_ADBC_DRIVER", "SUTURA_DUCKDB_ADBC_DRIVER"}
STARTUP_SECONDS = 1800
CLIENT_SECONDS = 300


def generate(work: Path, scale: int) -> Path:
    lake = work / "lake.duckdb"
    lake.unlink(missing_ok=True)
    # The Python package's release build, not the Nix CLI: the CLI names itself by a git describe
    # for which no `tpch` extension is published, so `dbgen` cannot load there.
    script = ["INSTALL tpch", "LOAD tpch", f"CALL dbgen(sf = {scale})"]
    script += [
        f"ALTER TABLE {table} ALTER {column} SET DATA TYPE BIGINT"
        for table, column in CROSS_KEYS
    ]
    for table, columns in DIMENSION_TABLES.items():
        script.append(
            f"COPY (SELECT {', '.join(columns)} FROM {table}) TO '{work / table}.csv' (HEADER)"
        )
        script.append(f"DROP TABLE {table}")
    script += ["DROP TABLE partsupp", "CHECKPOINT"]
    with duckdb.connect(str(lake)) as connection:
        for statement in script:
            connection.execute(statement)
    return lake


def load_dimensions(work: Path, tier: dict) -> None:
    script = []
    for table, columns in DIMENSION_TABLES.items():
        definitions = ", ".join(
            f"{column} {'BIGINT' if column.endswith('key') else 'VARCHAR'} NOT NULL{' PRIMARY KEY' if i == 0 else ''}"
            for i, column in enumerate(columns)
        )
        script += [
            f"DROP TABLE IF EXISTS {table};",
            f"CREATE TABLE {table} ({definitions});",
            f"\\copy {table} FROM '{work / table}.csv' WITH (FORMAT csv, HEADER true)",
            f"ANALYZE {table};",
        ]
    psql = [
        "psql",
        "-h",
        tier["socket"],
        "-p",
        str(tier["port"]),
        "-U",
        tier["user"],
        "-d",
        tier["db"],
    ]
    subprocess.run(
        [*psql, "-q", "-v", "ON_ERROR_STOP=1"],
        input="\n".join(script),
        text=True,
        check=True,
    )


def discover_tier(work: Path) -> dict:
    exported = subprocess.run(
        ["sutura-postgres-tier", "credentials"],
        capture_output=True,
        text=True,
        check=True,
    )
    values = dict(
        line.removeprefix("export ").split("=", 1)
        for line in exported.stdout.splitlines()
        if line.startswith("export ")
    )
    service = json.loads(Path(".sutura-dev/endpoints.json").read_text())["services"][
        "postgres"
    ]
    password = work / "postgres.password"
    password.unlink(missing_ok=True)
    with os.fdopen(
        os.open(password, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w"
    ) as handle:
        handle.write(values["SUTURA_POSTGRES_TIER_PASSWORD"])
    return {
        "socket": service["host"],
        "port": int(service["port"]),
        "user": values["SUTURA_POSTGRES_TIER_USER"],
        "db": values["SUTURA_POSTGRES_TIER_DB"],
        "anchor": values["SUTURA_POSTGRES_TIER_CA"],
        "password_file": password,
    }


def settings(
    corpus: Path, work: Path, lake: Path, tier: dict, token: str, extra: str
) -> str:
    return (
        'server:\n  host: "127.0.0.1"\n  port: 0\n'
        'security:\n  identity: "single-user"\n'
        '  single_user_because: "a dispatch-only measurement asks its own generated data as one identity"\n'
        f'  access_token: "{token}"\n'
        'telemetry:\n  format: "bunyan"\n'
        'catalogs:\n  - name: "model"\n    kind: "markdown"\n'
        f'    dir: "{corpus / "catalog"}"\n    data_dir: "{work}"\n    version: "tpch"\n'
        'sources:\n  lake:\n    kind: "duckdb"\n'
        f'    database_file: "{lake}"\n    posture: "shared-service-user"\n'
        '  crm:\n    kind: "postgres"\n    host: "127.0.0.1"\n'
        f'    port: {tier["port"]}\n    database: "{tier["db"]}"\n    user: "{tier["user"]}"\n'
        f'    password_file: "{tier["password_file"]}"\n    transport_mode: "verified"\n'
        f'    transport_anchors: "{tier["anchor"]}"\n    posture: "shared-service-user"\n'
    ) + extra


def start(config_dir: Path, log_path: Path) -> tuple[subprocess.Popen, str]:
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("SUTURA") or key in KEPT
    }
    env.pop("RUST_LOG", None)
    env |= {"SUTURA_ENVIRONMENT": "development", "SUTURA_CONFIG_DIR": str(config_dir)}
    process = subprocess.Popen(
        ["sutura", "serve"],
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    bound: queue.Queue = queue.Queue()

    def pump() -> None:
        with log_path.open("w") as log:
            for line in process.stdout:
                log.write(line)
                log.flush()
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if (
                    isinstance(event, dict)
                    and event.get("msg") == "listening"
                    and "bound" in event
                ):
                    bound.put(event["bound"])
        bound.put(None)

    threading.Thread(target=pump, daemon=True).start()
    try:
        address = bound.get(timeout=STARTUP_SECONDS)
    except queue.Empty:
        address = None
    if address is None:
        stop(process)
        sys.exit(
            f"tpch-refusal-rate: sutura serve did not report listening; see {log_path}"
        )
    return process, address


def stop(process: subprocess.Popen) -> None:
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=60)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def ask(opener, address: str, token: str, body: dict) -> dict:
    request = urllib.request.Request(
        f"http://{address}/v1/query",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    started = time.monotonic()
    try:
        with opener.open(request, timeout=CLIENT_SECONDS) as response:
            status, raw = response.status, response.read()
    except urllib.error.HTTPError as error:
        status, raw = error.code, error.read()
    except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
        return {
            "status": None,
            "outcome": "error",
            "detail": str(error),
            "ms": elapsed(started),
        }
    ms = elapsed(started)
    try:
        payload = json.loads(raw)
    except ValueError:
        payload = None
    outcome = payload.get("outcome") if isinstance(payload, dict) else None
    if outcome == "answer":
        return {
            "status": status,
            "outcome": "answer",
            "rows": len(payload.get("rows", [])),
            "ms": ms,
        }
    if outcome == "refusal":
        reason = payload["reason"]
        return {
            "status": status,
            "outcome": "refusal",
            "code": reason["code"],
            "detail": reason["detail"],
            "ms": ms,
        }
    return {
        "status": status,
        "outcome": "error",
        "detail": raw[:500].decode(errors="replace"),
        "ms": ms,
    }


def elapsed(started: float) -> float:
    return round((time.monotonic() - started) * 1000, 1)


def failed(result: dict) -> bool:
    return result["outcome"] == "error" or result.get("code") in CORPUS_ERRORS


def percentile(values: list, fraction: float):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(fraction * len(ordered)))]


def tally(results: list) -> dict:
    refused = [r for r in results if r["outcome"] == "refusal"]
    latencies = [r["ms"] for r in results]
    return {
        "questions": len(results),
        "answered": sum(r["outcome"] == "answer" for r in results),
        "refused": len(refused),
        "refusal_rate": round(len(refused) / len(results), 4) if results else None,
        "refusals_by_code": dict(Counter(r["code"] for r in refused).most_common()),
        "errors": [r["name"] for r in results if failed(r)],
        "latency_ms": {
            "p50": percentile(latencies, 0.5),
            "p90": percentile(latencies, 0.9),
            "max": max(latencies),
        },
    }


def cell(result: dict) -> str:
    if result["outcome"] == "answer":
        return f"answer, {result['rows']} rows, {result['ms']:.0f} ms"
    if result["outcome"] == "refusal":
        return f"`{result['code']}`, {result['ms']:.0f} ms"
    return f"error {result['status']}, {result['ms']:.0f} ms"


def summary(scale: int, report: dict) -> str:
    lines = [
        f"## TPC-H refusal rate, SF{scale}",
        "",
        "| point | path | questions | answered | refused | rate | errors | p50 ms | p90 ms | max ms |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for point, by_path in report["points"].items():
        for path, t in by_path.items():
            latency = t["latency_ms"]
            lines.append(
                f"| {point} | {path} | {t['questions']} | {t['answered']} | {t['refused']} | {t['refusal_rate']:.1%} "
                f"| {len(t['errors'])} | {latency['p50']:.0f} | {latency['p90']:.0f} | {latency['max']:.0f} |"
            )
    lines += [
        "",
        "| point | path | refusal code | count |",
        "| --- | --- | --- | --- |",
    ]
    for point, by_path in report["points"].items():
        for path, t in by_path.items():
            lines += [
                f"| {point} | {path} | `{code}` | {n} |"
                for code, n in t["refusals_by_code"].items()
            ]
    points = list(report["points"])
    lines += [
        "",
        f"| question | path | {' | '.join(points)} |",
        "| --- | --- |" + " --- |" * len(points),
    ]
    for name, question in report["questions"].items():
        lines.append(
            f"| {name} | {question['path']} | {' | '.join(cell(question[p]) for p in points)} |"
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--corpus",
        type=Path,
        required=True,
        help="directory holding catalog/ and questions.json",
    )
    parser.add_argument("--scale", type=int, required=True, help="TPC-H scale factor")
    parser.add_argument(
        "--out", type=Path, required=True, help="where results.json and summary.md land"
    )
    args = parser.parse_args()

    corpus = args.corpus.resolve()
    out = args.out.resolve()
    work = Path(".sutura-dev", f"tpch-sf{args.scale}").resolve()
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    questions = json.loads((corpus / "questions.json").read_text())

    started = time.monotonic()
    lake = generate(work, args.scale)
    tier = discover_tier(work)
    load_dimensions(work, tier)
    print(
        f"tpch-refusal-rate: SF{args.scale} loaded in {time.monotonic() - started:.0f} s",
        flush=True,
    )

    token = secrets.token_urlsafe(32)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    report = {
        "scale": args.scale,
        "points": {},
        "questions": {q["name"]: {"path": q["path"]} for q in questions},
    }
    for point, extra in POINTS.items():
        config_dir = work / point
        config_dir.mkdir(exist_ok=True)
        (config_dir / "base.yaml").write_text(
            settings(corpus, work, lake, tier, token, extra)
        )
        process, address = start(config_dir, out / f"serve-{point}.log")
        results = []
        try:
            for question in questions:
                result = {
                    "name": question["name"],
                    **ask(opener, address, token, question["body"]),
                }
                results.append(result)
                report["questions"][question["name"]][point] = result
                print(
                    f"tpch-refusal-rate: {point} {question['name']}: {cell(result)}",
                    flush=True,
                )
        finally:
            stop(process)
        by_path = {"all": tally(results)}
        for path in ("single", "federated"):
            by_path[path] = tally(
                [
                    r
                    for r, q in zip(results, questions, strict=True)
                    if q["path"] == path
                ]
            )
        report["points"][point] = by_path

    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    text = summary(args.scale, report)
    (out / "summary.md").write_text(text)
    if step_summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(step_summary, "a") as handle:
            handle.write(text)
    errors = sorted(
        {
            name
            for by_path in report["points"].values()
            for name in by_path["all"]["errors"]
        }
    )
    if errors:
        print(
            f"tpch-refusal-rate: corpus or deployment errors: {', '.join(errors)}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

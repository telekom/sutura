#!/usr/bin/env python3
"""Run the corpus parent; it forks one fresh measured child per case."""

import argparse
import json
import subprocess
import sys

RECORD_KEYS = {
    "status",
    "case",
    "topology",
    "ceiling",
    "peak",
    "reserved",
    "rss_baseline",
    "rss",
    "ratio",
    "answered",
    "refused",
    "failed",
}
STATUSES = ("ok", "refused", "failed")
TOPOLOGIES = ("one_source", "two_source")
PRODUCER_TIMEOUT_SECONDS = 30 * 60


def valid_record(record):
    if not isinstance(record, dict) or set(record) != RECORD_KEYS:
        return False
    if record["status"] not in STATUSES or record["topology"] not in TOPOLOGIES:
        return False
    if not isinstance(record["case"], str):
        return False
    try:
        case_topology, case_index = record["case"].split(":", maxsplit=1)
        case_index = int(case_index)
    except (ValueError, TypeError):
        return False
    if case_topology != record["topology"] or case_index < 0:
        return False
    if not all(
        type(record[key]) is int
        for key in (
            "ceiling",
            "peak",
            "reserved",
            "rss_baseline",
            "rss",
            "answered",
            "refused",
            "failed",
        )
    ):
        return False
    if record["ceiling"] <= 0 or record["peak"] < 0 or record["reserved"] != 0:
        return False
    if (
        record["peak"] > record["ceiling"]
        or record["rss"] < record["rss_baseline"]
        or record["rss"] <= 0
    ):
        return False
    ratio = record["ratio"]
    if ratio != {"numerator": record["peak"], "denominator": record["rss"]}:
        return False
    census = (record["answered"], record["refused"], record["failed"])
    wanted = {"ok": (1, 0, 0), "refused": (0, 1, 0), "failed": (0, 0, 1)}
    return census == wanted[record["status"]]


def failure_summary(records, expected, expected_failed, producer_failed, invalid):
    cases = [record["case"] for record in records]
    unique = set(cases)
    by_status = {
        status: sum(record["status"] == status for record in records)
        for status in STATUSES
    }
    expected_per_topology = (
        expected // len(TOPOLOGIES) if type(expected) is int else None
    )
    complete_cases = (
        expected_per_topology is not None and expected % len(TOPOLOGIES) == 0
    )
    if complete_cases:
        for topology in TOPOLOGIES:
            indices = sorted(
                int(record["case"].split(":", maxsplit=1)[1])
                for record in records
                if record["topology"] == topology
            )
            complete_cases = complete_cases and indices == list(
                range(expected_per_topology)
            )
    complete = (
        not producer_failed
        and not invalid
        and expected is not None
        and expected > 0
        and len(records) == expected
        and len(unique) == expected
        and complete_cases
        and by_status["failed"] == expected_failed
    )
    return complete, by_status, len(cases) - len(unique)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    if not arguments.command:
        parser.error("provide a measurement command")

    try:
        completed = subprocess.run(
            arguments.command,
            text=True,
            capture_output=True,
            check=False,
            timeout=PRODUCER_TIMEOUT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        if isinstance(error, subprocess.TimeoutExpired) and error.stderr:
            print(error.stderr, file=sys.stderr, end="")
        print(
            json.dumps(
                {
                    "status": "aggregate",
                    "records": 0,
                    "expected": None,
                    "ok": 0,
                    "refused": 0,
                    "failed": 0,
                    "duplicates": 0,
                    "failures": 1,
                },
                separators=(",", ":"),
            )
        )
        return 1
    if completed.stderr:
        print(completed.stderr, file=sys.stderr, end="")
    records = []
    invalid = False
    expected = None
    expected_failed = None
    for line in completed.stdout.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(record, dict):
            continue
        if record.get("status") == "manifest":
            if expected is not None or type(record.get("expected")) is not int:
                invalid = True
                continue
            if type(record.get("expected_failed")) is not int:
                invalid = True
                continue
            expected = record["expected"]
            expected_failed = record["expected_failed"]
            if expected <= 0 or expected_failed < 0 or expected_failed > expected:
                invalid = True
            continue
        if record.get("topology") not in TOPOLOGIES:
            continue
        if not valid_record(record):
            invalid = True
            continue
        records.append(record)
        print(json.dumps(record, separators=(",", ":")))
    producer_failed = completed.returncode != 0
    complete, counts, duplicates = failure_summary(
        records,
        expected,
        expected_failed,
        producer_failed,
        invalid,
    )
    factors = [
        (record["rss"] + record["peak"] - 1) // record["peak"]
        for record in records
        if record["peak"] > 0
    ]
    print(
        json.dumps(
            {
                "status": "aggregate",
                "records": len(records),
                "expected": expected,
                "ok": counts["ok"],
                "refused": counts["refused"],
                "failed": counts["failed"],
                "duplicates": duplicates,
                "max_pool_peak": max(
                    (record["peak"] for record in records), default=None
                ),
                "max_process_rss": max(
                    (record["rss"] for record in records), default=None
                ),
                "max_rss_per_pool_byte": max(factors, default=None),
                "failures": int(not complete),
            },
            separators=(",", ":"),
        )
    )
    return 0 if complete else 1


if __name__ == "__main__":
    raise SystemExit(main())

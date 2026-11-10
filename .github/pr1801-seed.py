"""Temporary CI-only fixture update; never retains credentials or resource names."""

import json
import os
import re
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

shared = os.environ["SUTURA_BQ_SPLIT_TABLE"]
only_a = os.environ["SUTURA_BQ_A_ONLY_TABLE"]
parts = shared.split(".")
assert len(parts) == 3 and all(re.fullmatch(r"[A-Za-z0-9_-]+", p) for p in parts)
project, dataset, table = parts
assert table == "shared" and only_a == f"{project}.{dataset}.a_only"
assert dataset.endswith("_split")
assert project == os.environ["SUTURA_BQ_PROJECT"]
base = f"https://bigquery.googleapis.com/bigquery/v2/projects/{project}"
token = os.environ.pop("SUTURA_BQ_FIXTURE_TOKEN")


def api(path, body=None):
    request = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(
            f"fixture API HTTP {error.code}; no response body retained"
        ) from None


def bindings(name):
    policy = api(
        f"/datasets/{dataset}/tables/{name}:getIamPolicy",
        {"options": {"requestedPolicyVersion": 3}},
    )
    return sorted(
        json.dumps({**b, "members": sorted(b["members"])}, sort_keys=True)
        for b in policy.get("bindings", [])
    )


def run(query, destination=None):
    config = {"query": query, "useLegacySql": False, "maximumBytesBilled": "1073741824"}
    if destination is not None:
        config.update(
            destinationTable=destination,
            createDisposition="CREATE_NEVER",
            writeDisposition="WRITE_TRUNCATE",
        )
    job_id = "sutura-split-fixture-" + uuid.uuid4().hex
    api(
        "/jobs",
        {
            "jobReference": {
                "projectId": project,
                "jobId": job_id,
                "location": location,
            },
            "configuration": {"query": config, "jobTimeoutMs": "600000"},
        },
    )
    end = time.monotonic() + 620
    while time.monotonic() < end:
        job = api(f"/jobs/{job_id}?location={location}")
        if job["status"]["state"] == "DONE":
            assert "errorResult" not in job["status"], "bounded fixture query failed"
            assert (
                int(job["statistics"]["query"].get("totalBytesBilled", 0)) <= 1073741824
            )
            return api(f"/queries/{job_id}?location={location}&maxResults=1")
        time.sleep(2)
    raise SystemExit("bounded fixture query timed out")


location = api(f"/datasets/{dataset}")["location"]
shared_before, only_a_before = bindings(table), bindings("a_only")
only_a_table = api(f"/datasets/{dataset}/tables/a_only")
assert len(shared_before) > 0 and len(only_a_before) > 0
run(
    Path("test-infra/pulumi/google/split-shared.sql").read_text(encoding="utf-8"),
    {"projectId": project, "datasetId": dataset, "tableId": table},
)
assert bindings(table) == shared_before, "shared table IAM changed"
assert bindings("a_only") == only_a_before, "A-only table IAM changed"
assert api(f"/datasets/{dataset}/tables/a_only") == only_a_table, "A-only table changed"
actual = api(f"/datasets/{dataset}/tables/{table}")
assert actual["schema"]["fields"] == [
    {"name": "id", "type": "STRING", "mode": "NULLABLE"}
]
assert int(actual["numRows"]) == 600000
assert int(actual["numBytes"]) < 256 * 1024 * 1024
proof = run(
    f"SELECT COUNT(*), COUNT(DISTINCT id), MIN(BYTE_LENGTH(id)), "
    f"MAX(BYTE_LENGTH(id)), COUNTIF(id IS NULL) FROM `{shared}`"
)
counts = [int(field["v"]) for field in proof["rows"][0]["f"]]
assert counts == [600000, 600000, 158, 158, 0], "live fixture shape differs from bound"
print("fixture: 600000 rows, 600000 distinct, 158 bytes per id, zero nulls")
print(
    "fixture: shared and A-only IAM unchanged; A-only table unchanged; STRING NULLABLE"
)
print("fixture: logical storage below 256 MiB; each query billed at most 1 GiB")

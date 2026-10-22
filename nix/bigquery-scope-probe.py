"""Which BigQuery calls Google accepts with a token scoped to bigquery.readonly (#1553).

A measurement, not a test. The CI service account mints two tokens from its own key under
the jwt-bearer grant: `bigquery` as the control and `bigquery.readonly` as the subject. No
`sub` claim, so no domain-wide delegation is involved. Each call the ADBC driver makes, or
a read-only driver would make, runs once per token and prints one line:

    PROBE <call> scope=<scope> status=<HTTP status or gRPC code> reason=<Google's reason>

A refusal is the answer being measured, so it exits 0 whatever Google says. It exits 1 when a
token cannot be minted, or tokeninfo reports it lacks the requested scope or also carries
`bigquery` or `cloud-platform`, because then no line measures what it names. A tokeninfo that
does not answer is printed and does not stop the run. A call that needs a job, table or stream it
could not create uses the control's, so each line measures that call alone.

It then sends DML (INSERT, UPDATE) and DDL (CREATE TABLE, DROP TABLE) through jobs.query on
two scratch tables with a unique suffix in the CI dataset. The control drops both in a
`finally`, and the run exits 1 when that cleanup fails, naming what is left.
"""

import json
import os
import sys
import uuid

import google.auth.exceptions
import google.auth.transport.requests
import requests
from google.api_core import exceptions
from google.cloud import bigquery_storage_v1
from google.cloud.bigquery_storage_v1 import types
from google.oauth2 import credentials, service_account

BIGQUERY = "https://www.googleapis.com/auth/bigquery"
SCOPES = {"bigquery": BIGQUERY, "bigquery.readonly": BIGQUERY + ".readonly"}
# A scope besides the one asked for that would also let BigQuery answer voids the measurement.
BROAD = {
    BIGQUERY,
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/cloud-platform.read-only",
}
API = "https://bigquery.googleapis.com/bigquery/v2/projects/"
QUERY = "SELECT x FROM UNNEST(GENERATE_ARRAY(1, 1000)) AS x"
TIMEOUT = 60


def env(name):
    value = os.environ.get(name, "")
    if not value:
        sys.exit(f"bigquery-scope-probe: {name} is unset or empty")
    return value


def mint(key, label, scope):
    minted = service_account.Credentials.from_service_account_file(key, scopes=[scope])
    try:
        minted.refresh(google.auth.transport.requests.Request())
    except google.auth.exceptions.RefreshError as refused:
        sys.exit(f"bigquery-scope-probe: could not mint the {label} token: {refused}")
    info = requests.post(
        "https://oauth2.googleapis.com/tokeninfo",
        data={"access_token": minted.token},
        timeout=TIMEOUT,
    )
    granted = info.json().get("scope", "") if info.ok else "unread"
    if info.ok and (
        scope not in granted.split() or BROAD & (set(granted.split()) - {scope})
    ):
        sys.exit(f"bigquery-scope-probe: the {label} token carries {granted!r}")
    print(f"PROBE tokeninfo scope={label} status={info.status_code} granted={granted}")
    return minted.token


def line(call, label, status, reason, message=""):
    print(f"PROBE {call} scope={label} status={status} reason={reason} {message[:200]}")


def rest_reason(body):
    error = body.get("error") or {}
    for item in error.get("details", []) + error.get("errors", []):
        if item.get("reason"):
            return item["reason"], error.get("message", "")
    return error.get("status", "-"), error.get("message", "")


def rest(call, label, token, method, path, **kwargs):
    answer = requests.request(
        method,
        API + path,
        headers={"Authorization": f"Bearer {token}"},
        timeout=TIMEOUT,
        **kwargs,
    )
    try:
        body = answer.json()
    except ValueError:
        body = {}
    reason, message = ("-", "") if answer.ok else rest_reason(body)
    line(call, label, answer.status_code, reason, message)
    return body if answer.ok else None


def grpc(call, label, run):
    try:
        out = run()
    except exceptions.GoogleAPICallError as refused:
        code = refused.grpc_status_code
        status = code.name if code else refused.code
        line(
            call,
            label,
            status,
            getattr(refused, "reason", None) or "-",
            refused.message,
        )
        return None
    line(call, label, "OK", "-")
    return out


def measure(project, label, token, control):
    asked = {"query": QUERY, "useLegacySql": False, "useQueryCache": False}
    queried = rest("jobs.query", label, token, "POST", f"{project}/queries", json=asked)
    rest(
        "jobs.query(dryRun)",
        label,
        token,
        "POST",
        f"{project}/queries",
        json={**asked, "dryRun": True},
    )
    job = (queried or {}).get("jobReference") or control.get("job")
    if job is None:
        line("dependent-calls", label, "skipped", "no-control-job")
        return {}
    jobs = f"{project}/jobs/{job['jobId']}"
    where = {"location": job.get("location")}
    rest(
        "jobs.getQueryResults",
        label,
        token,
        "GET",
        f"{project}/queries/{job['jobId']}",
        params={**where, "maxResults": 1, "timeoutMs": 30000},
    )
    got = rest("jobs.get", label, token, "GET", jobs, params=where)
    table = ((got or {}).get("configuration", {}).get("query", {})).get(
        "destinationTable"
    )
    path = (
        f"projects/{table['projectId']}/datasets/{table['datasetId']}/tables/{table['tableId']}"
        if table
        else control.get("table")
    )
    client = bigquery_storage_v1.BigQueryReadClient(
        credentials=credentials.Credentials(token)
    )
    session = path and grpc(
        "storage.CreateReadSession",
        label,
        lambda: client.create_read_session(
            parent=f"projects/{project}",
            read_session=types.ReadSession(
                table=path, data_format=types.DataFormat.AVRO
            ),
            max_stream_count=1,
            timeout=TIMEOUT,
        ),
    )
    stream = (
        session.streams[0].name
        if session and session.streams
        else control.get("stream")
    )
    if stream:
        grpc(
            "storage.ReadRows",
            label,
            lambda: sum(r.row_count for r in client.read_rows(stream, timeout=TIMEOUT)),
        )
    else:
        line("storage.*", label, "skipped", "no-control-table-or-stream")
    rest("jobs.cancel", label, token, "POST", f"{jobs}/cancel", params=where)
    insert = {"configuration": {"query": {"query": "SELECT 1", "useLegacySql": False}}}
    rest("jobs.insert", label, token, "POST", f"{project}/jobs", json=insert)
    insert["configuration"]["dryRun"] = True
    rest("jobs.insert(dryRun)", label, token, "POST", f"{project}/jobs", json=insert)
    return {"job": job, "table": path, "stream": stream}


def write(project, dataset, tokens):
    """DML and DDL through jobs.query on two scratch tables, dropped by the control after.

    The control creates the DML table, so the readonly INSERT has a target, and creates the
    DDL table when the readonly CREATE is refused, so the readonly DROP has one too.
    """

    def run(call, label, sql):
        asked = {"query": sql, "useLegacySql": False, "timeoutMs": 60000}
        path = f"{project}/queries"
        return rest(
            f"jobs.query({call})", label, tokens[label], "POST", path, json=asked
        )

    suffix = uuid.uuid4().hex[:12]
    dml, ddl = (
        f"`{project}.{dataset}.sutura_scope_probe_{t}_{suffix}`" for t in ("dml", "ddl")
    )
    try:
        run("DDL create", "bigquery", f"CREATE TABLE {dml} (x INT64)")
        for label in tokens:
            run("DML insert", label, f"INSERT INTO {dml} (x) VALUES (1)")
            run("DML update", label, f"UPDATE {dml} SET x = x + 1 WHERE TRUE")
        if (
            run("DDL create", "bigquery.readonly", f"CREATE TABLE {ddl} (x INT64)")
            is None
        ):
            run("setup DDL create", "bigquery", f"CREATE TABLE {ddl} (x INT64)")
        run("DDL drop", "bigquery.readonly", f"DROP TABLE {ddl}")
        run("DDL drop", "bigquery", f"DROP TABLE {dml}")
    finally:
        drop = "DROP TABLE IF EXISTS {}"
        left = [
            t for t in (dml, ddl) if run("cleanup", "bigquery", drop.format(t)) is None
        ]
        if left:
            names = ", ".join(t.rsplit(".", 1)[-1].strip("`") for t in left)
            sys.exit(
                f"bigquery-scope-probe: cleanup could not drop {names} in the CI dataset"
            )


def main():
    key = env("GOOGLE_APPLICATION_CREDENTIALS")
    project = env("SUTURA_BQ_PROJECT")
    dataset = env("SUTURA_BQ_DATASET")
    with open(key) as document:
        print(f"::add-mask::{json.load(document)['client_email']}")
    tokens = {label: mint(key, label, scope) for label, scope in SCOPES.items()}
    control = {}
    for label, token in tokens.items():
        control = measure(project, label, token, control)
    write(project, dataset, tokens)


if __name__ == "__main__":
    main()

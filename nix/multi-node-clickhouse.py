"""Fixed local-shard fleet acceptance, run only by the NixOS machine test."""

import csv
import json
import re
import shlex
from pathlib import Path

QUESTION = {
    "metrics": ["recurring_revenue"],
    "grain": "month",
    "range": {"start": "2026-06-01", "end": "2026-07-01"},
    "dimensions": ["region"],
}
COLUMNS = ["region", "period", "recurring_revenue"]
GOLDEN = {
    "analyst_a": [
        ["central", "2026-06-01", "51739"],
        ["east", "2026-06-01", "32598"],
        ["north", "2026-06-01", "42157"],
        ["south", "2026-06-01", "21203"],
        ["west", "2026-06-01", "49425"],
        ["null", "2026-06-01", "4999"],
    ],
    "analyst_b": [
        ["north", "2026-06-01", "42157"],
        ["null", "2026-06-01", "159964"],
    ],
}
PINNED = {
    "join_use_nulls": "1",
    "output_format_json_quote_denormals": "1",
    "output_format_json_quote_64bit_integers": "1",
    "output_format_json_quote_decimals": "1",
    "output_format_decimal_trailing_zeros": "1",
    "timeout_overflow_mode": "throw",
}


def main(config, query, reader1, reader2, db, idp, start_all, subtest, out):
    material = config["material"]
    addresses = config["addresses"]
    output = Path(out)

    def sql(statement, caller="sutura"):
        credential = f"--netrc-file {shlex.quote(f'{material}/curl-{caller}.netrc')}"
        command = (
            f"curl -fsS --max-time 120 {credential} --data-binary {shlex.quote(statement)} "
            "'http://127.0.0.1:8123/?default_format=JSONCompact'"
        )
        return db.succeed(command)

    def rows(statement, caller="sutura"):
        return json.loads(sql(statement, caller))["data"]

    def asking(caller="analyst_a"):
        authorization = ""
        if caller is not None:
            token = idp.succeed(
                f"{config['issuer']} mint {material}/issuer.key {config['kid']} "
                f"{shlex.quote(caller)} https://query sutura:metrics.ask"
            ).strip()
            authorization = f"-H 'Authorization: Bearer {token}'"
        return (
            f"curl -sS --max-time 150 --cacert {material}/ca.pem {authorization} "
            "-H 'Content-Type: application/json' "
            f"--data {shlex.quote(json.dumps(QUESTION))} https://query:8443/v1/query"
        )

    def ask(caller="analyst_a"):
        status = idp.succeed(
            f"{asking(caller)} -o /tmp/reply.json -w '%{{http_code}}'"
        ).strip()
        reply = json.loads(idp.succeed("cat /tmp/reply.json"))
        print("reply", caller, status, json.dumps(reply))
        return status, reply

    def answer(caller="analyst_a"):
        status, reply = ask(caller)
        assert status == "200" and reply.get("outcome") == "answer", (status, reply)
        assert reply["columns"] == COLUMNS and reply["rows"] == GOLDEN[caller], reply
        assert reply["executed_as"] == [
            {"source": "geo", "posture": "impersonation-at-source"},
            {"source": "local", "posture": "shared-service-user"},
        ], reply
        return reply

    def refused(code, status, caller="analyst_a"):
        actual, reply = ask(caller)
        assert actual == status and reply.get("reason", {}).get("code") == code, (
            actual,
            reply,
        )
        assert "rows" not in reply, reply

    def serving(service):
        query.succeed(f"systemctl start {service}")
        idp.wait_until_succeeds(
            f"curl -fsS --cacert {material}/ca.pem https://query:8443/health",
            timeout=120,
        )

    def readers(service="sutura"):
        for machine, name in [(reader1, "reader1"), (reader2, "reader2")]:
            machine.succeed(f"systemctl start {service}")
            query.wait_until_succeeds(
                f"curl -fsS --cacert {material}/ca.pem https://{name}:8443/health",
                timeout=120,
            )

    def checkpoint():
        sql("SYSTEM FLUSH LOGS")
        return int(rows("SELECT toUnixTimestamp64Micro(now64(6))")[0][0])

    def observed(label, since):
        sql("SYSTEM FLUSH LOGS")
        raw = sql(
            "SELECT toString(type) AS type, query_id, user, initial_user, "
            "toString(address) AS address, query_kind, query, read_rows, result_rows, "
            "Settings, exception_code, exception "
            "FROM system.query_log "
            f"WHERE toUnixTimestamp64Micro(event_time_microseconds) >= {since} "
            "AND type IN ('QueryStart', 'QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing') "
            "ORDER BY event_time_microseconds FORMAT JSONEachRow"
        )
        events = [json.loads(line) for line in raw.splitlines()]
        output.joinpath(f"clickhouse-{label}.json").write_text(
            json.dumps(events, indent=2)
        )
        return [
            event
            for event in events
            if event["address"].removeprefix("::ffff:") in addresses.values()
        ]

    def data(events):
        # Include failed and zero-row executions: a LIMIT 0 is the only metadata exception.
        return [
            event
            for event in events
            if "dim_customer_split" in event["query"]
            and "FROM system.parts" not in event["query"]
            and not re.search(r"\bLIMIT\s+0\b", event["query"], re.IGNORECASE)
        ]

    def no_data(label, since):
        events = observed(label, since)
        assert not data(events), (label, events)
        return events

    def check_settings(event):
        statement = event["query"]
        for name, value in PINNED.items():
            assert re.search(rf"\b{name}\s*=\s*'?{value}'?(?:\s|,|$)", statement), event
            assert (
                str(event["Settings"].get(name, defaults[event["user"]][name])) == value
            ), event
        deadline = re.search(r"\bmax_execution_time\s*=\s*([1-9]\d*)", statement)
        assert deadline and 0 < int(deadline[1]) <= 120, event
        assert 0 < float(event["Settings"].get("max_execution_time", "0")) <= 120, event

    def network_matrix():
        machines = {
            "query": query,
            "reader1": reader1,
            "reader2": reader2,
            "db": db,
            "idp": idp,
        }
        ports = {
            "query": 8443,
            "reader1": 8443,
            "reader2": 8443,
            "db": 8443,
            "idp": 9443,
        }
        allowed = {
            ("idp", "query"),
            ("query", "reader1"),
            ("query", "reader2"),
            ("query", "db"),
            ("query", "idp"),
            ("reader1", "db"),
            ("reader2", "db"),
        }
        matrix = []
        for source, machine in machines.items():
            for destination, port in ports.items():
                if source == destination:
                    continue
                status, text = machine.execute(f"nc -4 -z -w 2 {destination} {port}")
                matrix.append([source, destination, port, status, text])
                assert (status == 0) == ((source, destination) in allowed), matrix[-1]
        db.succeed("nc -4 -z -w 2 127.0.0.1 8123")
        for name, machine in machines.items():
            if name != "db":
                machine.fail("nc -4 -z -w 2 db 8123")
        output.joinpath("clickhouse-ipv4.json").write_text(json.dumps(matrix, indent=2))

    start_all()
    db.wait_for_unit("clickhouse.service")
    db.wait_until_succeeds("curl -fsS http://127.0.0.1:8123/ping")
    sql(
        "CREATE TABLE dim_customer_seed (customer_key Int64, customer_id String, "
        "segment String, region String) ENGINE = MergeTree ORDER BY customer_key"
    )
    db.succeed(
        f"curl -fsS --netrc-file {material}/curl-sutura.netrc --data-binary @{config['data']}/dim_customer.csv "
        "'http://127.0.0.1:8123/?query=INSERT%20INTO%20dim_customer_seed%20FORMAT%20CSVWithNames'"
    )
    sql("CREATE TABLE dim_customer_split AS dim_customer_seed")
    sql(
        "INSERT INTO dim_customer_split SELECT customer_key, customer_id, segment, region FROM dim_customer_seed CROSS JOIN numbers(30000)"
    )
    sql(
        "CREATE TABLE dim_region (region String, sales_area String) ENGINE = MergeTree ORDER BY region"
    )
    db.succeed(
        f"curl -fsS --netrc-file {material}/curl-sutura.netrc --data-binary @{config['data']}/dim_region.csv "
        "'http://127.0.0.1:8123/?query=INSERT%20INTO%20dim_region%20FORMAT%20CSVWithNames'"
    )
    assert list(
        map(
            int,
            rows("SELECT count(), uniqExact(customer_key) FROM dim_customer_split")[0],
        )
    ) == [1200000, 40]
    assert (
        int(
            rows(
                "SELECT sum(rows) FROM system.parts WHERE active AND database = 'default' AND table = 'dim_customer_split'"
            )[0][0]
        )
        == 1200000
    )
    defaults = {}
    for caller in ("sutura", *GOLDEN):
        names = ", ".join(f"'{name}'" for name in PINNED)
        defaults[caller] = dict(
            rows(
                f"SELECT name, value FROM system.settings WHERE name IN ({names})",
                caller,
            )
        )
    with Path(config["data"], "dim_customer.csv").open(newline="") as handle:
        customers = [
            [int(row["customer_key"]), row["region"]] for row in csv.DictReader(handle)
        ]
    references = {}
    for caller in GOLDEN:
        expected = (
            customers
            if caller == "analyst_a"
            else [row for row in customers if row[1] == "north"]
        )
        reference = rows(
            'SELECT customer_key AS "0_link", region FROM dim_customer_split GROUP BY customer_key, region ORDER BY customer_key, region',
            caller,
        )
        reference = [[int(key), region] for key, region in reference]
        assert reference == expected, (caller, reference, expected)
        buckets = rows(
            "SELECT cityHash64(COALESCE(customer_key, 0)) % 2 AS bucket, uniqExact(customer_key) FROM dim_customer_split GROUP BY bucket ORDER BY bucket",
            caller,
        )
        assert [int(row[0]) for row in buckets] == [0, 1] and all(
            int(row[1]) > 0 for row in buckets
        ), (caller, buckets)
        references[caller] = reference
    output.joinpath("clickhouse-references.json").write_text(
        json.dumps(references, indent=2)
    )
    idp.wait_for_unit("nginx.service")
    readers()
    serving("sutura-whole")
    whole = {}
    for caller in GOLDEN:
        with subtest(f"whole {caller} returns its exact ordered golden"):
            since = checkpoint()
            whole[caller] = answer(caller)
            events = observed(f"whole-{caller}", since)
            reads = [
                event
                for event in events
                if event["query_kind"] == "Select"
                and event["type"] == "QueryFinish"
                and "dim_customer_split" in event["query"]
                and not re.search(r"\bLIMIT\s+0\b", event["query"], re.IGNORECASE)
            ]
            assert reads and all(
                event["user"] == event["initial_user"] == caller for event in reads
            ), events
            for event in reads:
                check_settings(event)
    query.succeed("systemctl stop sutura-whole")
    serving("sutura")
    with subtest("all seven allowed and thirteen denied IPv4 role paths are exercised"):
        network_matrix()
    for caller in GOLDEN:
        with subtest(
            f"split {caller} matches whole and both readers fetch as that caller"
        ):
            since = checkpoint()
            split = answer(caller)
            assert (
                split["columns"] == whole[caller]["columns"]
                and split["rows"] == whole[caller]["rows"]
            ), (whole[caller], split)
            events = observed(f"split-{caller}", since)
            buckets = data(events)
            assert buckets, events
            assert not any(
                event["address"].removeprefix("::ffff:") == addresses["query"]
                for event in buckets
            ), events
            selects = [
                event
                for event in buckets
                if event["query_kind"] == "Select" and event["type"] == "QueryFinish"
            ]
            assert len(selects) == 2, events
            for name in ("reader1", "reader2"):
                found = [
                    event
                    for event in selects
                    if event["address"].removeprefix("::ffff:") == addresses[name]
                ]
                assert len(found) == 1 and int(found[0]["result_rows"]) > 0, (
                    caller,
                    name,
                    events,
                )
            for event in selects:
                assert (
                    event["type"] == "QueryFinish"
                    and event["user"] == event["initial_user"] == caller
                ), event
                assert "cityHash64(" in event["query"], event
                assert int(event["read_rows"]) > 0, event
                check_settings(event)
            wrappers = [
                event
                for event in buckets
                if event["query"].startswith("EXECUTE AS ")
                and event["type"] == "QueryFinish"
            ]
            assert len(wrappers) == 2 and all(
                event["user"] == "sutura"
                and re.match(rf'^EXECUTE AS ["`]?{caller}["`]?\s', event["query"])
                for event in wrappers
            ), events
            metadata = [
                event
                for event in events
                if "dim_customer_split" in event["query"]
                and re.search(r"\bLIMIT\s+0\b", event["query"], re.IGNORECASE)
            ]
            assert metadata and all(
                int(event["result_rows"]) == 0 for event in metadata
            ), events
            print("metadata separately counted", caller, len(metadata))
            receipts = [
                record for record in split["executions"] if record["kind"] == "sql"
            ]
            assert len(receipts) == 2, receipts
            pieces = []
            for record in receipts:
                assert (
                    record["source"] == "geo"
                    and record["dialect"] == "clickhouse"
                    and record["parameters"] == []
                ), record
                assert "cityHash64(" in record["sql"] and record["sql"].startswith(
                    f'EXECUTE AS "{caller}" '
                ), record
                piece = rows(record["sql"])
                assert piece, (caller, record)
                pieces.extend([[int(key), region] for key, region in piece])
            assert sorted(pieces) == references[caller] and len(pieces) == len(
                references[caller]
            ), (caller, pieces, references[caller])

    with subtest(
        "lost readers refuse without local bucket fallback and restart restores success"
    ):
        reader1.succeed("systemctl stop sutura")
        reader2.succeed("systemctl stop sutura")
        since = checkpoint()
        refused("node_lost", "503")
        no_data("lost", since)
        readers()
        answer()
    query.succeed("systemctl stop sutura")
    serving("sutura-unresolved")
    with subtest("unresolved readers refuse without local bucket fallback"):
        since = checkpoint()
        refused("source_unavailable", "503")
        no_data("unresolved", since)
    query.succeed("systemctl stop sutura-unresolved")
    for machine in (reader1, reader2):
        machine.succeed("systemctl stop sutura")
    readers("sutura-changed")
    serving("sutura")
    with subtest(
        "a changed reader catalog refuses re-certification before source data"
    ):
        since = checkpoint()
        refused("remote_leg_not_certified", "403")
        no_data("changed-catalog", since)
    for machine in (reader1, reader2):
        machine.succeed("systemctl stop sutura-changed")
    readers()
    answer()
    with subtest(
        "anonymous and own-service-user callers are refused before bucket data"
    ):
        since = checkpoint()
        status, reply = ask(None)
        assert (
            status == "401" and reply["code"] == "unauthorized" and "rows" not in reply
        ), (status, reply)
        refused("credential_unavailable", "403", "sutura")
        no_data("anonymous-service", since)
    for caller in ("analyst_ungranted", "analyst_readonly"):
        with subtest(f"{caller} is refused without fallback or dropped settings"):
            since = checkpoint()
            refused("source_refused", "403", caller)
            events = no_data(caller, since)
            expected_code = 497 if caller == "analyst_ungranted" else 164
            assert any(
                int(event["exception_code"]) == expected_code for event in events
            ), (caller, events)
    with subtest("missing and untrusted node certificates are refused"):
        since = checkpoint()
        endpoint = "https://reader1:8443/worker.WorkerService/ExecuteTask"
        request = f"curl -sS --max-time 10 --http2 --cacert {material}/ca.pem -X POST -H 'Content-Type: application/grpc' {endpoint}"
        query.fail(
            f"{request} --cert {material}/stranger.pem --key {material}/stranger.key"
        )
        status = query.succeed(f"{request} -o /dev/null -w '%{{http_code}}'").strip()
        assert status == "401", status
        no_data("certificates", since)
    query.succeed("systemctl stop sutura")
    for stimulus in ("missing", "mismatched", "expired"):
        service = f"sutura-{stimulus}"
        serving(service)
        with subtest(f"{stimulus} reading grant fails at its real receiver"):
            since = checkpoint()
            status, reply = ask()
            assert (
                status == "503"
                and reply["code"] == "unavailable"
                and "rows" not in reply
            ), (status, reply)
            diagnostic = (
                "this reading task's grant is past its expiry"
                if stimulus == "expired"
                else "this reading task carries no grant naming this read"
            )
            journal = query.succeed(f"journalctl -u {service} -o cat")
            output.joinpath(f"clickhouse-{stimulus}-journal.log").write_text(journal)
            assert diagnostic in journal, (stimulus, journal)
            no_data(stimulus, since)
        query.succeed(f"systemctl stop {service}")
        serving("sutura")
        answer()
        query.succeed("systemctl stop sutura")
    print(
        "clickhouse-fleet: fixed local shard; two policy-separated fixture callers; exact ordered whole/split answers and native references; source-observed identities and both reader addresses; no serving bucket data; IPv4 matrix; IPv6 disabled; no human sign-in or capacity claim"
    )

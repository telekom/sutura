"""Immutable clustered fixture; explicit shard phases, not automatic split fanout.

Only the NixOS venue runs this. Forty grouped keys do not measure capacity. The cluster
secret trusts a database peer to assert initial_user; shards do not verify end-user JWTs.
"""

import csv
import json
import re
import shlex
from collections import Counter, defaultdict
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
CUSTOMER = re.compile(r"\bdim_customer_(?:local|all|ref|seed)\b", re.IGNORECASE)
BUCKET = re.compile(r"cityHash64\(COALESCE\([^;]+?\)\)\s*%\s*2\s*=\s*([01])\b")


def main(config, machines, start_all, subtest, out):
    material = config["material"]
    addresses = config["addresses"]
    output = Path(out)
    databases = {name: machines[name] for name in ("router", "shard1", "shard2")}
    query, idp = machines["query"], machines["idp"]

    def save(label, value):
        text = json.dumps(value, indent=2)
        output.joinpath(f"{label}.json").write_text(text, encoding="utf-8")
        # The failed dependency may have no copyable output; retain observations in its log.
        print(label, text)

    def sql(host, statement, caller="sutura"):
        return databases[host].succeed(
            f"curl -fsS --max-time 120 --netrc-file {material}/curl-{caller}.netrc "
            f"--data-binary {shlex.quote(statement)} "
            "'http://127.0.0.1:8123/?default_format=JSONCompact'"
        )

    def rows(host, statement, caller="sutura"):
        return json.loads(sql(host, statement, caller))["data"]

    def lookup(host, table, caller):
        return typed(
            rows(
                host,
                f'SELECT customer_key AS "0_link", region FROM {table} '
                "GROUP BY customer_key, region ORDER BY customer_key, region",
                caller,
            )
        )

    def typed(values):
        assert all(
            len(row) == 2
            and type(row[0]) in (int, str)
            and re.fullmatch(r"[0-9]+", str(row[0]))
            and isinstance(row[1], str)
            for row in values
        ), ("lookup_types", values)
        return [[int(key), region] for key, region in values]

    def ask(caller, label):
        token = idp.succeed(
            f"{config['issuer']} mint {material}/issuer.key {config['kid']} "
            f"{shlex.quote(caller)} https://query sutura:metrics.ask"
        ).strip()
        status = idp.succeed(
            f"curl -sS --max-time 150 --cacert {material}/ca.pem "
            f"-H 'Authorization: Bearer {token}' -H 'Content-Type: application/json' "
            f"--data {shlex.quote(json.dumps(QUESTION))} https://query:8443/v1/query "
            "-o /tmp/reply.json -w '%{http_code}'"
        ).strip()
        reply = json.loads(idp.succeed("cat /tmp/reply.json"))
        save(f"reply-{label}", {"caller": caller, "status": status, "reply": reply})
        return status, reply

    def answer(status, reply, expected):
        assert status == "200" and reply.get("outcome") == "answer", (
            "complete_answer",
            status,
            reply,
        )
        assert reply["columns"] == COLUMNS and reply["rows"] == expected, (
            "application_golden",
            expected,
            reply,
        )
        assert reply["executed_as"] == [
            {"source": "geo", "posture": "impersonation-at-source"},
            {"source": "local", "posture": "shared-service-user"},
        ], reply
        return reply

    def serving(service):
        query.succeed(f"systemctl start {service}")
        idp.wait_until_succeeds(
            f"curl -fsS --cacert {material}/ca.pem https://query:8443/health",
            timeout=120,
        )

    def readers(phase, action):
        for name in ("reader1", "reader2"):
            machines[name].succeed(f"systemctl {action} sutura-{phase}")
            if action == "start":
                query.wait_until_succeeds(
                    f"curl -fsS --cacert {material}/ca.pem https://{name}:8443/health",
                    timeout=120,
                )

    def ready_database(host, service="clickhouse"):
        databases[host].wait_for_unit(f"{service}.service")
        databases[host].wait_until_succeeds("curl -fsS http://127.0.0.1:8123/ping")

    def checkpoint(hosts):
        marks = {}
        for host in hosts:
            sql(host, "SYSTEM FLUSH LOGS")
            marks[host] = int(
                rows(host, "SELECT toUnixTimestamp64Micro(now64(6))")[0][0]
            )
        return marks

    def observed(label, marks):
        # Each host compares only with its own timestamp. No cross-host clock ordering.
        events = {}
        for host, since in marks.items():
            sql(host, "SYSTEM FLUSH LOGS")
            raw = sql(
                host,
                "SELECT toString(type) AS type, query_id, initial_query_id, "
                "is_initial_query, user, initial_user, toString(address) AS address, "
                "toString(initial_address) AS initial_address, query_kind, query, "
                "read_rows, result_rows, Settings, exception_code, exception "
                "FROM system.query_log "
                f"WHERE toUnixTimestamp64Micro(event_time_microseconds) >= {since} "
                "AND type IN ('QueryStart', 'QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing') "
                "ORDER BY event_time_microseconds FORMAT JSONEachRow",
            )
            events[host] = [json.loads(line) for line in raw.splitlines()]
        save(f"observed-{label}", events)
        return events

    def address(event, field="address"):
        return event[field].removeprefix("::ffff:")

    def metadata(event):
        statement = re.sub(r'^EXECUTE AS ["`]?\w+["`]?\s+', "", event["query"])
        schema = bool(
            re.match(r"^SELECT \* FROM \(", statement)
            and re.search(r"\) LIMIT 0 SETTINGS ", statement)
        )
        estimate = bool(
            re.match(r"^SELECT sum\(rows\).* FROM system\.parts WHERE ", statement)
        )
        if schema or estimate:
            assert int(event["exception_code"]) == 0, ("metadata_refused", event)
            if schema:
                assert int(event["result_rows"]) == 0, ("schema_zero", event)
        return schema or estimate

    def data(events):
        return {
            host: [
                e for e in records if CUSTOMER.search(e["query"]) and not metadata(e)
            ]
            for host, records in events.items()
        }

    def check_settings(host, event, own_clause=True):
        for name, value in PINNED.items():
            if own_clause:
                assert re.search(
                    rf"\b{name}\s*=\s*'?{value}'?(?:\s|,|$)", event["query"]
                ), ("own_settings", event)
            assert (
                str(event["Settings"].get(name, defaults[host][event["user"]][name]))
                == value
            ), ("effective_settings", host, event)
        if own_clause:
            deadline = re.search(
                r"\bmax_execution_time\s*=\s*([1-9]\d*)", event["query"]
            )
            assert deadline and 0 < int(deadline[1]) <= 120, ("own_deadline", event)
        assert 0 < float(event["Settings"].get("max_execution_time", "0")) <= 120, (
            "effective_deadline",
            event,
        )
        assert str(event["Settings"].get("skip_unavailable_shards", "0")) == "0", event

    def receipts(reply, caller, table, count):
        records = [record for record in reply["executions"] if record["kind"] == "sql"]
        assert len(records) == count, ("receipt_count", records)
        for record in records:
            assert (
                record["source"] == "geo"
                and record["dialect"] == "clickhouse"
                and record["parameters"] == []
                and record["sql"].startswith(f'EXECUTE AS "{caller}" ')
                and table in record["sql"]
            ), ("literal_receipt", record)
        return records

    def positive(events, host, table, caller, source, count, forwarded=False):
        candidates = data(events)[host]
        inner = [
            e
            for e in candidates
            if e["type"] == "QueryFinish" and e["query_kind"] == "Select"
        ]
        assert len(inner) == count, ("inner_read_count", host, candidates)
        for event in inner:
            assert (
                event["user"] == event["initial_user"] == caller
                and address(event) in {addresses[name] for name in source}
                and int(event["read_rows"]) > 0
                and int(event["result_rows"]) > 0
                and table in event["query"]
                and event["query_id"]
                and event["initial_query_id"]
            ), ("effective_caller_positive", host, event)
            if not forwarded:
                assert address(event, "initial_address") == address(event), (
                    "direct_initial_address",
                    host,
                    event,
                )
            check_settings(host, event, own_clause=not forwarded)
        return inner

    def whole_observations(events, host, table, caller, records):
        selected = data(events)
        if table == "dim_customer_all":
            router_reads = positive(events, "router", table, caller, ["query"], 1)
            chain = {e["initial_query_id"] for e in router_reads}
            assert len(chain) == 1 and "" not in chain, (
                "router_query_chain",
                router_reads,
            )
            wrappers = [
                e
                for e in selected["router"]
                if e["type"] == "QueryFinish"
                and e["user"] == "sutura"
                and e["query"].startswith(f'EXECUTE AS "{caller}" ')
            ]
            assert len(wrappers) == 1 and wrappers[0]["query_id"] in chain, (
                "wrapper_query_chain",
                wrappers,
                router_reads,
            )
            for shard in ("shard1", "shard2"):
                reads = positive(
                    events,
                    shard,
                    "dim_customer_local",
                    caller,
                    ["router"],
                    1,
                    forwarded=True,
                )
                for event in reads:
                    assert (
                        event["initial_query_id"] in chain
                        and address(event, "initial_address") == addresses["query"]
                        and event["query_id"]
                        and not int(event["is_initial_query"])
                    ), ("native_query_chain", shard, event, router_reads)
        else:
            positive(events, host, table, caller, ["query"], 1)
            assert all(
                not events for name, events in selected.items() if name != host
            ), ("whole_unexpected_host", selected)
        for name, events_on_host in selected.items():
            for event in events_on_host:
                assert int(event["exception_code"]) == 0, (
                    "whole_failed_attempt",
                    name,
                    event,
                )
                assert event["user"] == caller or (
                    name == host
                    and event["user"] == "sutura"
                    and event["query"].startswith(f'EXECUTE AS "{caller}" ')
                ), ("whole_service_identity", name, event)
                assert (
                    address(event) == addresses["query" if name == host else "router"]
                ), ("whole_unexpected_address", name, event)
        assert all(
            any(e["query"].startswith(r["sql"]) for e in selected[host])
            for r in records
        ), ("whole_receipt_observed", records, selected)

    def split_observations(events, phase, caller):
        selected = data(events)
        assert all(not events for name, events in selected.items() if name != phase), (
            "split_unexpected_host",
            selected,
        )
        # Check ALL attempts before receipt counts: a failed/zero fallback must also kill this.
        for event in selected[phase]:
            assert (
                address(event) in {addresses["reader1"], addresses["reader2"]}
                and "dim_customer_local" in event["query"]
                and "dim_customer_all" not in event["query"]
                and "dim_customer_ref" not in event["query"]
                and BUCKET.search(event["query"])
                and int(event["exception_code"]) == 0
            ), ("no_serving_or_whole_fallback", phase, event)
            assert event["user"] == caller or (
                event["user"] == "sutura"
                and event["query"].startswith(f'EXECUTE AS "{caller}" ')
            ), ("split_service_identity", event)
        inner = positive(
            events,
            phase,
            "dim_customer_local",
            caller,
            ["reader1", "reader2"],
            2,
        )
        assert Counter(address(e) for e in inner) == Counter(
            [addresses["reader1"], addresses["reader2"]]
        ), ("both_reader_positives", inner)
        assert sorted(int(BUCKET.search(e["query"])[1]) for e in inner) == [0, 1], (
            "observed_both_buckets",
            inner,
        )
        wrappers = [
            e
            for e in selected[phase]
            if e["type"] == "QueryFinish" and e["query"].startswith("EXECUTE AS ")
        ]
        assert len(wrappers) == 2 and all(e["user"] == "sutura" for e in wrappers), (
            "split_wrappers",
            wrappers,
        )
        schema = [
            e for e in events[phase] if CUSTOMER.search(e["query"]) and metadata(e)
        ]
        assert schema, ("split_metadata_observed", events)

    def replay(label, host, records):
        marks = checkpoint(databases)
        pieces = [typed(rows(host, record["sql"])) for record in records]
        observed(f"native-replay-{label}", marks)
        return pieces

    def measured(label, host, table, caller, expected, split=False):
        marks = checkpoint(databases)
        status, reply = ask(caller, label)
        events = observed(label, marks)
        # Attribution precedes answer comparisons so a fallback cannot hide behind a golden.
        if split:
            split_observations(events, host, caller)
        answer(status, reply, expected)
        records = receipts(reply, caller, table, 2 if split else 1)
        if not split:
            whole_observations(events, host, table, caller, records)
        else:
            wrappers = [
                e
                for e in data(events)[host]
                if e["type"] == "QueryFinish" and e["user"] == "sutura"
            ]
            assert all(
                any(e["query"].startswith(record["sql"]) for e in wrappers)
                for record in records
            ), ("split_receipt_observed", records, wrappers)
        pieces = replay(label, host, records)
        if split:
            matches = [BUCKET.search(record["sql"]) for record in records]
            assert all(matches), ("receipt_bucket_predicate", records)
            indices = [int(match[1]) for match in matches]
            assert sorted(indices) == [0, 1], ("receipt_both_buckets", records)
            for index, piece in zip(indices, pieces, strict=True):
                assert piece == cells[caller][host][index] and piece, (
                    "exact_bucket_keys",
                    host,
                    caller,
                    index,
                    piece,
                    cells[caller][host][index],
                )
            joined = sorted(row for piece in pieces for row in piece)
            assert joined == wholes[caller][host] and len(joined) == len(
                wholes[caller][host]
            ), ("local_piece_multiplicity", host, caller, pieces)
        save(f"lookup-{label}", pieces)
        return pieces

    def network_matrix():
        ports = {name: 9443 if name == "idp" else 8443 for name in machines}
        allowed = {("idp", "query"), ("query", "idp"), ("query", "router")}
        allowed.update(("query", name) for name in ("reader1", "reader2"))
        allowed.update(
            (source, shard)
            for source in ("query", "reader1", "reader2")
            for shard in ("shard1", "shard2")
        )
        matrix = []
        for source, machine in machines.items():
            for target, port in ports.items():
                if source != target:
                    status, text = machine.execute(f"nc -4 -z -w 2 {target} {port}")
                    entry = [source, target, port, status, text]
                    matrix.append(entry)
                    assert (status == 0) == ((source, target) in allowed), (
                        "primary_network_pair",
                        entry,
                    )
        assert len(matrix) == 42 and sum(row[3] == 0 for row in matrix) == 11, matrix
        save("network-primary", matrix)
        native = []
        for source, machine in machines.items():
            for target in ("shard1", "shard2"):
                if source != target:
                    status, text = machine.execute(f"nc -4 -z -w 2 {target} 9440")
                    entry = [source, target, 9440, status, text]
                    native.append(entry)
                    assert (status == 0) == (source == "router"), (
                        "native_network_pair",
                        entry,
                    )
        assert len(native) == 12 and sum(row[3] == 0 for row in native) == 2, native
        save("network-native", native)
        plaintext = []
        for target, database in databases.items():
            for port in (8123, 9000):
                database.succeed(f"nc -4 -z -w 2 127.0.0.1 {port}")
                for source, machine in machines.items():
                    if source != target:
                        status, text = machine.execute(f"nc -4 -z -w 2 {target} {port}")
                        entry = [source, target, port, status, text]
                        plaintext.append(entry)
                        assert status != 0, ("plaintext_network_pair", entry)
        assert len(plaintext) == 36, plaintext
        save("network-plaintext", plaintext)

    def failure(label, hosts, code=None):
        marks = checkpoint(hosts)
        status, reply = ask("analyst_a", label)
        events = observed(label, marks)
        assert (
            status != "200" and reply.get("outcome") != "answer" and "rows" not in reply
        ), ("no_partial_answer", status, reply)
        if code is not None:
            assert status == "503" and reply.get("reason", {}).get("code") == code, (
                "unavailable_refusal",
                status,
                reply,
            )
        attempts = data(events)
        if label.startswith("lost-readers"):
            assert all(not values for values in attempts.values()), (
                "no_serving_fallback",
                attempts,
            )
        else:
            causes = [
                e
                for e in attempts["router"]
                if int(e["exception_code"]) != 0 and e["exception"]
            ]
            assert causes and any(address(e) == addresses["query"] for e in causes), (
                "native_hop_failure_reached",
                events,
            )
            targets = ("shard1", "shard2") if label == "bad-native-ca" else ("shard2",)
            assert any(
                "9440" in event["exception"]
                and any(
                    target in event["exception"]
                    or addresses[target] in event["exception"]
                    for target in targets
                )
                for event in causes
            ), ("native_endpoint_cause", label, causes)
            save(f"cause-{label}", causes)
            for host in hosts:
                save(
                    f"journal-{label}-{host}",
                    databases[host].succeed("journalctl -u 'clickhouse*' --no-pager"),
                )
        return events

    def golden(caller, shard=None):
        totals = defaultdict(int)
        for fact in facts:
            if (
                fact["status"] != "active"
                or not "2026-06-01" <= fact["month"] < "2026-07-01"
            ):
                continue
            key = int(fact["customer_key"])
            customer = customers.get(key)
            visible = customer is not None and (
                caller == "analyst_a" or customer["region"] == "north"
            )
            if shard is not None:
                visible = visible and key % 2 == int(shard[-1]) % 2
            region = customer["region"] if visible else "null"
            totals[region] += int(fact["mrr_cents"])
        return [
            [region, "2026-06-01", str(totals[region])]
            for region in sorted(totals, key=lambda region: (region == "null", region))
        ]

    start_all()
    save(
        "versions",
        {
            "pinned": config["versions"],
            "sutura": query.succeed(f"{config['binary']} --version").strip(),
            "clickhouse": {
                host: machine.succeed("clickhouse --version").strip()
                for host, machine in databases.items()
            },
            "driver_sha256": query.succeed(f"sha256sum {config['driver']}").strip(),
        },
    )
    for host in databases:
        ready_database(host)
    idp.wait_for_unit("nginx.service")
    with Path(config["data"], "dim_customer.csv").open(
        newline="", encoding="utf-8"
    ) as handle:
        customers = {int(row["customer_key"]): row for row in csv.DictReader(handle)}
    with Path(config["data"], "fct_subscription_monthly.csv").open(
        newline="", encoding="utf-8"
    ) as handle:
        facts = list(csv.DictReader(handle))
    assert set(customers) == set(range(1, 41)), ("fixture_keys", customers)
    ownership = {
        host: sorted(key for key in customers if key % 2 == int(host[-1]) % 2)
        for host in ("shard1", "shard2")
    }
    assert all(len(keys) == 20 for keys in ownership.values()) and not set(
        ownership["shard1"]
    ) & set(ownership["shard2"]), ownership
    assert sorted(ownership["shard1"] + ownership["shard2"]) == sorted(customers), (
        ownership
    )
    save("ownership", ownership)
    for caller, expected_golden in GOLDEN.items():
        assert golden(caller) == expected_golden, (
            "independent_full_golden",
            caller,
            golden(caller),
        )
    for host, database in databases.items():
        sql(
            host,
            "CREATE TABLE dim_customer_seed (customer_key Int64, customer_id String, segment String, region String) ENGINE = MergeTree ORDER BY customer_key",
        )
        database.succeed(
            f"curl -fsS --netrc-file {material}/curl-sutura.netrc --data-binary @{config['data']}/dim_customer.csv "
            "'http://127.0.0.1:8123/?query=INSERT%20INTO%20dim_customer_seed%20FORMAT%20CSVWithNames'"
        )
        table = "dim_customer_ref" if host == "router" else "dim_customer_local"
        sql(host, f"CREATE TABLE {table} AS dim_customer_seed")
        where = (
            "" if host == "router" else f" WHERE customer_key % 2 = {int(host[-1]) % 2}"
        )
        sql(
            host,
            f"INSERT INTO {table} SELECT customer_key, customer_id, segment, region FROM dim_customer_seed CROSS JOIN numbers(60000){where}",
        )
        expected_keys = sorted(customers) if host == "router" else ownership[host]
        actual = rows(
            host,
            f"SELECT customer_key, customer_id, segment, region, count() FROM {table} GROUP BY customer_key, customer_id, segment, region ORDER BY customer_key",
        )
        expected = [
            [
                key,
                customers[key]["customer_id"],
                customers[key]["segment"],
                customers[key]["region"],
                60000,
            ]
            for key in expected_keys
        ]
        actual = [
            [int(key), customer_id, segment, region, int(count)]
            for key, customer_id, segment, region, count in actual
        ]
        assert actual == expected, ("physical_per_key_fixture", host, actual, expected)
        count = 2400000 if host == "router" else 1200000
        assert int(rows(host, f"SELECT count() FROM {table}")[0][0]) == count, (
            "physical_count",
            host,
        )
        parts = rows(
            host,
            f"SELECT sum(rows), sum(bytes_on_disk), count() FROM system.parts WHERE active AND database = 'default' AND table = '{table}'",
        )[0]
        assert int(parts[0]) == count and int(parts[1]) > 0 and int(parts[2]) > 0, (
            "active_parts",
            host,
            parts,
        )
        save(f"fixture-{host}", {"keys": actual, "parts": parts})
        sql(host, "DROP TABLE dim_customer_seed")
        sql(
            host,
            "CREATE TABLE dim_region (region String, sales_area String) ENGINE = MergeTree ORDER BY region",
        )
        database.succeed(
            f"curl -fsS --netrc-file {material}/curl-sutura.netrc --data-binary @{config['data']}/dim_region.csv "
            "'http://127.0.0.1:8123/?query=INSERT%20INTO%20dim_region%20FORMAT%20CSVWithNames'"
        )
    sql(
        "router",
        "CREATE TABLE dim_customer_all AS dim_customer_ref ENGINE = Distributed(fixture, default, dim_customer_local)",
    )
    defaults = {}
    for host in databases:
        defaults[host] = {}
        for caller in ("sutura", *GOLDEN):
            names = ", ".join(f"'{name}'" for name in PINNED)
            defaults[host][caller] = dict(
                rows(
                    host,
                    f"SELECT name, value FROM system.settings WHERE name IN ({names})",
                    caller,
                )
            )
    references, wholes, cells = {}, {}, {}
    for caller in GOLDEN:
        expected = [
            [key, row["region"]]
            for key, row in sorted(customers.items())
            if caller == "analyst_a" or row["region"] == "north"
        ]
        reference = lookup("router", "dim_customer_ref", caller)
        assert reference == expected, ("native_reference", caller, reference, expected)
        references[caller] = reference
        wholes[caller], cells[caller] = {}, {}
        for host in ("shard1", "shard2"):
            expected_shard = [row for row in expected if row[0] in ownership[host]]
            whole = lookup(host, "dim_customer_local", caller)
            assert whole == expected_shard, (
                "native_shard_policy",
                caller,
                host,
                whole,
                expected_shard,
            )
            wholes[caller][host] = whole
            bucket_rows = rows(
                host,
                "SELECT cityHash64(COALESCE(customer_key, 0)) % 2 AS bucket, customer_key, region FROM dim_customer_local GROUP BY bucket, customer_key, region ORDER BY bucket, customer_key, region",
                caller,
            )
            cells[caller][host] = {
                bucket: [
                    [int(key), region]
                    for actual, key, region in bucket_rows
                    if int(actual) == bucket
                ]
                for bucket in (0, 1)
            }
            assert all(cells[caller][host].values()), (
                "four_positive_cells",
                caller,
                host,
                cells[caller][host],
            )
            assert sorted(cells[caller][host][0] + cells[caller][host][1]) == whole, (
                "native_bucket_coverage",
                caller,
                host,
            )
    save(
        "native-references", {"reference": references, "whole": wholes, "cells": cells}
    )

    # First source prerequisite: real switched Distributed reads, before reference/split credit.
    readers("shard1", "start")
    serving("sutura-all")
    distributed = {}
    for caller, expected_golden in GOLDEN.items():
        with subtest(
            f"identity prerequisite: Distributed executes as {caller} on all databases"
        ):
            pieces = measured(
                f"prerequisite-{caller}",
                "router",
                "dim_customer_all",
                caller,
                expected_golden,
            )
            assert pieces == [references[caller]], (
                "distributed_reference",
                caller,
                pieces,
                references[caller],
            )
            distributed[caller] = pieces[0]
            marks = checkpoint(databases)
            direct = lookup("router", "dim_customer_all", caller)
            observed(f"native-distributed-{caller}", marks)
            assert direct == references[caller], (
                "direct_distributed_reference",
                caller,
                direct,
            )
            save(f"native-distributed-{caller}", direct)
    with subtest(
        "IPv4 service-port reachability, separate from protocol and authorization"
    ):
        network_matrix()
    for label, host, broken, code in (
        ("lost-shard", "shard2", None, "source_unavailable"),
        ("bad-native-ca", "router", "clickhouse-bad-ca", None),
        ("bad-cluster-secret", "shard2", "clickhouse-bad-secret", None),
    ):
        with subtest(
            f"{label} fails without partial rows and restores both-shard positives"
        ):
            assert (
                lookup("shard1", "dim_customer_local", "analyst_a")
                == wholes["analyst_a"]["shard1"]
            ), ("healthy_shard1_before_fault", label)
            databases[host].succeed("systemctl stop clickhouse")
            if broken:
                databases[host].succeed(f"systemctl start {broken}")
                ready_database(host, broken)
            failure(label, [name for name in databases if name != host or broken], code)
            if broken:
                databases[host].succeed(f"systemctl stop {broken}")
            databases[host].succeed("systemctl start clickhouse")
            ready_database(host)
            for caller, expected_golden in GOLDEN.items():
                pieces = measured(
                    f"restored-{label}-{caller}",
                    "router",
                    "dim_customer_all",
                    caller,
                    expected_golden,
                )
                assert pieces == [references[caller]], (
                    "restored_full_lookup",
                    label,
                    caller,
                    pieces,
                )
    query.succeed("systemctl stop sutura-all")
    serving("sutura-ref")
    for caller, expected_golden in GOLDEN.items():
        with subtest(f"full independent local reference {caller}"):
            pieces = measured(
                f"reference-{caller}",
                "router",
                "dim_customer_ref",
                caller,
                expected_golden,
            )
            assert pieces == [references[caller]], ("literal_reference", caller, pieces)
    query.succeed("systemctl stop sutura-ref")
    readers("shard1", "stop")
    split_pieces = {caller: [] for caller in GOLDEN}
    for phase in ("shard1", "shard2"):
        readers(phase, "start")
        serving(f"sutura-{phase}-whole")
        for caller in GOLDEN:
            with subtest(f"{phase} whole {caller} independent application golden"):
                expected = golden(caller, phase)
                pieces = measured(
                    f"{phase}-whole-{caller}",
                    phase,
                    "dim_customer_local",
                    caller,
                    expected,
                )
                assert pieces == [wholes[caller][phase]], (
                    "whole_local_lookup",
                    phase,
                    caller,
                    pieces,
                )
        query.succeed(f"systemctl stop sutura-{phase}-whole")
        serving(f"sutura-{phase}-split")
        for caller in GOLDEN:
            with subtest(
                f"{phase} split {caller} has both readers and both bucket key sets"
            ):
                pieces = measured(
                    f"{phase}-split-{caller}",
                    phase,
                    "dim_customer_local",
                    caller,
                    golden(caller, phase),
                    split=True,
                )
                split_pieces[caller].extend(pieces)
        with subtest(
            f"{phase} loses both readers without serving fallback and recovers"
        ):
            readers(phase, "stop")
            failure(f"lost-readers-{phase}", databases, "node_lost")
            readers(phase, "start")
            for caller in GOLDEN:
                pieces = measured(
                    f"restored-readers-{phase}-{caller}",
                    phase,
                    "dim_customer_local",
                    caller,
                    golden(caller, phase),
                    split=True,
                )
                assert (
                    sorted(row for piece in pieces for row in piece)
                    == wholes[caller][phase]
                ), ("restored_bucket_lookup", phase, caller, pieces)
        query.succeed(f"systemctl stop sutura-{phase}-split")
        readers(phase, "stop")
    for caller in GOLDEN:
        with subtest(
            f"{caller} all four shard/bucket pieces equal full lookup with multiplicity"
        ):
            joined_whole = sorted(wholes[caller]["shard1"] + wholes[caller]["shard2"])
            joined_split = sorted(
                row for piece in split_pieces[caller] for row in piece
            )
            assert len(split_pieces[caller]) == 4 and all(split_pieces[caller]), (
                "four_pieces",
                caller,
                split_pieces,
            )
            assert (
                joined_split
                == joined_whole
                == distributed[caller]
                == references[caller]
            ), (
                "four_cell_differential",
                caller,
                joined_split,
                joined_whole,
                distributed[caller],
                references[caller],
            )
            assert len(joined_split) == len(references[caller]) and len(
                {row[0] for row in joined_split}
            ) == len(joined_split), ("full_key_multiplicity", caller, joined_split)
    save(
        "results",
        {
            "reference": references,
            "distributed": distributed,
            "whole": wholes,
            "pieces": split_pieces,
            "limits": "explicit immutable shard phases; no automatic cross-endpoint split routing, common snapshot, capacity or human sign-in proof",
        },
    )

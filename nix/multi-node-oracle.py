"""Oracle fleet acceptance. Rendered and run only by the NixOS machine test."""

import base64
import csv
import json
import re
import shlex
import time
from pathlib import Path

QUESTION = {
    "metrics": ["recurring_revenue"],
    "grain": "month",
    "range": {"start": "2026-06-01", "end": "2026-07-01"},
    "dimensions": ["region"],
}
COLUMNS = ["region", "period", "recurring_revenue"]
GOLDEN = [
    ["central", "2026-06-01", "51739"],
    ["east", "2026-06-01", "32598"],
    ["north", "2026-06-01", "42157"],
    ["south", "2026-06-01", "21203"],
    ["west", "2026-06-01", "49425"],
    ["null", "2026-06-01", "4999"],
]


def main(config, query, reader1, reader2, db, idp, start_all, subtest, out):
    material = config["material"]
    issuer = config["issuer"]
    kid = config["kid"]
    seed = config["seed"]
    addresses = config["addresses"]

    def asking(authenticated=True):
        token = idp.succeed(
            f"{issuer} mint {material}/issuer.key {kid} user@example.com https://query sutura:metrics.ask"
        ).strip()
        authorization = f"-H 'Authorization: Bearer {token}'" if authenticated else ""
        question = shlex.quote(json.dumps(QUESTION))
        return (
            f"curl -sS --max-time 180 --cacert {material}/ca.pem {authorization} "
            f"-H 'Content-Type: application/json' --data {question} "
            "https://query:8443/v1/query"
        )

    def ask():
        raw = idp.succeed(f"{asking()} -o /tmp/reply.json -w '%{{http_code}}'")
        reply = json.loads(idp.succeed("cat /tmp/reply.json"))
        return raw.strip(), reply

    def answer():
        status, reply = ask()
        try:
            assert status == "200" and reply.get("outcome") == "answer", (status, reply)
        except AssertionError:
            try:
                events = bucket_events(traces())
                totals = {
                    node: {
                        kind: sum(
                            event[3]
                            for event in events
                            if event[1] == node and event[2] == kind
                        )
                        for kind in ("EXEC", "FETCH")
                    }
                    for node in addresses
                }
                print("native EXEC/FETCH row totals: " + json.dumps(totals), flush=True)
            except Exception:  # noqa: BLE001 - preserve the original answer failure if trace collection fails.
                print("native EXEC/FETCH row totals unavailable", flush=True)
            raise
        assert reply["columns"] == COLUMNS and reply["rows"] == GOLDEN, reply
        assert reply["executed_as"] == [
            {"source": "geo", "posture": "shared-service-user"},
            {"source": "local", "posture": "shared-service-user"},
        ], reply
        return reply

    def refused(code, status):
        actual, reply = ask()
        assert actual == status and reply["reason"]["code"] == code, (actual, reply)
        assert "rows" not in reply, reply

    def serving(service):
        query.succeed(f"systemctl start {service}")
        idp.wait_until_succeeds(
            f"curl -fsS --cacert {material}/ca.pem https://query:8443/health",
            timeout=120,
        )

    def readers(service="sutura"):
        for reader, name in [(reader1, "reader1"), (reader2, "reader2")]:
            reader.succeed(f"systemctl start {service}")
            query.wait_until_succeeds(
                f"curl -fsS --cacert {material}/ca.pem https://{name}:8443/health",
                timeout=120,
            )

    def traces():
        return db.succeed(
            "docker exec oracle bash -c "
            + shlex.quote(
                "find \"$ORACLE_BASE/diag/rdbms\" -type f -name '*SUTURA*.trc' -print0 | "
                "while IFS= read -r -d '' file; do printf '\\nTRACEFILE %s\\n' \"$file\"; cat \"$file\"; done"
            )
        )

    def bucket_events(raw):
        """Source executions and fetches, including zero-row calls and execute-time prefetch."""
        events = []
        cursors = {}
        file = ""
        building = None
        for line in raw.splitlines():
            if line.startswith("TRACEFILE "):
                file = line.removeprefix("TRACEFILE ")
                building = None
            elif match := re.match(r"PARSING IN CURSOR #(\d+) .*?dep=(\d+)\b", line):
                building = (file, match[1])
                cursors[building] = {"sql": [], "depth": int(match[2])}
            elif line == "END OF STMT":
                building = None
            elif building is not None:
                cursors[building]["sql"].append(line)
            elif match := re.match(r"(EXEC|FETCH) #(\d+):.*?\br=(\d+)\b", line):
                statement = cursors.get((file, match[2]))
                if statement is None or statement["depth"] != 0:
                    continue
                sql = " ".join(statement["sql"])
                if "ORA_HASH(" not in sql:
                    continue
                owners = [
                    node
                    for node, address in addresses.items()
                    if re.search(
                        rf"SUTURA_{re.escape(address.replace('.', '_'))}(?:\D|$)", file
                    )
                ]
                assert len(owners) == 1, (file, owners, sql)
                events.append((file, owners[0], match[1], int(match[3]), sql))
        return events

    def counts():
        events = bucket_events(traces())
        return {
            node: sum(event[3] for event in events if event[1] == node)
            for node in addresses
        }

    def at_snapshot(statement):
        wrapped = (
            'ALTER SESSION SET CURRENT_SCHEMA="SUTURA";\n'
            'SELECT JSON_ARRAY("0_link", "region" NULL ON NULL RETURNING VARCHAR2(4000)) '
            f'FROM ({statement}) ORDER BY "0_link", "region";'
        )
        raw = db.succeed(f"printf '%s\\n' {shlex.quote(wrapped)} | oracle-sql")
        rows = [
            json.loads(line.strip())
            for line in raw.splitlines()
            if line.strip().startswith("[")
        ]
        assert rows, raw
        return rows

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
            "db": 2484,
            "idp": 9443,
        }
        allowed = {
            ("idp", "query"),
            ("query", "reader1"),
            ("query", "reader2"),
            ("query", "db"),
            ("query", "idp"),
            ("reader1", "query"),
            ("reader1", "reader2"),
            ("reader1", "db"),
            ("reader2", "query"),
            ("reader2", "reader1"),
            ("reader2", "db"),
        }
        for source, machine in machines.items():
            for destination, port in ports.items():
                if source == destination:
                    continue
                status, output = machine.execute(f"nc -4 -z -w 2 {destination} {port}")
                assert (status == 0) == ((source, destination) in allowed), (
                    source,
                    destination,
                    status,
                    output,
                )

        # The same source also listens on plaintext TCP inside its VM; no remote role may use it.
        db.succeed("nc -4 -z -w 2 127.0.0.1 1521")
        for name, machine in machines.items():
            if name != "db":
                machine.fail("nc -4 -z -w 2 db 1521")

    start_all()
    db.wait_for_unit("docker.service")
    db.succeed("systemctl start oracle", timeout=1800)
    db.wait_until_succeeds("docker exec oracle healthcheck.sh", timeout=900)
    db.succeed(f"oracle-sql < {seed}")
    fixture = db.succeed(
        'printf \'%s\\n\' \'SELECT COUNT(*), COUNT(DISTINCT "customer_key") FROM "SUTURA"."dim_customer_split";\' '
        '\'SELECT ORA_HASH("customer_key", 1), COUNT(*) FROM "SUTURA"."dim_customer_split" GROUP BY ORA_HASH("customer_key", 1) ORDER BY 1;\' | oracle-sql'
    )
    numbers = [
        tuple(map(int, line.split()))
        for line in fixture.splitlines()
        if re.fullmatch(r"\s*\d+\s+\d+\s*", line)
    ]
    assert len(numbers) == 3 and numbers[0] == (40, 40), fixture
    assert [row[0] for row in numbers[1:]] == [0, 1] and sum(
        row[1] for row in numbers[1:]
    ) == 40, fixture
    assert all(row[1] > 0 for row in numbers[1:]), fixture
    # TIMESTAMP_TO_SCN can precede a recent commit by several seconds.
    time.sleep(8)
    certificate = base64.b64encode(
        db.succeed("cat /run/oracle-tls/ca.pem").encode()
    ).decode()
    for machine in (query, reader1, reader2):
        machine.succeed(f"printf %s {certificate} | base64 -d > /run/oracle-ca.pem")
    idp.wait_for_unit("nginx.service")
    readers()
    serving("sutura-whole")
    with subtest("the whole Oracle lookup returns the exact ordered reference"):
        whole = answer()
    query.succeed("systemctl stop sutura-whole")
    serving("sutura")

    with subtest(
        "all declared IPv4 role paths connect and every other role pair is refused"
    ):
        network_matrix()

    before = counts()
    with subtest(
        "two source readers answer exactly what the whole Oracle lookup answers"
    ):
        split = answer()
        assert (
            split["columns"] == whole["columns"] and split["rows"] == whole["rows"]
        ), (
            whole,
            split,
        )
        submissions = [
            record for record in split["executions"] if record["kind"] == "sql"
        ]
        assert len(submissions) == 2, submissions
        scns = []
        buckets = set()
        for record in submissions:
            assert record["source"] == "geo" and record["dialect"] == "oracle", record
            assert record["parameters"] == [], record
            scn = re.search(r" AS OF SCN ([1-9]\d*) ", record["sql"])
            bucket = re.search(
                r'ORA_HASH\("dim_customer_split"\."customer_key", 1\) = ([01])',
                record["sql"],
            )
            assert scn and bucket, record
            scns.append(int(scn[1]))
            buckets.add(int(bucket[1]))
        assert scns[0] == scns[1] and buckets == {0, 1}, submissions

    with subtest("the two bucket statements equal the whole leg at their exact SCN"):
        # The source reference runs separately through SQL*Plus as the fixture administrator;
        # the served whole/split differential above runs through sutura as the shared user.
        prefix, filtered = submissions[0]["sql"].split(" WHERE ", 1)
        _, suffix = filtered.split(" GROUP BY ", 1)
        reference = at_snapshot(f"{prefix} GROUP BY {suffix}")
        pieces = [row for record in submissions for row in at_snapshot(record["sql"])]
        with Path(config["customers"]).open(newline="") as handle:
            expected = [
                [int(row["customer_key"]), row["region"]]
                for row in csv.DictReader(handle)
            ]
        assert len(reference) == len(pieces) == len(expected) == 40, (reference, pieces)
        assert sorted(pieces) == reference == sorted(expected), (
            reference,
            pieces,
            expected,
        )

    with subtest(
        "both reader addresses fetched bucket rows and the serving address fetched none"
    ):
        raw = traces()
        for address in addresses.values():
            assert f"SUTURA_{address.replace('.', '_')}" in raw, (address, raw)
        after = counts()
        assert all(after[node] > before[node] for node in ("reader1", "reader2")), (
            before,
            after,
        )
        assert after["query"] == before["query"], (before, after)
        # A per-session trace is source evidence independent of the successful-submission receipts.
        fresh = bucket_events(traces())
        assert not any(
            event[1] == "query" and f" AS OF SCN {scns[0]} " in event[4]
            for event in fresh
        ), fresh
        for node in ("reader1", "reader2"):
            assert any(
                event[1] == node and f" AS OF SCN {scns[0]} " in event[4]
                for event in fresh
            ), (node, fresh)

    with subtest(
        "lost readers refuse without a local bucket fallback, then restart restores the answer"
    ):
        reader1.succeed("systemctl stop sutura")
        reader2.succeed("systemctl stop sutura")
        before = counts()
        refused("node_lost", "503")
        assert counts() == before, (before, counts())
        readers()
        answer()

    with subtest("unresolved readers refuse without a local bucket fallback"):
        query.succeed("systemctl stop sutura")
        serving("sutura-unresolved")
        before = counts()
        refused("source_unavailable", "503")
        assert counts() == before, (before, counts())
        query.succeed("systemctl stop sutura-unresolved")

    with subtest("a changed catalog on both readers refuses re-certification"):
        reader1.succeed("systemctl stop sutura")
        reader2.succeed("systemctl stop sutura")
        readers("sutura-changed")
        serving("sutura")
        before = counts()
        refused("remote_leg_not_certified", "403")
        assert counts() == before, (before, counts())
        reader1.succeed("systemctl stop sutura-changed")
        reader2.succeed("systemctl stop sutura-changed")
        readers()
        answer()

    with subtest(
        "anonymous callers and untrusted or missing node certificates are refused"
    ):
        status = idp.succeed(
            f"{asking(False)} -o /dev/null -w '%{{http_code}}'"
        ).strip()
        assert status == "401", status
        endpoint = "https://reader1:8443/worker.WorkerService/ExecuteTask"
        request = f"curl -sS --max-time 10 --http2 --cacert {material}/ca.pem -X POST -H 'Content-Type: application/grpc' {endpoint}"
        query.fail(
            f"{request} --cert {material}/stranger.pem --key {material}/stranger.key"
        )
        status = query.succeed(f"{request} -o /dev/null -w '%{{http_code}}'").strip()
        assert status == "401", status

    Path(out, "oracle-traces.log").write_text(traces())
    print(
        "oracle-fleet: exact whole/split ordered answer; two source reader addresses and one SCN; no serving bucket row fetch; lost/unresolved/uncertified readers refused; IPv4 role matrix; shared service user and fixture issuer"
    )

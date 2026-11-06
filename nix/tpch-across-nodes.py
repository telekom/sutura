"""The driver half of `nix/tpch-across-nodes.nix`: TPC-H across nodes against one node.

`nix/tpch-across-nodes.nix` boots four VMs and hands this file's `main` the machines and a plan.
Three `sutura serve` topologies answer the same questions, one at a time, over the same star:

  REF   one node, every model on the DuckDB source. One statement, no federation: the answer.
  ONE   one node, the federated catalog, no `server.nodes`. A lookup leg past one stream's rows
        is refused `result_too_large`.
  MANY  a planner and two reading workers over the federated catalog. A split lookup leg is
        granted to the readers, which read its `ctid` ranges.

`nix/tpch/acceptance.json` names the questions that must answer on MANY, equal REF. `sutura-dev
agree` judges equal, by the policy the differential legs use: a decimal sum and an integer
exactly, and only a column that holds an `avg` or a ratio at 13 significant digits.

That the legs were read across nodes is the PostgreSQL statement log, one line per statement,
prefixed with the client's address. A listed question that splits a leg must show exactly that
leg's partitions as distinct ranges read by the readers, each reader reading at least one, and
none read by the serving node, which exported the snapshot. A question that splits no leg must
show no range at all.

Limits: the fact leg (DuckDB) is read whole on the serving node, so only lookup legs are
distributed. Every node reads PostgreSQL as one role, so the log tells nodes apart by address
alone. REF and MANY share the semantic compiler, so a defect in the plan they share agrees with
itself. A split leg's partitions are cut by pages, not by rows: a table of exactly two streams'
rows has no slack, and a partition past the bound is refused, so a question over such a leg is
recorded under `observed` rather than listed until a run says it answers.
"""

import datetime as dt
import json
import math
import re
import shlex
import time
from itertools import pairwise
from pathlib import Path

# One stream's rows (`MOST_RESULT_ROWS` in `sutura-exec-postgres`). A split leg is cut into the
# fewest partitions that each fit it.
STREAM_ROWS = 1_000_000

# The dimensions whose lookup leg is rooted at a PostgreSQL table, by the table.
LEG_OF = {
    "market_segment": "customer",
    "customer_nation": "customer",
    "customer_region": "customer",
    "part_brand": "part",
    "part_type": "part",
    "part_container": "part",
    "manufacturer": "part",
    "supplier_nation": "supplier",
    "supplier_region": "supplier",
}

SHAPE = (
    "{outcome: .outcome, code: .reason.code, detail: .reason.detail,"
    " rows: ((.rows // []) | length),"
    " text: (if .outcome == null then (tostring | .[0:300]) else null end)}"
)

# `table.ctid >= '(a,0)'` and `table.ctid < '(b,0)'`, the table and the quotes optional.
RANGE = re.compile(r"""(?:"?(\w+)"?\s*\.\s*)?"?ctid"?\s*(>=|<)\s*'\((\d+),\s*0\)'""")

OPEN = -1


def refuse(message: str) -> None:
    raise AssertionError(f"tpch-across-nodes: {message}")


def real_metrics(catalog: Path) -> set[str]:
    """The metrics whose answer is a Real: an `avg`, or a ratio."""
    names = set()
    for document in sorted((catalog / "metrics").glob("*.md")):
        head = document.read_text().split("\n---\n", 1)[0]
        name = re.search(r"^name: (\S+)$", head, re.MULTILINE)
        if name and re.search(r"aggregate: avg|^  ratio:", head, re.MULTILINE):
            names.add(name.group(1))
    return names


def legs_of(body: dict) -> list[str]:
    named = set(body["dimensions"]) | {f["dimension"] for f in body.get("filters", [])}
    return sorted({LEG_OF[d] for d in named if d in LEG_OF})


def partitions(counts: dict, table: str) -> int:
    return max(1, math.ceil(counts[table] / STREAM_ROWS))


class Corpus:
    """The questions, the three tiers they are held to, and what the data holds."""

    def __init__(self, plan: dict):
        corpus = Path(plan["corpus"])
        data = json.loads((Path(plan["data"]) / "counts.json").read_text())
        everything = {
            q["name"]: q for q in json.loads((corpus / "questions.json").read_text())
        }
        accepted = json.loads((corpus / "acceptance.json").read_text())
        self.scale = plan["scale"]
        self.counts = data["counts"]
        self.real = real_metrics(corpus / "catalog")
        self.listed = accepted["questions"]
        self.observed = accepted["observed"]
        self.controls = accepted["controls"]
        self.questions = everything

        if data["scale"] != self.scale:
            refuse(f"the data is SF{data['scale']}, the plan SF{self.scale}")
        names = self.listed + self.observed + self.controls
        if len(set(names)) != len(names):
            refuse("acceptance.json names a question twice")
        for name in self.listed + self.observed:
            if everything.get(name, {}).get("path") != "federated":
                refuse(f"{name} is not a federated question of questions.json")
        for name in self.controls:
            if everything.get(name, {}).get("path") != "single":
                refuse(f"{name} is not a single-source question of questions.json")
        for table in ("customer", "part"):
            if partitions(self.counts, table) < 2:
                refuse(
                    f"SF{self.scale} does not split {table}: nothing is across nodes"
                )
            if not any(table in self.legs(n) for n in self.listed):
                refuse(f"acceptance.json lists no question over a {table} leg")

    def body(self, name: str) -> dict:
        return self.questions[name]["body"]

    def legs(self, name: str) -> list[str]:
        return legs_of(self.body(name))

    def splits(self, name: str) -> dict[str, int]:
        """The leg tables the question reads in partitions, and into how many."""
        return {
            t: partitions(self.counts, t)
            for t in self.legs(name)
            if partitions(self.counts, t) > 1
        }

    def names(self) -> list[str]:
        return self.listed + self.observed + self.controls

    def tier(self, name: str) -> str:
        if name in self.listed:
            return "listed"
        return "observed" if name in self.observed else "control"


class Venue:
    def __init__(self, plan: dict, query, db, readers: list):
        self.plan = plan
        self.query = query
        self.db = db
        self.readers = readers
        self.node = {
            address: node
            for node, addresses in plan["addresses"].items()
            for address in addresses
        }
        self.ca = f"{plan['material']}/ca.pem"

    def say(self, text: str) -> None:
        print(f"tpch-across-nodes: {text}", flush=True)

    def wait_healthy(self, machine, unit: str, port: int, host: str) -> None:
        """Waits for `unit` on `machine` to answer `/health`, and fails at once if it stops."""
        deadline = time.monotonic() + 1800
        while time.monotonic() < deadline:
            status, _ = self.query.execute(
                f"curl -fsS --max-time 10 --cacert {self.ca} https://{host}:{port}/health",
                timeout=dt.timedelta(seconds=30),
            )
            if status == 0:
                return
            _, state = machine.execute(f"systemctl show -p ActiveState --value {unit}")
            if state.strip() not in ("active", "activating"):
                refuse(f"{unit} on {machine.name} is {state.strip()!r}, not serving")
            time.sleep(1)
        refuse(f"{unit} on {machine.name} did not answer /health in 30 minutes")

    def ask(self, topology: str, name: str, body: dict, suffix: str = "") -> dict:
        port = self.plan["ports"][topology]
        out = f"/var/tmp/answers/{topology}/{name}{suffix}.json"
        token = self.query.succeed(self.plan["mint"]).strip()
        command = (
            "curl -sS --max-time 330"
            f" --cacert {self.ca}"
            f" -H {shlex.quote('Authorization: Bearer ' + token)}"
            " -H 'Content-Type: application/json'"
            f" --data {shlex.quote(json.dumps(body))}"
            f" -o {shlex.quote(out)} -w '%{{http_code}}'"
            f" https://query:{port}/v1/query 2>&1"
        )
        started = time.monotonic()
        status, printed = self.query.execute(command)
        ms = round((time.monotonic() - started) * 1000)
        if status != 0:
            return {"outcome": "error", "detail": printed[-300:], "ms": ms, "file": out}
        shaped, shape = self.query.execute(
            f"jq -c {shlex.quote(SHAPE)} {shlex.quote(out)} 2>&1"
        )
        if shaped != 0:
            return {
                "outcome": "error",
                "status": printed.strip()[-3:],
                "detail": shape[-300:],
                "ms": ms,
                "file": out,
            }
        reply = {
            **json.loads(shape),
            "status": printed.strip()[-3:],
            "ms": ms,
            "file": out,
        }
        text = reply.pop("text")
        if reply["outcome"] not in ("answer", "refusal"):
            # An answer and a refusal are the two bodies this service means to send; anything
            # else, a 503 included, is a failure and is never a refusal.
            return {**reply, "outcome": "error", "detail": text}
        return reply

    def agree(self, left: str, right: str, body: dict, real: set[str]) -> str | None:
        """Why the two answers differ, or `None` where they are the same answer."""
        flags = [] if "top" not in body else ["--ordered"]
        for metric in body["metrics"]:
            if metric in real:
                flags += ["--real", metric]
        words = [self.plan["agree"], "agree", left, right, *flags]
        command = " ".join(shlex.quote(w) for w in words) + " 2>&1"
        status, said = self.query.execute(command)
        return None if status == 0 else said.strip()

    def tied(self, body: dict, probe: dict) -> bool:
        """Whether two neighbouring values of the probe answer's ranking are the same."""
        metric = body["metrics"][0]
        expression = f'(.columns | index("{metric}")) as $i | [.rows[][$i]]'
        values = json.loads(
            self.query.succeed(f"jq -c {shlex.quote(expression)} {probe['file']}")
        )
        return any(a == b for a, b in pairwise(values))

    def log_lines(self) -> int:
        counted = self.db.succeed(
            "journalctl --sync; journalctl -u postgresql -o cat | wc -l"
        )
        return int(counted.split()[-1])

    def log_since(self, mark: int) -> list[str]:
        return self.db.succeed(
            f"journalctl --sync; journalctl -u postgresql -o cat | tail -n +{mark + 1}"
        ).splitlines()

    def statements(self, lines: list[str]) -> list[tuple[str, str]]:
        """Each logged statement and the node it came from, continuation lines joined."""
        found: list[list[str]] = []
        for line in lines:
            head = line.split(" ", 1)[0]
            if head in self.node or head == "[local]":
                found.append([self.node.get(head, head), line])
            elif found:
                found[-1][1] += "\n" + line
        return [(client, text) for client, text in found]


def ranges_read(statements: list[tuple[str, str]], node: str) -> list[tuple]:
    """The distinct `(table, from, to)` page ranges `node` read, `to` OPEN for the last."""
    found = set()
    for client, text in statements:
        if client != node or "ctid" not in text:
            continue
        matches = RANGE.findall(text)
        table = next((m[0] for m in matches if m[0]), "?")
        lower = next((int(m[2]) for m in matches if m[1] == ">="), 0)
        upper = next((int(m[2]) for m in matches if m[1] == "<"), OPEN)
        found.add((table, lower, upper))
    return sorted(found)


def cut_whole(rows: list[tuple]) -> bool:
    """Whether the ranges, in page order, meet end to end and the last is open."""
    ordered = sorted(rows, key=lambda r: r[1])
    return (
        ordered[0][1] == 0
        and ordered[-1][2] == OPEN
        and all(a[2] == b[1] for a, b in pairwise(ordered))
    )


def judge_ranges(
    splits: dict[str, int], statements: list[tuple[str, str]], readers: list[str]
) -> tuple[dict, list[str]]:
    """The ranges each node read, and what a question that splits `splits` is refused for."""
    read = {node: ranges_read(statements, node) for node in ["query", *readers]}
    problems = []
    if read["query"]:
        problems.append(f"the serving node read ranges: {read['query']}")
    union = {r for node in readers for r in read[node]}
    for table in sorted({t for t, _, _ in union} - set(splits)):
        problems.append(f"a range of {table}, which this question does not split")
    for table, wanted in splits.items():
        mine = [r for r in union if r[0] == table]
        if len(mine) != wanted:
            problems.append(
                f"{table}: {len(mine)} distinct ranges read, {wanted} wanted"
            )
        elif not cut_whole(mine):
            problems.append(f"{table}: the ranges do not cover the table: {mine}")
    if splits:
        for node in readers:
            if not read[node]:
                problems.append(f"{node} read no range")
        exported = any(
            "pg_export_snapshot" in text
            for client, text in statements
            if client == "query"
        )
        if not exported:
            problems.append("the serving node exported no snapshot")
    return read, problems


def cell(result: dict) -> str:
    if result["outcome"] == "answer":
        return f"answer, {result['rows']} rows, {result['ms']} ms"
    if result["outcome"] == "refusal":
        return f"`{result['code']}`, {result['ms']} ms"
    return f"error {result.get('status')}, {result['ms']} ms"


def table_of(report: dict) -> str:
    lines = [
        f"## TPC-H across nodes, SF{report['scale']}",
        "",
        "| question | tier | splits | reader1 | reader2 | query | MANY | verdict |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]

    def span(rows: list) -> str:
        return " ".join(f"{t}[{a},{'' if b == OPEN else b})" for t, a, b in rows) or "-"

    for name, record in report["questions"].items():
        ranges = record["ranges"]
        lines.append(
            f"| {name} | {record['tier']} | {record['splits'] or '-'} "
            f"| {span(ranges.get('reader1', []))} | {span(ranges.get('reader2', []))} "
            f"| {span(ranges.get('query', []))} | {cell(record['many'])} "
            f"| {'; '.join(record['problems']) or 'ok'} |"
        )
    return "\n".join(lines) + "\n"


def start_service(venue: Venue, topology: str) -> None:
    venue.query.succeed(f"systemctl start sutura-{topology}")
    unit = f"sutura-{topology}"
    venue.wait_healthy(venue.query, unit, venue.plan["ports"][topology], "query")


def preflight(venue: Venue) -> None:
    """Every node's settings and catalog, loaded as `sutura serve` loads them.

    `sutura prompt` reads the same settings and applies every refusal a deployment is held to at
    startup, then the catalog, and starts no listener and opens no source. A refusal is found here
    in seconds on each node, together, and not after a service has failed to start.
    """
    machines = {"query": venue.query, **{r.name: r for r in venue.readers}}
    problems = []
    for node in venue.plan["preflight"]:
        command = (
            "SUTURA_ENVIRONMENT=development "
            f"{shlex.quote(venue.plan['sutura'])} prompt"
            f" {shlex.quote(node['catalog'])} {shlex.quote(node['config'])} 2>&1 >/dev/null"
        )
        status, said = machines[node["machine"]].execute(command)
        venue.say(f"preflight {node['name']}: {'ok' if status == 0 else 'refused'}")
        if status != 0:
            problems.append(f"{node['name']}:\n{said.strip()}")
    if problems:
        refuse("a node's settings are refused at load:\n" + "\n".join(problems))


def stop_service(venue: Venue, topology: str) -> None:
    venue.query.succeed(f"systemctl stop sutura-{topology}")


def load_dimensions(venue: Venue, corpus: Corpus) -> None:
    """The five dimension tables in PostgreSQL, counted against the data's own counts."""
    venue.db.succeed(
        f"runuser -u postgres -- psql -v ON_ERROR_STOP=1 -d sutura -f {venue.plan['load']}"
    )
    for table in venue.plan["tables"]:
        sql = (
            f"SELECT count(*), (SELECT reltuples::bigint FROM pg_class"
            f" WHERE oid = '{table}'::regclass) FROM {table}"
        )
        printed = venue.db.succeed(
            f"runuser -u postgres -- psql -d sutura -At -F , -c {shlex.quote(sql)}"
        )
        rows, estimate = (int(n) for n in printed.strip().split(","))
        if rows != corpus.counts[table]:
            refuse(f"{table} holds {rows} rows, the data {corpus.counts[table]}")
        if estimate != rows:
            refuse(f"{table}: pg_class says {estimate} rows, the table holds {rows}")
        venue.say(f"{table}: {rows} rows, estimated exactly")


def run(venue: Venue, corpus: Corpus, subtest) -> dict:
    """Every pass, then every verdict. Returns the report, failures in `failures`."""
    outcomes: dict[str, dict] = {name: {} for name in corpus.names()}
    logs: dict[str, list[tuple[str, str]]] = {}
    failures: list[str] = []

    for topology in ("ref", "one", "many"):
        with subtest(f"{topology.upper()} answers every question"):
            venue.query.succeed(f"install -d -m 0755 /var/tmp/answers/{topology}")
            start_service(venue, topology)
            for name in corpus.names():
                mark = venue.log_lines() if topology == "many" else 0
                result = venue.ask(topology, name, corpus.body(name))
                if topology == "many":
                    lines = venue.log_since(mark)
                    logs[name] = venue.statements(lines)
                outcomes[name][topology] = result
                venue.say(f"{topology} {name}: {cell(result)}")
                if topology == "ref" and "top" in corpus.body(name):
                    top = corpus.body(name)["top"]
                    body = {**corpus.body(name), "top": {**top, "n": top["n"] + 1}}
                    probe = venue.ask("ref", name, body, ".top")
                    if probe["outcome"] != "answer" or venue.tied(body, probe):
                        failures.append(
                            f"{name}: its top values tie, so its order is not one"
                        )
            stop_service(venue, topology)

    readers = ["reader1", "reader2"]
    questions: dict[str, dict] = {}
    for name in corpus.names():
        tier, body = corpus.tier(name), corpus.body(name)
        ref, one, many = (outcomes[name][t] for t in ("ref", "one", "many"))
        problems: list[str] = []
        splits = corpus.splits(name) if tier != "control" else {}
        statements = logs[name]
        read, range_problems = judge_ranges(splits, statements, readers)
        # A failure that is no refusal is a defect on any question: listed, observed or control.
        for result, label in ((ref, "REF"), (one, "ONE"), (many, "MANY")):
            if result["outcome"] == "error":
                problems.append(
                    f"{label} failed with no refusal: {cell(result)}"
                    f" {str(result.get('detail'))[:200]}"
                )
        if tier == "listed" or tier == "control":
            if ref["outcome"] == "refusal" or (
                ref["outcome"] == "answer" and ref["rows"] == 0
            ):
                problems.append(f"REF did not answer with rows: {cell(ref)}")
            if many["outcome"] == "refusal":
                problems.append(f"MANY did not answer: {cell(many)}")
            if one["outcome"] == "refusal" and one["code"] != "result_too_large":
                problems.append(f"ONE refused `{one['code']}`, not result_too_large")
            if many["outcome"] == "answer":
                problems += range_problems
        for other, label in ((many, "MANY"), (one, "ONE")):
            if ref["outcome"] == "answer" and other["outcome"] == "answer":
                differs = venue.agree(ref["file"], other["file"], body, corpus.real)
                if differs:
                    problems.append(f"{label} disagrees with REF: {differs}")
        questions[name] = {
            "tier": tier,
            "legs": corpus.legs(name),
            "splits": splits,
            "ref": ref,
            "one": one,
            "many": many,
            "ranges": read,
            "problems": problems,
        }
        failures += [f"{name}: {p}" for p in problems]

    return {
        "scale": corpus.scale,
        "counts": corpus.counts,
        "questions": questions,
        "failures": failures,
    }


def dump(venue: Venue) -> None:
    for unit in ("sutura-ref", "sutura-one", "sutura-many"):
        _, tail = venue.query.execute(f"journalctl -u {unit} -o cat | tail -n 40")
        venue.say(f"{unit}:\n{tail}")
    for reader in venue.readers:
        _, tail = reader.execute("journalctl -u sutura -o cat | tail -n 40")
        venue.say(f"{reader.name}:\n{tail}")


def main(plan: dict, query, db, readers: list, start_all, subtest, out: Path) -> None:
    corpus = Corpus(plan)
    venue = Venue(plan, query, db, readers)
    start_all()
    try:
        with subtest("every node's settings and catalog load"):
            preflight(venue)
        with subtest("PostgreSQL holds the dimension tables, counted"):
            db.wait_for_unit("postgresql.service")
            db.succeed(
                f"runuser -u postgres -- psql -v ON_ERROR_STOP=1 -f {plan['role']}"
            )
            load_dimensions(venue, corpus)
        with subtest("the readers and the star are up"):
            for reader in readers:
                reader.succeed("systemctl start sutura")
            for reader in readers:
                venue.wait_healthy(reader, "sutura", 8443, reader.name)
            # A writable copy on local disk, given the two role-playing tables REF reads, then
            # sealed. The DuckDB that wrote the file adds them, and a left WAL would be a bug.
            query.succeed(
                "install -d -m 0755 /var/lib/tpch"
                f" && install -m 0644 {plan['data']}/star.duckdb /var/lib/tpch/star.duckdb"
                f" && HOME=/tmp {plan['duckdb']} /var/lib/tpch/star.duckdb < {plan['roleplay']}"
                " && test ! -e /var/lib/tpch/star.duckdb.wal"
                " && chmod 0444 /var/lib/tpch/star.duckdb"
            )
        report = run(venue, corpus, subtest)
    except Exception:
        dump(venue)
        raise
    text = table_of(report)
    venue.say("\n" + text)
    venue.say("results " + json.dumps(report, sort_keys=True))
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "ranges.md").write_text(text)
    if report["failures"]:
        dump(venue)
        refuse("\n" + "\n".join(report["failures"]))

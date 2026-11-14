"""One-off planned-statement generation and allowlisted collection; Python stdlib only."""

import base64
import gzip
import hashlib
import hmac
import ipaddress
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "source"
BINDING = ROOT / ".github/payloads/sql-planned.json"
ENGINES = (
    "datafusion",
    "duckdb",
    "postgres",
    "clickhouse",
    "bigquery",
    "oracle",
    "cube",
)
LANES = ("core", "cube", "oracle-verified", "oracle-native", "bigquery")
SNAPS = "crates/sutura-app/tests/snapshots/"
META = "crates/sutura-catalog-cube/src/fixture/meta.json"
SHAPES = [SNAPS + f"planned_statement_shapes@{engine}.snap" for engine in ENGINES]
ROWS = {engine: SNAPS + f"planned_statement_rows@{engine}.snap" for engine in ENGINES}
CATALOG = [
    SNAPS + f"catalog_{kind}@cube.snap"
    for kind in ("definitions", "digest", "knowledge")
]
OUTPUTS = {
    "core": SHAPES + [ROWS[engine] for engine in ENGINES[:4]],
    "cube": [META, *CATALOG, SHAPES[-1], ROWS["cube"]],
    "oracle-verified": [SHAPES[-2], ROWS["oracle"]],
    "oracle-native": [ROWS["oracle"]],
    "bigquery": [SHAPES[-3], ROWS["bigquery"]],
}
ALLOWED = set().union(*map(set, OUTPUTS.values()))
CORE_FILTER = (
    "package(=sutura-app) & binary(=golden) & "
    "(test(/^statements::(datafusion|duckdb|postgres|clickhouse|bigquery|oracle|cube)::pins_planned_source_statements$/)"
    " | test(/^statements::engines::(datafusion|duckdb|postgres|clickhouse)::every_planned_shape_answers_at_the_native_engine$/))"
)


def require(ok, message):
    if not ok:
        raise SystemExit(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def git(*args, cwd=SOURCE):
    return subprocess.check_output(["git", *args], cwd=cwd).decode().strip()


def binding():
    value = json.loads(BINDING.read_bytes())
    require(value["status"] == "READY_FOR_EXACT_REVIEW", "Unbound draft: HOLD")
    for key in ("main", "base", "parent_tree", "source_tree"):
        require(re.fullmatch(r"[0-9a-f]{40}", value[key]), "Unbound Git object")
    for key in ("parent_patch_sha256", "behavior_patch_sha256"):
        require(re.fullmatch(r"[0-9a-f]{64}", value[key]), "Unbound source patch")
    require(
        str(uuid.UUID(value["request_id"])) == value["request_id"],
        "Invalid request UUID",
    )
    for key in ("helper_ref", "api_branch", "behavior_branch"):
        require(value[key] and "PENDING" not in value[key], "Unbound ref")
    require(value["api_branch"] != value["behavior_branch"], "Source refs must differ")
    return value


def context():
    value = binding()
    return {
        "binding_sha256": sha(BINDING.read_bytes()),
        "helper_sha": os.environ["EXPECTED_HELPER_SHA"],
        "source_tree": value["source_tree"],
        "parent_tree": value["parent_tree"],
        "request_id": value["request_id"],
        "run_id": os.environ["GITHUB_RUN_ID"],
        "attempt": os.environ["GITHUB_RUN_ATTEMPT"],
    }


def guard():
    value = binding()
    require(os.environ["GITHUB_REPOSITORY"] == "telekom/sutura", "Wrong repository")
    require(os.environ["GITHUB_RUN_ATTEMPT"] == "1", "Reruns are refused")
    require(os.environ["CONSUMER"] == "sql-planned-native", "Wrong consumer")
    require(
        os.environ["GITHUB_REF"] == "refs/heads/" + value["helper_ref"],
        "Wrong helper ref",
    )
    expected = os.environ["EXPECTED_HELPER_SHA"]
    require(re.fullmatch(r"[0-9a-f]{40}", expected), "Unbound helper commit")
    require(
        os.environ["GITHUB_SHA"]
        == os.environ["WORKFLOW_SHA"]
        == git("rev-parse", "HEAD", cwd=ROOT)
        == expected,
        "Helper SHA mismatch",
    )
    require(
        not git("status", "--porcelain=v1", "--untracked-files=all", cwd=ROOT),
        "Dirty helper",
    )
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write("base=" + value["base"] + "\n")
    for key in (
        "BASH_ENV",
        "SKIP",
        "PREK_SKIP",
        "PRE_COMMIT_ALLOW_NO_CONFIG",
        "PREK_ALLOW_NO_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "SUTURA_ORACLE_BASE_SHA",
        "SUTURA_ORACLE_BASE_PROOF",
        "SUTURA_ORACLE_CLAIM_PROOF",
        "SUTURA_ORACLE_SPLIT_CLAIM_PROOF",
    ):
        require(
            not os.environ.get(key), "Bypass/proof-mode environment is refused: " + key
        )


def apply_source(which):
    value = binding()
    expected_before = (
        value["base"] + "^{tree}" if which == "api" else value["parent_tree"]
    )
    require(
        git("write-tree") == git("rev-parse", expected_before), "Wrong source preimage"
    )
    require(
        not git("diff", "--name-only")
        and not git("ls-files", "--others", "--exclude-standard"),
        "Dirty source",
    )
    data = gzip.decompress(
        base64.b64decode(
            b"".join(
                (ROOT / f".github/payloads/sql-planned-{which}.patch.b64")
                .read_bytes()
                .split()
            ),
            validate=True,
        )
    )
    require(
        sha(data)
        == value["parent_patch_sha256" if which == "api" else "behavior_patch_sha256"],
        "Source patch hash mismatch",
    )
    subprocess.run(
        ["git", "apply", "--index", "--check", "-"], input=data, cwd=SOURCE, check=True
    )
    subprocess.run(["git", "apply", "--index", "-"], input=data, cwd=SOURCE, check=True)
    expected_after = value["parent_tree" if which == "api" else "source_tree"]
    require(
        git("write-tree") == expected_after and not git("diff", "--name-only"),
        "Source postimage mismatch",
    )


def planned(engine):
    return f"statements::engines::{engine}::every_planned_shape_answers_at_the_native_engine"


def passed(log, name, runner):
    if runner == "nextest":
        pattern = (
            r"^\s*PASS\s+\[[^\]\n]+\]\s+\(\s*\d+/\d+\)\s+sutura-app::golden\s+"
            + re.escape(name)
            + r"\s*$"
        )
    else:
        pattern = r"^test " + re.escape(name) + r" \.\.\. ok\s*$"
    return len(re.findall(pattern, log, re.MULTILINE)) == 1


def check_results(lane, log):
    runner = "nextest" if lane in ("core", "bigquery") else "libtest"
    names = (
        (
            [
                f"statements::{engine}::pins_planned_source_statements"
                for engine in ENGINES
            ]
            + [planned(engine) for engine in ENGINES[:4]]
        )
        if lane == "core"
        else [planned("oracle" if lane.startswith("oracle") else lane)]
    )
    for name in names:
        require(passed(log, name, runner), "Missing exactly-once actual PASS: " + name)
    systems = (
        ENGINES[:4]
        if lane == "core"
        else ("oracle" if lane.startswith("oracle") else lane,)
    )
    for system in systems:
        require(
            not re.search(
                r"exempt:\s*"
                + system
                + r"\s+from\s+(?:Unavailable|SUTURA_ONLY_SYSTEMS)\b",
                log,
            ),
            "Native availability/narrowing exemption refused: " + system,
        )
    if lane == "core":
        require(
            not re.search(r"\bexempt:|\bSKIPPED\b", log), "Core exemption/skip refused"
        )
        require(
            re.search(r"\b11 tests run: 11 passed\b", log),
            "Core did not run exactly 11 selected cells",
        )
    if lane == "cube":
        for name in (
            "tests::the_live_cube_serves_the_recorded_definitions",
            "catalogs::cube::the_catalog_is_pinned_as_a_whole",
        ):
            require(
                passed(log, name, "libtest"),
                "Missing live Cube equality/catalog PASS: " + name,
            )
    return names


def run(command, path, env):
    with path.open("xb") as output:
        process = subprocess.Popen(
            command,
            cwd=SOURCE,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        for line in iter(process.stdout.readline, b""):
            output.write(line)
            sys.stdout.buffer.write(line)
            sys.stdout.buffer.flush()
        require(process.wait() == 0, "Venue failed; see " + path.name)
    return path.read_text(errors="replace")


def files(directory):
    result = {}
    for path in directory.rglob("*"):
        require(not path.is_symlink(), "Artifact symlink refused")
        if path.is_file() and path != directory / "manifest.json":
            result[str(path.relative_to(directory))] = sha(path.read_bytes())
    return result


def read_artifact(directory):
    manifest = json.loads((directory / "manifest.json").read_bytes())
    require(
        manifest["context"] == context(), "Artifact source/helper/request/run mismatch"
    )
    require(manifest["files"] == files(directory), "Artifact file/hash mismatch")
    name = manifest["lane"]
    require(name in LANES, "Unknown artifact lane")
    expected = {phase + "/venue.log" for phase in ("update", "no-update")}
    expected |= {
        phase + "/generated/" + path
        for phase in ("update", "no-update")
        for path in OUTPUTS[name]
    }
    if name in ("cube", "bigquery"):
        expected.add("provision.log")
    if name == "core":
        expected.add("nextest-help.log")
    if name == "cube":
        expected |= {phase + "/cube-meta.raw.json" for phase in ("update", "no-update")}
    if name == "oracle-native":
        expected |= {
            phase + "/" + file
            for phase in ("update", "no-update")
            for file in (
                "sqlnet.log",
                "server.log",
                "public-dh.log",
                "planned-golden.log",
            )
        }
    require(set(manifest["files"]) == expected, "Unexpected/missing artifact file")
    return manifest


def cube_meta(out, update, env):
    endpoint = (
        subprocess.check_output(
            ["nix", "run", ".#just", "--", "dev-endpoint", "cube"], cwd=SOURCE, env=env
        )
        .decode()
        .strip()
    )
    parsed = urllib.parse.urlsplit("http://" + endpoint)
    require(
        parsed.hostname
        and ipaddress.ip_address(parsed.hostname).is_loopback
        and parsed.port,
        "Cube endpoint must be loopback",
    )
    require(
        not parsed.username
        and not parsed.password
        and not parsed.path
        and not parsed.query
        and not parsed.fragment,
        "Invalid Cube endpoint",
    )

    def encode(data):
        return base64.urlsafe_b64encode(data).rstrip(b"=")

    now = int(time.time())
    message = (
        encode(b'{"alg":"HS256","typ":"JWT"}')
        + b"."
        + encode(
            json.dumps({"sub": "sutura-catalog", "iat": now, "exp": now + 600}).encode()
        )
    )
    token = (
        message
        + b"."
        + encode(
            hmac.digest(b"sutura-dev-cube-secret-with-256-bits", message, "sha256")
        )
    ).decode()
    request = urllib.request.Request(
        "http://" + endpoint + "/cubejs-api/v1/meta",
        headers={"Authorization": "Bearer " + token},
    )

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args):
            return None

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(request, timeout=30) as response:
        raw = response.read((1 << 20) + 1)
    require(len(raw) <= 1 << 20, "Cube metadata exceeds 1 MiB")
    (out / "cube-meta.raw.json").write_bytes(raw)
    actual = json.loads(raw)
    require(
        isinstance(actual, dict) and isinstance(actual.get("cubes"), list),
        "Invalid live Cube metadata",
    )
    if update:
        (SOURCE / META).write_text(
            json.dumps(actual, indent=2, ensure_ascii=False) + "\n"
        )
    else:
        require(
            actual == json.loads((SOURCE / META).read_bytes()),
            "Live Cube metadata differs from collected fixture",
        )


def changed_paths():
    return set(git("diff", "--name-only").splitlines()) | set(
        git("ls-files", "--others", "--exclude-standard").splitlines()
    )


def lane(name, out):
    require(name in LANES, "Unknown native lane")
    require(
        git("write-tree") == binding()["source_tree"] and not changed_paths(),
        "Wrong initial source",
    )
    out.mkdir(parents=True, exist_ok=False)
    env = dict(
        os.environ,
        NO_COLOR="1",
        CLICOLOR="0",
        CARGO_TERM_COLOR="never",
        NEXTEST_SUCCESS_OUTPUT="immediate",
    )
    if name == "core":
        # Help inspection only: the tests themselves still run through the named task.
        help_text = subprocess.check_output(
            ["cargo", "nextest", "run", "--help"], cwd=SOURCE, env=env
        )
        require(
            b"NEXTEST_SUCCESS_OUTPUT" in help_text,
            "Pinned nextest cannot expose passing-test exemptions",
        )
        (out / "nextest-help.log").write_bytes(help_text)
    env.pop("SUTURA_ONLY_SYSTEMS", None)
    env.pop("SUTURA_DEV_REQUIRE_TIER", None)
    env.pop("SUTURA_ORACLE_FIXTURE_TRANSPORT", None)
    if name == "bigquery":
        env["GOOGLE_APPLICATION_CREDENTIALS"] = str(
            Path(os.environ["RUNNER_TEMP"]) / "bq-key.json"
        )
        env["SUTURA_BQ_PROJECT"] = os.environ["SUTURA_BQ_PROJECT"]
        run(
            ["nix", "run", ".#bigquery-provision", "--", "--no-capture"],
            out / "provision.log",
            env,
        )
    if name == "cube":
        run(["nix", "run", ".#just", "--", "dev-up-cube"], out / "provision.log", env)
    if name == "oracle-verified":
        env["SUTURA_ORACLE_FIXTURE_TRANSPORT"] = "verified"
    command = {
        "core": ["nix", "run", ".#just", "--", "test-only", CORE_FILTER],
        "cube": ["nix", "run", ".#just", "--", "ci-cube-tier"],
        "oracle-verified": ["nix", "run", ".#oracle-acceptance", "--", "--no-capture"],
        "oracle-native": ["nix", "run", ".#oracle-native-acceptance"],
        "bigquery": ["nix", "run", ".#bigquery-conformance", "--", "--no-capture"],
    }[name]
    phases = {}
    for phase, setting in (("update", "always"), ("no-update", "no")):
        evidence = out / phase
        evidence.mkdir()
        env["INSTA_UPDATE"] = setting
        if name == "cube":
            cube_meta(evidence, setting == "always", env)
        log = run(command, evidence / "venue.log", env)
        names = check_results(name, log)
        if name == "oracle-native":
            for file in (
                "sqlnet.log",
                "server.log",
                "public-dh.log",
                "planned-golden.log",
            ):
                shutil.copyfile(
                    SOURCE / "target/oracle-native-probe" / file, evidence / file
                )
            strong = (evidence / "sqlnet.log").read_text()
            require(
                all(
                    line in strong
                    for line in (
                        "SQLNET.ENCRYPTION_SERVER=REQUIRED",
                        "SQLNET.ENCRYPTION_TYPES_SERVER=(AES256)",
                        "SQLNET.CRYPTO_CHECKSUM_SERVER=REQUIRED",
                        "SQLNET.CRYPTO_CHECKSUM_TYPES_SERVER=(SHA256)",
                    )
                ),
                "Missing strong native service witness",
            )
            corpus = (evidence / "planned-golden.log").read_text()
            check_results(name, corpus)
            require(
                re.search(
                    r"^test result: ok\. 1 passed; 0 failed; 0 ignored;",
                    corpus,
                    re.MULTILINE,
                ),
                "Native strong-phase corpus did not run once",
            )
        changed = changed_paths()
        require(
            changed <= set(OUTPUTS[name]),
            "Venue changed an unapproved source/generated path",
        )
        if setting == "no":
            require(
                not changed and git("write-tree") == phases["update"]["tree"],
                "No-update venue mutated its exact lane tree",
            )
        else:
            subprocess.run(["git", "add", "--", *OUTPUTS[name]], cwd=SOURCE, check=True)
        for path in OUTPUTS[name]:
            source = SOURCE / path
            require(
                source.is_file() and not source.is_symlink(),
                "Missing generated file: " + path,
            )
            if name == "bigquery":
                require(
                    all(
                        value.encode() not in source.read_bytes()
                        for value in (
                            env["SUTURA_BQ_PROJECT"],
                            env["SUTURA_BQ_DATASET"],
                        )
                    ),
                    "Resource identifier in generated output",
                )
            target = evidence / "generated" / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        phases[phase] = {
            "tree": git("write-tree"),
            "passed": names,
            "generated": {
                path: sha((SOURCE / path).read_bytes()) for path in OUTPUTS[name]
            },
        }
    require(
        phases["update"]["generated"] == phases["no-update"]["generated"],
        "Lane snapshot drift",
    )
    dump(
        out / "manifest.json",
        {"context": context(), "lane": name, "phases": phases, "files": files(out)},
    )


def collect(incoming, out):
    require(
        git("write-tree") == binding()["source_tree"] and not changed_paths(),
        "Collector source mismatch",
    )
    require(
        {p.name for p in incoming.iterdir()}
        == {"sql-planned-" + lane for lane in LANES},
        "Missing/unexpected lane artifact",
    )
    merged = {}
    witnesses = {}
    for name in LANES:
        directory = incoming / ("sql-planned-" + name)
        manifest = read_artifact(directory)
        require(
            manifest["lane"] == name
            and set(manifest["phases"]) == {"update", "no-update"},
            "Wrong lane/phase",
        )
        for phase in ("update", "no-update"):
            check_results(
                name, (directory / phase / "venue.log").read_text(errors="replace")
            )
            if name == "cube":
                require(
                    json.loads((directory / phase / "cube-meta.raw.json").read_bytes())
                    == json.loads(
                        (directory / phase / "generated" / META).read_bytes()
                    ),
                    "Cube fixture is not its recorded live response",
                )
            if name == "oracle-native":
                check_results(
                    name,
                    (directory / phase / "planned-golden.log").read_text(
                        errors="replace"
                    ),
                )
            require(
                set(manifest["phases"][phase]["generated"]) == set(OUTPUTS[name]),
                "Wrong generated path set",
            )
            for path, digest in manifest["phases"][phase]["generated"].items():
                data = (directory / phase / "generated" / path).read_bytes()
                require(sha(data) == digest, "Generated postimage hash mismatch")
                require(
                    path not in merged or merged[path] == data,
                    "Different duplicate generated witness: " + path,
                )
                merged[path] = data
        require(
            manifest["phases"]["update"] == manifest["phases"]["no-update"],
            "Update/no-update witness drift",
        )
        witnesses[name] = {
            "manifest_sha256": sha((directory / "manifest.json").read_bytes()),
            "lane_tree": manifest["phases"]["no-update"]["tree"],
        }
    require(
        set(merged) == ALLOWED and set(SHAPES + list(ROWS.values())) <= set(merged),
        "Missing 14 planned snapshots",
    )
    out.mkdir(parents=True, exist_ok=False)
    for path, data in merged.items():
        target = SOURCE / path
        require(not target.is_symlink(), "Source symlink refused")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    subprocess.run(["git", "add", "--", *sorted(merged)], cwd=SOURCE, check=True)
    require(
        set(
            git(
                "diff", "--cached", "--name-only", binding()["source_tree"]
            ).splitlines()
        )
        <= ALLOWED,
        "Non-generated collector delta",
    )
    patch = subprocess.check_output(
        ["git", "diff", "--cached", "--binary", binding()["source_tree"]], cwd=SOURCE
    )
    (out / "generated.patch").write_bytes(patch)
    dump(
        out / "generated.json",
        {
            "context": context(),
            "tree": git("write-tree"),
            "generated_patch_sha256": sha(patch),
            "generated": {path: sha(data) for path, data in merged.items()},
            "witnesses": witnesses,
            "draft_only": True,
            "final_merged_tree_native_proof": "ordinary exact-head PR CI required",
            "readiness": False,
        },
    )


def receipt(out):
    value = binding()
    generated = json.loads((out / "generated.json").read_bytes())
    api = git("rev-parse", value["api_branch"])
    child = git("rev-parse", value["behavior_branch"])
    require(
        git("show", "-s", "--format=%P", api) == value["base"]
        and git("show", "-s", "--format=%T", api) == value["parent_tree"],
        "API parent mismatch",
    )
    require(
        git("show", "-s", "--format=%P", child) == api
        and git("show", "-s", "--format=%T", child) == generated["tree"],
        "Behavior child mismatch",
    )
    require(
        not git("status", "--porcelain=v1", "--untracked-files=all"),
        "Dirty packaged source",
    )
    refs = {
        "refs/heads/" + value["api_branch"]: api,
        "refs/heads/" + value["behavior_branch"]: child,
    }
    require(
        dict(
            line.split()[::-1]
            for line in git(
                "bundle", "list-heads", str(out / "source.bundle")
            ).splitlines()
        )
        == refs,
        "Bundle heads mismatch",
    )
    dump(
        out / "receipt.json",
        {
            **generated,
            "api_commit": api,
            "behavior_commit": child,
            "base": value["base"],
            "refs": refs,
            "bundle_sha256": sha((out / "source.bundle").read_bytes()),
            "normal_hooks": True,
            "hosted_push": False,
            "files": files(out),
        },
    )


def self_check():
    names = [
        f"statements::{engine}::pins_planned_source_statements" for engine in ENGINES
    ]
    names += [planned(engine) for engine in ENGINES[:4]]
    log = "\n".join(
        f"        PASS [   0.010s] ({index:2}/11) sutura-app::golden {name}"
        for index, name in enumerate(names, 1)
    )
    log += "\nSummary [1s] 11 tests run: 11 passed, 100 skipped\n"
    assert len(check_results("core", log)) == 11
    for bad in (
        log.replace("PASS", "FAIL", 1),
        log + "exempt: postgres from Unavailable\n",
    ):
        try:
            check_results("core", bad)
        except SystemExit:
            pass
        else:
            raise AssertionError("False native proof accepted")
    live = f"        PASS [   0.010s] (1/1) sutura-app::golden {planned('bigquery')}\n"
    live += "exempt: bigquery from DryRun - unrelated capability cell\n"
    live += "exempt: bigquery from KeyProbe - unrelated capability cell\n"
    assert check_results("bigquery", live) == [planned("bigquery")]
    for notice in ("Unavailable", "SUTURA_ONLY_SYSTEMS"):
        try:
            check_results(
                "bigquery",
                live + f"exempt: bigquery from {notice} - native corpus unavailable\n",
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("Native availability/narrowing exemption accepted")
    assert (
        len(SHAPES + list(ROWS.values()))
        == len(set(SHAPES + list(ROWS.values())))
        == 14
    )
    assert (
        ROWS["oracle"] in OUTPUTS["oracle-native"]
        and ROWS["oracle"] in OUTPUTS["oracle-verified"]
    )
    print("collector self-check passed")


if __name__ == "__main__":
    task, *args = sys.argv[1:]
    if task == "guard":
        guard()
    elif task == "prepare":
        apply_source(args[0])
    elif task == "lane":
        lane(args[0], Path(args[1]).resolve())
    elif task == "collect":
        collect(Path(args[0]).resolve(), Path(args[1]).resolve())
    elif task == "receipt":
        receipt(Path(args[0]).resolve())
    elif task == "self-check":
        self_check()
    else:
        raise SystemExit("Unknown task")

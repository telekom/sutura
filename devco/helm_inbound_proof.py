#!/usr/bin/env python3
"""Private, unbound draft. No execution authority until separately frozen and reviewed."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile

SCRIPT = "charts/sutura/testdata/inbound-cells.sh"
HELPERS = "templates/_helpers.tpl"
FIXTURE = "testdata/values/pool/reading-workers.yaml"
PIN_INPUTS = {"flake.nix", "flake.lock", "nix/helm-chart.nix", "nix/run-gate.sh", "justfile"}
HELM_APP = {"installable": ".#helm", "system": "x86_64-linux", "program_attribute": ".#apps.x86_64-linux.helm.program"}
LOCK_FLAGS = ["--no-update-lock-file", "--no-write-lock-file"]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def inventory(chart):
    rows = []
    for path in sorted(chart.rglob("*")):
        require(not path.is_symlink(), f"unreviewed chart symlink: {path}")
        if path.is_file():
            rows.append([path.relative_to(chart).as_posix(), path.stat().st_mode & 0o777, digest(path.read_bytes())])
    return {"files": len(rows), "sha256": digest(json.dumps(rows, separators=(",", ":")).encode())}


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def guard(root, expected):
    require(set(expected["pin_inputs"]) == PIN_INPUTS, "pin input census drift")
    observed = {
        "sha": git(root, "rev-parse", "HEAD").decode().strip(),
        "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
        "index_sha256": digest(git(root, "ls-files", "--stage", "-z")),
        "chart": inventory(root / "charts/sutura"),
        "pin_inputs": {relative: digest((root / relative).read_bytes()) for relative in sorted(PIN_INPUTS)},
    }
    require(not git(root, "status", "--porcelain=v1", "--untracked-files=all"), "source/index is dirty")
    require(observed == expected, "source/index/chart/pin identity drift")
    return dict(observed, clean=True, matched=True)


def split_script(raw, census):
    starts = list(re.finditer(rb"^(accepts|refuses) ([a-z][a-z-]+) ", raw, re.M))
    require(len(starts) == len(census["cases"]) == 12, "case count drift")
    header = raw[:starts[0].start()]
    bodies = [raw[m.start():starts[i + 1].start() if i + 1 < len(starts) else len(raw)] for i, m in enumerate(starts)]
    require(header + b"".join(bodies) == raw, "script partition lost bytes")
    require(digest(raw) == census["script_sha256"] and digest(header) == census["header_sha256"], "header/script drift")
    for match, body, expected in zip(starts, bodies, census["cases"], strict=True):
        require(match.group(2).decode() == expected["name"] and digest(body) == expected["sha256"], "case census/hash drift")
    return header, bodies


def command(args, cwd, log, timeout, env=None):
    with log.open("wb") as output:
        process = subprocess.Popen(args, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            code = None
    return {"exit": code, "timeout": code is None, "log": log.name, "log_sha256": digest(log.read_bytes())}


def cases(label, chart, output, header, bodies, census, env, timeout):
    folder = output / label
    folder.mkdir()
    rows = {}
    # Each fresh bash owns set -e and exit; a failed child never skips a later case.
    for body, cell in zip(bodies, census["cases"], strict=True):
        name = cell["name"]
        work = folder / name
        work.mkdir()
        shell = work / "case.sh"
        shell.write_bytes(header + body)
        row = command(["bash", str(shell), str(chart), str(work)], output, work / "shell.log", timeout, env)
        row["case_script_sha256"] = digest(shell.read_bytes())
        row["case_body_sha256"] = digest(body)
        log = (work / "shell.log").read_text(errors="replace")
        row["status"] = "PASS" if row["exit"] == 0 and log.splitlines().count(f"inbound/{name}: PASS") == 1 else "FAIL" if row["exit"] == 1 else "NO_VERDICT"
        render = work / "inbound-cells" / f"{name}.log"
        text = render.read_text(errors="replace") if render.exists() else ""
        row["render_log_sha256"] = digest(render.read_bytes()) if render.exists() else None
        row["own_observation_mismatch"] = row["exit"] == 1 and bool(re.search(
            rf"inbound-observation\.yaml:\d+:\d+\): inbound/{re.escape(name)}: reader (?:orders|finance) lost or changed its declaration(?:\r?\n|$)", text))
        row["causal_verdict"] = None  # Base FAIL alone never becomes causal credit.
        rows[name] = row
        write(folder / "results.json", rows)
    require(list(rows) == [cell["name"] for cell in census["cases"]], "incomplete case outcomes")
    return rows


def green(rows):
    return rows is not None and len(rows) == 12 and all(row["status"] == "PASS" for row in rows.values())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("plan", "head", "base", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    plan_bytes = args.plan.read_bytes()
    runner_bytes = Path(__file__).read_bytes()
    plan = json.loads(plan_bytes)
    binding = plan["binding"]
    require(plan["execution_authorized"] is True and plan["independent_review_accepted"] is True, "draft: no execution authority")
    require(all(binding.get(key) for key in ("head", "base", "runner_sha256", "helm_app")), "draft: source/tool/helper binding is null")
    require(digest(runner_bytes) == binding["runner_sha256"], "runner drift")
    require(binding["helm_app"] == HELM_APP, "Helm app/system binding drift")
    require(os.environ.get("RUNNER_OS") == "Linux" and os.environ.get("RUNNER_ARCH") == "X64", "hosted x86_64 Linux only")
    head, base, output = args.head.resolve(), args.base.resolve(), args.output.resolve()
    require(head != base and not output.is_relative_to(head) and not output.is_relative_to(base), "separate source/output roots required")
    output.mkdir(parents=True, exist_ok=False)
    (output / "plan.json").write_bytes(plan_bytes)
    (output / "helm_inbound_proof.py").write_bytes(runner_bytes)
    summary = {"head": None, "base": None, "mutants": [], "restored": None, "collection_complete": False, "base_causal_review_required": True, "final_guard_success": False,
               "plan_sha256": digest(plan_bytes), "runner_sha256": digest(runner_bytes), "observed_workflow_sha": os.environ.get("GITHUB_WORKFLOW_SHA"),
               "run_id": os.environ.get("GITHUB_RUN_ID"), "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT")}
    write(output / "summary.json", summary)
    summary["checked_initial_sources"] = {"head": guard(head, binding["head"]), "base": guard(base, binding["base"])}
    write(output / "summary.json", summary)
    census = plan["census"]
    header, bodies = split_script((head / SCRIPT).read_bytes(), census)
    require((head / "charts/sutura" / FIXTURE).read_bytes() == (base / "charts/sutura" / FIXTURE).read_bytes(), "base fixture differs; overlay needs separate review")
    require(digest((head / "charts/sutura" / FIXTURE).read_bytes()) == census["fixture_sha256"], "fixture drift")
    require([m["cell"] for m in plan["mutants"]] == ["map-over-list", "map-merge"], "mutation scope drift")
    try:
        summary["helm_realize"] = command(["nix", "run", *LOCK_FLAGS, "--system", HELM_APP["system"], HELM_APP["installable"], "--", "version", "--short"], head, output / "helm-realize.log", plan["chart_timeout_seconds"])
        write(output / "summary.json", summary)
        require(summary["helm_realize"]["exit"] == 0, "final-source Helm app did not realize")
        program = subprocess.check_output(["nix", "eval", *LOCK_FLAGS, "--raw", HELM_APP["program_attribute"]], cwd=head).decode().strip()
        require(program.startswith("/nix/store/"), "Helm app is not a Nix store program")
        summary["tool"] = {"locked_app": HELM_APP, "lock_flags": LOCK_FLAGS, "source_sha": binding["head"]["sha"], "program": program,
                           "sha256": digest(Path(program).read_bytes()), "version": subprocess.check_output([program, "version", "--short"], cwd=head).decode().strip()}
        write(output / "summary.json", summary)
        summary["chart_head"] = command(["just", "chart"], head, output / "chart-head.log", plan["chart_timeout_seconds"])
        with tempfile.TemporaryDirectory(prefix="helm-cells-", dir=output) as temporary:
            scratch = Path(temporary)
            (scratch / "bin").mkdir()
            (scratch / "bin/helm").symlink_to(program)
            env = dict(os.environ, PATH=str(scratch / "bin") + os.pathsep + os.environ.get("PATH", ""))
            summary["head"] = cases("head", head / "charts/sutura", output, header, bodies, census, env, plan["case_timeout_seconds"])
            summary["base"] = cases("base", base / "charts/sutura", output, header, bodies, census, env, plan["case_timeout_seconds"])
            for mutant in plan["mutants"]:
                name = mutant["cell"]
                record = {"cell": name, "status": "NOT_RUN", "kill": False}
                summary["mutants"].append(record)
                if not (summary["chart_head"]["exit"] == 0 and green(summary["head"]) and summary["base"][name]["status"] == "PASS"):
                    record["reason"] = "requires whole-chart/head green and actual named base PASS"
                    if green(summary["head"]) and summary["base"][name]["status"] == "FAIL":
                        record["status"] = "NOT_APPLICABLE_BASE_FAIL"
                    continue
                chart = scratch / name
                shutil.copytree(head / "charts/sutura", chart)
                original = (chart / HELPERS).read_bytes()
                before, after = mutant["before"].encode(), mutant["after"].encode()
                require(original.count(before) == 1, "mutation anchor drift")
                try:
                    (chart / HELPERS).write_bytes(original.replace(before, after, 1))
                    rows = cases(name + "-mutant", chart, output, header, bodies, census, env, plan["case_timeout_seconds"])
                    record["status"] = "OBSERVED"
                    record["own_mismatch_only"] = rows[name]["status"] == "FAIL" and rows[name]["own_observation_mismatch"] and all(row["status"] == "PASS" for cell, row in rows.items() if cell != name)
                finally:
                    (chart / HELPERS).write_bytes(original)
                require(inventory(chart) == binding["head"]["chart"], "scratch chart did not restore exactly")
                restored = cases(name + "-restored", chart, output, header, bodies, census, env, plan["case_timeout_seconds"])
                record["kill"] = record.get("own_mismatch_only", False) and green(restored)
            summary["restored"] = cases("restored", head / "charts/sutura", output, header, bodies, census, env, plan["case_timeout_seconds"])
    finally:
        write(output / "summary.json", summary)
        try:
            summary["checked_pre_restored_sources"] = {"head": guard(head, binding["head"]), "base": guard(base, binding["base"])}
            summary["chart_restored"] = command(["just", "chart"], head, output / "chart-restored.log", plan["chart_timeout_seconds"])
            summary["checked_final_sources"] = {"head": guard(head, binding["head"]), "base": guard(base, binding["base"])}
            if "tool" in summary:
                summary["tool"]["final_sha256"] = digest(Path(summary["tool"]["program"]).read_bytes())
                require(summary["tool"]["final_sha256"] == summary["tool"]["sha256"], "observed Helm binary changed")
                summary["final_guard_success"] = True
        finally:
            write(output / "summary.json", summary)
    summary["collection_complete"] = summary["final_guard_success"] and summary["chart_head"]["exit"] == summary["chart_restored"]["exit"] == 0 and green(summary["head"]) and green(summary["restored"]) and len(summary["base"] or {}) == 12 and all(row["status"] != "NO_VERDICT" for row in summary["base"].values()) and all(record["kill"] or record["status"] == "NOT_APPLICABLE_BASE_FAIL" for record in summary["mutants"])
    write(output / "summary.json", summary)
    require(summary["collection_complete"], "incomplete: inspect per-case outcomes; no automatic base causal verdict")


if __name__ == "__main__":
    main()

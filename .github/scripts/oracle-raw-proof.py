"""Hosted raw-SQL base and killing-mutation evidence, using the existing fixture venue."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

SOURCE = "976bf04b52724dc45fd6bf95e0277e5d5ddec033"
TREE = "4eaae03d1f117799821d534f7648ec4d7fe24e53"
BASE = "fadd25ee2ae393e9312ef0afe24140a16c7b12e3"
RUNNER = "nix/oracle-raw-cells.sh"
SERVED = "crates/sutura-cli/tests/served/oracle_raw.rs"
PORT = "crates/sutura-exec-oracle/tests/raw.rs"
AUDIT = "crates/sutura-app/src/surface/tests.rs"
RAW = "crates/sutura-exec-oracle/src/raw.rs"
PREFIX = "oracle_native::raw::"
CELLS = {
    "read-only": (
        RAW,
        '        connection\n            .execute("SET TRANSACTION READ ONLY", &[])\n            .map_err(execute_err_mapped)?;\n',
        "",
        PREFIX + "oracle_raw_for_update_and_adversarial_with_are_refused_without_changing_rows",
        SERVED,
        'assert_eq!(reply.status, 403, "{}", reply.body);',
        ["left: 200", "right: 403"],
        "c0a34b47405a8fa4b225d374dce3716c7819eaa6",
    ),
    "admission": (
        RAW,
        "    if !statement.is_query() {\n        return Err(OracleError::RawNotQuery);\n    }\n",
        "",
        "acceptance::oracle_raw_nonqueries_and_delimiters_are_refused_before_execution",
        PORT,
        "assert_eq!(\n                error.to_string(),",
        ['right: "this source accepts one SELECT or WITH query without statement delimiters"'],
        "7d5f54baa2e981560e0d9cd1e83ecf7ade51662c",
    ),
    "cap": (
        RAW,
        "most_rows: Some(sutura_domain::plan::MAX_ROWS as usize + 1)",
        "most_rows: None",
        "acceptance::oracle_raw_retains_the_cap_plus_one_refusal_witness",
        PORT,
        "assert_eq!(rows.rows().len(), cap as usize + 1);",
        ["left: 10002", "right: 10001"],
        "bd49938aa0c4063ca6cf2c2835c0fc4b61484a5f",
    ),
    "audit": (
        "crates/sutura-app/src/surface.rs",
        "                answered.executed_as(),\n",
        "                None,\n",
        "surface::tests::run_sql_writes_one_record_per_outcome_carrying_the_statement_text",
        AUDIT,
        'assert!(lines[0].contains("local=shared-service-user"), "{lines:?}");',
        ["executed_as=None"],
        "118caa8a7ff42202822e8f36d5b19aac8cf5389b",
    ),
}


def git(*args, cwd=None, env=None, data=None):
    return subprocess.check_output(["git", *args], cwd=cwd, env=env, input=data)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def record(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def manifest():
    result = {}
    for name in git("ls-files", "-z").decode().split("\0"):
        if name:
            path = Path(name)
            link = path.is_symlink()
            data = os.fsencode(os.readlink(path)) if link else path.read_bytes()
            result[name] = [digest(data), link, path.lstat().st_mode & 0o777]
    return result


def tree(normalize=False):
    with tempfile.TemporaryDirectory() as temporary:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(temporary) / "index"))
        git("read-tree", "HEAD", env=env)
        git("add", "--pathspec-from-file=-", "--pathspec-file-nul", env=env, data=git("ls-files", "-z"))
        if normalize:
            blob = git("rev-parse", f"{SOURCE}:{RUNNER}").decode().strip()
            mode = git("ls-tree", SOURCE, RUNNER).decode().split()[0]
            git("update-index", "--cacheinfo", f"{mode},{blob},{RUNNER}", env=env)
        return git("write-tree", env=env).decode().strip()


def state(out, label, normalize=False):
    current = manifest()
    record(out / f"{label}.json", {"committed_tree": git("rev-parse", "HEAD^{tree}").decode().strip(), "overlay_tree": tree(), "product_tree": tree(True) if normalize else None, "manifest_sha256": digest(json.dumps(current, sort_keys=True).encode())})
    (out / f"{label}.patch").write_bytes(git("diff", "--binary"))
    return current


def execute(command, log):
    with tempfile.TemporaryFile(mode="w+") as output:
        result = subprocess.run(["timeout", "--kill-after=30s", "90m", *command], stdout=output, stderr=subprocess.STDOUT, check=False)
        output.seek(0)
        text = output.read()
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets += [os.environ.get(key, "") for key in ["ORACLE_PASSWORD", "SUTURA_ORACLE_ACCEPTANCE_PASSWORD", "SUTURA_ORACLE_ACCEPTANCE_LEGACY_PASSWORD"]]
    for secret in sorted(filter(None, secrets), key=len, reverse=True):
        text = text.replace(secret, "***")
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    log.write_text(text)
    print(text, end="", flush=True)
    log.with_suffix(".status").write_text(f"{result.returncode}\n")
    return result.returncode, text


def named(text, cell, verdict):
    assert re.search(rf"^test {re.escape(cell)} \.\.\. {verdict}$", text, re.MULTILINE), (cell, verdict)


def command(cell):
    start = ["cargo", "test", "--locked", "--profile", os.environ["SUTURA_ORACLE_CARGO_PROFILE"]]
    if cell.startswith("acceptance::"):
        start += ["-p", "sutura-exec-oracle", "--features", "fixtures", "--test", "raw"]
    elif cell.startswith("surface::"):
        start += ["-p", "sutura-app", "--all-features", "--lib"]
    else:
        start += ["-p", "sutura-cli", "--features", "oracle,rdbms", "--test", "served"]
    return [*start, cell, "--", "--exact", "--nocapture", *([] if cell.startswith("surface::") else ["--ignored"])]


def assertion_line(path, marker, cell):
    source = Path(path).read_text()
    begin = source.index("fn " + cell.rsplit("::", 1)[-1] + "(")
    position = source.index(marker, begin)
    return source[:position].count("\n") + 1


def red(status, text, cell, path, line, needles):
    assert status != 0 and "test result: FAILED." in text
    named(text, cell, "FAILED")
    assert f"panicked at {path}:{line}:" in text, (cell, line)
    for needle in needles:
        assert needle in text, (cell, needle)


def diagnostic(out):
    baseline = state(out, "diagnostic-before")
    paths = ["crates/sutura-exec-oracle/src/lib.rs", SERVED]
    saved = {name: (Path(name).read_bytes(), Path(name).stat().st_mode & 0o777) for name in paths}
    try:
        path = Path(paths[0])
        before = "pub(crate) fn execute_err_mapped(cause: oracledb::Error) -> OracleError {\n"
        after = before + '''    let (code, kind) = match cause.kind() {
        oracledb::ErrorKind::DbError(error) => (Some(error.code()), "db_error"),
        oracledb::ErrorKind::CallTimeoutExceeded => (None, "call_timeout"),
        oracledb::ErrorKind::UnableToRecover => (None, "unable_to_recover"),
        oracledb::ErrorKind::NotConnected => (None, "not_connected"),
        _ => (None, "other_driver_error"),
    };
    eprintln!("oracle_raw_diagnostic code={code:?} kind={kind}");
'''
        assert path.read_text().count(before) == 1
        path.write_text(path.read_text().replace(before, after))
        path = Path(SERVED)
        before = '        let reply = ran(&served, statement);\n        assert_eq!(reply.status, 403, "{}", reply.body);\n'
        after = '''        let reply = ran(&served, statement);
        for line in served.awaiting("oracle_raw_diagnostic").iter().filter(|line| line.starts_with("oracle_raw_diagnostic ")) {
            println!("{line}");
        }
        assert_eq!(reply.status, 403, "{}", reply.body);
'''
        assert path.read_text().count(before) == 1
        path.write_text(path.read_text().replace(before, after))
        before = '    let audit = served.awaiting("raw_answered");\n'
        after = before + '''    for line in audit.iter().filter(|line| line.contains("raw_answered")) {
        println!("oracle_raw_diagnostic_audit={line}");
    }
'''
        assert path.read_text().count(before) == 1
        path.write_text(path.read_text().replace(before, after))
        state(out, "diagnostic-overlay")
        cell = PREFIX + "oracle_raw_for_update_and_adversarial_with_are_refused_without_changing_rows"
        line = assertion_line(SERVED, 'assert_eq!(reply.status, 403, "{}", reply.body);', cell)
        status, text = execute(command(cell), out / "read-only-diagnostic.log")
        red(status, text, cell, SERVED, line, ["left: 422", "right: 403"])
        codes = re.findall(r"^oracle_raw_diagnostic code=(None|Some\(\d+\)) kind=([a-z_]+)$", text, re.MULTILINE)
        assert codes
        record(out / "read-only-diagnostic.json", {"codes": codes, "diagnostic_only": True})
        cell = PREFIX + "oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix"
        line = assertion_line(SERVED, '.expect("the raw call has an audit record")', cell)
        status, text = execute(command(cell), out / "audit-diagnostic.log")
        red(status, text, cell, SERVED, line, ["the raw call has an audit record"])
        records = [json.loads(value) for value in re.findall(r"^oracle_raw_diagnostic_audit=(\{.*\})$", text, re.MULTILINE)]
        assert records
        record(out / "audit-diagnostic.json", {"records": records, "diagnostic_only": True})
    finally:
        for name, (data, mode) in saved.items():
            Path(name).write_bytes(data)
            Path(name).chmod(mode)
        assert state(out, "diagnostic-restored") == baseline


def claims(out, args):
    baseline = state(out, "overlay-before", True)
    assert tree(True) == TREE
    status, text = execute(["bash", os.environ["RAW_PROOF_ORIGINAL"], *args], out / "head.log")
    assert status == 0
    for cell in [
        "oracle_raw_select_and_with_keep_exact_numbers_and_dates",
        "oracle_raw_nonqueries_and_delimiters_are_refused_before_execution",
        "oracle_raw_lob_long_and_raw_values_are_refused_without_materialising_them",
        "oracle_raw_retains_the_cap_plus_one_refusal_witness",
        "oracle_raw_wide_metadata_and_collected_bytes_are_refused",
        "oracle_raw_spent_deadlines_and_other_credentials_never_run",
    ]:
        named(text, "acceptance::" + cell, "ok")
    for cell in [
        "oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix",
        "oracle_raw_for_update_and_adversarial_with_are_refused_without_changing_rows",
        "oracle_raw_deadline_discards_its_session_and_following_call_is_healthy",
        "oracle_raw_planned_quoted_aliases_and_offset_execute_at_the_source",
        "oracle_raw_federation_reads_two_registered_sources_only_when_listed_and_allowed",
    ]:
        named(text, PREFIX + cell, "ok")
    assert text.count("test " + PREFIX + "oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix ... ok") == 2
    assert manifest() == baseline
    for name, (path, before, after, cell, test, marker, needles, expected) in CELLS.items():
        if name == "audit" and os.environ["SUTURA_ORACLE_ACCEPTANCE_MODE"] != "native":
            continue
        status, text = execute(command(cell), out / f"{name}-green.log")
        assert status == 0
        named(text, cell, "ok")
        line = assertion_line(test, marker, cell)
        source = Path(path).read_bytes()
        assert source.count(before.encode()) == 1
        try:
            Path(path).write_bytes(source.replace(before.encode(), after.encode()))
            state(out, name + "-mutant", True)
            assert tree(True) == expected
            status, text = execute(command(cell), out / f"{name}-red.log")
            red(status, text, cell, test, line, needles)
        finally:
            Path(path).write_bytes(source)
            assert state(out, name + "-restore", True) == baseline
            assert tree(True) == TREE
        status, text = execute(command(cell), out / f"{name}-restored.log")
        assert status == 0
        named(text, cell, "ok")
        assert manifest() == baseline


def base_cells(out):
    baseline = state(out, "base-overlay-before")
    for cell in [
        "oracle_raw_select_and_with_keep_exact_numbers_and_dates",
        "oracle_raw_nonqueries_and_delimiters_are_refused_before_execution",
        "oracle_raw_lob_long_and_raw_values_are_refused_without_materialising_them",
        "oracle_raw_retains_the_cap_plus_one_refusal_witness",
        "oracle_raw_wide_metadata_and_collected_bytes_are_refused",
        "oracle_raw_spent_deadlines_and_other_credentials_never_run",
    ]:
        full = "acceptance::" + cell
        if "select_and_with" in cell:
            marker = "assert!(warehouse.accepts_raw_statements());"
            line = assertion_line(PORT, marker, full)
            needles = ["assertion failed: warehouse.accepts_raw_statements()"]
        elif "spent_deadlines" in cell:
            marker = '.expect("the source accepts raw text")'
            line = assertion_line(PORT, marker, full)
            needles = ["the source accepts raw text"]
        else:
            marker = '.expect("a shared Oracle source accepts raw statements")'
            line = Path(PORT).read_text().splitlines().index("            " + marker) + 1
            needles = ["a shared Oracle source accepts raw statements"]
        status, text = execute(command(full), out / f"{cell}.log")
        red(status, text, full, PORT, line, needles)
        assert manifest() == baseline
    for features, cell in [
        ("oracle,rdbms", "oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix"),
        ("oracle,rdbms,sql", "oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix"),
        ("oracle,rdbms,sql", "oracle_raw_planned_quoted_aliases_and_offset_execute_at_the_source"),
        ("oracle,rdbms,sql", "oracle_raw_federation_reads_two_registered_sources_only_when_listed_and_allowed"),
    ]:
        full = PREFIX + cell
        marker = 'assert_eq!(reply.status, 200, "{}", reply.body);'
        line = assertion_line(SERVED, marker, full)
        invocation = ["cargo", "test", "--locked", "--profile", os.environ["SUTURA_ORACLE_CARGO_PROFILE"], "-p", "sutura-cli", "--features", features, "--test", "served", full, "--", "--exact", "--ignored", "--nocapture"]
        status, text = execute(invocation, out / f"{features.replace(',', '-')}-{cell}.log")
        red(status, text, full, SERVED, line, ["right: 200"])
        assert manifest() == baseline
    state(out, "base-overlay-after")


def main():
    if sys.argv[1] == "cell":
        out = Path(os.environ["RAW_PROOF_OUT"]) / os.environ["SUTURA_ORACLE_ACCEPTANCE_MODE"]
        out.mkdir(parents=True, exist_ok=True)
        if sys.argv[2] == "raw-diagnostic":
            diagnostic(out)
        elif sys.argv[2] == "raw-claims":
            claims(out, sys.argv[3:])
        else:
            base_cells(out)
        return
    operation = sys.argv[1]
    assert operation in ("raw-base", "raw-claims", "raw-diagnostic")
    assert os.environ["GITHUB_ACTIONS"] == "true"
    assert git("rev-parse", "HEAD").decode().strip() == SOURCE
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert not git("status", "--porcelain")
    root = Path.cwd()
    out = root / "target/oracle-raw-proof" / operation
    out.mkdir(parents=True, exist_ok=False)
    original = manifest()
    record(out / "source-files.json", original)
    record(out / "inputs.json", {"source": SOURCE, "tree": TREE, "base": BASE, "operation": operation, "helper_sha": os.environ["GITHUB_WORKFLOW_SHA"], "payload_sha256": digest(Path(__file__).read_bytes())})
    base_root = None
    base_original = None
    saved = {}
    index = None
    try:
        if operation == "raw-base":
            assert os.environ["RAW_BASE_SHA"] == BASE
            git("merge-base", "--is-ancestor", BASE, SOURCE)
            base_root = Path(tempfile.mkdtemp(prefix="raw-base-", dir=root / "target"))
            git("worktree", "add", "--detach", str(base_root), BASE)
            os.chdir(base_root)
            base_original = manifest()
            record(out / "base-files.json", base_original)
            parent = "crates/sutura-cli/tests/served/oracle_native.rs"
            append = b'\n#[path = "oracle_raw.rs"]\nmod raw;\n'
            assert git("show", f"{SOURCE}:{parent}") == Path(parent).read_bytes() + append
            paths = [parent, SERVED, PORT, "nix/oracle-login-fixtures.sh", RUNNER]
        else:
            paths = [RUNNER]
            if operation == "raw-diagnostic":
                paths += ["nix/oracle-login-fixtures.sh", "nix/oracle-native-cells.sh"]
        index_path = Path(git("rev-parse", "--git-path", "index").decode().strip()).resolve()
        index = (index_path, index_path.read_bytes())
        saved = {name: (Path(name).read_bytes(), Path(name).stat().st_mode & 0o777) if Path(name).exists() else None for name in paths}
        if operation == "raw-base":
            for name in paths[:-1]:
                Path(name).write_bytes(git("show", f"{SOURCE}:{name}"))
        elif operation == "raw-claims":
            original_runner = out / "original-runner.sh"
            original_runner.write_bytes(Path(RUNNER).read_bytes())
            os.environ["RAW_PROOF_ORIGINAL"] = str(original_runner)
        if operation == "raw-diagnostic":
            path = Path("nix/oracle-login-fixtures.sh")
            marker = 'if [ "${SUTURA_ORACLE_CLAIM_PROOF:-0}" = 1 ]; then\n'
            assert path.read_text().count(marker) == 1
            path.write_text(path.read_text().split(marker)[0] + 'bash nix/oracle-raw-cells.sh "$name" "$service" "$legacy_user"\n')
            Path("nix/oracle-native-cells.sh").write_text("#!/usr/bin/env bash\nset -euo pipefail\n")
        script = Path(__file__).resolve()
        Path(RUNNER).write_text(f'#!/usr/bin/env bash\nset -euo pipefail\nexec python3 {script} cell {operation} "$@"\n')
        if operation == "raw-base":
            git("add", "-N", SERVED, PORT, RUNNER)
        state(out, "injected", operation == "raw-claims")
        for key in ["SUTURA_ORACLE_BASE_SHA", "SUTURA_ORACLE_BASE_PROOF", "SUTURA_ORACLE_CLAIM_PROOF"]:
            os.environ.pop(key, None)
        os.environ["RAW_PROOF_OUT"] = str(out)
        status, _ = execute(["nix", "run", "--no-write-lock-file", ".#oracle-native-acceptance"], out / "venue.log")
        cleanup_status, cleanup_text = execute(["docker", "ps", "-a", "--format", "{{.Names}}"], out / "fixture-cleanup.log")
        assert cleanup_status == 0
        for mode in ["native", "legacy"]:
            assert f"sutura-oracle-{mode}-{os.environ['GITHUB_RUN_ID']}" not in cleanup_text.splitlines()
        assert status == 0
    finally:
        for name, previous in saved.items():
            if previous is None:
                Path(name).unlink(missing_ok=True)
            else:
                Path(name).write_bytes(previous[0])
                Path(name).chmod(previous[1])
        if index is not None:
            assert index[0].read_bytes() == index[1] or operation == "raw-base"
            index[0].write_bytes(index[1])
            record(out / "index-restored.json", {"before": digest(index[1]), "after": digest(index[0].read_bytes())})
        if base_root is not None:
            assert manifest() == base_original
            assert not git("status", "--porcelain")
            state(out, "base-restored")
            for fixture in ["oracle-native-probe", "oracle-legacy-probe"]:
                path = Path("target") / fixture
                if path.exists():
                    shutil.copytree(path, out / fixture)
            os.chdir(root)
            git("worktree", "remove", str(base_root))
        assert manifest() == original
        assert not git("status", "--porcelain")
        assert git("rev-parse", "HEAD").decode().strip() == SOURCE
        state(out, "restored")


if __name__ == "__main__":
    main()

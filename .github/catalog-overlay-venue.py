"""Bind the existing native venue and unchanged mutation runner to one source."""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path


def guard() -> dict:
    helper = Path(__file__).resolve().parent
    root = helper.parent
    binding = json.loads((helper / "payloads/catalog-overlay.json").read_text())
    expected = os.environ["EXPECTED_HELPER_SHA"]
    if not (
        os.environ["GITHUB_ACTIONS"] == "true"
        and len(expected) == 40
        and expected == os.environ["GITHUB_SHA"] == os.environ["WORKFLOW_SHA"]
        and subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root)
        .decode()
        .strip()
        == expected
        and not subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=no"], cwd=root
        )
    ):
        raise ValueError("The reviewed helper binding is required")
    for name, digest in binding["helper_files"].items():
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f"Helper bytes differ: {name}")
    return binding


def main() -> None:
    binding = guard()
    if sys.argv[1] == "guard":
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write(f"source={binding['source_sha']}\n")
        return
    if sys.argv[1] != "venue":
        raise ValueError("Expected guard or venue")
    helper = Path(__file__).resolve().parent
    source = helper.parent / "source"
    for expression, expected in (
        ("HEAD", binding["source_sha"]),
        ("HEAD^{tree}", binding["source_tree"]),
    ):
        actual = (
            subprocess.check_output(["git", "-C", str(source), "rev-parse", expression])
            .decode()
            .strip()
        )
        if actual != expected:
            raise ValueError(f"Source differs: {expression}")
    parents = (
        subprocess.check_output(
            ["git", "-C", str(source), "show", "-s", "--format=%P", "HEAD"]
        )
        .decode()
        .split()
    )
    if parents != [binding["source_parent_sha"]] or subprocess.check_output(
        ["git", "-C", str(source), "status", "--porcelain"]
    ):
        raise ValueError("The clean source and exact parent are required")
    out = Path(os.environ["RUNNER_TEMP"]) / "catalog-overlay-venue"
    out.mkdir(exist_ok=False)
    installable = ".#apps.x86_64-linux.oracle-native-acceptance.program"
    app = Path(
        subprocess.check_output(
            ["nix", "eval", "--raw", installable], cwd=source
        ).decode()
    )
    context = json.loads(
        subprocess.check_output(
            ["nix", "eval", "--json", "--apply", "builtins.getContext", installable],
            cwd=source,
        )
    )
    if len(context) != 1 or not all(path.endswith(".drv") for path in context):
        raise ValueError("One generated venue derivation is required")
    subprocess.run(["nix-store", "--realise", *context], check=True, cwd=source)
    original = app.read_text(encoding="utf-8")
    script = original
    for needle, replacement, count in (
        (
            "bash nix/oracle-native-probe.sh",
            'bash "$SUTURA_CATALOG_HELPER_DIR/oracle-catalog-overlay-native-probe.sh"',
            2,
        ),
        (
            "bash nix/oracle-legacy-probe.sh",
            'bash "$SUTURA_CATALOG_HELPER_DIR/oracle-catalog-overlay-legacy-probe.sh"',
            1,
        ),
    ):
        if script.count(needle) != count:
            raise ValueError(f"Unexpected generated venue: {needle}")
        script = script.replace(needle, replacement)
    needle = "export SUTURA_ORACLE_RUN_ACCEPTANCE=1"
    if script.count(needle) != 1:
        raise ValueError("The generated venue must expose one acceptance binding")
    provision = """
bash nix/oracle-tier-tls.sh
export SUTURA_DEV_USER=sutura
export SUTURA_DEV_PASSWORD="$(openssl rand -hex 12)"
printf '::add-mask::%s\\n' "$SUTURA_DEV_PASSWORD"
cargo run -q -p xtask -- dev-up --with oracle
export SUTURA_DEV_REQUIRE_TIER=1
export SUTURA_ORACLE_TIER_CA="$PWD/target/oracle-tier-tls/ca.pem"
"""
    script = script.replace(needle, needle + "\n" + provision, 1)
    (out / "venue-original.sh").write_text(original, encoding="utf-8")
    (out / "venue-proof.sh").write_text(script, encoding="utf-8")
    (out / "context.json").write_text(
        json.dumps(
            {
                "app": str(app),
                "context": context,
                "original_sha256": hashlib.sha256(original.encode()).hexdigest(),
                "proof_sha256": hashlib.sha256(script.encode()).hexdigest(),
                "binding": binding,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    env = dict(os.environ)
    for name in (
        "BASH_ENV",
        "CARGO_NET_OFFLINE",
        "CARGO_PROFILE_DEV_CODEGEN_BACKEND",
        "CARGO_UNSTABLE_CODEGEN_BACKEND",
        "SUTURA_ONLY_SYSTEMS",
        "SUTURA_PROOF_BOUND",
    ):
        env.pop(name, None)
    env.update(
        SUTURA_PROOF_HELPER=str(helper / "inbound_verifier_proof.py"),
        SUTURA_PROOF_PLAN=str(helper / "sql1909-finishing-mutations.json"),
        SUTURA_CATALOG_HELPER_DIR=str(helper),
        SUTURA_CATALOG_JUSTFILE=str(helper / "catalog-overlay-proof.just"),
        INSTA_UPDATE="no",
        NO_COLOR="1",
        CARGO_TERM_COLOR="never",
    )
    result = subprocess.run(
        ["bash", str(out / "venue-proof.sh")], cwd=source, env=env, check=False
    )
    (out / "exit.status").write_text(f"{result.returncode}\n", encoding="utf-8")
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()

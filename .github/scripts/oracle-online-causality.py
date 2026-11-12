#!/usr/bin/env python3
"""Hosted full causality with an online registry; source and gate remain unchanged."""
import hashlib, json, os, re, shutil, subprocess, sys, tempfile
from pathlib import Path

SOURCE = "5a74787ebe89037bc48c4a8177e27b6a3447003c"
TREE = "7b80bf5a421ce7889c6d15cc42c31081721475a0"
BASE = "745bf195f8b4e7598b1efd5b294664bf79b7bf6d"
def run(*args): return subprocess.check_output(args)
def sha(data): return hashlib.sha256(data).hexdigest()
def record(path, value): path.write_text(json.dumps(value, indent=2) + "\n")
def snapshot():
    probe = subprocess.run(["git", "rev-parse", "--is-inside-work-tree"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if probe.returncode: return {"cwd": str(Path.cwd()), "repository": None}
    files = {}
    for raw in run("git", "ls-files", "-z").split(b"\0"):
        if raw:
            p = Path(os.fsdecode(raw))
            if not p.exists() and not p.is_symlink(): files[str(p)] = {"missing": True}; continue
            link = p.is_symlink()
            files[str(p)] = [sha(os.fsencode(os.readlink(p)) if link else p.read_bytes()), link, p.lstat().st_mode & 0o777]
    return {"cwd": str(Path.cwd()), "head": run("git", "rev-parse", "HEAD").decode().strip(),
            "files": files, "index_stage_sha256": sha(run("git", "ls-files", "--stage", "-z")),
            "diff_sha256": sha(run("git", "diff", "--binary"))}

def cargo():
    out = Path(os.environ["SUTURA_PROOF_OUT"])
    call = Path(tempfile.mkdtemp(prefix="cargo-", dir=out))
    record(call / "before.json", snapshot())
    record(call / "argv.json", sys.argv[1:])
    # Only the source app's pinned cargo is used, including in the gate's nested worktrees.
    result = subprocess.run([os.environ["SUTURA_PROOF_REAL_CARGO"], *sys.argv[1:]], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    text = result.stdout.decode(errors="replace")
    secrets = re.findall(r"^::add-mask::([^\r\n]+)", text, re.MULTILINE)
    secrets += [value for name, value in os.environ.items() if name.startswith("SUTURA_") and name.endswith("PASSWORD")]
    for secret in sorted(filter(None, secrets), key=len, reverse=True): text = text.replace(secret, "***")
    text = re.sub(r"^::add-mask::.*\n", "", text, flags=re.MULTILINE)
    (call / "output.log").write_text(text)
    (call / "exit.status").write_text(str(result.returncode) + "\n")
    record(call / "after.json", snapshot())
    print(text, end="", flush=True)
    sys.exit(result.returncode if result.returncode >= 0 else 128 - result.returncode)

def main():
    assert os.environ["GITHUB_ACTIONS"] == "true"
    assert run("git", "rev-parse", "HEAD").decode().strip() == SOURCE
    assert run("git", "rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert not run("git", "status", "--porcelain")
    root = Path.cwd()
    out = root / "target/oracle-online-causality"
    out.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    record(out / "before.json", before)
    home = Path(tempfile.mkdtemp(prefix="online-cargo-", dir=root / "target"))
    wrapper = Path(tempfile.mkdtemp(prefix="online-cargo-bin-", dir=root / "target"))
    shutil.copyfile(__file__, wrapper / "cargo")
    (wrapper / "cargo").chmod(0o755)
    installable = ".#apps.x86_64-linux.causality.program"
    app = Path(run("nix", "eval", "--raw", installable).decode())
    context = json.loads(run("nix", "eval", "--json", "--apply", "builtins.getContext", installable))
    assert len(context) == 1 and all(path.endswith(".drv") for path in context)
    subprocess.run(["nix-store", "--realise", *context], check=True)
    original = app.read_text()
    needle = 'exec cargo run -q --profile ci -p xtask -- test-causality "$@"'
    assert original.count(needle) == 1
    replacement = '''export CARGO_HOME="$SUTURA_PROOF_CARGO_HOME"
test ! -e "$CARGO_HOME/config.toml" && test ! -e "$CARGO_HOME/config" || exit 1
export SUTURA_PROOF_REAL_CARGO="$(command -v cargo)"
export PATH="$SUTURA_PROOF_BIN:$PATH"
''' + needle
    script = original.replace(needle, replacement, 1)
    (out / "environment-original.sh").write_text(original)
    (out / "environment-online.sh").write_text(script)
    record(out / "inputs.json", {"source": SOURCE, "tree": TREE, "base": BASE,
           "helper": os.environ["GITHUB_WORKFLOW_SHA"], "payload_sha256": sha(Path(__file__).read_bytes()),
           "app": str(app), "app_context": context, "original_sha256": sha(original.encode()),
           "online_sha256": sha(script.encode()), "registry": "fresh online Cargo home; no source replacement config"})
    env = dict(os.environ, SUTURA_PROOF_CARGO_HOME=str(home), SUTURA_PROOF_BIN=str(wrapper), SUTURA_PROOF_OUT=str(out))
    for name in ["BASH_ENV", "CARGO_NET_OFFLINE", "CARGO_PROFILE_DEV_CODEGEN_BACKEND", "CARGO_UNSTABLE_CODEGEN_BACKEND", "NEXTEST_PROFILE"]:
        env.pop(name, None)
    result = subprocess.run(["timeout", "--kill-after=30s", "100m", "bash", str(out / "environment-online.sh"), "--since", BASE], env=env)
    (out / "gate.status").write_text(str(result.returncode) + "\n")
    record(out / "after.json", snapshot())
    assert snapshot() == before and not run("git", "status", "--porcelain")
    assert result.returncode == 0, "nonzero causality is retained as a non-pass, never suppressed"

if __name__ == "__main__":
    cargo() if Path(sys.argv[0]).name == "cargo" else main()

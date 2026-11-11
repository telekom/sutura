#!/usr/bin/env bash
# Prepared hosted-only route: produces artifacts, never installs them in the input checkout.
set -euo pipefail
worker_repo=$(cd "$1" && pwd)
mkdir -p "$2"
worker_out=$(cd "$2" && pwd)
worker_pin=4cfa166d233207ee188a277a7849bee7d9dfd2de
worker_scratch=$(mktemp -d)
trap 'rm -rf "$worker_scratch"' EXIT

git clone --no-checkout https://github.com/datafusion-contrib/datafusion-distributed "$worker_scratch/upstream"
git -C "$worker_scratch/upstream" checkout --detach "$worker_pin"
test "$(git -C "$worker_scratch/upstream" rev-parse HEAD)" = "$worker_pin"
test "$(git -C "$worker_scratch/upstream" rev-parse HEAD:codegen/regen.sh)" = 533056c856d13f564e1875532f19bdeb91c10ed9
test "$(git -C "$worker_scratch/upstream" rev-parse HEAD:codegen/Cargo.toml)" = 9819d07ac90725badd49fcb33aae8f593ffac4e6
test "$(git -C "$worker_scratch/upstream" rev-parse HEAD:codegen/src/main.rs)" = e9b65a40ea7ba2f9eee9006e053872a3d046f372

# Preserve all vendored protocol modifications, including those predating this patch.
cp "$worker_repo/vendor/datafusion-distributed/src/protocol/grpc/worker.proto" "$worker_scratch/upstream/src/protocol/grpc/worker.proto"
cp "$worker_repo/vendor/datafusion-distributed/src/protocol/grpc/observability/proto/observability.proto" "$worker_scratch/upstream/src/protocol/grpc/observability/proto/observability.proto"
cp "$worker_repo/rust-toolchain.toml" "$worker_scratch/upstream/rust-toolchain.toml"
export WORKER_CODEGEN_ROOT="$worker_scratch/upstream"
cat > "$worker_out/generator.just" <<'JUST'
set shell := ["bash", "-euo", "pipefail", "-c"]
resolve:
    cd "$WORKER_CODEGEN_ROOT" && cargo generate-lockfile --manifest-path codegen/Cargo.toml
regenerate:
    cd "$WORKER_CODEGEN_ROOT" && cargo run --locked --all-features --manifest-path codegen/Cargo.toml
JUST
just --justfile "$worker_out/generator.just" resolve
cp "$worker_scratch/upstream/codegen/Cargo.lock" "$worker_out/codegen.Cargo.lock"
just --justfile "$worker_out/generator.just" regenerate > "$worker_out/generation.log" 2>&1
cp "$worker_scratch/upstream/src/protocol/grpc/generated/worker.rs" "$worker_out/worker.rs"
cp "$worker_scratch/upstream/src/protocol/grpc/observability/generated/observability.rs" "$worker_out/observability.rs"
just --justfile "$worker_out/generator.just" regenerate > "$worker_out/regeneration.log" 2>&1
cmp "$worker_out/worker.rs" "$worker_scratch/upstream/src/protocol/grpc/generated/worker.rs"
cmp "$worker_out/observability.rs" "$worker_scratch/upstream/src/protocol/grpc/observability/generated/observability.rs"
cmp "$worker_out/codegen.Cargo.lock" "$worker_scratch/upstream/codegen/Cargo.lock"
{
    rustc --version --verbose
    cargo --version
    protoc --version
} > "$worker_out/tool-versions.txt"
python3 - "$worker_out" "$worker_repo" <<'PY'
import difflib, hashlib, json, sys
from pathlib import Path
out, repo = map(Path, sys.argv[1:])
name = 'vendor/datafusion-distributed/src/protocol/grpc/generated/worker.rs'
old = (repo/name).read_text().splitlines(keepends=True)
new = (out/'worker.rs').read_text().splitlines(keepends=True)
(out/'generated-worker.patch').write_text(''.join(difflib.unified_diff(old,new,fromfile='a/'+name,tofile='b/'+name)))
name = 'vendor/datafusion-distributed/src/protocol/grpc/observability/generated/observability.rs'
old = (repo/name).read_text().splitlines(keepends=True)
new = (out/'observability.rs').read_text().splitlines(keepends=True)
(out/'generated-observability.patch').write_text(''.join(difflib.unified_diff(old,new,fromfile='a/'+name,tofile='b/'+name)))
(out/'generation-manifest.json').write_text(json.dumps({
    'upstream_commit':'4cfa166d233207ee188a277a7849bee7d9dfd2de',
    'sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in out.iterdir() if p.is_file() and p.name!='generation-manifest.json'},
},indent=2)+'\n')
PY

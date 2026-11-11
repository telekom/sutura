import ast
import difflib
import hashlib
import json
import os
import subprocess
from pathlib import Path

out = Path('/tmp/oracle-export')
locked = json.loads(Path('flake.lock').read_text())['nodes']['arrow-adbc-src']['locked']
assert locked['rev'] == 'a3f81c6feb1133ba80600ac32e6d7c0b0b407e18'
(out / 'arrow-locked.json').write_text(json.dumps(locked, indent=2) + '\n')
metadata = json.loads(subprocess.check_output(['nix', 'flake', 'prefetch', '--json', '--refresh', f"github:apache/arrow-adbc/{locked['rev']}"]))
(out / 'arrow-prefetched.json').write_text(json.dumps(metadata, indent=2) + '\n')
root = Path(metadata['storePath'])
actual = subprocess.check_output(['nix', 'hash', 'path', str(root)], text=True).strip()
assert actual == metadata['hash'], metadata
repo = Path('/tmp/arrow-identity')
subprocess.run(['git', 'init', str(repo)], check=True)
subprocess.run(['git', '-C', str(repo), 'fetch', '--depth=1', 'https://github.com/apache/arrow-adbc.git', locked['rev']], check=True)
assert subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'FETCH_HEAD'], text=True).strip() == locked['rev']
tree = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'FETCH_HEAD^{tree}'], text=True).strip()
entries = subprocess.check_output(['git', '-C', str(repo), 'ls-tree', '-rz', 'FETCH_HEAD']).split(b'\0')
substituted = {f'python/{p}/{p}/_static_version.py' for p in ['adbc_driver_manager', 'adbc_driver_postgresql', 'adbc_driver_sqlite', 'adbc_driver_flightsql']}
manifest = []
changed = []
paths = set()
for raw in entries:
    if not raw:
        continue
    header, name = raw.split(b'\t', 1)
    mode, kind, oid = header.decode().split()
    assert kind == 'blob', (mode, kind, name)
    name = os.fsdecode(name)
    paths.add(name)
    path = root / name
    assert path.is_symlink() == (mode == '120000'), name
    original = subprocess.check_output(['git', '-C', str(repo), 'cat-file', 'blob', oid])
    fetched = os.fsencode(os.readlink(path)) if path.is_symlink() else path.read_bytes()
    if mode != '120000':
        assert bool(path.stat().st_mode & 0o111) == (mode == '100755'), name
    manifest.append({'path': name, 'git_blob': oid, 'mode': mode, 'git_sha256': hashlib.sha256(original).hexdigest(), 'fetched_sha256': hashlib.sha256(fetched).hexdigest()})
    if original != fetched:
        assert name in substituted, name
        before, after = original.decode().splitlines(), fetched.decode().splitlines()
        assert len(before) == len(after), name
        for old, new in zip(before, after, strict=True):
            if old != new:
                key = old.split(' = ', 1)[0]
                assert key in {'refnames', 'git_hash'}, (name, old, new)
                value = ast.literal_eval(new.split(' = ', 1)[1])
                assert isinstance(value, str) and value.isascii(), (name, new)
                if key == 'git_hash':
                    assert len(value) >= 7 and locked['rev'].startswith(value), value
                assert old == f'{key} = "$Format:%D$"' if key == 'refnames' else old == 'git_hash = "$Format:%h$"'
        changed.append({'path': name, 'diff': '\n'.join(difflib.unified_diff(before, after, fromfile='git/' + name, tofile='archive/' + name))})
actual_paths = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() or p.is_symlink()}
assert paths == actual_paths, sorted(paths ^ actual_paths)
(out / 'arrow-files.json').write_text(json.dumps(manifest, indent=2) + '\n')
summary = {'commit': locked['rev'], 'tree': tree, 'locked_nar': locked['narHash'], 'fetched_nar': actual, 'files': len(manifest), 'changes': changed}
(out / 'arrow-identity.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2))

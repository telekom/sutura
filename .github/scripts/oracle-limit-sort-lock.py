import copy
import gzip
import hashlib
import json
import os
import stat
import subprocess
import sys
import tomllib
from pathlib import Path

OUT = Path('/tmp/oracle-export')
RECEIPT = json.loads((OUT / 'lock-overlay-receipt.json').read_text())


def git(*args):
    return subprocess.check_output(['git', *args])


def sha(data):
    return hashlib.sha256(data).hexdigest()


def manifest():
    records = {}
    for raw in git('ls-files', '-z').split(b'\0'):
        if raw:
            path = Path(os.fsdecode(raw))
            symlink = path.is_symlink()
            data = os.fsencode(os.readlink(path)) if symlink else path.read_bytes()
            records[str(path)] = [sha(data), symlink, stat.S_IMODE(path.lstat().st_mode)]
    return records


if sys.argv[1:] == ['apply']:
    assert git('rev-parse', 'HEAD').decode().strip() == RECEIPT['source']
    assert git('rev-parse', 'HEAD^{tree}').decode().strip() == RECEIPT['source_tree']
    assert not git('status', '--porcelain')
    patch = OUT / 'lock-overlay-product.patch'
    patch.write_bytes(gzip.decompress((OUT / 'lock-overlay-product.patch.gz').read_bytes()))
    assert sha(patch.read_bytes()) == RECEIPT['patch_sha256']
    assert not any(name.endswith('Cargo.lock') for name in RECEIPT['changed_files'])
    initial = manifest()
    (OUT / 'lock-overlay-original-files.json').write_text(json.dumps(initial, indent=2) + '\n')
    index = Path(git('rev-parse', '--git-path', 'index').decode().strip())
    (OUT / 'lock-overlay-original.index').write_bytes(index.read_bytes())
    subprocess.run(['git', 'apply', '--check', str(patch)], check=True)
    subprocess.run(['git', 'apply', str(patch)], check=True)
    subprocess.run(['git', 'add', '-N', '--', *RECEIPT['new_files']], check=True)
    assert sorted(git('diff', '--name-only').decode().splitlines()) == sorted(RECEIPT['changed_files'])
    injected = manifest()
    expected = dict(initial)
    expected.update({name: [digest, False, 0o644] for name, digest in RECEIPT['changed_files'].items()})
    assert injected == expected
    (OUT / 'lock-overlay-injected-files.json').write_text(json.dumps(injected, indent=2) + '\n')
    temporary = OUT / 'lock-overlay-tree.index'
    assert not temporary.exists()
    env = dict(os.environ, GIT_INDEX_FILE=str(temporary))
    current_index = index.read_bytes()
    try:
        subprocess.run(['git', 'read-tree', 'HEAD'], env=env, check=True)
        subprocess.run(['git', 'add', '--', *RECEIPT['changed_files']], env=env, check=True)
        tree = subprocess.check_output(['git', 'write-tree'], env=env).decode().strip()
        assert tree == RECEIPT['injected_tree']
    finally:
        temporary.unlink(missing_ok=True)
    assert index.read_bytes() == current_index
    (OUT / 'lock-overlay-injected-tree').write_text(tree + '\n')
    (OUT / 'lock-overlay-injected.index').write_bytes(current_index)
elif sys.argv[1:] == ['check']:
    records = {}
    for lock, metadata in [('Cargo.lock', 'metadata.json'), ('fuzz/Cargo.lock', 'fuzz-metadata.json')]:
        before = tomllib.loads(git('show', 'HEAD:' + lock).decode())
        expected = copy.deepcopy(before)
        packages = [package for package in expected['package'] if package['name'] == 'datafusion-sql']
        assert len(packages) == 1 and packages[0]['version'] == '55.1.0'
        assert packages[0].pop('source') == 'registry+https://github.com/rust-lang/crates.io-index'
        assert packages[0].pop('checksum') == '5b4848be66dd008d55adbe1dbc8cbd6a2502588ad0c0b8483bab0f43fd4495d0'
        actual = tomllib.loads(Path(lock).read_text())
        assert actual == expected, lock
        graph = json.loads((OUT / metadata).read_text())
        packages = [package for package in graph['packages'] if package['name'] == 'datafusion-sql']
        assert len(packages) == 1 and packages[0]['version'] == '55.1.0'
        assert packages[0]['source'] is None
        assert Path(packages[0]['manifest_path']).resolve() == Path('vendor/datafusion-sql/Cargo.toml').resolve()
        records[lock] = sha(Path(lock).read_bytes())
    initial = json.loads((OUT / 'lock-overlay-injected-files.json').read_text())
    current = manifest()
    allowed = {'Cargo.lock', 'fuzz/Cargo.lock'}
    assert set(current) == set(initial)
    assert {name for name in initial if initial[name] != current[name]} == allowed
    (OUT / 'lock-overlay-generated-files.json').write_text(json.dumps(current, indent=2) + '\n')
    (OUT / 'lock-overlay-locks.json').write_text(json.dumps(records, indent=2) + '\n')
else:
    raise SystemExit('expected apply or check')

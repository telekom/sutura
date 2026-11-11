import hashlib
import json
import os
import re
import sqlite3
import subprocess
import tempfile
from pathlib import Path
from urllib.parse import quote

out = Path('/tmp/oracle-export')
locked = json.loads(Path('flake.lock').read_text())['nodes']['arrow-adbc-src']['locked']
uri = f"github:apache/arrow-adbc/{locked['rev']}?narHash={quote(locked['narHash'], safe='')}"
(out / 'arrow-cache-locked.json').write_text(json.dumps(locked, indent=2) + '\n')
(out / 'nix-version.txt').write_text(subprocess.check_output(['nix', '--version'], text=True))
cache = Path(os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))) / 'nix'


def snapshot(phase):
    records = []
    for path in sorted(cache.glob('*.sqlite')):
        record = {'file': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'arrow_rows': []}
        connection = sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)
        try:
            for (table,) in connection.execute("SELECT name FROM sqlite_master WHERE type='table'"):
                identifier = '"' + table.replace('"', '""') + '"'
                for row in connection.execute(f'SELECT * FROM {identifier}'):
                    text = ' '.join(str(value) for value in row)
                    if locked['rev'] in text or 'arrow-adbc' in text:
                        record['arrow_rows'].append({
                            'table': table,
                            'row_sha256': hashlib.sha256(text.encode()).hexdigest(),
                            'hashes': sorted(set(re.findall(r'sha256-[A-Za-z0-9+/]+=*', text))),
                            'revisions': sorted(set(re.findall(r'\b[0-9a-f]{40}\b', text))),
                        })
        finally:
            connection.close()
        records.append(record)
    (out / f'arrow-cache-{phase}.json').write_text(json.dumps(records, indent=2) + '\n')


def probe(name, env, refresh=False):
    command = ['nix', 'flake', 'prefetch', '--json']
    if refresh:
        command.append('--refresh')
    result = subprocess.run(command + [uri], env=env, text=True, capture_output=True)
    (out / f'arrow-cache-{name}.stdout').write_text(result.stdout)
    (out / f'arrow-cache-{name}.stderr').write_text(result.stderr)
    (out / f'arrow-cache-{name}.status').write_text(str(result.returncode) + '\n')
    print(name, result.returncode, result.stdout, result.stderr, flush=True)


snapshot('before')
probe('original', os.environ.copy())
snapshot('after-original')
probe('refresh', os.environ.copy(), refresh=True)
snapshot('after-refresh')
probe('clean', os.environ | {'XDG_CACHE_HOME': tempfile.mkdtemp(prefix='arrow-cache-')}, refresh=True)

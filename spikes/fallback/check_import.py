#!/usr/bin/env python3
"""Exercise bounded import and reconcile the exact committed prefix after SIGKILL."""
import hashlib
import json
import os
import pathlib
import selectors
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'helix'))
from run_case import digest, process_identity, publish


def main():
    binary, output = pathlib.Path(sys.argv[1]).resolve(strict=True), pathlib.Path(sys.argv[2]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 fallback import receipts\n')
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-import-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        publish(output / 'admission.json', dict(binary_sha256=digest(binary),
            harness_sha256=digest(pathlib.Path(__file__)), controller=process_identity(os.getpid()),
            temporary_root=temporary, scope='synthetic bounded-import contract only'))
        source, manifest = root / 'source.jsonl', root / 'manifest.json'
        rows = [dict(key=f'fixture:{i}', body=f'synthetic {i} ' + ''.join(
                     hashlib.sha256(f'{i}:{j}'.encode()).hexdigest() for j in range(64)),
                     partition=f'p{i % 8}') for i in range(1057)]

        def prepare(values):
            data = b''.join((json.dumps(row, separators=(',', ':')) + '\n').encode() for row in values)
            source.write_bytes(data)
            expected = dict(documents=len(values), vectors=sum('embedding' in row for row in values),
                prepared_bytes=len(data), prepared_sha256=hashlib.sha256(data).hexdigest(),
                indexed_body_utf8_bytes=sum(len(row['body'].encode()) for row in values))
            manifest.write_text(json.dumps(dict(corpus=expected)))
            return data, expected

        data, expected = prepare(rows)

        def invoke(case, *arguments, error=None, kill=False):
            name = f'{len(records):03d}-{case}'
            command = [str(binary), str(root / case), *map(str, arguments)]
            env = dict(os.environ, AKASHA_SPIKE_PROFILE='low')
            env.pop('AKASHA_SPIKE_IMPORT_HOLD_AFTER', None)
            if kill:
                env['AKASHA_SPIKE_IMPORT_HOLD_AFTER'] = '128'
            with (output / (name + '.stdout')).open('xb') as stdout, (output / (name + '.stderr')).open('xb') as stderr:
                child = subprocess.Popen(command, stdout=stdout, stderr=subprocess.PIPE, env=env)
                identity = process_identity(child.pid)
                try:
                    prefix = b''
                    if kill:
                        with selectors.DefaultSelector() as selector:
                            selector.register(child.stderr, selectors.EVENT_READ)
                            until = time.monotonic() + 60
                            while time.monotonic() < until:
                                assert child.poll() is None, (name, prefix)
                                if selector.select(timeout=1):
                                    prefix += os.read(child.stderr.fileno(), 4096)
                                    if b'IMPORT_CHECKPOINT:128\n' in prefix:
                                        break
                            else:
                                raise TimeoutError('durable import checkpoint absent')
                        assert process_identity(child.pid)['start_ticks'] == identity['start_ticks']
                        child.kill()
                    _, tail = child.communicate(timeout=180)
                    stderr.write(prefix + tail)
                    assert child.returncode == (-9 if kill else 1 if error else 0), (name, tail[-3000:])
                    if error:
                        assert error.encode() in tail, (name, tail[-3000:])
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            records.append(dict(name=name, command=command, child=identity, exit=child.returncode,
                                expected_error=error, killed_after=128 if kill else None))
            elapsed = time.monotonic() - started
            publish(output / 'progress.json', dict(completed=len(records), total=21,
                elapsed_s=elapsed, rate_per_s=len(records)/elapsed,
                eta_s=max(0, 21-len(records))*elapsed/len(records), phase=name))
            if not error and not kill:
                return json.loads((output / (name + '.stdout')).read_text().splitlines()[-1])

        completed = invoke('complete', '--load', source, manifest)
        assert completed['documents'] == 1057 and completed['complete']
        assert completed.get('storage_format') == 2, 'compressed corpus/scope layout absent'
        verified = invoke('complete', 'verify-import')
        assert verified['documents'] == 1057 and verified['prefix'] == expected['prepared_sha256']
        assert invoke('complete', 'corpus-get', 1_001_056) == rows[-1]
        assert invoke('complete', '--resume-load', source, manifest) == completed
        invoke('complete', '--load', source, manifest, error='already exists')
        invoke('crash', '--load', source, manifest, kill=True)
        state = invoke('crash', 'import-state')
        assert state['documents'] == 128 and not state['complete']
        assert invoke('crash', 'verify-import')['documents'] == 128
        source.write_bytes(data.replace(b'synthetic 0', b'different 0', 1))
        invoke('crash', '--resume-load', source, manifest, error='committed prefix')
        assert invoke('crash', 'import-state') == state
        source.write_bytes(data)
        assert invoke('crash', '--resume-load', source, manifest) == completed
        assert invoke('crash', 'verify-import') == verified
        assert invoke('crash', 'corpus-get', 1_000_127) == rows[127]
        changed = dict(expected, documents=1058)
        manifest.write_text(json.dumps(dict(corpus=changed)))
        invoke('crash', '--resume-load', source, manifest, error='manifest')
        prepare([rows[0], rows[0]])
        invoke('duplicate', '--load', source, manifest, error='duplicate source key')
        assert invoke('duplicate', 'verify-import')['documents'] == 0
        prepare([dict(rows[0], body='x' * (512 * 1024))])
        invoke('oversized', '--load', source, manifest, error='record exceeds')
        assert invoke('oversized', 'verify-import')['documents'] == 0
        prepare([dict(rows[0], embedding=[0.0] * 1536)])
        invoke('zero-vector', '--load', source, manifest, error='invalid embedding')
        assert invoke('zero-vector', 'verify-import')['documents'] == 0
        assert invoke('crash', 'corpus-get', 1_001_057) is None
        assert len(records) == 21
    elapsed = time.monotonic() - started
    publish(output / 'progress.json', dict(phase='completed', completed=21, total=21,
        elapsed_s=elapsed, rate_per_s=21/elapsed, eta_s=0))
    publish(output / 'result.json', dict(status='passed', checks=21, seconds=elapsed, records=records,
        artifact_sha256={str(p.relative_to(output)):digest(p) for p in output.iterdir() if p.is_file()},
        not_tested=['full corpus', 'search indexes', 'power loss', 'disk exhaustion']))
    print(json.dumps(dict(status='passed', checks=21, seconds=elapsed)))


if __name__ == '__main__':
    main()
